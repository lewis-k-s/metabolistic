use crate::GameState;
use bevy::prelude::*;

/// 2D top-down pseudo scene plugin
pub struct Scene2DPlugin;

impl Plugin for Scene2DPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Scene2D), setup_2d_scene)
            .add_systems(
                Update,
                (handle_2d_movement, update_2d_camera).run_if(in_state(GameState::Scene2D)),
            )
            .add_systems(OnExit(GameState::Scene2D), cleanup_2d_scene);
    }
}

/// Marker component for 2D scene entities
#[derive(Component)]
struct Scene2DEntity;

/// 2D player representation (could be a circle or sprite)
#[derive(Component)]
struct Player2D {
    pub speed: f32,
}

impl Default for Player2D {
    fn default() -> Self {
        Self { speed: 200.0 }
    }
}

/// 2D camera component
#[derive(Component)]
struct Camera2D;

/// Setup the 2D top-down scene
fn setup_2d_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    info!("Setting up 2D top-down scene");

    // Setup orthographic camera for top-down view
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
        Projection::Orthographic(OrthographicProjection::default_3d()),
        Camera2D,
        Scene2DEntity,
    ));

    // Create a ground plane (viewed from above)
    let ground_size = 50.0;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(ground_size, ground_size))),
        MeshMaterial3d(materials.add(Color::srgb(0.2, 0.6, 0.2))),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Scene2DEntity,
    ));

    // Create a 2D player representation (circle viewed from above)
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(1.0).mesh())),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.4, 0.2))),
        Transform::from_xyz(0.0, 0.1, 0.0),
        Player2D::default(),
        Scene2DEntity,
    ));

    // Add some ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 800.0,
    });

    // Spawn initial metabolic block entities for this scene
    let sugar_entity = crate::blocks::genome::spawn_metabolic_block(
        &mut commands,
        crate::blocks::genome::BlockKind::SugarCatabolism,
    );
    let fermentation_entity =
        crate::blocks::genome::spawn_metabolic_block(&mut commands, crate::blocks::genome::BlockKind::Fermentation);
    let amino_entity = crate::blocks::genome::spawn_metabolic_block(
        &mut commands,
        crate::blocks::genome::BlockKind::AminoAcidBiosynthesis,
    );

    // Mark them as part of this scene for cleanup
    commands.entity(sugar_entity).insert(Scene2DEntity);
    commands.entity(fermentation_entity).insert(Scene2DEntity);
    commands.entity(amino_entity).insert(Scene2DEntity);

    info!("2D scene setup complete");
    info!("Controls:");
    info!("  WASD - Move in 2D plane");
    info!("  Escape - Return to menu");
}

/// Handle 2D movement using keyboard input
fn handle_2d_movement(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player_query: Query<(&mut Transform, &Player2D)>,
) {
    for (mut transform, player) in player_query.iter_mut() {
        let mut movement = Vec3::ZERO;

        if input.pressed(KeyCode::KeyW) {
            movement.z -= 1.0;
        }
        if input.pressed(KeyCode::KeyS) {
            movement.z += 1.0;
        }
        if input.pressed(KeyCode::KeyA) {
            movement.x -= 1.0;
        }
        if input.pressed(KeyCode::KeyD) {
            movement.x += 1.0;
        }

        if movement.length() > 0.0 {
            movement = movement.normalize();
            transform.translation += movement * player.speed * time.delta_secs();
        }
    }
}

/// Update camera to follow the 2D player
fn update_2d_camera(
    player_query: Query<&Transform, (With<Player2D>, Without<Camera2D>)>,
    mut camera_query: Query<&mut Transform, (With<Camera2D>, Without<Player2D>)>,
    time: Res<Time>,
) {
    if let (Ok(player_transform), Ok(mut camera_transform)) =
        (player_query.get_single(), camera_query.get_single_mut())
    {
        let target_position = Vec3::new(
            player_transform.translation.x,
            20.0, // Keep camera height constant
            player_transform.translation.z,
        );

        // Smoothly follow the player
        camera_transform.translation = camera_transform
            .translation
            .lerp(target_position, 5.0 * time.delta_secs());
    }
}

/// Clean up 2D scene entities when leaving
fn cleanup_2d_scene(
    mut commands: Commands,
    scene_entities: Query<Entity, With<Scene2DEntity>>,
    camera_entities: Query<Entity, With<Camera2D>>,
) {
    info!("Cleaning up 2D scene");

    // Remove scene-specific entities (including cameras)
    for entity in scene_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }

    // Remove any remaining cameras (safety check)
    for entity in camera_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }
}
