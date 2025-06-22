use bevy::prelude::*;
use crate::GameState;

/// Main menu plugin that manages the main menu state
pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(OnEnter(GameState::MainMenu), setup_menu)
            .add_systems(Update, menu_input.run_if(in_state(GameState::MainMenu)))
            .add_systems(OnExit(GameState::MainMenu), cleanup_menu);
    }
}

/// Marker component for menu entities
#[derive(Component)]
struct MenuEntity;

/// Setup the main menu UI
fn setup_menu(mut commands: Commands) {
    info!("Setting up main menu");
    
    // Spawn a simple camera for the menu
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        MenuEntity,
    ));
    
    // Add some ambient light for visibility
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
    });
    
    info!("Main menu setup complete");
    info!("Controls:");
    info!("  Press '1' for 3D rolling scene");
    info!("  Press '2' for 2D top-down scene");
}

/// Handle menu-specific input
fn menu_input(
    input: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if input.just_pressed(KeyCode::Digit1) {
        next_state.set(GameState::Scene3D);
    } else if input.just_pressed(KeyCode::Digit2) {
        next_state.set(GameState::Scene2D);
    }
}

/// Clean up menu entities when leaving the menu state
fn cleanup_menu(
    mut commands: Commands,
    menu_entities: Query<Entity, With<MenuEntity>>,
    camera_entities: Query<Entity, (With<Camera3d>, Without<MenuEntity>)>,
) {
    info!("Cleaning up main menu");
    
    // Remove menu-specific entities
    for entity in menu_entities.iter() {
        commands.entity(entity).despawn_recursive();
    }
    
    // Remove any stray cameras that might not be marked with MenuEntity
    for entity in camera_entities.iter() {
        if let Some(entity_commands) = commands.get_entity(entity) {
            entity_commands.despawn_recursive();
        }
    }
} 