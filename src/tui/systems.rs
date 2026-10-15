//! TUI Systems and Rendering Logic
//!
//! This module contains the systems that handle TUI rendering, input handling,
//! and data collection for the cell state display.

use bevy::prelude::*;

#[cfg(feature = "tui")]
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Text,
    widgets::{Block, Paragraph},
};

use crate::blocks::genome::GeneState;
use crate::cell::Cell;
use crate::molecules::Currency;
use crate::tui::*;
use std::collections::HashMap;

// Explicitly import Bevy's KeyCode to avoid ambiguity
use bevy::input::keyboard::KeyCode;

/// Main TUI update system
#[cfg(feature = "tui")]
pub fn tui_update_system(
    mut tui_state: ResMut<TuiState>,
    mut tui_events: MessageReader<TuiUpdateEvent>,
    time: Res<Time>,
) {
    if !tui_state.running {
        return;
    }

    // Check if it's time to update
    let current_time = time.elapsed_secs();
    if current_time - tui_state.last_update < 0.2 {
        // 5 FPS
        return;
    }

    tui_state.last_update = current_time;

    // Get the latest data
    if let Some(event) = tui_events.read().last()
        && let Err(e) = render_tui(&mut tui_state, &event.cell_data, &event.environment_data)
    {
        error!("Failed to render TUI: {}", e);
    }
}

/// Render the main TUI interface
#[cfg(feature = "tui")]
fn render_tui(
    tui_state: &mut TuiState,
    cell_data: &CellSnapshot,
    env_data: &EnvironmentSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(terminal) = tui_state.terminal.as_mut() else {
        return Ok(());
    };
    let mut frame = terminal.get_frame();

    // Clear the frame
    frame.render_widget(
        Paragraph::new("Metabolistic3D Cell State Monitor"),
        frame.area(),
    );

    // Create main layout
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60), // Top section
            Constraint::Percentage(40), // Bottom section
        ])
        .split(frame.area());

    // Create top horizontal layout
    let top_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left panel
            Constraint::Percentage(50), // Right panel
        ])
        .split(main_layout[0]);

    // Create bottom horizontal layout
    let bottom_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left panel
            Constraint::Percentage(50), // Right panel
        ])
        .split(main_layout[1]);

    // Render panels
    render_currency_panel(&mut frame, top_layout[0], cell_data)?;
    render_genome_panel(&mut frame, top_layout[1], cell_data)?;
    render_blocks_panel(&mut frame, bottom_layout[0], cell_data)?;
    render_environment_panel(&mut frame, bottom_layout[1], env_data)?;

    Ok(())
}

/// Render the currency panel
#[cfg(feature = "tui")]
fn render_currency_panel(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    cell_data: &CellSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let block = Block::bordered()
        .title("Currency Reserves")
        .title_style(Style::default().fg(Color::Yellow));

    let mut text = Text::default();
    text.push_line("Cell Currency Levels:");
    text.push_line("");

    for (currency, amount) in &cell_data.currencies {
        text.push_line(format!("{:?}: {:.2}", currency, amount));
    }

    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    frame.render_widget(paragraph, area);
    Ok(())
}

/// Render the genome panel
#[cfg(feature = "tui")]
fn render_genome_panel(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    cell_data: &CellSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let block = Block::bordered()
        .title("Genome Expression")
        .title_style(Style::default().fg(Color::Green));

    let mut text = Text::default();
    text.push_line("Gene Expression States:");
    text.push_line("");

    for (block_kind, gene_state) in &cell_data.genome {
        let status_symbol = match gene_state {
            GeneState::Expressed => "✓",
            GeneState::Silent => "○",
            GeneState::Mutated => "✗",
        };
        text.push_line(format!("{} {:?}", status_symbol, block_kind));
    }

    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    frame.render_widget(paragraph, area);
    Ok(())
}

/// Render the metabolic blocks panel
#[cfg(feature = "tui")]
fn render_blocks_panel(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    cell_data: &CellSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let block = Block::bordered()
        .title("Metabolic Blocks")
        .title_style(Style::default().fg(Color::Cyan));

    let mut text = Text::default();
    text.push_line("Active Metabolic Blocks:");
    text.push_line("");

    if cell_data.metabolic_blocks.is_empty() {
        text.push_line("No active blocks");
    } else {
        for block_snapshot in &cell_data.metabolic_blocks {
            text.push_line(format!(
                "{:?} - {}",
                block_snapshot.kind, block_snapshot.status
            ));
        }
    }

    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    frame.render_widget(paragraph, area);
    Ok(())
}

/// Render the environment panel
#[cfg(feature = "tui")]
fn render_environment_panel(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    env_data: &EnvironmentSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    let block = Block::bordered()
        .title("Environment")
        .title_style(Style::default().fg(Color::Magenta));

    let mut text = Text::default();
    text.push_line("Environmental State:");
    text.push_line("");
    text.push_line(format!("Waste Level: {:.2}", env_data.waste_level));
    text.push_line(format!("Nutrient Level: {:.2}", env_data.nutrient_level));
    text.push_line("");
    text.push_line("Environmental Currencies:");

    for (currency, amount) in &env_data.currencies {
        text.push_line(format!("{:?}: {:.2}", currency, amount));
    }

    let paragraph = Paragraph::new(text).block(block).style(Style::default());

    frame.render_widget(paragraph, area);
    Ok(())
}

/// Handle keyboard input for TUI
#[cfg(feature = "tui")]
pub fn handle_input_system(
    mut tui_state: ResMut<TuiState>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut tui_input_events: MessageWriter<TuiInputEvent>,
) {
    let Some(keyboard) = keyboard else {
        return; // No input plugin (headless)
    };
    if !tui_state.running {
        return;
    }

    // Check for exit key
    if keyboard.just_pressed(KeyCode::Escape) {
        tui_state.running = false;
        tui_input_events.write(TuiInputEvent::Exit);
        return;
    }

    // Handle other key presses
    if keyboard.just_pressed(KeyCode::KeyC) {
        tui_input_events.write(TuiInputEvent::TogglePanel(PanelType::Currency));
    }

    if keyboard.just_pressed(KeyCode::KeyG) {
        tui_input_events.write(TuiInputEvent::TogglePanel(PanelType::Genome));
    }

    if keyboard.just_pressed(KeyCode::KeyB) {
        tui_input_events.write(TuiInputEvent::TogglePanel(PanelType::Blocks));
    }

    if keyboard.just_pressed(KeyCode::KeyE) {
        tui_input_events.write(TuiInputEvent::TogglePanel(PanelType::Environment));
    }
}

/// Collect cell data for TUI display
pub fn collect_cell_data_system(
    cell_query: Query<
        (
            &crate::molecules::CurrencyPools,
            &crate::blocks::genome::Genome,
        ),
        With<Cell>,
    >,
    mut tui_events: MessageWriter<TuiUpdateEvent>,
    time: Res<Time>,
) {
    for (pools, genome) in cell_query.iter() {
        let cell_snapshot = CellSnapshot {
            currencies: get_currency_snapshot(pools),
            genome: genome.table.clone(),
            metabolic_blocks: Vec::new(), // TODO: Implement block data collection
            timestamp: time.elapsed_secs(),
        };

        let env_snapshot = EnvironmentSnapshot {
            currencies: HashMap::new(), // TODO: Implement environment data collection
            waste_level: 0.0,
            nutrient_level: 0.0,
            timestamp: time.elapsed_secs(),
        };

        tui_events.write(TuiUpdateEvent {
            cell_data: cell_snapshot,
            environment_data: env_snapshot,
        });
    }
}

/// All internal pools of a cell, for the TUI.
fn get_currency_snapshot(
    currency_pools: &crate::molecules::CurrencyPools,
) -> HashMap<Currency, f32> {
    Currency::ALL
        .iter()
        .map(|&c| (c, currency_pools.get(c)))
        .collect()
}

// Fallback implementations for when TUI feature is not enabled
#[cfg(not(feature = "tui"))]
pub fn tui_update_system(
    _tui_state: ResMut<TuiState>,
    _tui_events: MessageReader<TuiUpdateEvent>,
    _time: Res<Time>,
) {
    // No-op when TUI feature is not enabled
}

#[cfg(not(feature = "tui"))]
pub fn handle_input_system(
    _tui_state: ResMut<TuiState>,
    _keyboard: Res<ButtonInput<KeyCode>>,
    _tui_input_events: MessageWriter<TuiInputEvent>,
) {
    // No-op when TUI feature is not enabled
}
