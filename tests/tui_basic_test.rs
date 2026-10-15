//! Basic TUI functionality tests
//!
//! These tests verify that the TUI system can be initialized and basic
//! components work correctly.

use metabolistic3d::tui::*;
use metabolistic3d::blocks::genome::{BlockKind, GeneState};
use metabolistic3d::molecules::Currency;
use std::collections::HashMap;

#[test]
fn test_cell_snapshot_creation() {
    let snapshot = CellSnapshot::new();
    
    // Test default values
    assert!(snapshot.currencies.is_empty());
    assert!(snapshot.genome.is_empty());
    assert!(snapshot.metabolic_blocks.is_empty());
    assert_eq!(snapshot.timestamp, 0.0);
}

#[test]
fn test_cell_snapshot_with_currencies() {
    let mut currencies = HashMap::new();
    currencies.insert(Currency::ATP, 100.0);
    currencies.insert(Currency::ReducingPower, 50.0);
    
    let snapshot = CellSnapshot::new()
        .with_currencies(currencies)
        .with_timestamp(1.5);
    
    assert_eq!(snapshot.currencies.len(), 2);
    assert_eq!(snapshot.currencies.get(&Currency::ATP), Some(&100.0));
    assert_eq!(snapshot.currencies.get(&Currency::ReducingPower), Some(&50.0));
    assert_eq!(snapshot.timestamp, 1.5);
}

#[test]
fn test_cell_snapshot_with_genome() {
    let mut genome = HashMap::new();
    genome.insert(BlockKind::Fermentation, GeneState::Expressed);
    genome.insert(BlockKind::LipidMetabolism, GeneState::Silent);
    
    let snapshot = CellSnapshot::new()
        .with_genome(genome);
    
    assert_eq!(snapshot.genome.len(), 2);
    assert_eq!(snapshot.genome.get(&BlockKind::Fermentation), Some(&GeneState::Expressed));
    assert_eq!(snapshot.genome.get(&BlockKind::LipidMetabolism), Some(&GeneState::Silent));
}

#[test]
fn test_environment_snapshot_creation() {
    let snapshot = EnvironmentSnapshot::new();
    
    // Test default values
    assert!(snapshot.currencies.is_empty());
    assert_eq!(snapshot.waste_level, 0.0);
    assert_eq!(snapshot.nutrient_level, 0.0);
    assert_eq!(snapshot.timestamp, 0.0);
}

#[test]
fn test_environment_snapshot_with_data() {
    let mut currencies = HashMap::new();
    currencies.insert(Currency::OrganicWaste, 25.0);
    currencies.insert(Currency::CarbonSkeletons, 75.0);
    
    let snapshot = EnvironmentSnapshot::new()
        .with_currencies(currencies)
        .with_waste_level(30.0)
        .with_nutrient_level(60.0)
        .with_timestamp(2.0);
    
    assert_eq!(snapshot.currencies.len(), 2);
    assert_eq!(snapshot.waste_level, 30.0);
    assert_eq!(snapshot.nutrient_level, 60.0);
    assert_eq!(snapshot.timestamp, 2.0);
}

#[test]
fn test_block_snapshot_creation() {
    let block = BlockSnapshot::new(BlockKind::Fermentation);
    
    assert_eq!(block.kind, BlockKind::Fermentation);
    assert_eq!(block.status, "Active");
    assert_eq!(block.efficiency, 1.0);
    assert!(block.inputs.is_empty());
    assert!(block.outputs.is_empty());
}

#[test]
fn test_panel_config_creation() {
    let config = PanelConfig::new("Test Panel");
    
    assert_eq!(config.title, "Test Panel");
    assert_eq!(config.width_percent, 50);
    assert_eq!(config.height_percent, 50);
    assert!(config.border);
}

#[test]
fn test_panel_config_with_custom_size() {
    let config = PanelConfig::new("Custom Panel")
        .with_size(75, 25)
        .with_border(false);
    
    assert_eq!(config.title, "Custom Panel");
    assert_eq!(config.width_percent, 75);
    assert_eq!(config.height_percent, 25);
    assert!(!config.border);
}

#[test]
fn test_tui_config_defaults() {
    let config = TuiConfig::default();
    
    assert_eq!(config.update_interval_ms, 200);
    assert!(config.show_detailed_currencies);
    assert!(config.show_genome_descriptions);
    assert!(config.show_block_details);
    assert!(config.color_enabled);
}

#[test]
fn test_tui_command_from_key_event() {
    use crossterm::event::{KeyEvent, KeyCode};
    
    // Test exit command
    let key_event = KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE);
    let command = TuiCommand::from_key_event(key_event);
    assert_eq!(command, Some(TuiCommand::Exit));
    
    // Test currency toggle
    let key_event = KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::NONE);
    let command = TuiCommand::from_key_event(key_event);
    assert_eq!(command, Some(TuiCommand::ToggleCurrencyMode));
    
    // Test unknown key
    let key_event = KeyEvent::new(KeyCode::Char('x'), crossterm::event::KeyModifiers::NONE);
    let command = TuiCommand::from_key_event(key_event);
    assert_eq!(command, None);
}

#[test]
fn test_tui_command_help_text() {
    let help_text = TuiCommand::get_help_text();
    
    assert!(help_text.contains("ESC/q - Exit TUI"));
    assert!(help_text.contains("c     - Toggle currency display mode"));
    assert!(help_text.contains("g     - Toggle genome display mode"));
    assert!(help_text.contains("b     - Toggle block display mode"));
    assert!(help_text.contains("e     - Toggle environment display mode"));
    assert!(help_text.contains("r     - Reset view to default"));
    assert!(help_text.contains("h     - Show this help"));
}