use bevy::prelude::*;
use bevy::state::app::StatesPlugin;

// Simulation modules: always compiled, no graphics dependencies.
pub mod blocks;
pub mod cell;
pub mod environment;
pub mod membrane;
pub mod metabolism;
pub mod molecules;
pub mod player;
pub mod proteins;
pub mod shared;
#[cfg(feature = "tui")]
pub mod tui;

// Graphical modules: only in `full` builds.
#[cfg(feature = "full")]
pub mod debug;
#[cfg(feature = "full")]
pub mod dev_tools;
#[cfg(feature = "full")]
pub mod inspector;
#[cfg(feature = "full")]
pub mod screens;
#[cfg(feature = "full")]
pub mod smoke_test;
#[cfg(feature = "full")]
pub mod sprites;

/// The top-level screens. See `screens` for each one.
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Load assets, then open the menu.
    #[default]
    Loading,
    /// Main menu and its submenus (settings).
    Menu,
    /// The main loop: the player's cell moves and interacts with the world.
    Run,
    /// Metabolism editor: view and edit the cell's metabolic diagram.
    Manage,
}

/// Main app configuration
pub struct MetabolisticApp;

impl MetabolisticApp {
    pub fn new() -> App {
        let mut app = App::new();

        #[cfg(feature = "full")]
        {
            app.add_plugins(
                DefaultPlugins
                    .set(WindowPlugin {
                        primary_window: Some(Window {
                            title: "Metabolistic".into(),
                            ..default()
                        }),
                        ..default()
                    })
                    // Pixel art: sharp edges, not smoothed.
                    .set(ImagePlugin::default_nearest()),
            );
        }

        app // State management
            .init_state::<GameState>()
            // Shared systems (available in all states)
            .add_plugins(molecules::CurrencyPlugin)
            .add_plugins(blocks::genome::GenomePlugin)
            .add_plugins(blocks::MetabolicBlocksPlugin)
            .add_plugins(metabolism::MetabolicFlowPlugin)
            .add_plugins(proteins::ProteinPlugin);

        #[cfg(feature = "tui")]
        app.add_plugins(tui::CellTuiPlugin);

        #[cfg(feature = "full")]
        {
            app.add_plugins(dev_tools::plugin)
                .add_plugins(debug::plugin)
                .add_plugins(inspector::plugin)
                .add_plugins(sprites::plugin)
                .add_plugins(screens::plugin)
                .add_systems(Startup, shared::spawn_starter_cell);
            if smoke_test::requested() {
                app.add_plugins(smoke_test::plugin);
            }
        }

        app
    }

    /// Create a headless app for testing (no graphics, no windowing)
    pub fn new_headless() -> App {
        let mut app = App::new();

        app
            // Use minimal plugins for testing - no windowing, no graphics
            .add_plugins(MinimalPlugins)
            .add_plugins(StatesPlugin)
            // State management
            .init_state::<GameState>()
            // Only add plugins that don't require graphics/windowing
            .add_plugins(molecules::CurrencyPlugin)
            .add_plugins(blocks::genome::GenomePlugin)
            .add_plugins(blocks::MetabolicBlocksPlugin)
            .add_plugins(metabolism::MetabolicFlowPlugin)
            .add_plugins(proteins::ProteinPlugin)
            // Deterministic for tests: no random mutations, and metabolic ticks run
            // only on `metabolism::run_metabolic_tick`, not from real time.
            .insert_resource(blocks::genome::MutationConfig::deterministic())
            .insert_resource(metabolism::FlowmapTimingConfig::manual());

        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        // Test that we can create the app without panicking
        let _app = MetabolisticApp::new_headless();
        // Verify the app has a world (basic validity check)
    }

    #[test]
    fn test_app_has_required_plugins() {
        let app = MetabolisticApp::new_headless();

        // Verify the app has the expected structure
        // This test ensures the basic setup doesn't panic and has a valid world

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

        assert!(
            app.world().contains_resource::<State<GameState>>(),
            "GameState should be initialized"
        );
    }
}
