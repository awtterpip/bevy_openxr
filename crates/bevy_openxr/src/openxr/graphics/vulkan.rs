use std::ffi::{c_void, CString};

use ash::vk::Handle;
use bevy::log::{debug, error};
use bevy::math::UVec2;
use bevy::render::render_resource::TextureFormat;
use openxr::{sys, Version};
use wgpu_hal::api::Vulkan;
use wgpu_hal::Api;

use super::{GraphicsExt, GraphicsType, GraphicsWrap};
use crate::error::OxrError;
use crate::session::OxrSessionCreateNextChain;
use crate::types::{AppInfo, OxrExtensions, Result, WgpuGraphics};

#[cfg(not(target_os = "android"))]
const VK_TARGET_VERSION: Version = Version::new(1, 2, 0);
#[cfg(target_os = "android")]
const VK_TARGET_VERSION: Version = Version::new(1, 1, 0);

const VK_TARGET_VERSION_ASH: u32 = ash::vk::make_api_version(
    0,
    VK_TARGET_VERSION.major() as u32,
    VK_TARGET_VERSION.minor() as u32,
    VK_TARGET_VERSION.patch(),
);

unsafe impl GraphicsExt for openxr::Vulkan {
    fn wrap<T: GraphicsType>(item: T::Inner<Self>) -> GraphicsWrap<T> {
        GraphicsWrap::Vulkan(item)
    }

    fn required_exts() -> OxrExtensions {
        let mut extensions = openxr::ExtensionSet::default();
        extensions.khr_vulkan_enable2 = true;
        extensions.into()
    }

    fn from_wgpu_format(format: wgpu::TextureFormat) -> Option<Self::Format> {
        wgpu_to_vulkan(format).map(|f| f.as_raw() as _)
    }

    fn into_wgpu_format(format: Self::Format) -> Option<wgpu::TextureFormat> {
        vulkan_to_wgpu(ash::vk::Format::from_raw(format as _))
    }

    unsafe fn to_wgpu_img(
        color_image: Self::SwapchainImage,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        resolution: UVec2,
    ) -> Result<wgpu::Texture> {
        let color_image = ash::vk::Image::from_raw(color_image);
        let wgpu_hal_texture = unsafe {
            <wgpu_hal::vulkan::Api as wgpu_hal::Api>::Device::texture_from_raw(
                color_image,
                &wgpu_hal::TextureDescriptor {
                    label: Some("VR Swapchain"),
                    size: wgpu::Extent3d {
                        width: resolution.x,
                        height: resolution.y,
                        depth_or_array_layers: 2,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu_hal::TextureUses::COLOR_TARGET | wgpu_hal::TextureUses::COPY_DST,
                    memory_flags: wgpu_hal::MemoryFlags::empty(),
                    view_formats: vec![],
                },
                None,
            )
        };
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu_hal::vulkan::Api>(
                wgpu_hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("VR Swapchain"),
                    size: wgpu::Extent3d {
                        width: resolution.x,
                        height: resolution.y,
                        depth_or_array_layers: 2,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
            )
        };
        Ok(texture)
    }

    fn init_graphics(
        app_info: &AppInfo,
        instance: &openxr::Instance,
        system_id: openxr::SystemId,
    ) -> Result<(WgpuGraphics, Self::SessionCreateInfo)> {
        let reqs = instance.graphics_requirements::<openxr::Vulkan>(system_id)?;
        if VK_TARGET_VERSION < reqs.min_api_version_supported
            || VK_TARGET_VERSION.major() > reqs.max_api_version_supported.major()
        {
            error!(
                "OpenXR runtime requires Vulkan version > {}, < {}.0.0",
                reqs.min_api_version_supported,
                reqs.max_api_version_supported.major() + 1
            );
            return Err(OxrError::FailedGraphicsRequirements);
        };
        let vk_entry = unsafe { ash::Entry::load() }?;
        let flags = wgpu::InstanceFlags::default().with_env();
        let extensions =
            <Vulkan as Api>::Instance::desired_extensions(&vk_entry, VK_TARGET_VERSION_ASH, flags)?;
        let device_extensions = [
            ash::khr::swapchain::NAME,
            ash::khr::draw_indirect_count::NAME,
            ash::khr::timeline_semaphore::NAME,
            ash::khr::imageless_framebuffer::NAME,
            ash::khr::image_format_list::NAME,
            #[cfg(target_os = "macos")]
            ash::khr::portability_subset::NAME,
            #[cfg(target_os = "macos")]
            ash::ext::metal_objects::NAME,
        ];

        let vk_instance = unsafe {
            let extensions_cchar: Vec<_> = extensions.iter().map(|s| s.as_ptr()).collect();

            let app_name = CString::new(app_info.name.clone().into_owned())?;
            let vk_app_info = ash::vk::ApplicationInfo::default()
                .application_name(&app_name)
                .application_version(1)
                .engine_name(&app_name)
                .engine_version(1)
                .api_version(VK_TARGET_VERSION_ASH);

            let vk_instance = instance
                .create_vulkan_instance(
                    system_id,
                    #[allow(clippy::missing_transmute_annotations)]
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    &ash::vk::InstanceCreateInfo::default()
                        .application_info(&vk_app_info)
                        .enabled_extension_names(&extensions_cchar) as *const _
                        as *const _,
                )?
                .map_err(ash::vk::Result::from_raw)?;

            ash::Instance::load(
                vk_entry.static_fn(),
                ash::vk::Instance::from_raw(vk_instance as _),
            )
        };
        let api_layers = unsafe { vk_entry.enumerate_instance_layer_properties()? };
        let has_nv_optimus = api_layers.iter().any(|v| {
            v.layer_name_as_c_str()
                .is_ok_and(|v| v == c"VK_LAYER_NV_optimus")
        });

        drop(api_layers);

        let vk_instance_ptr = vk_instance.handle().as_raw() as *const c_void;

        let vk_physical_device = ash::vk::PhysicalDevice::from_raw(unsafe {
            instance.vulkan_graphics_device(system_id, vk_instance.handle().as_raw() as _)? as _
        });
        let vk_physical_device_ptr = vk_physical_device.as_raw() as *const c_void;

        let vk_device_properties =
            unsafe { vk_instance.get_physical_device_properties(vk_physical_device) };

        if vk_device_properties.api_version < VK_TARGET_VERSION_ASH {
            unsafe { vk_instance.destroy_instance(None) }
            error!(
                "Vulkan physical device doesn't support version {}.{}.{}",
                VK_TARGET_VERSION.major(),
                VK_TARGET_VERSION.minor(),
                VK_TARGET_VERSION.patch()
            );
            return Err(OxrError::FailedGraphicsRequirements);
        }

        // the android_sdk_version stuff is copied from wgpu
        #[cfg(target_os = "android")]
        let android_sdk_version = {
            let properties = android_system_properties::AndroidSystemProperties::new();
            // See: https://developer.android.com/reference/android/os/Build.VERSION_CODES
            if let Some(val) = properties.get("ro.build.version.sdk") {
                match val.parse::<u32>() {
                    Ok(sdk_ver) => sdk_ver,
                    Err(err) => {
                        error!(
                            concat!(
                                "Couldn't parse Android's ",
                                "ro.build.version.sdk system property ({}): {}",
                            ),
                            val,
                            err,
                        );
                        0
                    }
                }
            } else {
                error!("Couldn't read Android's ro.build.version.sdk system property");
                0
            }
        };
        #[cfg(not(target_os = "android"))]
        let android_sdk_version = 0;

        let wgpu_vk_instance = unsafe {
            <Vulkan as Api>::Instance::from_raw(
                vk_entry.clone(),
                vk_instance.clone(),
                vk_device_properties.api_version,
                android_sdk_version,
                None,
                extensions,
                flags,
                has_nv_optimus,
                None,
            )?
        };

        let Some(wgpu_exposed_adapter) = wgpu_vk_instance.expose_adapter(vk_physical_device) else {
            error!("WGPU failed to provide an adapter");
            return Err(OxrError::FailedGraphicsRequirements);
        };
        let wgpu_features = wgpu_exposed_adapter.features;
        debug!("wgpu features: {wgpu_features:#?}");

        let enabled_extensions = wgpu_exposed_adapter
            .adapter
            .required_device_extensions(wgpu_features);

        let (wgpu_open_device, vk_device_ptr, queue_family_index) = {
            let extensions_cchar: Vec<_> = device_extensions.iter().map(|s| s.as_ptr()).collect();
            let mut enabled_phd_features = wgpu_exposed_adapter
                .adapter
                .physical_device_features(&enabled_extensions, wgpu_features);
            let family_index = 0;
            let family_info = ash::vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family_index)
                .queue_priorities(&[1.0]);
            let family_infos = [family_info];
            let mut physical_device_multiview_features = ash::vk::PhysicalDeviceMultiviewFeatures {
                multiview: ash::vk::TRUE,
                ..Default::default()
            };
            let info = enabled_phd_features
                .add_to_device_create(
                    ash::vk::DeviceCreateInfo::default()
                        .queue_create_infos(&family_infos)
                        .push_next(&mut physical_device_multiview_features),
                )
                .enabled_extension_names(&extensions_cchar);
            let vk_device = unsafe {
                let vk_device = instance
                    .create_vulkan_device(
                        system_id,
                        #[allow(clippy::missing_transmute_annotations)]
                        std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                        vk_physical_device.as_raw() as _,
                        &info as *const _ as *const _,
                    )?
                    .map_err(ash::vk::Result::from_raw)?;

                ash::Device::load(
                    vk_instance.fp_v1_0(),
                    ash::vk::Device::from_raw(vk_device as _),
                )
            };
            let vk_device_ptr = vk_device.handle().as_raw() as *const c_void;

            let wgpu_open_device = unsafe {
                wgpu_exposed_adapter.adapter.device_from_raw(
                    vk_device,
                    None,
                    &enabled_extensions,
                    wgpu_features,
                    &wgpu::MemoryHints::Performance,
                    family_info.queue_family_index,
                    0,
                )
            }?;

            (
                wgpu_open_device,
                vk_device_ptr,
                family_info.queue_family_index,
            )
        };

        let wgpu_instance =
            unsafe { wgpu::Instance::from_hal::<wgpu_hal::api::Vulkan>(wgpu_vk_instance) };
        let wgpu_adapter = unsafe { wgpu_instance.create_adapter_from_hal(wgpu_exposed_adapter) };
        let limits = wgpu_adapter.limits();
        debug!("wgpu_limits: {limits:#?}");
        let (wgpu_device, wgpu_queue) = unsafe {
            wgpu_adapter.create_device_from_hal(
                wgpu_open_device,
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu_features,
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
        }?;

        Ok((
            WgpuGraphics(
                wgpu_device,
                wgpu_queue,
                wgpu_adapter.get_info(),
                wgpu_adapter,
                wgpu_instance,
            ),
            openxr::vulkan::SessionCreateInfo {
                instance: vk_instance_ptr,
                physical_device: vk_physical_device_ptr,
                device: vk_device_ptr,
                queue_family_index,
                queue_index: 0,
            },
        ))
    }

    unsafe fn create_session(
        instance: &openxr::Instance,
        system_id: openxr::SystemId,
        info: &Self::SessionCreateInfo,
        session_create_info_chain: &mut OxrSessionCreateNextChain,
    ) -> openxr::Result<(
        openxr::Session<Self>,
        openxr::FrameWaiter,
        openxr::FrameStream<Self>,
    )> {
        let next_ptr = session_create_info_chain.chain_pointer();
        let binding = sys::GraphicsBindingVulkanKHR {
            ty: sys::GraphicsBindingVulkanKHR::TYPE,
            next: next_ptr,
            instance: info.instance,
            physical_device: info.physical_device,
            device: info.device,
            queue_family_index: info.queue_family_index,
            queue_index: info.queue_index,
        };
        let info = sys::SessionCreateInfo {
            ty: sys::SessionCreateInfo::TYPE,
            next: &binding as *const _ as *const _,
            create_flags: Default::default(),
            system_id,
        };
        let mut out = sys::Session::NULL;
        cvt((instance.fp().create_session)(
            instance.as_raw(),
            &info,
            &mut out,
        ))?;
        Ok(openxr::Session::from_raw(
            instance.clone(),
            out,
            Box::new(()),
        ))
    }
}

fn cvt(x: sys::Result) -> openxr::Result<sys::Result> {
    if x.into_raw() >= 0 {
        Ok(x)
    } else {
        Err(x)
    }
}

fn vulkan_to_wgpu(format: ash::vk::Format) -> Option<wgpu::TextureFormat> {
    use ash::vk::Format as F;
    use wgpu::TextureFormat as Tf;
    use wgpu::{AstcBlock, AstcChannel};
    Some(match format {
        F::R8_UNORM => Tf::R8Unorm,
        F::R8_SNORM => Tf::R8Snorm,
        F::R8_UINT => Tf::R8Uint,
        F::R8_SINT => Tf::R8Sint,
        F::R16_UINT => Tf::R16Uint,
        F::R16_SINT => Tf::R16Sint,
        F::R16_UNORM => Tf::R16Unorm,
        F::R16_SNORM => Tf::R16Snorm,
        F::R16_SFLOAT => Tf::R16Float,
        F::R8G8_UNORM => Tf::Rg8Unorm,
        F::R8G8_SNORM => Tf::Rg8Snorm,
        F::R8G8_UINT => Tf::Rg8Uint,
        F::R8G8_SINT => Tf::Rg8Sint,
        F::R16G16_UNORM => Tf::Rg16Unorm,
        F::R16G16_SNORM => Tf::Rg16Snorm,
        F::R32_UINT => Tf::R32Uint,
        F::R32_SINT => Tf::R32Sint,
        F::R32_SFLOAT => Tf::R32Float,
        F::R16G16_UINT => Tf::Rg16Uint,
        F::R16G16_SINT => Tf::Rg16Sint,
        F::R16G16_SFLOAT => Tf::Rg16Float,
        F::R8G8B8A8_UNORM => Tf::Rgba8Unorm,
        F::R8G8B8A8_SRGB => Tf::Rgba8UnormSrgb,
        F::B8G8R8A8_SRGB => Tf::Bgra8UnormSrgb,
        F::R8G8B8A8_SNORM => Tf::Rgba8Snorm,
        F::B8G8R8A8_UNORM => Tf::Bgra8Unorm,
        F::R8G8B8A8_UINT => Tf::Rgba8Uint,
        F::R8G8B8A8_SINT => Tf::Rgba8Sint,
        F::A2B10G10R10_UINT_PACK32 => Tf::Rgb10a2Uint,
        F::A2B10G10R10_UNORM_PACK32 => Tf::Rgb10a2Unorm,
        F::B10G11R11_UFLOAT_PACK32 => Tf::Rg11b10Ufloat,
        F::R32G32_UINT => Tf::Rg32Uint,
        F::R32G32_SINT => Tf::Rg32Sint,
        F::R32G32_SFLOAT => Tf::Rg32Float,
        F::R16G16B16A16_UINT => Tf::Rgba16Uint,
        F::R16G16B16A16_SINT => Tf::Rgba16Sint,
        F::R16G16B16A16_UNORM => Tf::Rgba16Unorm,
        F::R16G16B16A16_SNORM => Tf::Rgba16Snorm,
        F::R16G16B16A16_SFLOAT => Tf::Rgba16Float,
        F::R32G32B32A32_UINT => Tf::Rgba32Uint,
        F::R32G32B32A32_SINT => Tf::Rgba32Sint,
        F::R32G32B32A32_SFLOAT => Tf::Rgba32Float,
        F::D32_SFLOAT => Tf::Depth32Float,
        F::D32_SFLOAT_S8_UINT => Tf::Depth32FloatStencil8,
        F::D16_UNORM => Tf::Depth16Unorm,
        F::G8_B8R8_2PLANE_420_UNORM => Tf::NV12,
        F::E5B9G9R9_UFLOAT_PACK32 => Tf::Rgb9e5Ufloat,
        F::BC1_RGBA_UNORM_BLOCK => Tf::Bc1RgbaUnorm,
        F::BC1_RGBA_SRGB_BLOCK => Tf::Bc1RgbaUnormSrgb,
        F::BC2_UNORM_BLOCK => Tf::Bc2RgbaUnorm,
        F::BC2_SRGB_BLOCK => Tf::Bc2RgbaUnormSrgb,
        F::BC3_UNORM_BLOCK => Tf::Bc3RgbaUnorm,
        F::BC3_SRGB_BLOCK => Tf::Bc3RgbaUnormSrgb,
        F::BC4_UNORM_BLOCK => Tf::Bc4RUnorm,
        F::BC4_SNORM_BLOCK => Tf::Bc4RSnorm,
        F::BC5_UNORM_BLOCK => Tf::Bc5RgUnorm,
        F::BC5_SNORM_BLOCK => Tf::Bc5RgSnorm,
        F::BC6H_UFLOAT_BLOCK => Tf::Bc6hRgbUfloat,
        F::BC6H_SFLOAT_BLOCK => Tf::Bc6hRgbFloat,
        F::BC7_UNORM_BLOCK => Tf::Bc7RgbaUnorm,
        F::BC7_SRGB_BLOCK => Tf::Bc7RgbaUnormSrgb,
        F::ETC2_R8G8B8_UNORM_BLOCK => Tf::Etc2Rgb8Unorm,
        F::ETC2_R8G8B8_SRGB_BLOCK => Tf::Etc2Rgb8UnormSrgb,
        F::ETC2_R8G8B8A1_UNORM_BLOCK => Tf::Etc2Rgb8A1Unorm,
        F::ETC2_R8G8B8A1_SRGB_BLOCK => Tf::Etc2Rgb8A1UnormSrgb,
        F::ETC2_R8G8B8A8_UNORM_BLOCK => Tf::Etc2Rgba8Unorm,
        F::ETC2_R8G8B8A8_SRGB_BLOCK => Tf::Etc2Rgba8UnormSrgb,
        F::EAC_R11_UNORM_BLOCK => Tf::EacR11Unorm,
        F::EAC_R11_SNORM_BLOCK => Tf::EacR11Snorm,
        F::EAC_R11G11_UNORM_BLOCK => Tf::EacRg11Unorm,
        F::EAC_R11G11_SNORM_BLOCK => Tf::EacRg11Snorm,
        F::ASTC_4X4_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B4x4,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_5X4_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B5x4,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_5X5_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B5x5,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_6X5_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B6x5,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_6X6_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B6x6,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_8X5_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B8x5,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_8X6_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B8x6,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_8X8_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B8x8,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_10X5_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B10x5,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_10X6_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B10x6,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_10X8_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B10x8,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_10X10_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B10x10,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_12X10_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B12x10,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_12X12_UNORM_BLOCK => Tf::Astc {
            block: AstcBlock::B12x12,
            channel: AstcChannel::Unorm,
        },
        F::ASTC_4X4_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B4x4,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_5X4_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B5x4,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_5X5_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B5x5,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_6X5_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B6x5,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_6X6_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B6x6,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_8X5_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B8x5,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_8X6_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B8x6,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_8X8_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B8x8,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_10X5_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B10x5,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_10X6_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B10x6,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_10X8_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B10x8,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_10X10_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B10x10,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_12X10_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B12x10,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_12X12_SRGB_BLOCK => Tf::Astc {
            block: AstcBlock::B12x12,
            channel: AstcChannel::UnormSrgb,
        },
        F::ASTC_4X4_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B4x4,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_5X4_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B5x4,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_5X5_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B5x5,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_6X5_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B6x5,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_6X6_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B6x6,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_8X5_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B8x5,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_8X6_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B8x6,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_8X8_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B8x8,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_10X5_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B10x5,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_10X6_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B10x6,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_10X8_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B10x8,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_10X10_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B10x10,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_12X10_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B12x10,
            channel: AstcChannel::Hdr,
        },
        F::ASTC_12X12_SFLOAT_BLOCK_EXT => Tf::Astc {
            block: AstcBlock::B12x12,
            channel: AstcChannel::Hdr,
        },
        _ => return None,
    })
}

fn wgpu_to_vulkan(format: wgpu::TextureFormat) -> Option<ash::vk::Format> {
    // Copied with minor modification from:
    // https://github.com/gfx-rs/wgpu/blob/a7defb723f856d946d6d220e9897d20dbb7b8f61/wgpu-hal/src/vulkan/conv.rs#L5-L151
    // license: MIT OR Apache-2.0
    use ash::vk::Format as F;
    use wgpu::TextureFormat as Tf;
    use wgpu::{AstcBlock, AstcChannel};
    Some(match format {
        Tf::R8Unorm => F::R8_UNORM,
        Tf::R8Snorm => F::R8_SNORM,
        Tf::R8Uint => F::R8_UINT,
        Tf::R8Sint => F::R8_SINT,
        Tf::R16Uint => F::R16_UINT,
        Tf::R16Sint => F::R16_SINT,
        Tf::R16Unorm => F::R16_UNORM,
        Tf::R16Snorm => F::R16_SNORM,
        Tf::R16Float => F::R16_SFLOAT,
        Tf::Rg8Unorm => F::R8G8_UNORM,
        Tf::Rg8Snorm => F::R8G8_SNORM,
        Tf::Rg8Uint => F::R8G8_UINT,
        Tf::Rg8Sint => F::R8G8_SINT,
        Tf::Rg16Unorm => F::R16G16_UNORM,
        Tf::Rg16Snorm => F::R16G16_SNORM,
        Tf::R32Uint => F::R32_UINT,
        Tf::R32Sint => F::R32_SINT,
        Tf::R32Float => F::R32_SFLOAT,
        Tf::Rg16Uint => F::R16G16_UINT,
        Tf::Rg16Sint => F::R16G16_SINT,
        Tf::Rg16Float => F::R16G16_SFLOAT,
        Tf::Rgba8Unorm => F::R8G8B8A8_UNORM,
        Tf::Rgba8UnormSrgb => F::R8G8B8A8_SRGB,
        Tf::Bgra8UnormSrgb => F::B8G8R8A8_SRGB,
        Tf::Rgba8Snorm => F::R8G8B8A8_SNORM,
        Tf::Bgra8Unorm => F::B8G8R8A8_UNORM,
        Tf::Rgba8Uint => F::R8G8B8A8_UINT,
        Tf::Rgba8Sint => F::R8G8B8A8_SINT,
        Tf::Rgb10a2Uint => F::A2B10G10R10_UINT_PACK32,
        Tf::Rgb10a2Unorm => F::A2B10G10R10_UNORM_PACK32,
        Tf::Rg11b10Ufloat => F::B10G11R11_UFLOAT_PACK32,
        Tf::Rg32Uint => F::R32G32_UINT,
        Tf::Rg32Sint => F::R32G32_SINT,
        Tf::Rg32Float => F::R32G32_SFLOAT,
        Tf::Rgba16Uint => F::R16G16B16A16_UINT,
        Tf::Rgba16Sint => F::R16G16B16A16_SINT,
        Tf::Rgba16Unorm => F::R16G16B16A16_UNORM,
        Tf::Rgba16Snorm => F::R16G16B16A16_SNORM,
        Tf::Rgba16Float => F::R16G16B16A16_SFLOAT,
        Tf::Rgba32Uint => F::R32G32B32A32_UINT,
        Tf::Rgba32Sint => F::R32G32B32A32_SINT,
        Tf::Rgba32Float => F::R32G32B32A32_SFLOAT,
        Tf::Depth32Float => F::D32_SFLOAT,
        Tf::Depth32FloatStencil8 => F::D32_SFLOAT_S8_UINT,
        Tf::Depth24Plus | Tf::Depth24PlusStencil8 | Tf::Stencil8 => return None, // Dependent on device properties
        Tf::Depth16Unorm => F::D16_UNORM,
        Tf::NV12 => F::G8_B8R8_2PLANE_420_UNORM,
        Tf::Rgb9e5Ufloat => F::E5B9G9R9_UFLOAT_PACK32,
        Tf::Bc1RgbaUnorm => F::BC1_RGBA_UNORM_BLOCK,
        Tf::Bc1RgbaUnormSrgb => F::BC1_RGBA_SRGB_BLOCK,
        Tf::Bc2RgbaUnorm => F::BC2_UNORM_BLOCK,
        Tf::Bc2RgbaUnormSrgb => F::BC2_SRGB_BLOCK,
        Tf::Bc3RgbaUnorm => F::BC3_UNORM_BLOCK,
        Tf::Bc3RgbaUnormSrgb => F::BC3_SRGB_BLOCK,
        Tf::Bc4RUnorm => F::BC4_UNORM_BLOCK,
        Tf::Bc4RSnorm => F::BC4_SNORM_BLOCK,
        Tf::Bc5RgUnorm => F::BC5_UNORM_BLOCK,
        Tf::Bc5RgSnorm => F::BC5_SNORM_BLOCK,
        Tf::Bc6hRgbUfloat => F::BC6H_UFLOAT_BLOCK,
        Tf::Bc6hRgbFloat => F::BC6H_SFLOAT_BLOCK,
        Tf::Bc7RgbaUnorm => F::BC7_UNORM_BLOCK,
        Tf::Bc7RgbaUnormSrgb => F::BC7_SRGB_BLOCK,
        Tf::Etc2Rgb8Unorm => F::ETC2_R8G8B8_UNORM_BLOCK,
        Tf::Etc2Rgb8UnormSrgb => F::ETC2_R8G8B8_SRGB_BLOCK,
        Tf::Etc2Rgb8A1Unorm => F::ETC2_R8G8B8A1_UNORM_BLOCK,
        Tf::Etc2Rgb8A1UnormSrgb => F::ETC2_R8G8B8A1_SRGB_BLOCK,
        Tf::Etc2Rgba8Unorm => F::ETC2_R8G8B8A8_UNORM_BLOCK,
        Tf::Etc2Rgba8UnormSrgb => F::ETC2_R8G8B8A8_SRGB_BLOCK,
        Tf::EacR11Unorm => F::EAC_R11_UNORM_BLOCK,
        Tf::EacR11Snorm => F::EAC_R11_SNORM_BLOCK,
        Tf::EacRg11Unorm => F::EAC_R11G11_UNORM_BLOCK,
        Tf::EacRg11Snorm => F::EAC_R11G11_SNORM_BLOCK,
        Tf::Astc { block, channel } => match channel {
            AstcChannel::Unorm => match block {
                AstcBlock::B4x4 => F::ASTC_4X4_UNORM_BLOCK,
                AstcBlock::B5x4 => F::ASTC_5X4_UNORM_BLOCK,
                AstcBlock::B5x5 => F::ASTC_5X5_UNORM_BLOCK,
                AstcBlock::B6x5 => F::ASTC_6X5_UNORM_BLOCK,
                AstcBlock::B6x6 => F::ASTC_6X6_UNORM_BLOCK,
                AstcBlock::B8x5 => F::ASTC_8X5_UNORM_BLOCK,
                AstcBlock::B8x6 => F::ASTC_8X6_UNORM_BLOCK,
                AstcBlock::B8x8 => F::ASTC_8X8_UNORM_BLOCK,
                AstcBlock::B10x5 => F::ASTC_10X5_UNORM_BLOCK,
                AstcBlock::B10x6 => F::ASTC_10X6_UNORM_BLOCK,
                AstcBlock::B10x8 => F::ASTC_10X8_UNORM_BLOCK,
                AstcBlock::B10x10 => F::ASTC_10X10_UNORM_BLOCK,
                AstcBlock::B12x10 => F::ASTC_12X10_UNORM_BLOCK,
                AstcBlock::B12x12 => F::ASTC_12X12_UNORM_BLOCK,
            },
            AstcChannel::UnormSrgb => match block {
                AstcBlock::B4x4 => F::ASTC_4X4_SRGB_BLOCK,
                AstcBlock::B5x4 => F::ASTC_5X4_SRGB_BLOCK,
                AstcBlock::B5x5 => F::ASTC_5X5_SRGB_BLOCK,
                AstcBlock::B6x5 => F::ASTC_6X5_SRGB_BLOCK,
                AstcBlock::B6x6 => F::ASTC_6X6_SRGB_BLOCK,
                AstcBlock::B8x5 => F::ASTC_8X5_SRGB_BLOCK,
                AstcBlock::B8x6 => F::ASTC_8X6_SRGB_BLOCK,
                AstcBlock::B8x8 => F::ASTC_8X8_SRGB_BLOCK,
                AstcBlock::B10x5 => F::ASTC_10X5_SRGB_BLOCK,
                AstcBlock::B10x6 => F::ASTC_10X6_SRGB_BLOCK,
                AstcBlock::B10x8 => F::ASTC_10X8_SRGB_BLOCK,
                AstcBlock::B10x10 => F::ASTC_10X10_SRGB_BLOCK,
                AstcBlock::B12x10 => F::ASTC_12X10_SRGB_BLOCK,
                AstcBlock::B12x12 => F::ASTC_12X12_SRGB_BLOCK,
            },
            AstcChannel::Hdr => match block {
                AstcBlock::B4x4 => F::ASTC_4X4_SFLOAT_BLOCK_EXT,
                AstcBlock::B5x4 => F::ASTC_5X4_SFLOAT_BLOCK_EXT,
                AstcBlock::B5x5 => F::ASTC_5X5_SFLOAT_BLOCK_EXT,
                AstcBlock::B6x5 => F::ASTC_6X5_SFLOAT_BLOCK_EXT,
                AstcBlock::B6x6 => F::ASTC_6X6_SFLOAT_BLOCK_EXT,
                AstcBlock::B8x5 => F::ASTC_8X5_SFLOAT_BLOCK_EXT,
                AstcBlock::B8x6 => F::ASTC_8X6_SFLOAT_BLOCK_EXT,
                AstcBlock::B8x8 => F::ASTC_8X8_SFLOAT_BLOCK_EXT,
                AstcBlock::B10x5 => F::ASTC_10X5_SFLOAT_BLOCK_EXT,
                AstcBlock::B10x6 => F::ASTC_10X6_SFLOAT_BLOCK_EXT,
                AstcBlock::B10x8 => F::ASTC_10X8_SFLOAT_BLOCK_EXT,
                AstcBlock::B10x10 => F::ASTC_10X10_SFLOAT_BLOCK_EXT,
                AstcBlock::B12x10 => F::ASTC_12X10_SFLOAT_BLOCK_EXT,
                AstcBlock::B12x12 => F::ASTC_12X12_SFLOAT_BLOCK_EXT,
            },
        },
        TextureFormat::R64Uint => {
            panic!()
        }
    })
}
