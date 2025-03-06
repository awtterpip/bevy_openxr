use bevy::prelude::*;
use bevy::render::Render;
use bevy::render::RenderApp;
use bevy::render::RenderSet;
use openxr::sys::SystemPassthroughProperties2FB;
use openxr::PassthroughCapabilityFlagsFB;

use crate::layer_builder::PassthroughLayer;
use crate::resources::*;
use crate::session::OxrSession;
use crate::types::*;

pub struct OxrPassthroughPlugin;

impl Plugin for OxrPassthroughPlugin {
    fn build(&self, app: &mut App) {
        let resources = app
            .world()
            .get_resource::<OxrInstance>()
            .and_then(|instance| {
                app.world()
                    .get_resource::<OxrSystemId>()
                    .map(|system_id| (instance, system_id))
            });
        if resources.is_some_and(|(instance, system)| {
            supports_passthrough(instance, *system).is_ok_and(|s| s)
        }) {
            app.sub_app_mut(RenderApp).add_systems(
                Render,
                insert_passthrough
                    .in_set(RenderSet::PrepareAssets)
                    .run_if(resource_added::<OxrSession>),
            );
        } else {
            error!("Passthrough is not supported with this runtime")
        }
    }
}

pub fn insert_passthrough(world: &mut World) {
    let session = world.resource::<OxrSession>();

    if let Ok((passthrough, passthrough_layer)) = create_passthrough(
        session,
        openxr::PassthroughFlagsFB::IS_RUNNING_AT_CREATION,
        openxr::PassthroughLayerPurposeFB::RECONSTRUCTION,
    ) {
        world
            .resource_mut::<OxrRenderLayers>()
            .insert(0, Box::new(PassthroughLayer));
        world.insert_resource(passthrough);
        world.insert_resource(passthrough_layer);
    }
}

pub fn resume_passthrough(
    passthrough: Res<OxrPassthrough>,
    passthrough_layer: Res<OxrPassthroughLayer>,
) {
    passthrough.start().unwrap();
    passthrough_layer.resume().unwrap();
}

pub fn pause_passthrough(
    passthrough: Res<OxrPassthrough>,
    passthrough_layer: Res<OxrPassthroughLayer>,
) {
    passthrough_layer.pause().unwrap();
    passthrough.pause().unwrap();
}

pub fn create_passthrough(
    session: &OxrSession,
    flags: openxr::PassthroughFlagsFB,
    purpose: openxr::PassthroughLayerPurposeFB,
) -> crate::types::Result<(OxrPassthrough, OxrPassthroughLayer)> {
    let passthrough = session.create_passthrough(flags)?;

    let passthrough_layer = session.create_passthrough_layer(&passthrough, purpose)?;

    Ok((passthrough, passthrough_layer))
}

#[inline]
pub fn supports_passthrough(instance: &OxrInstance, system: OxrSystemId) -> crate::types::Result<bool> {
    if instance.exts().fb_passthrough.is_none() {
        return Ok(false);
    }
    unsafe {
        let mut hand = openxr::sys::SystemPassthroughProperties2FB {
            ty: SystemPassthroughProperties2FB::TYPE,
            next: std::ptr::null(),
            capabilities: PassthroughCapabilityFlagsFB::PASSTHROUGH_CAPABILITY,
        };
        let mut p = openxr::sys::SystemProperties::out(&mut hand as *mut _ as _);
        cvt((instance.fp().get_system_properties)(
            instance.as_raw(),
            system.0,
            p.as_mut_ptr(),
        ))?;
        bevy::log::info!(
            "From supports_passthrough: Passthrough capabilities: {:?}",
            hand.capabilities
        );
        Ok(
            (hand.capabilities & PassthroughCapabilityFlagsFB::PASSTHROUGH_CAPABILITY)
                == PassthroughCapabilityFlagsFB::PASSTHROUGH_CAPABILITY,
        )
    }
}

fn cvt(x: openxr::sys::Result) -> openxr::Result<openxr::sys::Result> {
    if x.into_raw() >= 0 {
        Ok(x)
    } else {
        Err(x)
    }
}
