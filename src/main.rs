use bevy::prelude::*;
use avian3d::prelude::*;

mod dev_tools;
mod inspector;
mod debug;
mod player;
// mod molecules;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PhysicsDebugPlugin::default())
        .add_plugins(player::PlayerPlugin)
        .add_plugins(dev_tools::plugin)
        .add_plugins(debug::plugin)
        .add_plugins(inspector::plugin)
        .add_systems(Startup, setup)
        .run();
}

// STARTUP

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let floor_size = 500.0;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(floor_size, floor_size))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
        RigidBody::Static,
        Collider::cuboid(floor_size / 2.0, 0.1, floor_size / 2.0),
    ));

    commands.spawn( (        
        PointLight {
            intensity: 1_000_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
}

// UPDATE
