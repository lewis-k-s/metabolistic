//! # Slime (`pelA`)
//!
//! Many bacteria secrete extracellular polysaccharides (EPS), the slime of a
//! biofilm. The Pel polysaccharide of *Pseudomonas aeruginosa* (gene `pelA`)
//! is one. The matrix makes the water round the cells thick and protects them
//! from grazers.
//!
//! Rules:
//!
//! - A cell in slime has more drag: the field drag is multiplied by
//!   `drag_multiplier(level)` = `1 + MAX_EXTRA_DRAG x s / (HALF + s)`.
//! - The producer is exempt (a game rule: a real producer sits in its own
//!   matrix). A cell that carries `pelA` (any state except Silent) is not
//!   slowed.
//! - The slime is not used up. It only decays, slowly.
//!
//! Cost: ATP and CarbonSkeletons (a polysaccharide is made of sugars).

use crate::blocks::genome::{BlockKind, Genome};
use crate::molecules::Currency;

use super::has_active_gene;

// --- Tuning. All of these need **playtest**. ---

/// What one turnover of the block uses.
pub const COSTS: &[(Currency, f32)] = &[(Currency::ATP, 1.0), (Currency::CarbonSkeletons, 0.6)];
/// Grid units of slime that one turnover puts out.
pub const YIELD: f32 = 10.0;
/// Diffusion of the gel: slow.
pub const DIFFUSION: f32 = 0.3;
/// Decay per second: a half-life of about 14 s.
pub const DECAY: f32 = 0.05;
/// Share of the water drift that moves the gel.
pub const DRIFT_SHARE: f32 = 0.2;
/// Most extra drag: in thick slime the drag is `1 + MAX_EXTRA_DRAG` times the
/// normal drag.
pub const MAX_EXTRA_DRAG: f32 = 5.0;
/// Slime level at which the extra drag is half of its maximum.
pub const HALF: f32 = 6.0;

/// The factor on the drag of a cell at slime level `level`: 1 with no slime,
/// up to `1 + MAX_EXTRA_DRAG`.
pub fn drag_multiplier(level: f32) -> f32 {
    let s = level.max(0.0);
    1.0 + MAX_EXTRA_DRAG * s / (HALF + s)
}

/// Is the cell exempt from slime? It makes slime itself.
pub fn exempt(genome: &Genome) -> bool {
    has_active_gene(genome, BlockKind::Slime)
}

/// The drag factor of a cell: 1 for a producer, else `drag_multiplier`.
pub fn cell_drag_multiplier(level: f32, genome: &Genome) -> f32 {
    if exempt(genome) { 1.0 } else { drag_multiplier(level) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::GeneState;

    #[test]
    fn drag_grows_with_the_level_and_saturates() {
        assert_eq!(drag_multiplier(0.0), 1.0);
        assert_eq!(drag_multiplier(-3.0), 1.0);
        assert!((drag_multiplier(HALF) - (1.0 + MAX_EXTRA_DRAG / 2.0)).abs() < 1e-6);
        assert!(drag_multiplier(10.0) > drag_multiplier(5.0));
        assert!(drag_multiplier(1e6) < 1.0 + MAX_EXTRA_DRAG + 1e-3);
    }

    #[test]
    fn the_producer_is_not_slowed() {
        let mut genome = Genome::default();
        assert!(cell_drag_multiplier(20.0, &genome) > 1.0);
        genome.table.insert(BlockKind::Slime, GeneState::Expressed);
        assert_eq!(cell_drag_multiplier(20.0, &genome), 1.0);
    }
}
