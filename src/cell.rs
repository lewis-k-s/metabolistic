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
//! - `Transform`: the position in the world (`EnvGrid` is sampled there)
//! - `MoveInput`, `ThrustEffort`: the wanted direction, and the thrust used
//! - `ExchangeStats`: minerals taken from the grid and used
//! - O2 and ROS pools: see `oxygen`
//! - `Hydration`, `CuticleShield`: water in the cell (see `desiccation`)
//! - `GenomeIntegrity`, `MutationLoad`, `RepairProgress`: genome damage from UV
//!   (see `genome_damage`)
//!
//! Each metabolic block is a separate entity with `OwnedBy(cell)`. The block
//! sync system in `blocks` keeps `CellBlocks` in line with the genome. When a
//! cell is removed, its blocks are despawned too.

use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::PI;

use crate::blocks::genome::{BlockKind, Genome};
use crate::environment::LocalEnvironment;
use crate::exchange::ExchangeStats;
use crate::actions::{ActionBar, ActionState, PathwayLevels, SlotHeld};
use crate::pilus::PilusTether;
use crate::shelter::InShelter;
use crate::genome_damage::{GenomeIntegrity, InducedMutations, MutationLoad, RepairProgress};
use crate::desiccation::{CuticleShield, Hydration};
use crate::engulf::EngulfIntent;
use crate::membrane::Membrane;
use crate::metabolism::{CellTick, CurrencyPools, FlowLedger, MetabolicGraph};
use crate::molecules::{Currency, StoredLipids};
use crate::movement::{MoveInput, ThrustEffort};
use crate::proteins::{Proteome, SynthesisQueue};

/// Marker for a cell entity. Adding it also adds the cell's data components.
#[derive(Component, Default, Debug)]
#[require(
    Name::new("Cell"),
    Genome,
    CurrencyPools,
    FlowLedger,
    StoredLipids,
    CellBlocks,
    MetabolicGraph,
    Proteome,
    SynthesisQueue,
    CellSize,
    Membrane,
    LocalEnvironment,
    Transform,
    MoveInput,
    ThrustEffort,
    ExchangeStats,
    DamageTaken,
    GenomeIntegrity,
    MutationLoad,
    InducedMutations,
    RepairProgress,
    InShelter,
    PathwayLevels,
    ActionBar,
    ActionState,
    SlotHeld,
    PilusTether,
    CellTick,
    EngulfIntent,
    Hydration,
    CuticleShield
)]
pub struct Cell;

/// Where membrane damage came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    /// Heat above `exchange::HEAT_DAMAGE_THRESHOLD`.
    Heat,
    /// UV light that pigments did not screen.
    Uv,
    /// Reactive oxygen species inside the cell (see `oxygen`).
    Ros,
    /// Too little water in the cell (see `desiccation`).
    Desiccation,
}

/// Membrane damage that a cell took, by source, since it was spawned. It lets
/// the run-over screen say what did the most harm.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct DamageTaken {
    pub heat: f32,
    pub uv: f32,
    pub ros: f32,
    pub desiccation: f32,
}

impl DamageTaken {
    /// Records `amount` of damage from `source`.
    pub fn add(&mut self, source: DamageSource, amount: f32) {
        match source {
            DamageSource::Heat => self.heat += amount.max(0.0),
            DamageSource::Uv => self.uv += amount.max(0.0),
            DamageSource::Ros => self.ros += amount.max(0.0),
            DamageSource::Desiccation => self.desiccation += amount.max(0.0),
        }
    }

    /// The source that did the most damage. `None` if there was none.
    pub fn main_source(&self) -> Option<DamageSource> {
        let all = [
            (DamageSource::Heat, self.heat),
            (DamageSource::Uv, self.uv),
            (DamageSource::Ros, self.ros),
            (DamageSource::Desiccation, self.desiccation),
        ];
        all.into_iter()
            .filter(|&(_, amount)| amount > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(source, _)| source)
    }
}

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
/// but it costs more ATP to maintain. It also sets mass and drag (see
/// `movement`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CellSize {
    /// Radius in size units. One unit is `movement::PIXELS_PER_SIZE_UNIT` pixels.
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

/// ATP at or below this counts as 0. In a cold zone, the grid still gives a
/// tiny heat and tiny chemosynthesis, so ATP stays a little above 0 and never
/// reaches it exactly.
pub const STARVATION_ATP: f32 = 0.01;

/// Why a cell died.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathCause {
    /// ATP reached 0.
    Starved,
    /// Membrane integrity reached 0 (for example from heat).
    MembraneBroken,
    /// A bigger cell engulfed the player (see `engulf`).
    Engulfed,
}

/// The cause of death, or `None` while the cell is alive. A cell is alive while
/// it has ATP and an intact membrane. If both fail, the membrane counts.
pub fn death_cause(pools: &CurrencyPools, membrane: &Membrane) -> Option<DeathCause> {
    if membrane.integrity <= 0.0 {
        Some(DeathCause::MembraneBroken)
    } else if pools.get(Currency::ATP) <= STARVATION_ATP {
        Some(DeathCause::Starved)
    } else {
        None
    }
}

/// A cell is alive while it has ATP and an intact membrane.
pub fn is_alive(pools: &CurrencyPools, membrane: &Membrane) -> bool {
    death_cause(pools, membrane).is_none()
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
    fn damage_taken_names_the_main_source() {
        let mut taken = DamageTaken::default();
        assert_eq!(taken.main_source(), None);
        taken.add(DamageSource::Heat, 3.0);
        taken.add(DamageSource::Heat, -5.0);
        assert_eq!(taken.main_source(), Some(DamageSource::Heat));
        assert_eq!(taken.heat, 3.0);
        taken.add(DamageSource::Uv, 5.0);
        assert_eq!(taken.main_source(), Some(DamageSource::Uv));
        taken.add(DamageSource::Ros, 9.0);
        assert_eq!(taken.main_source(), Some(DamageSource::Ros));
        taken.add(DamageSource::Desiccation, 20.0);
        assert_eq!(taken.main_source(), Some(DamageSource::Desiccation));
    }

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

    #[test]
    fn death_cause_names_the_failure() {
        let mut pools = CurrencyPools::default();
        let membrane = Membrane::default();
        assert_eq!(death_cause(&pools, &membrane), Some(DeathCause::Starved));
        pools.set(Currency::ATP, 1.0);
        assert_eq!(death_cause(&pools, &membrane), None);
        let broken = Membrane {
            integrity: 0.0,
            ..default()
        };
        assert_eq!(death_cause(&pools, &broken), Some(DeathCause::MembraneBroken));
    }
}
