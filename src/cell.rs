//! # Cells
//!
//! A cell is an entity with the `Cell` marker. Bevy adds the cell's data as
//! required components:
//!
//! - `Genome`: which metabolic blocks the cell has, and their gene state
//! - `CurrencyPools`: the cell's metabolite pools
//! - `CellBlocks`: the block entities that the cell owns, by kind
//! - `MetabolicGraph`: the solve order from the last metabolic tick
//!
//! Each metabolic block is a separate entity with `OwnedBy(cell)`. The block
//! sync system in `blocks` keeps `CellBlocks` in line with the genome. When a
//! cell is removed, its blocks are despawned too.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::blocks::genome::{BlockKind, Genome};
use crate::metabolism::{CurrencyPools, MetabolicGraph};

/// Marker for a cell entity. Adding it also adds the cell's data components.
#[derive(Component, Default, Debug)]
#[require(Name::new("Cell"), Genome, CurrencyPools, CellBlocks, MetabolicGraph)]
pub struct Cell;

/// The metabolic block entities that a cell owns, keyed by block kind.
#[derive(Component, Default, Debug, Clone)]
pub struct CellBlocks(pub HashMap<BlockKind, Entity>);

impl CellBlocks {
    pub fn get(&self, kind: BlockKind) -> Option<Entity> {
        self.0.get(&kind).copied()
    }

    pub fn contains(&self, kind: BlockKind) -> bool {
        self.0.contains_key(&kind)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Points from a metabolic block back to the cell that owns it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedBy(pub Entity);

/// Spawns a cell with the given genome and pools.
pub fn spawn_cell(commands: &mut Commands, genome: Genome, pools: CurrencyPools) -> Entity {
    commands.spawn((Cell, genome, pools)).id()
}

/// Observer: despawn the blocks of a cell when the cell is removed.
pub(crate) fn despawn_blocks_of_removed_cell(
    removed: On<Remove, Cell>,
    cells: Query<&CellBlocks>,
    mut commands: Commands,
) {
    if let Ok(blocks) = cells.get(removed.entity) {
        for &block in blocks.0.values() {
            if let Ok(mut entity) = commands.get_entity(block) {
                entity.despawn();
            }
        }
    }
}
