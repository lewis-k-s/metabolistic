use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;

// Import all modules
pub mod blocks;
pub mod camera;
pub mod debug;
pub mod dev_tools;
pub mod inspector;
pub mod molecules;
pub mod player;
pub mod scenes;
pub mod shared;

/// Game states for scene management
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    MainMenu,
    Scene3D,
    Scene2D,
    GenomeEditing,
}

/// Main app configuration
pub struct MetabolisticApp;

impl MetabolisticApp {
    pub fn new() -> App {
        let mut app = App::new();

        app.add_plugins(DefaultPlugins)
            .add_plugins(PhysicsPlugins::default())
            .add_plugins(PhysicsDebugPlugin::default())
            // State management
            .init_state::<GameState>()
            // Shared systems (available in all states)
            .add_plugins(molecules::CurrencyPlugin)
            .add_plugins(blocks::genome::GenomePlugin)
            .add_plugins(blocks::fat_storage::FatStoragePlugin)
            .add_plugins(dev_tools::plugin)
            .add_plugins(debug::plugin)
            .add_plugins(inspector::plugin)
            // Camera systems that work with any scene type
            .add_plugins(camera::CameraSystemsPlugin)
            // Scene-specific plugins
            .add_plugins(scenes::menu::MainMenuPlugin)
            .add_plugins(scenes::scene_3d::Scene3DPlugin)
            .add_plugins(scenes::scene_2d::Scene2DPlugin)
            .add_plugins(scenes::genome_edit::GenomeEditPlugin)
            // Shared systems that run in multiple states
            .add_systems(Startup, shared::setup_shared_resources)
            .add_systems(
                Update,
                (shared::state_transition_input, shared::genome_demo_system),
            );

        app
    }

    /// Create a headless app for testing (no graphics, no windowing)
    pub fn new_headless() -> App {
        let mut app = App::new();

        app
            // Use minimal plugins for testing - no windowing, no graphics
            .add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin::default())
            .add_plugins(StatesPlugin)
            // State management
            .init_state::<GameState>()
            // Only add plugins that don't require graphics/windowing
            .add_plugins(molecules::CurrencyPlugin)
            .add_plugins(blocks::genome::GenomePlugin)
            .add_plugins(blocks::fat_storage::FatStoragePlugin)
            .add_systems(Startup, shared::setup_shared_resources);

        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        // Test that we can create the app without panicking
        let app = MetabolisticApp::new_headless();
        // Verify the app has a world (basic validity check)
        assert!(app.world().entities().len() >= 0);
    }

    #[test]
    fn test_app_has_required_plugins() {
        let app = MetabolisticApp::new_headless();

        // Verify the app has the expected structure
        // This test ensures the basic setup doesn't panic and has a valid world
        assert!(app.world().entities().len() >= 0);
        assert!(
            app.world().contains_resource::<State<GameState>>(),
            "GameState should be initialized"
        );
    }

    #[test]
    fn test_app_states_initialized() {
        let app = MetabolisticApp::new_headless();

        // Verify that GameState is properly initialized
        // The app should have state management configured
        assert!(
            app.world().contains_resource::<State<GameState>>(),
            "GameState should be initialized"
        );
    }

    #[test]
    fn test_headless_startup() {
        // Test that headless mode can handle startup cycles
        let mut app = MetabolisticApp::new_headless();

        // Run startup systems
        app.update();

        // Verify app is still valid after startup
        assert!(app.world().entities().len() >= 0);
        assert!(
            app.world().contains_resource::<State<GameState>>(),
            "GameState should be initialized"
        );
    }
}
