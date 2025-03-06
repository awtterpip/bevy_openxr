use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;

use crate::error::OxrError;
use crate::graphics::*;
use crate::layer_builder::{CompositionLayer, LayerProvider};
use crate::session::{OxrSession, OxrSessionCreateNextChain};
use crate::types::*;

/// Wrapper around an [`Entry`](openxr::Entry) with some methods overridden to use bevy types.
///
/// See [`openxr::Entry`] for other available methods.
#[derive(Deref, Clone)]
pub struct OxrEntry(pub openxr::Entry);

impl OxrEntry {
    /// Enumerate available extensions for this OpenXR runtime.
    pub fn enumerate_extensions(&self) -> crate::types::Result<OxrExtensions> {
        Ok(self.0.enumerate_extensions().map(Into::into)?)
    }

    /// Creates an [`OxrInstance`].
    ///
    /// Calls [`create_instance`](openxr::Entry::create_instance) internally.
    pub fn create_instance(
        &self,
        app_info: AppInfo,
        exts: OxrExtensions,
        layers: &[&str],
        backend: GraphicsBackend,
    ) -> crate::types::Result<OxrInstance> {
        let available_exts = self.enumerate_extensions()?;

        if !backend.is_available(&available_exts) {
            return Err(OxrError::UnavailableBackend(backend));
        }

        let required_exts = exts | backend.required_exts();

        let instance = self.0.create_instance(
            &openxr::ApplicationInfo {
                application_name: &app_info.name,
                application_version: app_info.version.to_u32(),
                engine_name: "Bevy",
                engine_version: Version::BEVY.to_u32(),
                api_version: openxr::Version::new(1, 0, 34),
            },
            &required_exts.into(),
            layers,
        )?;

        Ok(OxrInstance(instance, backend, app_info))
    }

    /// Returns a list of all of the backends the OpenXR runtime supports.
    pub fn available_backends(&self) -> crate::types::Result<Vec<GraphicsBackend>> {
        Ok(GraphicsBackend::available_backends(
            &self.enumerate_extensions()?,
        ))
    }
}

/// Wrapper around [`openxr::Instance`] with additional data for safety and some methods overriden to use bevy types.
///
/// See [`openxr::Instance`] for other available methods.
#[derive(Resource, Deref, Clone)]
pub struct OxrInstance(
    #[deref] pub(crate) openxr::Instance,
    /// [`GraphicsBackend`] is stored here to let us know what graphics API the current instance wants to target.
    pub(crate) GraphicsBackend,
    pub(crate) AppInfo,
);

impl OxrInstance {
    /// Creates an [`OxrInstance`] from an [`openxr::Instance`] if needed.
    /// In the majority of cases, you should use [`create_instance`](OxrEntry::create_instance) instead.
    ///
    /// # Safety
    ///
    /// The OpenXR instance passed in *must* have support for the backend specified.
    pub unsafe fn from_inner(
        instance: openxr::Instance,
        backend: GraphicsBackend,
        info: AppInfo,
    ) -> Self {
        Self(instance, backend, info)
    }

    /// Consumes self and returns the inner [`openxr::Instance`]
    pub fn into_inner(self) -> openxr::Instance {
        self.0
    }

    /// Returns the current backend being used by this instance.
    pub fn backend(&self) -> GraphicsBackend {
        self.1
    }

    /// Returns the [`AppInfo`] being used by this instance.
    pub fn app_info(&self) -> &AppInfo {
        &self.2
    }

    /// Initialize graphics. This is used to create [WgpuGraphics] for the bevy app and to get the [SessionCreateInfo] needed to make an XR session.
    pub fn init_graphics(
        &self,
        system_id: openxr::SystemId,
    ) -> crate::types::Result<(WgpuGraphics, SessionCreateInfo)> {
        graphics_match!(
            self.1;
            _ => {
                let (graphics, session_info) = Api::init_graphics(&self.2, self, system_id)?;

                Ok((graphics, SessionCreateInfo(Api::wrap(session_info))))
            }
        )
    }

    /// Creates an [OxrSession]
    ///
    /// Calls [`create_session`](openxr::Instance::create_session) internally.
    ///
    /// # Safety
    ///
    /// `info` must contain valid handles for the graphics api
    pub unsafe fn create_session(
        &self,
        system_id: openxr::SystemId,
        info: SessionCreateInfo,
        chain: &mut OxrSessionCreateNextChain,
    ) -> crate::types::Result<(OxrSession, OxrFrameWaiter, OxrFrameStream)> {
        if !info.0.using_graphics_of_val(&self.1) {
            return crate::types::Result::Err(OxrError::GraphicsBackendMismatch {
                item: std::any::type_name::<SessionCreateInfo>(),
                backend: info.0.graphics_name(),
                expected_backend: self.1.graphics_name(),
            });
        }
        graphics_match!(
            info.0;
            info => {
                let (session, frame_waiter, frame_stream) = Api::create_session(self,system_id, &info,chain)?;
                Ok((session.into(), OxrFrameWaiter(frame_waiter), OxrFrameStream(Api::wrap(frame_stream))))
            }
        )
    }
}

/// Graphics agnostic wrapper around [openxr::FrameStream]
#[derive(Resource)]
pub struct OxrFrameStream(pub GraphicsWrap<Self>);

impl GraphicsType for OxrFrameStream {
    type Inner<G: GraphicsExt> = openxr::FrameStream<G>;
}

impl OxrFrameStream {
    /// Creates a new [`OxrFrameStream`] from an [`openxr::FrameStream`].
    /// In the majority of cases, you should use [`create_session`](OxrInstance::create_session) instead.
    pub fn from_inner<G: GraphicsExt>(frame_stream: openxr::FrameStream<G>) -> Self {
        Self(G::wrap(frame_stream))
    }

    /// Indicate that graphics device work is beginning.
    ///
    /// Calls [`begin`](openxr::FrameStream::begin) internally.
    pub fn begin(&mut self) -> openxr::Result<()> {
        graphics_match!(
            &mut self.0;
            stream => stream.begin()
        )
    }

    /// Indicate that all graphics work for the frame has been submitted
    ///
    /// `layers` is an array of references to any type of composition layer that implements [`CompositionLayer`],
    /// e.g. [`CompositionLayerProjection`](crate::layer_builder::CompositionLayerProjection)
    pub fn end(
        &mut self,
        display_time: openxr::Time,
        environment_blend_mode: openxr::EnvironmentBlendMode,
        layers: &[&dyn CompositionLayer],
    ) -> crate::types::Result<()> {
        graphics_match!(
            &mut self.0;
            stream => {
                let mut new_layers = vec![];

                for (i, layer) in layers.iter().enumerate() {
                    if let Some(swapchain) = layer.swapchain() {
                        if !swapchain.0.using_graphics::<Api>() {
                            error!(
                                "Composition layer {i} is using graphics api '{}', expected graphics api '{}'. Excluding layer from frame submission.",
                                swapchain.0.graphics_name(),
                                std::any::type_name::<Api>(),
                            );
                            continue;
                        }
                    }
                    new_layers.push(unsafe {
                        #[allow(clippy::missing_transmute_annotations)]
                        std::mem::transmute(layer.header())
                    });
                }

                Ok(stream.end(display_time, environment_blend_mode, new_layers.as_slice())?)
            }
        )
    }
}

/// Handle for waiting to render a frame.
///
/// See [`FrameWaiter`](openxr::FrameWaiter) for available methods.
#[derive(Resource, Deref, DerefMut)]
pub struct OxrFrameWaiter(pub openxr::FrameWaiter);

/// Graphics agnostic wrapper around [openxr::Swapchain]
#[derive(Resource)]
pub struct OxrSwapchain(pub GraphicsWrap<Self>);

impl GraphicsType for OxrSwapchain {
    type Inner<G: GraphicsExt> = openxr::Swapchain<G>;
}

impl OxrSwapchain {
    /// Creates a new [`OxrSwapchain`] from an [`openxr::Swapchain`].
    /// In the majority of cases, you should use [`create_swapchain`](OxrSession::create_swapchain) instead.
    pub fn from_inner<G: GraphicsExt>(swapchain: openxr::Swapchain<G>) -> Self {
        Self(G::wrap(swapchain))
    }

    /// Determine the index of the next image to render to in the swapchain image array.
    ///
    /// Calls [`acquire_image`](openxr::Swapchain::acquire_image) internally.
    pub fn acquire_image(&mut self) -> crate::types::Result<u32> {
        graphics_match!(
            &mut self.0;
            swap => Ok(swap.acquire_image()?)
        )
    }

    /// Wait for the compositor to finish reading from the oldest unwaited acquired image.
    ///
    /// Calls [`wait_image`](openxr::Swapchain::wait_image) internally.
    pub fn wait_image(&mut self, timeout: openxr::Duration) -> crate::types::Result<()> {
        graphics_match!(
            &mut self.0;
            swap => Ok(swap.wait_image(timeout)?)
        )
    }

    /// Release the oldest acquired image.
    ///
    /// Calls [`release_image`](openxr::Swapchain::release_image) internally.
    pub fn release_image(&mut self) -> crate::types::Result<()> {
        graphics_match!(
            &mut self.0;
            swap => Ok(swap.release_image()?)
        )
    }

    /// Enumerates swapchain images and converts them to wgpu [`Texture`](wgpu::Texture)s.
    ///
    /// Calls [`enumerate_images`](openxr::Swapchain::enumerate_images) internally.
    pub fn enumerate_images(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        resolution: UVec2,
    ) -> crate::types::Result<OxrSwapchainImages> {
        graphics_match!(
            &self.0;
            swap => {
                let mut images = vec![];
                for image in swap.enumerate_images()? {
                    unsafe {
                        images.push(Api::to_wgpu_img(image, device, format, resolution)?);
                    }
                }
                Ok(OxrSwapchainImages(images.leak()))
            }
        )
    }
}

/// Stores the generated swapchain images.
#[derive(Debug, Deref, Resource, Clone, Copy, ExtractResource)]
pub struct OxrSwapchainImages(pub &'static [wgpu::Texture]);

/// Thread safe wrapper around [openxr::Space] representing the stage.
// #[derive(Deref, Clone, Resource)]
// pub struct OxrStage(pub Arc<openxr::Space>);

/// Stores the latest generated [OxrViews]
#[derive(Clone, Resource, ExtractResource, Deref, DerefMut, Default)]
pub struct OxrViews(pub Vec<openxr::View>);

/// Wrapper around [openxr::SystemId] to allow it to be stored as a resource.
#[derive(Debug, Copy, Clone, Deref, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Resource)]
pub struct OxrSystemId(pub openxr::SystemId);

/// Wrapper around [`openxr::Passthrough`].
///
/// Used to [`start`](openxr::Passthrough::start) or [`pause`](openxr::Passthrough::pause) passthrough on the physical device.
///
/// See [`openxr::Passthrough`] for available methods.
#[derive(Resource, Deref, DerefMut)]
pub struct OxrPassthrough(
    #[deref] pub openxr::Passthrough,
    /// The flags are stored here so that they don't need to be passed in again when creating an [`OxrPassthroughLayer`].
    pub openxr::PassthroughFlagsFB,
);

impl OxrPassthrough {
    /// This function can create an [`OxrPassthrough`] from raw openxr types if needed.
    /// In the majority of cases, you should use [`create_passthrough`](OxrSession::create_passthrough) instead.
    pub fn from_inner(passthrough: openxr::Passthrough, flags: openxr::PassthroughFlagsFB) -> Self {
        Self(passthrough, flags)
    }
}

/// Wrapper around [`openxr::Passthrough`].
///
/// Used to create a [`CompositionLayerPassthrough`](crate::layer_builder::CompositionLayerPassthrough), and to [`pause`](openxr::PassthroughLayer::pause) or [`resume`](openxr::PassthroughLayer::resume) rendering of the passthrough layer.
///
/// See [`openxr::PassthroughLayer`] for available methods.
#[derive(Resource, Deref, DerefMut)]
pub struct OxrPassthroughLayer(pub openxr::PassthroughLayer);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct OxrRenderLayers(pub Vec<Box<dyn LayerProvider + Send + Sync>>);

/// Resource storing graphics info for the currently running session.
#[derive(Clone, Copy, Resource, ExtractResource)]
pub struct OxrGraphicsInfo {
    pub blend_mode: EnvironmentBlendMode,
    pub resolution: UVec2,
    pub format: wgpu::TextureFormat,
}

#[derive(Clone)]
/// This is used to store information from startup that is needed to create the session after the instance has been created.
pub struct SessionConfigInfo {
    /// List of blend modes the openxr session can use. If [None], pick the first available blend mode.
    pub blend_modes: Option<Vec<EnvironmentBlendMode>>,
    /// List of formats the openxr session can use. If [None], pick the first available format
    pub formats: Option<Vec<wgpu::TextureFormat>>,
    /// List of resolutions that the openxr swapchain can use. If [None] pick the first available resolution.
    pub resolutions: Option<Vec<UVec2>>,
    /// Graphics info used to create a session.
    pub graphics_info: SessionCreateInfo,
}

#[derive(ExtractResource, Resource, Clone, Default)]
pub struct OxrSessionStarted(pub bool);

/// The frame state returned from [FrameWaiter::wait_frame](openxr::FrameWaiter::wait)
#[derive(Clone, Deref, DerefMut, Resource, ExtractResource)]
pub struct OxrFrameState(pub openxr::FrameState);

/// Instructs systems to add display period
#[derive(Clone, Copy, Default, Resource)]
pub struct Pipelined;
