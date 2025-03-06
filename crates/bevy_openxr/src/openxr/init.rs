use std::sync::atomic::Ordering;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::render::renderer::RenderAdapter;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::renderer::RenderDevice;
use bevy::render::renderer::RenderInstance;
use bevy::render::renderer::RenderQueue;
use bevy::render::renderer::WgpuWrapper;
use bevy::render::settings::RenderCreation;
use bevy::render::MainWorld;
use bevy::render::Render;
use bevy::render::RenderApp;
use bevy::render::RenderPlugin;
use bevy::winit::UpdateMode;
use bevy::winit::WinitSettings;
use bevy_mod_xr::session::*;
use openxr::Event;

use crate::error::OxrError;
use crate::graphics::*;
use crate::resources::*;
use crate::session::OxrSession;
use crate::session::OxrSessionCreateNextChain;
use crate::types::*;

use super::exts::OxrEnabledExtensions;
use super::poll_events::OxrEvent;
use super::poll_events::OxrEventHandlerExt;

pub fn session_started(started: Option<Res<OxrSessionStarted>>) -> bool {
    started.is_some_and(|started| started.0)
}

pub fn should_run_frame_loop(
    started: Option<Res<OxrSessionStarted>>,
    state: Option<Res<XrState>>,
) -> bool {
    started.is_some_and(|started| started.0)
        && state.is_some_and(|state| *state != XrState::Stopping)
}

pub fn should_render(frame_state: Option<Res<OxrFrameState>>) -> bool {
    frame_state.is_some_and(|frame_state| frame_state.should_render)
}

pub struct OxrInitPlugin {
    /// Information about the app this is being used to build.
    pub app_info: AppInfo,
    /// Extensions wanted for this session.
    // TODO!() This should be changed to take a simpler list of features wanted that this crate supports. i.e. hand tracking
    pub exts: OxrExtensions,
    /// List of blend modes the openxr session can use. If [None], pick the first available blend mode.
    pub blend_modes: Option<Vec<EnvironmentBlendMode>>,
    /// List of backends the openxr session can use. If [None], pick the first available backend.
    pub backends: Option<Vec<GraphicsBackend>>,
    /// List of formats the openxr session can use. If [None], pick the first available format
    pub formats: Option<Vec<wgpu::TextureFormat>>,
    /// List of resolutions that the openxr swapchain can use. If [None] pick the first available resolution.
    pub resolutions: Option<Vec<UVec2>>,
    /// Passed into the render plugin when added to the app.
    pub synchronous_pipeline_compilation: bool,
}
impl Default for OxrInitPlugin {
    fn default() -> Self {
        Self {
            app_info: default(),
            exts: {
                let mut exts = OxrExtensions::default();
                exts.enable_fb_passthrough();
                exts.enable_hand_tracking();
                exts
            },
            blend_modes: default(),
            backends: default(),
            formats: Some(vec![wgpu::TextureFormat::Rgba8UnormSrgb]),
            resolutions: default(),
            synchronous_pipeline_compilation: false,
        }
    }
}

impl Plugin for OxrInitPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<OxrInteractionProfileChanged>();
        match self.init_xr() {
            Ok((
                instance,
                system_id,
                WgpuGraphics(device, queue, adapter_info, adapter, wgpu_instance),
                session_create_info,
                enabled_exts,
            )) => {
                app.insert_resource(enabled_exts)
                    .add_plugins((
                        RenderPlugin {
                            render_creation: RenderCreation::manual(
                                device.into(),
                                RenderQueue(Arc::new(WgpuWrapper::new(queue))),
                                RenderAdapterInfo(WgpuWrapper::new(adapter_info)),
                                RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
                                RenderInstance(Arc::new(WgpuWrapper::new(wgpu_instance))),
                            ),
                            synchronous_pipeline_compilation: self.synchronous_pipeline_compilation,
                            debug_flags: Default::default(),
                        },
                        ExtractResourcePlugin::<OxrSessionStarted>::default(),
                    ))
                    .add_oxr_event_handler(handle_events)
                    .add_systems(
                        XrFirst,
                        (
                            create_xr_session
                                .run_if(state_equals(XrState::Available))
                                .run_if(on_event::<XrCreateSessionEvent>),
                            (
                                destroy_xr_session,
                                (|v: Res<XrDestroySessionRender>| {
                                    v.0.store(true, Ordering::Relaxed);
                                    debug!("setting destroy render session");
                                }),
                            )
                                .chain()
                                .run_if(state_matches!(XrState::Exiting { .. }))
                                .run_if(on_event::<XrDestroySessionEvent>),
                            begin_xr_session
                                .run_if(state_equals(XrState::Ready))
                                .run_if(on_event::<XrBeginSessionEvent>),
                            end_xr_session
                                .run_if(state_equals(XrState::Stopping))
                                .run_if(on_event::<XrEndSessionEvent>),
                            request_exit_xr_session
                                .run_if(session_created)
                                .run_if(on_event::<XrRequestExitEvent>),
                            detect_session_destroyed,
                        )
                            .in_set(XrHandleEvents::SessionStateUpdateEvents),
                    )
                    .insert_resource(instance.clone())
                    .insert_resource(system_id)
                    .insert_resource(XrState::Available)
                    .insert_resource(WinitSettings {
                        focused_mode: UpdateMode::Continuous,
                        unfocused_mode: UpdateMode::Continuous,
                    })
                    .insert_resource(OxrSessionStarted(false))
                    .insert_non_send_resource(session_create_info)
                    .init_non_send_resource::<OxrSessionCreateNextChain>();

                app.world_mut()
                    .resource_mut::<Events<XrStateChanged>>()
                    .send(XrStateChanged(XrState::Available));

                let render_app = app.sub_app_mut(RenderApp);

                render_app
                    .add_systems(ExtractSchedule, transfer_xr_resources)
                    .insert_resource(instance)
                    .insert_resource(system_id)
                    .insert_resource(XrState::Available)
                    .insert_resource(OxrSessionStarted(false));
            }
            Err(e) => {
                error!("Failed to initialize openxr: {e}");
                app.add_plugins(RenderPlugin::default())
                    .insert_resource(XrState::Unavailable);
            }
        };
    }

    fn finish(&self, app: &mut App) {
        app.sub_app_mut(RenderApp).add_systems(
            Render,
            (
                destroy_xr_session,
                |v: Res<XrDestroySessionRender>, mut cmds: Commands| {
                    v.0.store(false, Ordering::Relaxed);
                    cmds.insert_resource(XrState::Available);
                },
            )
                .chain()
                .run_if(
                    resource_exists::<XrDestroySessionRender>
                        .and(|v: Res<XrDestroySessionRender>| v.0.load(Ordering::Relaxed)),
                ),
        );
    }
}

fn detect_session_destroyed(
    mut last_state: Local<bool>,
    state: Res<XrDestroySessionRender>,
    mut sender: EventWriter<XrSessionDestroyedEvent>,
    mut cmds: Commands,
) {
    let state = state.0.load(Ordering::Relaxed);
    if *last_state && !state {
        debug!("XrSession was fully destroyed!");
        sender.send_default();
        cmds.insert_resource(XrState::Available);
    }
    *last_state = state;
}

impl OxrInitPlugin {
    fn init_xr(
        &self,
    ) -> crate::types::Result<(
        OxrInstance,
        OxrSystemId,
        WgpuGraphics,
        SessionConfigInfo,
        OxrEnabledExtensions,
    )> {
        #[cfg(windows)]
        let entry = OxrEntry(openxr::Entry::linked());
        #[cfg(not(windows))]
        let entry = OxrEntry(unsafe { openxr::Entry::load()? });

        #[cfg(target_os = "android")]
        entry.initialize_android_loader()?;

        let available_exts = entry.enumerate_extensions()?;

        // check available extensions and send a warning for any wanted extensions that aren't available.
        for ext in available_exts.unavailable_exts(&self.exts) {
            error!(
                "Extension \"{ext}\" not available in the current OpenXR runtime. Disabling extension."
            );
        }

        let available_backends = GraphicsBackend::available_backends(&available_exts);

        // Backend selection
        let backend = if let Some(wanted_backends) = &self.backends {
            let mut backend = None;
            for wanted_backend in wanted_backends {
                if available_backends.contains(wanted_backend) {
                    backend = Some(*wanted_backend);
                    break;
                }
            }
            backend
        } else {
            available_backends.first().copied()
        }
        .ok_or(OxrError::NoAvailableBackend)?;

        let exts = self.exts.clone() & available_exts;

        let instance = entry.create_instance(
            self.app_info.clone(),
            exts.clone(),
            // &["XR_APILAYER_LUNARG_api_dump"],
            &[],
            backend,
        )?;
        let instance_props = instance.properties()?;

        info!(
            "Loaded OpenXR runtime: {} {}",
            instance_props.runtime_name, instance_props.runtime_version
        );

        let system_id = instance.system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)?;
        let system_props = instance.system_properties(system_id)?;

        info!(
            "Using system: {}",
            if system_props.system_name.is_empty() {
                "<unnamed>"
            } else {
                &system_props.system_name
            }
        );

        let (graphics, graphics_info) = instance.init_graphics(system_id)?;

        let session_create_info = SessionConfigInfo {
            blend_modes: self.blend_modes.clone(),
            formats: self.formats.clone(),
            resolutions: self.resolutions.clone(),
            graphics_info,
        };

        Ok((
            instance,
            OxrSystemId(system_id),
            graphics,
            session_create_info,
            OxrEnabledExtensions(exts),
        ))
    }
}
#[derive(Event, Clone, Copy, Debug, Default)]
pub struct OxrInteractionProfileChanged;

pub fn handle_events(
    event: In<OxrEvent>,
    mut status: ResMut<XrState>,
    mut changed_event: EventWriter<XrStateChanged>,
    mut interaction_profile_changed_event: EventWriter<OxrInteractionProfileChanged>,
) {
    use openxr::Event::*;
    // this unwrap will never panic since we are in a valid scope
    match unsafe { event.get() }.unwrap() {
        SessionStateChanged(state) => {
            use openxr::SessionState;

            let state = state.state();

            info!("entered XR state {:?}", state);

            let new_status = match state {
                SessionState::IDLE => XrState::Idle,
                SessionState::READY => XrState::Ready,
                SessionState::SYNCHRONIZED | SessionState::VISIBLE | SessionState::FOCUSED => {
                    XrState::Running
                }
                SessionState::STOPPING => XrState::Stopping,
                SessionState::EXITING => XrState::Exiting {
                    should_restart: false,
                },
                SessionState::LOSS_PENDING => XrState::Exiting {
                    should_restart: true,
                },
                _ => unreachable!(),
            };
            changed_event.send(XrStateChanged(new_status));
            *status = new_status;
        }
        InstanceLossPending(_) => {}
        EventsLost(e) => warn!("lost {} XR events", e.lost_event_count()),
        // we might want to check if this is the correct session?
        Event::InteractionProfileChanged(_) => {
            interaction_profile_changed_event.send_default();
        }
        _ => {}
    }
}

fn init_xr_session(
    device: &wgpu::Device,
    instance: &OxrInstance,
    system_id: openxr::SystemId,
    chain: &mut OxrSessionCreateNextChain,
    SessionConfigInfo {
        blend_modes,
        formats,
        resolutions,
        graphics_info,
    }: SessionConfigInfo,
) -> crate::types::Result<(
    OxrSession,
    OxrFrameWaiter,
    OxrFrameStream,
    OxrSwapchain,
    OxrSwapchainImages,
    OxrGraphicsInfo,
)> {
    let (session, frame_waiter, frame_stream) =
        unsafe { instance.create_session(system_id, graphics_info, chain)? };

    // TODO!() support other view configurations
    let available_view_configurations = instance.enumerate_view_configurations(system_id)?;
    if !available_view_configurations.contains(&openxr::ViewConfigurationType::PRIMARY_STEREO) {
        return Err(OxrError::NoAvailableViewConfiguration);
    }

    let view_configuration_type = openxr::ViewConfigurationType::PRIMARY_STEREO;

    let view_configuration_views =
        instance.enumerate_view_configuration_views(system_id, view_configuration_type)?;

    let (resolution, _view) = if let Some(resolutions) = &resolutions {
        let mut preferred = None;
        for resolution in resolutions {
            for view_config in view_configuration_views.iter() {
                if view_config.recommended_image_rect_height == resolution.y
                    && view_config.recommended_image_rect_width == resolution.x
                {
                    preferred = Some((*resolution, *view_config));
                }
            }
        }

        if preferred.is_none() {
            for resolution in resolutions {
                for view_config in view_configuration_views.iter() {
                    if view_config.max_image_rect_height >= resolution.y
                        && view_config.max_image_rect_width >= resolution.x
                    {
                        preferred = Some((*resolution, *view_config));
                    }
                }
            }
        }

        preferred
    } else {
        view_configuration_views.first().map(|config| {
            (
                UVec2::new(
                    config.recommended_image_rect_width,
                    config.recommended_image_rect_height,
                ),
                *config,
            )
        })
    }
    .ok_or(OxrError::NoAvailableViewConfiguration)?;

    let available_formats = session.enumerate_swapchain_formats()?;

    let format = if let Some(formats) = &formats {
        let mut format = None;
        for wanted_format in formats {
            if available_formats.contains(wanted_format) {
                format = Some(*wanted_format);
            }
        }
        format
    } else {
        available_formats.first().copied()
    }
    .ok_or(OxrError::NoAvailableFormat)?;

    let swapchain = session.create_swapchain(SwapchainCreateInfo {
        create_flags: SwapchainCreateFlags::EMPTY,
        usage_flags: SwapchainUsageFlags::COLOR_ATTACHMENT | SwapchainUsageFlags::SAMPLED,
        format,
        // TODO() add support for multisampling
        sample_count: 1,
        width: resolution.x,
        height: resolution.y,
        face_count: 1,
        array_size: 2,
        mip_count: 1,
    })?;

    let images = swapchain.enumerate_images(device, format, resolution)?;

    let available_blend_modes =
        instance.enumerate_environment_blend_modes(system_id, view_configuration_type)?;

    // blend mode selection
    let blend_mode = if let Some(wanted_blend_modes) = &blend_modes {
        let mut blend_mode = None;
        for wanted_blend_mode in wanted_blend_modes {
            if available_blend_modes.contains(wanted_blend_mode) {
                blend_mode = Some(*wanted_blend_mode);
                break;
            }
        }
        blend_mode
    } else {
        available_blend_modes.first().copied()
    }
    .ok_or(OxrError::NoAvailableBackend)?;

    let graphics_info = OxrGraphicsInfo {
        blend_mode,
        resolution,
        format,
    };

    Ok((
        session,
        frame_waiter,
        frame_stream,
        swapchain,
        images,
        graphics_info,
    ))
}

pub fn create_xr_session(world: &mut World) {
    let mut chain = world
        .remove_non_send_resource::<OxrSessionCreateNextChain>()
        .unwrap();
    let device = world.resource::<RenderDevice>();
    let instance = world.resource::<OxrInstance>();
    let create_info = world.non_send_resource::<SessionConfigInfo>();
    let system_id = world.resource::<OxrSystemId>();
    match init_xr_session(
        device.wgpu_device(),
        instance,
        **system_id,
        &mut chain,
        create_info.clone(),
    ) {
        Ok((session, frame_waiter, frame_stream, swapchain, images, graphics_info)) => {
            world.insert_resource(session.clone());
            world.insert_resource(frame_waiter);
            world.insert_resource(images);
            world.insert_resource(graphics_info);
            world.insert_resource(OxrRenderResources {
                session,
                frame_stream,
                swapchain,
                images,
                graphics_info,
                session_destroy_flag: world
                    .get_resource::<XrDestroySessionRender>()
                    .expect("added by xr session plugin")
                    .clone(),
            });
        }
        Err(e) => error!("Failed to initialize XrSession: {e}"),
    }
    world.insert_non_send_resource(chain);
    world.run_schedule(XrSessionCreated);
    world.send_event(XrSessionCreatedEvent);
}

pub fn destroy_xr_session(world: &mut World) {
    world.run_schedule(XrPreDestroySession);
    world.remove_resource::<OxrSession>();
    world.remove_resource::<OxrFrameWaiter>();
    world.remove_resource::<OxrFrameStream>();
    world.remove_resource::<OxrSwapchain>();
    world.remove_resource::<OxrSwapchainImages>();
    world.remove_resource::<OxrGraphicsInfo>();
    world.insert_resource(XrState::Available);
}

pub fn begin_xr_session(
    world: &mut World,
    // session: Res<OxrSession>, mut session_started: ResMut<OxrSessionStarted>
) {
    let _span = debug_span!("xr_begin_session").entered();
    world
        .get_resource::<OxrSession>()
        .unwrap()
        .begin(openxr::ViewConfigurationType::PRIMARY_STEREO)
        .expect("Failed to begin session");
    drop(_span);
    world.get_resource_mut::<OxrSessionStarted>().unwrap().0 = true;
    world.run_schedule(XrPostSessionBegin);
}

pub fn end_xr_session(
    world: &mut World,
    // session: Res<OxrSession>, mut session_started: ResMut<OxrSessionStarted>
) {
    // Maybe this could be an event?
    world.run_schedule(XrPreSessionEnd);
    let _span = debug_span!("xr_end_session").entered();
    world
        .get_resource::<OxrSession>()
        .unwrap()
        .end()
        .expect("Failed to end session");
    world.get_resource_mut::<OxrSessionStarted>().unwrap().0 = false;
}

pub fn request_exit_xr_session(session: Res<OxrSession>) {
    session.request_exit().expect("Failed to request exit");
}

/// This is used solely to transport resources from the main world to the render world.
#[derive(Resource)]
struct OxrRenderResources {
    session: OxrSession,
    frame_stream: OxrFrameStream,
    swapchain: OxrSwapchain,
    images: OxrSwapchainImages,
    graphics_info: OxrGraphicsInfo,
    session_destroy_flag: XrDestroySessionRender,
}

/// This system transfers important render resources from the main world to the render world when a session is created.
pub fn transfer_xr_resources(mut commands: Commands, mut world: ResMut<MainWorld>) {
    let Some(OxrRenderResources {
        session,
        frame_stream,
        swapchain,
        images,
        graphics_info,
        session_destroy_flag,
    }) = world.remove_resource()
    else {
        return;
    };

    commands.insert_resource(session);
    commands.insert_resource(frame_stream);
    commands.insert_resource(swapchain);
    commands.insert_resource(images);
    commands.insert_resource(graphics_info);
    commands.insert_resource(session_destroy_flag);
}
