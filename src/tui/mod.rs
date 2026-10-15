//! # Terminal User Interface (TUI) for Cell State Monitoring
//!
//! This module provides a terminal-based interface for monitoring and interacting
//! with the cell's metabolic state in real-time. It displays currency reserves,
//! gene expression, metabolic blocks, and environmental interactions.
//!
//! ## Usage
//!
//! Add the TUI plugin to your Bevy app:
//! ```rust,no_run
//! use bevy::prelude::*;
//! use metabolistic3d::tui::CellTuiPlugin;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(CellTuiPlugin)
//!         .run();
//! }
//! ```
//!
//! The TUI will automatically start and display the cell state when the app runs.

use bevy::prelude::*;
use std::io::{stdout, Stdout};

#[cfg(feature = "tui")]
use ratatui::{backend::CrosstermBackend, Terminal};

#[cfg(feature = "tui")]
use crossterm::{
    cursor::Hide,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

pub mod components;
pub mod events;
pub mod systems;

pub use components::*;
pub use events::*;
pub use systems::*;

/// Plugin that enables the TUI for cell state monitoring
#[derive(Default)]
pub struct CellTuiPlugin;

impl Plugin for CellTuiPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "tui")]
        {
            app.init_resource::<TuiState>()
                .init_resource::<TuiConfig>()
                .add_event::<TuiUpdateEvent>()
                .add_event::<TuiInputEvent>()
                .add_systems(
                    Update,
                    (
                        tui_update_system,
                        handle_input_system,
                        collect_cell_data_system,
                    )
                        .chain(),
                );
        }

        #[cfg(not(feature = "tui"))]
        {
            info!("TUI feature not enabled. Add 'tui' feature to enable terminal interface.");
        }
    }
}

/// Main TUI state resource
#[cfg(feature = "tui")]
#[derive(Resource)]
pub struct TuiState {
    /// `None` when stdout is not a terminal (tests, CI, IDE runners). Data
    /// collection and events still work; only rendering is skipped.
    pub terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
    pub running: bool,
    pub last_update: f32,
}

#[cfg(feature = "tui")]
impl Default for TuiState {
    fn default() -> Self {
        use std::io::IsTerminal;
        let terminal = if stdout().is_terminal() {
            Terminal::new(CrosstermBackend::new(stdout())).ok()
        } else {
            warn!("stdout is not a terminal; the TUI will not render");
            None
        };

        Self {
            terminal,
            running: true,
            last_update: 0.0,
        }
    }
}

/// Initialize the TUI terminal
#[cfg(feature = "tui")]
pub fn init_tui() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, Hide)?;
    Ok(())
}

/// Cleanup and restore the terminal
#[cfg(feature = "tui")]
pub fn cleanup_tui() -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}
