// use actions::XrActionPlugin;
use bevy::{
    app::{PluginGroup, PluginGroupBuilder},
    prelude::Res,
    render::RenderPlugin,
    utils::default,
    window::{PresentMode, Window, WindowPlugin},
};
use bevy::render::camera::CameraPlugin;
use bevy_mod_xr::session::XrSessionPlugin;
use bevy_mod_xr::{camera::XrCameraPlugin, session::XrState};
use init::OxrInitPlugin;
use poll_events::OxrEventsPlugin;
use render::OxrRenderPlugin;
use resources::OxrInstance;
use session::OxrSession;

use self::{
    features::{handtracking::HandTrackingPlugin, passthrough::OxrPassthroughPlugin},
    reference_space::OxrReferenceSpacePlugin,
};

pub mod action_binding;
pub mod action_set_attaching;
pub mod action_set_syncing;
pub mod error;
pub mod exts;
pub mod features;
pub mod graphics;
pub mod helper_traits;
pub mod init;
pub mod layer_builder;
pub mod next_chain;
pub mod poll_events;
pub mod reference_space;
pub mod render;
pub mod resources;
pub mod session;
pub mod spaces;
pub mod types;

/// A [`Condition`](bevy::ecs::schedule::Condition) system that says if the OpenXR session is available.
pub fn openxr_session_available(
    status: Option<Res<XrState>>,
    instance: Option<Res<OxrInstance>>,
) -> bool {
    status.is_some_and(|s| *s != XrState::Unavailable) && instance.is_some()
}

/// A [`Condition`](bevy::ecs::schedule::Condition) system that says if the OpenXR is running.
/// use this when working with OpenXR specific things
pub fn openxr_session_running(
    status: Option<Res<XrState>>,
    session: Option<Res<OxrSession>>,
) -> bool {
    matches!(status.as_deref(), Some(XrState::Running)) & session.is_some()
}

pub fn add_xr_plugins<G: PluginGroup>(plugins: G) -> PluginGroupBuilder {
    plugins
        .build()
        .disable::<RenderPlugin>()
        // .disable::<PipelinedRenderingPlugin>()
        .add_before::<RenderPlugin>(XrSessionPlugin { auto_handle: true })
        .add_before::<RenderPlugin>(OxrInitPlugin::default())
        .add(OxrEventsPlugin)
        .add(OxrReferenceSpacePlugin::default())
        .add(OxrRenderPlugin::default())
        .add(OxrPassthroughPlugin)
        .add(HandTrackingPlugin::default())
        .add(XrCameraPlugin)
        .add(action_set_attaching::OxrActionAttachingPlugin)
        .add(action_binding::OxrActionBindingPlugin)
        .add(action_set_syncing::OxrActionSyncingPlugin)
        .add(features::overlay::OxrOverlayPlugin)
        .add(spaces::OxrSpatialPlugin)
        .add(spaces::OxrSpacePatchingPlugin)
        // .add(XrActionPlugin)
        // we should probably handle the exiting ourselfs so that we can correctly end the
        // session and instance
        .set(WindowPlugin {
            primary_window: Some(Window {
                transparent: true,
                present_mode: PresentMode::AutoNoVsync,
                // title: self.app_info.name.clone(),
                ..default()
            }),
            // #[cfg(target_os = "android")]
            // exit_condition: bevy::window::ExitCondition::DontExit,
            #[cfg(target_os = "android")]
            close_when_requested: true,
            ..default()
        })
}
