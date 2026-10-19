//! The level files in `assets/levels`: each parses, builds and passes the
//! layout rules; the classic files are the stage base levels; a scenario can
//! name a level.

use metabolistic3d::environment::Channel;
use metabolistic3d::layout::validate;
use metabolistic3d::level_file::{self, LevelFile, stage_base};
use metabolistic3d::scenario::{Scenario, assets_dir};
use metabolistic3d::stage::{STAGES, Stage};

fn all_levels() -> Vec<(String, LevelFile)> {
    let found = level_file::list(&assets_dir());
    assert!(!found.is_empty(), "no level files in assets/levels");
    found
        .into_iter()
        .map(|(id, path)| {
            let file = level_file::load(&path).unwrap_or_else(|e| panic!("{e}"));
            (id, file)
        })
        .collect()
}

#[test]
fn every_level_file_builds_and_passes_the_layout_rules() {
    for (id, file) in all_levels() {
        let def = file.to_def().unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(validate(&def), Ok(()), "{id}");
        assert!(id.starts_with(match file.stage {
            Stage::Vents => "vents/",
            Stage::ShallowWater => "shallow/",
            Stage::Land => "land/",
        }), "{id} is in the folder of another stage");
        // Every catalog entry is used, so the roster in the viewer is true.
        for (mob, _, count) in file.roster() {
            assert!(count > 0, "{id}: mob {mob} has no spawn");
        }
    }
}

#[test]
fn every_vent_level_has_minerals_and_a_start_that_does_not_burn() {
    for (id, file) in all_levels().into_iter().filter(|(_, f)| f.stage == Stage::Vents) {
        let def = file.to_def().unwrap();
        let grid = def.grid();
        assert!(grid.total(Channel::Minerals) > 0.0, "{id}");
        assert!(grid.sample(Channel::Heat, def.start) < metabolistic3d::exchange::HEAT_DAMAGE_THRESHOLD, "{id}: the start burns");
    }
}

#[test]
fn the_classic_files_are_the_stage_base_levels() {
    for stage in STAGES {
        let folder = match stage {
            Stage::Vents => "vents",
            Stage::ShallowWater => "shallow",
            Stage::Land => "land",
        };
        let mut def = level_file::load_def(&format!("{folder}/classic"), &assets_dir()).unwrap();
        let base = stage_base(stage);
        def.name = base.name.clone();
        assert_eq!(def, base, "{folder}/classic.json is out of date: run `cargo levels export-bases`");
    }
}

#[test]
fn a_scenario_can_name_a_level() {
    let scenario = Scenario { level: Some("vents/plume_river".into()), ..Default::default() };
    let (stage, def) = scenario.level_file(&assets_dir()).unwrap().unwrap();
    assert_eq!(stage, Stage::Vents);
    let mut run = scenario.begin_run();
    run.level = Some(def.clone());
    assert_eq!(run.level_def(), Some(def));

    let missing = Scenario { level: Some("vents/no_such_level".into()), ..Default::default() };
    assert!(missing.level_file(&assets_dir()).is_err());
    assert_eq!(Scenario::default().level_file(&assets_dir()), Ok(None));
}

#[test]
fn the_built_in_vent_levels_equal_the_files() {
    for (id, text) in level_file::VENT_LEVELS {
        let disk = std::fs::read_to_string(level_file::resolve_path(id, &assets_dir())).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(text, disk, "{id}: the built-in copy is old; rebuild after editing a level file");
        assert_eq!(LevelFile::from_json(text).unwrap().stage, Stage::Vents, "{id}");
    }
}

#[test]
fn the_first_stage_picks_a_built_in_level_from_the_seed() {
    let mut seen = std::collections::HashSet::new();
    for seed in 0..64u64 {
        let def = Stage::Vents.level_def(seed).unwrap();
        assert_eq!(validate(&def), Ok(()), "seed {seed}");
        assert_eq!(def, Stage::Vents.level_def(seed).unwrap(), "same seed, same level");
        seen.insert(def.name.clone());
    }
    assert_eq!(seen.len(), level_file::VENT_LEVELS.len(), "every built-in level is reachable");
}

#[test]
fn the_vent_levels_use_shapes_and_their_rock_is_in_the_grid_mask() {
    use metabolistic3d::walls::WallKind;
    for (id, text) in level_file::VENT_LEVELS {
        let file = LevelFile::from_json(text).unwrap();
        assert!(!file.shapes.is_empty(), "{id}: no shapes");
        let def = file.to_def().unwrap();
        let rock = def.wall_tiles().into_iter().filter(|(_, k)| *k == WallKind::Rock).count();
        assert!(rock > 0 || id == "vents/mat_terraces", "{id}: no rock tiles");
        let grid = def.grid();
        for ((x, y), kind) in def.wall_tiles() {
            if kind == WallKind::Rock {
                assert!(grid.is_wall(x, y), "{id}: rock tile ({x}, {y}) is not blocked in the grid");
            }
        }
    }
}
