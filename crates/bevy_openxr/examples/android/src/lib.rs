//! A simple 3D scene with light shining over a cube sitting on a plane.

use bevy::{core::TaskPoolThreadAssignmentPolicy, prelude::*};
use bevy_openxr::{add_xr_plugins, init::OxrInitPlugin, types::OxrExtensions};

#[bevy_main]
fn main() {
    App::new()
        .add_plugins(
            add_xr_plugins(DefaultPlugins)
                .set(OxrInitPlugin {
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
                })
                .set(TaskPoolPlugin {
                    task_pool_options: TaskPoolOptions {
                        compute: TaskPoolThreadAssignmentPolicy {
                            min_threads: 5,
                            max_threads: usize::MAX,
                            percent: 1.0,
                        },
                        ..default()
                    },
                }),
        )
        .add_plugins(bevy_xr_utils::hand_gizmos::HandGizmosPlugin)
        .insert_resource(Msaa::Off)
        .add_systems(Startup, setup)
        .insert_resource(AmbientLight {
            color: Default::default(),
            brightness: 500.0,
        })
        .insert_resource(ClearColor(Color::NONE))
        .run();
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
    commands.spawn(PbrBundle {
        mesh: meshes.add(Circle::new(4.0)),
        material: materials.add(white),
        transform: Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ..default()
    });
    let mut cube_mat: StandardMaterial = Color::rgb_u8(124, 144, 255).into();
    cube_mat.unlit = true;
    // cube
    commands.spawn(PbrBundle {
        mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        material: materials.add(cube_mat),
        transform: Transform::from_xyz(0.0, 0.5, 0.0),
        ..default()
    });
}
