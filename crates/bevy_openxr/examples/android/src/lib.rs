//! A simple 3D scene with light shining over a cube sitting on a plane.

use bevy::prelude::*;
use bevy_mod_openxr::{add_xr_plugins, init::OxrInitPlugin, types::OxrExtensions};

#[bevy_main]
fn main() {
    App::new()
        .add_plugins(add_xr_plugins(DefaultPlugins).set(OxrInitPlugin {
            app_info: default(),
            exts: {
                let mut exts = OxrExtensions::default();
                exts.enable_fb_passthrough();
                exts.enable_hand_tracking();
                exts
            },
            blend_modes: default(),
            backends: default(),
            formats: default(),
            resolutions: default(),
            synchronous_pipeline_compilation: default(),
        }))
        .add_plugins(bevy_xr_utils::hand_gizmos::HandGizmosPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, modify_msaa)
        .insert_resource(AmbientLight {
            color: Default::default(),
            brightness: 500.0,
            affects_lightmapped_meshes: false,
        })
        .insert_resource(ClearColor(Color::NONE))
        .run();
}

#[derive(Component)]
struct MsaaModified;

fn modify_msaa(cams: Query<Entity, (With<Camera>, Without<MsaaModified>)>, mut commands: Commands) {
    for cam in &cams {
        commands.entity(cam).insert(Msaa::Off).insert(MsaaModified);
    }
}

/// set up a simple 3D scene
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut white: StandardMaterial = Color::WHITE.into();
    white.unlit = true;
    // circular base
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(white)),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    let mut cube_mat: StandardMaterial = Color::srgb_u8(124, 144, 255).into();
    cube_mat.unlit = true;
    // cube
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(cube_mat)),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
}
