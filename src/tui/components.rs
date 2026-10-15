//! TUI Components and Data Structures
//!
//! This module defines the data structures and components used by the TUI
//! to represent and display cell state information.

use crate::blocks::genome::{BlockKind, GeneState};
use crate::molecules::Currency;
use bevy::prelude::*;
use std::collections::HashMap;

/// Event containing updated TUI data
#[derive(Event, Debug, Clone)]
pub struct TuiUpdateEvent {
    pub cell_data: CellSnapshot,
    pub environment_data: EnvironmentSnapshot,
}

/// Snapshot of cell state for TUI display
#[derive(Debug, Clone, Default)]
pub struct CellSnapshot {
    pub currencies: HashMap<Currency, f32>,
    pub genome: HashMap<BlockKind, GeneState>,
    pub metabolic_blocks: Vec<BlockSnapshot>,
    pub timestamp: f32,
}

impl CellSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_currencies(mut self, currencies: HashMap<Currency, f32>) -> Self {
        self.currencies = currencies;
        self
    }

    pub fn with_genome(mut self, genome: HashMap<BlockKind, GeneState>) -> Self {
        self.genome = genome;
        self
    }

    pub fn with_timestamp(mut self, timestamp: f32) -> Self {
        self.timestamp = timestamp;
        self
    }
}

/// Snapshot of a metabolic block
#[derive(Debug, Clone)]
pub struct BlockSnapshot {
    pub kind: BlockKind,
    pub status: String,
    pub efficiency: f32,
    pub inputs: Vec<(Currency, f32)>,
    pub outputs: Vec<(Currency, f32)>,
}

impl BlockSnapshot {
    pub fn new(kind: BlockKind) -> Self {
        Self {
            kind,
            status: "Active".to_string(),
            efficiency: 1.0,
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

/// Snapshot of environmental state
#[derive(Debug, Clone, Default)]
pub struct EnvironmentSnapshot {
    pub currencies: HashMap<Currency, f32>,
    pub waste_level: f32,
    pub nutrient_level: f32,
    pub timestamp: f32,
}

impl EnvironmentSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_currencies(mut self, currencies: HashMap<Currency, f32>) -> Self {
        self.currencies = currencies;
        self
    }

    pub fn with_waste_level(mut self, waste_level: f32) -> Self {
        self.waste_level = waste_level;
        self
    }

    pub fn with_nutrient_level(mut self, nutrient_level: f32) -> Self {
        self.nutrient_level = nutrient_level;
        self
    }

    pub fn with_timestamp(mut self, timestamp: f32) -> Self {
        self.timestamp = timestamp;
        self
    }
}

/// TUI configuration options
#[derive(Resource, Debug, Clone)]
pub struct TuiConfig {
    pub update_interval_ms: u64,
    pub show_detailed_currencies: bool,
    pub show_genome_descriptions: bool,
    pub show_block_details: bool,
    pub color_enabled: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            update_interval_ms: 200, // 5 FPS
            show_detailed_currencies: true,
            show_genome_descriptions: true,
            show_block_details: true,
            color_enabled: true,
        }
    }
}

/// TUI panel configuration
#[derive(Debug, Clone)]
pub struct PanelConfig {
    pub title: String,
    pub width_percent: u16,
    pub height_percent: u16,
    pub border: bool,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            title: "Panel".to_string(),
            width_percent: 50,
            height_percent: 50,
            border: true,
        }
    }
}

impl PanelConfig {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            ..Default::default()
        }
    }

    pub fn with_size(mut self, width_percent: u16, height_percent: u16) -> Self {
        self.width_percent = width_percent;
        self.height_percent = height_percent;
        self
    }

    pub fn with_border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }
}
