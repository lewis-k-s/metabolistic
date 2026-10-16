//! # Stages
//!
//! A run is a sequence of stages (`STAGES`). A stage is data: `Stage::level_def`
//! gives the `LevelDef` that the world spawns from. A stage without a
//! `LevelDef` is not built yet; the run is complete when the next stage has
//! none. All three stages have a level now, so winning land completes the run
//! (a fourth stage would only need a new `Stage` and a `LevelDef`).

use crate::level::LevelDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Vents,
    ShallowWater,
    Land,
}

/// The order of a run.
pub const STAGES: [Stage; 3] = [Stage::Vents, Stage::ShallowWater, Stage::Land];

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Vents => "Deep-sea vents",
            Stage::ShallowWater => "Shallow water",
            Stage::Land => "Land",
        }
    }

    /// The level of this stage for a run seed. `None` for a stage that is not
    /// built.
    pub fn level_def(self, run_seed: u64) -> Option<LevelDef> {
        match self {
            Stage::Vents => Some(crate::layout::generate_vents(stage_seed(run_seed, 0))),
            Stage::ShallowWater => Some(crate::layout::generate_shallow(stage_seed(run_seed, 1))),
            Stage::Land => Some(crate::layout::generate_land(stage_seed(run_seed, 2))),
        }
    }

    pub fn is_playable(self) -> bool {
        self.level_def(0).is_some()
    }
}

/// The seed of one stage of a run. Stages of one run get different seeds.
pub fn stage_seed(run_seed: u64, stage_index: usize) -> u64 {
    run_seed ^ (stage_index as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03)
}

/// The index of the stage after `current` if it can be played, else `None`
/// (the run is complete). `playable` says if a stage has a level.
pub fn next_stage_index(current: usize, playable: impl Fn(Stage) -> bool) -> Option<usize> {
    let next = current + 1;
    STAGES.get(next).copied().filter(|s| playable(*s)).map(|_| next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_a_level() {
        assert!(Stage::Vents.level_def(5).is_some());
        assert!(Stage::ShallowWater.level_def(5).is_some());
        assert!(Stage::Land.level_def(5).is_some());
        assert_eq!(Stage::Land.level_def(5), Stage::Land.level_def(5));
        assert_eq!(Stage::Vents.level_def(5), Stage::Vents.level_def(5));
        assert_eq!(Stage::ShallowWater.level_def(5), Stage::ShallowWater.level_def(5));
    }

    #[test]
    fn the_run_is_complete_when_the_next_stage_has_no_level() {
        assert_eq!(next_stage_index(0, Stage::is_playable), Some(1));
        assert_eq!(next_stage_index(1, Stage::is_playable), Some(2), "land follows shallow water");
        assert_eq!(next_stage_index(2, Stage::is_playable), None, "land is the last stage");
        assert_eq!(next_stage_index(0, |_| false), None);
        assert_eq!(next_stage_index(1, |s| s != Stage::Land), None, "a stage without a level ends the run");
        assert_eq!(next_stage_index(2, |_| true), None, "no stage after the last");
    }

    #[test]
    fn stage_seeds_differ() {
        assert_ne!(stage_seed(1, 0), stage_seed(1, 1));
        assert_ne!(stage_seed(1, 0), stage_seed(2, 0));
    }
}
