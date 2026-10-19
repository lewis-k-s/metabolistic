//! # Microcin (`cvaC`)
//!
//! Microcin V is a small peptide bacteriocin of *E. coli*. A type I pump
//! (CvaA, CvaB and TolC) exports it, so the producer stays alive. A target
//! takes it in through its outer-membrane receptor (Cir, with TonB), and it
//! breaks the target's inner membrane. The producer makes an immunity protein
//! (`cvi`) from the next gene. A microcin acts only on close Gram-negative
//! relatives, which have the receptor.
//!
//! Rules:
//!
//! - **Who it hits** (`susceptible`): prey only. The predator (a protist
//!   grazer), the gate cell (a green sulfur bacterium) and the symbiont have no
//!   receptor. The player is not a target either.
//! - **Immunity**: a cell that carries `cvaC` (any state except Silent) takes
//!   nothing.
//! - **Uptake** (`uptake_request`): a target takes `level x (1 - exp(-UPTAKE
//!   x dt))` from the grid. The cloud is used up as it kills.
//! - **Damage** (`damage`): `taken x DAMAGE_PER_UNIT` membrane points.
//!   Shelters do not stop it: it is a chemical, not heat.
//! - **Avoidance**: prey steer away from a level above `AVOID` (see `npc`).
//!
//! Cost: ATP (the pump) and AminoAcids (it is a ribosomal peptide). No Metals:
//! a microcin has no metal cofactor.

use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, Genome};
use crate::cell::{Cell, DamageSource, DamageTaken};
use crate::envgrid::EnvGrid;
use crate::environment::Channel;
use crate::membrane::Membrane;
use crate::metabolism::{CellTick, FlowmapTimingConfig};
use crate::molecules::Currency;
use crate::npc::{Archetype, Npc};

use super::has_active_gene;

// --- Tuning. All of these need **playtest**. ---

/// What one turnover of the block uses.
pub const COSTS: &[(Currency, f32)] = &[(Currency::ATP, 1.5), (Currency::AminoAcids, 0.4)];
/// Grid units of microcin that one turnover puts out.
pub const YIELD: f32 = 10.0;
/// Diffusion of the cloud (see `envgrid::FieldParams`). With `DECAY` the
/// cloud round a still cell has a scale of `sqrt(D / k)` = 2.2 grid cells
/// (about 70 px): a thick, local cloud.
pub const DIFFUSION: f32 = 1.5;
/// Decay per second: a half-life of about 2.3 s.
pub const DECAY: f32 = 0.3;
/// Share of the local level that a target takes up per second (a rate: the
/// rule uses `1 - exp(-UPTAKE x dt)`).
pub const UPTAKE: f32 = 1.0;
/// Membrane points for each unit of microcin taken up. Next to a still
/// producer the level is about 1.2, so a stunned prey there loses about 15
/// points per second.
pub const DAMAGE_PER_UNIT: f32 = 12.0;
/// Level above which prey swim away from the cloud.
pub const AVOID: f32 = 0.3;

/// Can microcin hit a cell of this kind? `None` is the player's cell.
pub fn susceptible(archetype: Option<Archetype>) -> bool {
    matches!(archetype, Some(Archetype::Prey))
}

/// Is the cell immune? It carries the gene, and the immunity gene with it.
pub fn immune(genome: &Genome) -> bool {
    has_active_gene(genome, BlockKind::Microcin)
}

/// Microcin that a target at `level` asks the grid for in `dt` seconds.
pub fn uptake_request(level: f32, dt: f32) -> f32 {
    level.max(0.0) * (1.0 - (-UPTAKE * dt.max(0.0)).exp())
}

/// Membrane damage from `taken` units of microcin.
pub fn damage(taken: f32) -> f32 {
    taken.max(0.0) * DAMAGE_PER_UNIT
}

/// Susceptible cells in a cloud take up microcin and lose membrane.
#[allow(clippy::type_complexity)]
pub fn microcin_system(
    timing: Res<FlowmapTimingConfig>,
    mut grid: ResMut<EnvGrid>,
    mut cells: Query<
        (&Transform, &Genome, Option<&Npc>, &mut Membrane, &mut DamageTaken, &CellTick),
        With<Cell>,
    >,
) {
    if grid.is_dormant(Channel::Microcin) {
        return;
    }
    for (transform, genome, npc, mut membrane, mut taken_damage, tick) in &mut cells {
        if !tick.due || !susceptible(npc.map(|n| n.archetype)) || immune(genome) {
            continue;
        }
        let position = transform.translation.truncate();
        let dt = tick.dt(timing.target_delta());
        let request = uptake_request(grid.sample(Channel::Microcin, position), dt);
        if request <= 0.0 {
            continue;
        }
        let taken = grid.take(Channel::Microcin, position, request);
        let hurt = damage(taken);
        if hurt > 0.0 {
            membrane.damage(hurt);
            taken_damage.add(DamageSource::Microcin, hurt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::GeneState;

    #[test]
    fn only_prey_are_targets() {
        assert!(susceptible(Some(Archetype::Prey)));
        assert!(!susceptible(Some(Archetype::Predator)), "a protist has no receptor");
        assert!(!susceptible(Some(Archetype::Symbiont)));
        assert!(!susceptible(Some(Archetype::Gate)));
        assert!(!susceptible(None), "the player");
    }

    #[test]
    fn the_gene_gives_immunity_unless_silent() {
        let mut genome = Genome::default();
        assert!(!immune(&genome));
        genome.table.insert(BlockKind::Microcin, GeneState::Silent);
        assert!(!immune(&genome));
        genome.table.insert(BlockKind::Microcin, GeneState::Mutated);
        assert!(immune(&genome));
    }

    #[test]
    fn uptake_does_not_depend_on_the_tick_length() {
        // Two half ticks at a level that falls by what the first took equal
        // one whole tick.
        let level = 3.0;
        let whole = uptake_request(level, 0.4);
        let first = uptake_request(level, 0.2);
        let second = uptake_request(level - first, 0.2);
        assert!((whole - (first + second)).abs() < 1e-5);
        assert!(whole < level, "never more than the level");
        assert_eq!(uptake_request(0.0, 1.0), 0.0);
        assert_eq!(damage(2.0), 2.0 * DAMAGE_PER_UNIT);
    }
}
