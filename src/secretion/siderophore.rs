//! # Siderophore (`entF`)
//!
//! In water with oxygen, iron is Fe(III), which forms insoluble oxides such as
//! ferrihydrite ("rust"). Bacteria release siderophores, small molecules that
//! bind Fe(III) very strongly and pull it off mineral surfaces. Enterobactin
//! is the siderophore of *E. coli*: EntC, EntB and EntA make
//! 2,3-dihydroxybenzoate from chorismate, and EntE and EntF join three of
//! them with three serines into a ring. EntS and TolC export it. The cell
//! takes the iron complex back in through the receptor FepA, and the esterase
//! Fes breaks it to free the iron, so each molecule is used once.
//!
//! In the anoxic vents iron is soluble Fe(II) and iron sulfide, which a
//! siderophore does not dissolve. So only the rust pickups of the oxic stages
//! (`particles::RustPickup`) need it; the vent metal pickups stay as they are.
//!
//! Rules:
//!
//! 1. **Chelation** (`chelate`): a rust pickup in the cloud loses Metals at a
//!    rate that grows with the free siderophore level there. Each Metal uses
//!    `SIDEROPHORE_PER_METAL` of free siderophore from the grid, and it goes
//!    into `FeSiderophore` at the pickup. A pickup with nothing left despawns.
//! 2. **Uptake** (`uptake_request`): a cell with `entF` (it has FepA) takes
//!    the complex at its position into its `Metals` pool, one Metal for each
//!    unit. A cell without the gene takes nothing.
//!
//! Every Metal that leaves a pickup comes into the grid, and every unit that
//! leaves the grid comes into a pool: Metals are conserved.
//!
//! Cost: ATP and CarbonSkeletons (chorismate comes from central carbon).

use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, Genome};
use crate::cell::Cell;
use crate::envgrid::EnvGrid;
use crate::environment::Channel;
use crate::metabolism::{CellTick, CurrencyPools, FlowLedger, FlowSource, FlowmapTimingConfig};
use crate::molecules::Currency;
use crate::particles::RustPickup;

use super::has_active_gene;

// --- Tuning. All of these need **playtest**. ---

/// What one turnover of the block uses.
pub const COSTS: &[(Currency, f32)] = &[(Currency::ATP, 1.0), (Currency::CarbonSkeletons, 0.6)];
/// Grid units of free siderophore that one turnover puts out.
pub const YIELD: f32 = 6.0;
/// Diffusion of the free siderophore. With `DECAY`, a scale of 2.2 grid
/// cells (about 70 px).
pub const DIFFUSION: f32 = 1.0;
/// Decay of the free siderophore: a half-life of about 3.5 s.
pub const DECAY: f32 = 0.2;
/// Diffusion of the iron complex.
pub const FE_DIFFUSION: f32 = 1.0;
/// Decay of the iron complex: it is stable, a half-life of about 35 s.
pub const FE_DECAY: f32 = 0.02;
/// Chelation rate per unit of free siderophore level, per second.
pub const CHELATE_RATE: f32 = 0.6;
/// Free siderophore that one Metal uses up (one molecule binds one iron).
pub const SIDEROPHORE_PER_METAL: f32 = 1.0;
/// Most complex that a cell takes in per second.
pub const FE_UPTAKE_PER_SEC: f32 = 2.0;
/// A pickup with less than this left despawns; the rest goes into the grid.
pub const EMPTY: f32 = 1e-3;

/// Metals that a rust pickup with `remaining` loses in `dt` seconds at a free
/// siderophore level `level`, before the siderophore limit.
pub fn chelate(remaining: f32, level: f32, dt: f32) -> f32 {
    let rate = CHELATE_RATE * level.max(0.0) * dt.max(0.0);
    remaining.max(0.0) * (1.0 - (-rate).exp())
}

/// Complex that a cell with the receptor asks the grid for in `dt` seconds.
pub fn uptake_request(dt: f32) -> f32 {
    FE_UPTAKE_PER_SEC * dt.max(0.0)
}

/// Can the cell take in the complex? It has the gene, and FepA with it.
pub fn has_receptor(genome: &Genome) -> bool {
    has_active_gene(genome, BlockKind::Siderophore)
}

/// What a pickup gives up in one step: the Metals that go into the complex,
/// from the free siderophore that the grid gave (`got`). Returns
/// `(metals, left)`: `left` is what stays in the pickup.
pub fn dissolve(remaining: f32, got_siderophore: f32) -> (f32, f32) {
    let metals = (got_siderophore / SIDEROPHORE_PER_METAL).min(remaining).max(0.0);
    let left = remaining - metals;
    if left < EMPTY { (remaining, 0.0) } else { (metals, left) }
}

/// Free siderophore dissolves the rust pickups in its cloud.
pub fn chelate_rust_system(
    timing: Res<FlowmapTimingConfig>,
    mut grid: ResMut<EnvGrid>,
    mut pickups: Query<(Entity, &Transform, &mut RustPickup)>,
    mut commands: Commands,
) {
    if grid.is_dormant(Channel::Siderophore) {
        return;
    }
    let dt = timing.target_delta();
    for (entity, transform, mut pickup) in &mut pickups {
        let position = transform.translation.truncate();
        let wanted = chelate(pickup.remaining, grid.sample(Channel::Siderophore, position), dt);
        if wanted <= 0.0 {
            continue;
        }
        let got = grid.take(Channel::Siderophore, position, wanted * SIDEROPHORE_PER_METAL);
        let (metals, left) = dissolve(pickup.remaining, got);
        if metals > 0.0 {
            grid.deposit(Channel::FeSiderophore, position, metals);
        }
        pickup.remaining = left;
        if left <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// A cell with the receptor takes in the iron complex as Metals.
pub fn siderophore_uptake_system(
    timing: Res<FlowmapTimingConfig>,
    mut grid: ResMut<EnvGrid>,
    mut cells: Query<
        (&Transform, &Genome, &mut CurrencyPools, &mut FlowLedger, &CellTick),
        With<Cell>,
    >,
) {
    if grid.is_dormant(Channel::FeSiderophore) {
        return;
    }
    for (transform, genome, mut pools, mut ledger, tick) in &mut cells {
        if !tick.due || !has_receptor(genome) {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let taken = grid.take(Channel::FeSiderophore, transform.translation.truncate(), uptake_request(dt));
        if taken > 0.0 {
            ledger.apply(&mut pools, FlowSource::Intake, Currency::Metals, taken);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::GeneState;

    #[test]
    fn chelation_grows_with_the_level_and_never_takes_more_than_there_is() {
        assert_eq!(chelate(2.0, 0.0, 1.0), 0.0, "no siderophore, no chelation");
        assert!(chelate(2.0, 3.0, 1.0) > chelate(2.0, 1.0, 1.0));
        assert!(chelate(2.0, 1e6, 1.0) <= 2.0);
        // Exact decay: two half steps equal one whole step.
        let whole = chelate(2.0, 1.5, 0.4);
        let first = chelate(2.0, 1.5, 0.2);
        let second = chelate(2.0 - first, 1.5, 0.2);
        assert!((whole - (first + second)).abs() < 1e-5);
    }

    #[test]
    fn dissolving_is_limited_by_the_siderophore_and_empties_the_pickup() {
        assert_eq!(dissolve(2.0, 0.5), (0.5 / SIDEROPHORE_PER_METAL, 2.0 - 0.5 / SIDEROPHORE_PER_METAL));
        assert_eq!(dissolve(2.0, 0.0), (0.0, 2.0));
        // Nearly empty: the rest goes too, so nothing is lost.
        let (metals, left) = dissolve(2.0, 2.0 - EMPTY / 2.0);
        assert_eq!((metals, left), (2.0, 0.0));
    }

    #[test]
    fn only_a_cell_with_the_gene_has_the_receptor() {
        let mut genome = Genome::default();
        assert!(!has_receptor(&genome));
        genome.table.insert(BlockKind::Siderophore, GeneState::Expressed);
        assert!(has_receptor(&genome));
        assert_eq!(uptake_request(0.5), FE_UPTAKE_PER_SEC * 0.5);
    }
}
