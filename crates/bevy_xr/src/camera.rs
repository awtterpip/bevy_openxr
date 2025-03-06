use core::panic;

use bevy::app::{App, Plugin, PostUpdate};
use bevy::core_pipeline::core_3d::Camera3d;
use bevy::ecs::component::{Component, HookContext, Mutable, StorageType};
use bevy::ecs::reflect::ReflectComponent;
use bevy::ecs::schedule::IntoSystemConfigs;
use bevy::ecs::world::DeferredWorld;
use bevy::math::{Mat4, Vec3A};
use bevy::pbr::{PbrPlugin, PbrProjectionPlugin};
use bevy::prelude::{Projection, SystemSet};
use bevy::reflect::std_traits::ReflectDefault;
use bevy::reflect::Reflect;
use bevy::render::camera::{CameraProjection, CameraProjectionPlugin, CustomProjection};
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::view::{update_frusta, VisibilitySystems};
use bevy::transform::TransformSystem;

use crate::session::XrTracker;

pub struct XrCameraPlugin;

impl Plugin for XrCameraPlugin {
    fn build(&self, app: &mut App) {
        /*app.add_plugins(CameraProjectionPlugin::<XrProjection>::default());
        app.add_systems(
            PostUpdate,
            update_frusta::<XrProjection>
                .after(TransformSystem::TransformPropagate)
                .before(VisibilitySystems::UpdateFrusta),
        );
        if app.is_plugin_added::<PbrPlugin>() {
            app.add_plugins(PbrProjectionPlugin::<XrProjection>::default());
        }*/
        app.add_plugins((ExtractComponentPlugin::<XrCamera>::default(),));
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash, SystemSet)]
pub struct XrViewInit;

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
pub struct XrProjection {
    pub projection_matrix: Mat4,
    pub near: f32,
}

fn on_projection_add(mut world: DeferredWorld, HookContext { entity, .. }: HookContext) {
    world.commands().entity(entity).remove::<Projection>();
}

impl Default for XrProjection {
    fn default() -> Self {
        Self {
            near: 0.1,
            projection_matrix: Mat4::IDENTITY,
        }
    }
}

/// Marker component for an XR view. It is the backends responsibility to update this.
#[derive(Clone, Copy, Component, ExtractComponent, Debug, Default)]
#[require(Camera3d, XrTracker)]
pub struct XrCamera(pub u32);

impl CameraProjection for XrProjection {
    fn update(&mut self, _width: f32, _height: f32) {}

    fn far(&self) -> f32 {
        self.projection_matrix.to_cols_array()[14]
            / (self.projection_matrix.to_cols_array()[10] + 1.0)
    }

    // TODO calculate this properly
    fn get_frustum_corners(&self, _z_near: f32, _z_far: f32) -> [Vec3A; 8] {
        let ndc_corners = [
            Vec3A::new(1.0, -1.0, 1.0),   // Bottom-right far
            Vec3A::new(1.0, 1.0, 1.0),    // Top-right far
            Vec3A::new(-1.0, 1.0, 1.0),   // Top-left far
            Vec3A::new(-1.0, -1.0, 1.0),  // Bottom-left far
            Vec3A::new(1.0, -1.0, -1.0),  // Bottom-right near
            Vec3A::new(1.0, 1.0, -1.0),   // Top-right near
            Vec3A::new(-1.0, 1.0, -1.0),  // Top-left near
            Vec3A::new(-1.0, -1.0, -1.0), // Bottom-left near
        ];

        let mut view_space_corners = [Vec3A::ZERO; 8];
        let inverse_matrix = self.projection_matrix.inverse();
        for (i, corner) in ndc_corners.into_iter().enumerate() {
            view_space_corners[i] = inverse_matrix.transform_point3a(corner);
        }

        view_space_corners
    }

    fn get_clip_from_view(&self) -> Mat4 {
        self.projection_matrix
    }

    fn get_clip_from_view_for_sub(&self, _sub_view: &bevy::render::camera::SubCameraView) -> Mat4 {
        panic!("sub view not supported for xr camera");
    }
}
