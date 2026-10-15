//! TUI Events and Input Handling
//!
//! This module defines events and input handling for the TUI system.

use bevy::prelude::*;
use crossterm::event::{KeyCode, KeyEvent};
use crate::molecules::Currency;

/// Events for TUI user interactions
#[derive(Event, Debug)]
pub enum TuiInputEvent {
    /// Key press event
    KeyPress(KeyCode),
    /// Request to exit TUI
    Exit,
    /// Toggle panel visibility
    TogglePanel(PanelType),
    /// Request detailed view
    ShowDetails(DetailType),
}

/// Types of TUI panels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelType {
    Currency,
    Genome,
    Blocks,
    Environment,
    System,
}

/// Types of detailed views
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailType {
    CurrencyDetails(Currency),
    GeneDetails(crate::blocks::genome::BlockKind),
    BlockDetails(crate::blocks::genome::BlockKind),
    EnvironmentDetails,
}

/// TUI control commands
#[derive(Debug, Clone, PartialEq)]
pub enum TuiCommand {
    /// Exit the TUI
    Exit,
    /// Toggle currency display mode
    ToggleCurrencyMode,
    /// Toggle genome display mode
    ToggleGenomeMode,
    /// Toggle block display mode
    ToggleBlockMode,
    /// Toggle environment display mode
    ToggleEnvironmentMode,
    /// Reset view to default
    ResetView,
    /// Show help
    ShowHelp,
}

impl TuiCommand {
    pub fn from_key_event(key_event: KeyEvent) -> Option<Self> {
        match key_event.code {
            KeyCode::Esc => Some(Self::Exit),
            KeyCode::Char('c') => Some(Self::ToggleCurrencyMode),
            KeyCode::Char('g') => Some(Self::ToggleGenomeMode),
            KeyCode::Char('b') => Some(Self::ToggleBlockMode),
            KeyCode::Char('e') => Some(Self::ToggleEnvironmentMode),
            KeyCode::Char('r') => Some(Self::ResetView),
            KeyCode::Char('h') => Some(Self::ShowHelp),
            KeyCode::Char('q') => Some(Self::Exit),
            _ => None,
        }
    }
    
    pub fn get_help_text() -> &'static str {
        r#"
TUI Controls:
  ESC/q - Exit TUI
  c     - Toggle currency display mode
  g     - Toggle genome display mode
  b     - Toggle block display mode
  e     - Toggle environment display mode
  r     - Reset view to default
  h     - Show this help
        "#
    }
}