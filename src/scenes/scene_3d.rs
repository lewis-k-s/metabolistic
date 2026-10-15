use crate::{GameState, camera, player};
use avian3d::prelude::*;
use bevy::prelude::*;

/// 3D rolling scene plugin
pub struct Scene3DPlugin;

impl Plugin for Scene3DPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Scene3D), setup_3d_scene)
            // Player and camera systems are handled by their respective plugins
            .add_systems(OnExit(GameState::Scene3D), cleanup_3d_scene)
            // Add 3D-specific plugins
            .add_plugins(player::PlayerPlugin);
    }
}

/// Marker component for 3D scene entities
#[derive(Component)]
struct Scene3DEntity;

/// Marker component for 3D camera
#[derive(Component)]
struct Camera3D;

/// Setup the 3D rolling scene
fn setup_3d_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    info!("Setting up 3D rolling scene");

    // Create the floor
    let floor_size = 500.0;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(floor_size, floor_size))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RigidBody::Static,
        Collider::cuboid(floor_size / 2.0, 0.1, floor_size / 2.0),
        Friction {
            dynamic_coefficient: 1.0,
            static_coefficient: 1.0,
            combine_rule: CoefficientCombine::Multiply,
        },
        Scene3DEntity,
    ));

    // Add lighting
    commands.spawn((
        PointLight {
            intensity: 1_000_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
        Scene3DEntity,
    ));

    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 500.0,
        ..default()
    });

    // Spawn the 3D camera
    let camera_entity = camera::spawn_3d_camera(&mut commands);
    commands
        .entity(camera_entity)
        .insert((Camera3D, Scene3DEntity));

    info!("3D scene setup complete");
    info!("Controls:");
    info!("  WASD - Move player");
    info!("  Mouse - Look around");
    info!("  Space - Jump");
    info!("  Escape - Return to menu");
}

/// Clean up 3D scene entities when leaving
fn cleanup_3d_scene(
    mut commands: Commands,
    scene_entities: Query<Entity, (With<Scene3DEntity>, Without<ChildOf>)>,
    // Only the physics ball: the player-cell link must survive scene changes.
    player_entities: Query<Entity, (With<player::PlayerAvatar>, Without<Scene3DEntity>)>,
    camera_entities: Query<Entity, (With<Camera3D>, Without<Scene3DEntity>)>,
) {
    info!("Cleaning up 3D scene");

    // Remove scene-specific entities (including cameras)
    for entity in scene_entities.iter() {
        commands.entity(entity).despawn();
    }

    // Remove player entities
    for entity in player_entities.iter() {
        commands.entity(entity).despawn();
    }

    // Remove any remaining cameras (safety check)
    for entity in camera_entities.iter() {
        commands.entity(entity).despawn();
    }
}
