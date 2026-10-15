//! # Cells
//!
//! A cell is an entity with the `Cell` marker. Bevy adds the cell's data as
//! required components:
//!
//! - `Genome`: which metabolic blocks the cell has, and their gene state
//! - `CurrencyPools`: the cell's internal pools
//! - `StoredLipids`: lipids kept as storage beads
//! - `CellBlocks`: the block entities that the cell owns, by kind
//! - `MetabolicGraph`: the block graph from the last metabolic tick
//! - `Proteome`: enzymes per block and membrane proteins
//! - `SynthesisQueue`: proteins ordered but not yet built
//! - `CellSize`: sets membrane slots and maintenance cost
//! - `Membrane`: membrane integrity
//! - `LocalEnvironment`: outside channel levels at the cell
//!
//! Each metabolic block is a separate entity with `OwnedBy(cell)`. The block
//! sync system in `blocks` keeps `CellBlocks` in line with the genome. When a
//! cell is removed, its blocks are despawned too.

use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::PI;

use crate::blocks::genome::{BlockKind, Genome};
use crate::environment::LocalEnvironment;
use crate::membrane::Membrane;
use crate::metabolism::{CurrencyPools, MetabolicGraph};
use crate::molecules::{Currency, StoredLipids};
use crate::proteins::{Proteome, SynthesisQueue};

/// Marker for a cell entity. Adding it also adds the cell's data components.
#[derive(Component, Default, Debug)]
#[require(
    Name::new("Cell"),
    Genome,
    CurrencyPools,
    StoredLipids,
    CellBlocks,
    MetabolicGraph,
    Proteome,
    SynthesisQueue,
    CellSize,
    Membrane,
    LocalEnvironment
)]
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

/// Size of a cell. A bigger cell has more membrane area for membrane proteins,
/// but it costs more ATP to maintain. (In M2 it also sets mass and drag.)
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CellSize {
    /// Radius in world units.
    pub radius: f32,
}

impl Default for CellSize {
    fn default() -> Self {
        Self { radius: 1.0 }
    }
}

/// Membrane protein slots per unit of membrane length. **Playtest.**
pub const SLOTS_PER_UNIT_LENGTH: f32 = 1.0;
/// Maintenance ATP per second per unit of cell area. **Playtest.**
pub const MAINTENANCE_ATP_PER_AREA: f32 = 0.1;

/// Number of membrane protein slots. It grows with the membrane length
/// (the cell is a 2D disc).
pub fn membrane_slots(size: &CellSize) -> u32 {
    (SLOTS_PER_UNIT_LENGTH * 2.0 * PI * size.radius.max(0.0)).floor() as u32
}

/// ATP per second that the cell uses to stay alive. It grows with cell area.
pub fn maintenance_atp_per_sec(size: &CellSize) -> f32 {
    MAINTENANCE_ATP_PER_AREA * PI * size.radius.max(0.0).powi(2)
}

/// A cell is alive while it has ATP and an intact membrane.
pub fn is_alive(pools: &CurrencyPools, membrane: &Membrane) -> bool {
    pools.get(Currency::ATP) > 0.0 && membrane.integrity > 0.0
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigger_cells_have_more_slots_and_cost_more() {
        let small = CellSize { radius: 1.0 };
        let big = CellSize { radius: 2.0 };
        assert_eq!(membrane_slots(&small), 6);
        assert_eq!(membrane_slots(&big), 12);
        assert!(maintenance_atp_per_sec(&big) > maintenance_atp_per_sec(&small));
    }

    #[test]
    fn cell_dies_without_atp_or_membrane() {
        let mut pools = CurrencyPools::default();
        let membrane = Membrane::default();
        assert!(!is_alive(&pools, &membrane));
        pools.set(Currency::ATP, 1.0);
        assert!(is_alive(&pools, &membrane));
        let broken = Membrane {
            integrity: 0.0,
            ..default()
        };
        assert!(!is_alive(&pools, &broken));
    }
}
