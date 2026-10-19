//! # AI-2 call (`luxS`)
//!
//! Quorum sensing: each cell releases a small signal, and when the local level
//! is high, the cells switch genes on together. LuxS makes autoinducer-2
//! (AI-2) from the methyl cycle: SAM gives SAH, Pfs makes
//! S-ribosylhomocysteine, and LuxS makes DPD, which closes into AI-2. Many
//! species sense AI-2. *E. coli* swims towards AI-2 (receptor Tsr with LsrB)
//! and forms clumps (Laganenka et al. 2016). Clumps are too large for many
//! grazers to eat.
//!
//! This is the "chemokine" of the game: a chemical call that brings helper
//! cells to a place and changes what they do.
//!
//! Rules:
//!
//! - **Recruit** (`recruit`): a symbiont with no player in sight swims up the
//!   AI-2 gradient above `SENSE`.
//! - **Quorum** (`in_quorum`): at a level of `QUORUM` or more, a symbiont
//!   closes in to contact (`follow_gap`) and the symbiont exchange runs
//!   `QUORUM_RATE_MULTIPLIER` times faster (`exchange_rate`).
//! - **Clump** (`clump_radius`): the player and the symbionts in quorum that
//!   touch it count as one clump for the engulf size rule (`Clump`).
//! - **Eavesdrop** (a game rule, `eavesdrop`): a predator with no prey in
//!   sight swims up the AI-2 gradient above `EAVESDROP`. The real case is
//!   *Myxococcus xanthus*, which senses the AHL signals of its prey. The call
//!   brings your allies and your enemy.
//!
//! Cost: ATP and CarbonSkeletons (the ribose of SAM came from ATP).

use bevy::prelude::*;

use crate::cell::{Cell, CellSize};
use crate::envgrid::EnvGrid;
use crate::environment::Channel;
use crate::molecules::Currency;
use crate::movement::world_radius;
use crate::npc::{Archetype, Npc, SYMBIONT_CONTACT_GAP, SYMBIONT_RATE};

// --- Tuning. All of these need **playtest**. ---

/// What one turnover of the block uses.
pub const COSTS: &[(Currency, f32)] = &[(Currency::ATP, 0.5), (Currency::CarbonSkeletons, 0.2)];
/// Grid units of AI-2 that one turnover puts out.
pub const YIELD: f32 = 5.0;
/// Diffusion: a small molecule, fast. With `DECAY`, a scale of 2.8 grid
/// cells (about 90 px).
pub const DIFFUSION: f32 = 2.0;
/// Decay per second: a half-life of about 2.8 s.
pub const DECAY: f32 = 0.25;
/// Level above which a symbiont senses the call.
pub const SENSE: f32 = 0.03;
/// Level at which a symbiont is in quorum. A symbiont in contact with a still
/// caller with one enzyme sees about 0.4.
pub const QUORUM: f32 = 0.3;
/// Level above which a predator follows the call: far out in the cloud.
pub const EAVESDROP: f32 = 0.08;
/// Factor on the symbiont exchange in quorum.
pub const QUORUM_RATE_MULTIPLIER: f32 = 3.0;
/// Share of a symbiont's area that adds to the clump (cells in a clump do not
/// fill all the space).
pub const PACKING: f32 = 0.5;

/// Is a symbiont at this level in quorum?
pub fn in_quorum(level: f32) -> bool {
    level >= QUORUM
}

/// The gap (pixels) at which a symbiont stops moving towards the player: 0 in
/// quorum (contact), else `follow_gap`.
pub fn follow_gap(level: f32, normal: f32) -> f32 {
    if in_quorum(level) { 0.0 } else { normal }
}

/// The rate of the symbiont exchange at an AI-2 level.
pub fn exchange_rate(level: f32) -> f32 {
    if in_quorum(level) {
        SYMBIONT_RATE * QUORUM_RATE_MULTIPLIER
    } else {
        SYMBIONT_RATE
    }
}

/// The direction up the gradient above `threshold`, or `None`.
fn up_gradient(level: f32, gradient: Vec2, threshold: f32) -> Option<Vec2> {
    (level > threshold && gradient.length_squared() > 0.0).then(|| gradient.normalize())
}

/// Where a symbiont with no player goes: up the call.
pub fn recruit(level: f32, gradient: Vec2) -> Option<Vec2> {
    up_gradient(level, gradient, SENSE)
}

/// Where a predator with no prey in sight goes: up the call.
pub fn eavesdrop(level: f32, gradient: Vec2) -> Option<Vec2> {
    up_gradient(level, gradient, EAVESDROP)
}

/// The radius of a clump: a cell of radius `own` and the symbionts of radii
/// `others` that stick to it. Areas add, times `PACKING` for the symbionts.
pub fn clump_radius(own: f32, others: &[f32]) -> f32 {
    let extra: f32 = others.iter().map(|r| r * r).sum();
    (own * own + PACKING * extra).sqrt()
}

/// What a clump adds to a cell's radius in the engulf size rule (pixels).
/// Not the body: only the size check.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct Clump(pub f32);

/// Sets the `Clump` of each cell that is not an NPC (the player): the
/// symbionts in quorum that touch it.
#[allow(clippy::type_complexity)]
pub fn clump_system(
    grid: Res<EnvGrid>,
    symbionts: Query<(&Npc, &Transform, &CellSize), With<Cell>>,
    mut cells: Query<(&Transform, &CellSize, &mut Clump), (With<Cell>, Without<Npc>)>,
) {
    let quorum = !grid.is_dormant(Channel::Autoinducer);
    for (transform, size, mut clump) in &mut cells {
        let position = transform.translation.truncate();
        let own = world_radius(size);
        let others: Vec<f32> = if quorum {
            symbionts
                .iter()
                .filter(|(npc, other, other_size)| {
                    let other_position = other.translation.truncate();
                    npc.archetype == Archetype::Symbiont
                        && in_quorum(grid.sample(Channel::Autoinducer, other_position))
                        && position.distance(other_position)
                            <= own + world_radius(other_size) + SYMBIONT_CONTACT_GAP
                })
                .map(|(_, _, other_size)| world_radius(other_size))
                .collect()
        } else {
            Vec::new()
        };
        let bonus = clump_radius(own, &others) - own;
        if clump.0 != bonus {
            clump.0 = bonus;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engulf::can_engulf;

    #[test]
    fn quorum_closes_the_gap_and_speeds_the_exchange() {
        assert!(!in_quorum(QUORUM - 0.01));
        assert!(in_quorum(QUORUM));
        assert_eq!(follow_gap(0.0, 5.0), 5.0);
        assert_eq!(follow_gap(QUORUM, 5.0), 0.0);
        assert_eq!(exchange_rate(0.0), SYMBIONT_RATE);
        assert_eq!(exchange_rate(QUORUM * 2.0), SYMBIONT_RATE * QUORUM_RATE_MULTIPLIER);
    }

    #[test]
    fn the_call_is_followed_only_above_its_threshold() {
        let up = Vec2::new(0.0, 3.0);
        assert_eq!(recruit(SENSE * 2.0, up), Some(Vec2::Y));
        assert_eq!(recruit(SENSE * 0.5, up), None);
        assert_eq!(recruit(1.0, Vec2::ZERO), None, "no gradient, no direction");
        assert_eq!(eavesdrop(EAVESDROP * 0.5, up), None);
        assert_eq!(eavesdrop(EAVESDROP * 2.0, up), Some(Vec2::Y));
    }

    #[test]
    fn a_clump_of_two_symbionts_stops_the_predator() {
        let (player, symbiont, predator) = (20.0, 9.5, 25.0);
        assert!(can_engulf(predator, player), "alone, the player is food");
        assert_eq!(clump_radius(player, &[]), player);
        let clump = clump_radius(player, &[symbiont, symbiont]);
        assert!(clump > player);
        assert!(!can_engulf(predator, clump), "a clump is too big");
    }
}
