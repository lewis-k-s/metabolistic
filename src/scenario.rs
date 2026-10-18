//! # Scenarios: launch a run with a prepared cell
//!
//! A scenario is a JSON file that describes the start of a run: the stage, the
//! seed, the cell (genes, enzymes, membrane proteins, action slots), the pools,
//! and a few test flags. The game opens in `GameState::Run` with that cell, and
//! skips the menu and the Prepare screen. Use it to test one mechanic fast.
//!
//! ## Launch
//!
//! - `METABOLISTIC_SCENARIO=vent_motile cargo run`
//! - `cargo run -- --scenario vent_motile` (the argument wins over the variable)
//!
//! A bare name (no `/`, no `.json`) is the file `assets/scenarios/<name>.json`.
//! Anything else is a path. A file that is missing or does not parse stops the
//! game at start with the error. It never falls back to the menu.
//!
//! A scenario run never writes the real codex file: the codex stays in memory.
//!
//! ## File format
//!
//! Every field is optional. A missing field takes the starter value.
//!
//! ```json
//! { "stage": "Vents", "membrane": { "Flagellum": 3 } }
//! ```
//!
//! | Field          | Meaning                                                                     |
//! |----------------|-----------------------------------------------------------------------------|
//! | `stage`        | `Vents` (default), `ShallowWater` or `Land`. `Vent` and `Shallow` also work. |
//! | `seed`         | Run seed (default 1), so the level is the same every time.                  |
//! | `spawn`        | `[x, y]`: where the cell starts. Default: the start of the level.           |
//! | `genes`        | Gene states that replace the starter ones: `{"Catalase": "Expressed"}`.     |
//! | `enzymes`      | Enzyme counts that replace the starter ones: `{"Catalase": 1}`.             |
//! | `membrane`     | Membrane protein counts: `{"Flagellum": 3}`. The starter cell has none.     |
//! | `actions`      | The action slots (LMB, RMB, M4, M5): `["Pilus", null]`. Default: starter bar. |
//! | `pools`        | Pool values that replace the starter ones: `{"ATP": 400}`.                  |
//! | `codex`        | Genes to put into the (in-memory) codex.                                    |
//! | `invulnerable` | The cell cannot die or be engulfed.                                         |
//! | `level`        | A level file for the first stage: `"vents/chimney_forest"` (see `level_file`). Its stage replaces `stage`. |
//!
//! Slots hold active skills only. Pathway levels are set on the metabolic map
//! during the run.
//!
//! ## Add a preset
//!
//! Put a new file in `assets/scenarios/`. The test `every_preset_loads` finds
//! it by itself and checks that it parses and that the cell can be built.
//!
//! ## Save a played state
//!
//! In a full build, F9 writes the state of the player's cell as a scenario to
//! `scenario-dump.json` in the save directory (see `codex`). Copy it to
//! `assets/scenarios/` to keep it, or pass its path to `METABOLISTIC_SCENARIO`.
//!
//! The functions here are pure. Systems fetch the data, call them, and apply
//! the result.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::actions::{ActionBar, ActionKind, SLOT_COUNT};
use crate::blocks::genome::{BlockKind, GeneState};
use crate::codex::{Codex, CodexStore};
use crate::molecules::{Currency, CurrencyPools};
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::MembraneProtein;
use crate::run::{Carry, RunState, starter_carry};
use crate::stage::{STAGES, Stage};

/// Environment variable that names the scenario.
pub const SCENARIO_VAR: &str = "METABOLISTIC_SCENARIO";
/// Command line option that names the scenario.
pub const SCENARIO_ARG: &str = "--scenario";
/// The folder of the presets, below the assets folder.
pub const PRESET_DIR: &str = "scenarios";
/// Name of the file that the dump key writes.
pub const DUMP_FILE: &str = "scenario-dump.json";
/// The run seed of a scenario that gives none.
pub const DEFAULT_SEED: u64 = 1;

/// The start of a run. See the module docs for the file format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scenario {
    pub stage: Stage,
    pub seed: u64,
    pub spawn: Option<[f32; 2]>,
    pub genes: BTreeMap<BlockKind, GeneState>,
    pub enzymes: BTreeMap<BlockKind, u32>,
    pub membrane: BTreeMap<MembraneProtein, u32>,
    pub actions: Option<Vec<Option<ActionKind>>>,
    pub pools: BTreeMap<Currency, f32>,
    pub codex: Vec<BlockKind>,
    pub invulnerable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            stage: Stage::default(),
            seed: DEFAULT_SEED,
            spawn: None,
            genes: BTreeMap::new(),
            enzymes: BTreeMap::new(),
            membrane: BTreeMap::new(),
            actions: None,
            pools: BTreeMap::new(),
            codex: Vec::new(),
            invulnerable: false,
            level: None,
        }
    }
}

impl Scenario {
    /// Parses a scenario from JSON text.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The scenario as JSON text.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The cell that the run starts with: the starter cell, changed by the
    /// scenario's genes, enzymes, membrane proteins and actions.
    pub fn carry(&self) -> Carry {
        let mut carry = starter_carry(&Default::default());
        for (kind, state) in &self.genes {
            carry.genome.table.insert(*kind, state.clone());
        }
        for (kind, count) in &self.enzymes {
            carry.proteome.enzymes.insert(*kind, *count);
        }
        for (protein, count) in &self.membrane {
            carry.proteome.membrane.insert(*protein, *count);
        }
        if let Some(actions) = &self.actions {
            carry.action_bar = action_bar_from(actions);
        }
        carry
    }

    /// The pool values of the cell: the defaults, changed by `pools`.
    pub fn start_pools(&self) -> CurrencyPools {
        let mut pools = CurrencyPools::with_defaults();
        for (currency, amount) in &self.pools {
            pools.set(*currency, *amount);
        }
        pools
    }

    /// The level file of the scenario, read and built. `None` if it names none.
    pub fn level_file(&self, assets_dir: &Path) -> Result<Option<(Stage, crate::level::LevelDef)>, String> {
        let Some(id) = &self.level else { return Ok(None) };
        let path = crate::level_file::resolve_path(id, assets_dir);
        let file = crate::level_file::load(&path)?;
        let def = file.to_def().map_err(|e| format!("level {}: {e}", path.display()))?;
        Ok(Some((file.stage, def)))
    }

    /// The run state for the first frame: stage, seed and cell.
    pub fn begin_run(&self) -> RunState {
        let mut run = RunState::begin(self.seed, Default::default());
        run.stage_index = STAGES.iter().position(|s| *s == self.stage).unwrap_or(0);
        run.carry = Some(self.carry());
        run.spawn = self.spawn.map(Vec2::from);
        run
    }

    /// The scenario that describes a cell now. Every gene, enzyme and protein
    /// is written, so loading it gives the same cell (the starter values only
    /// remain for what the file leaves out).
    pub fn from_state(
        stage: Stage,
        seed: u64,
        carry: &Carry,
        pools: &CurrencyPools,
        position: Vec2,
        invulnerable: bool,
    ) -> Self {
        let mut enzymes: BTreeMap<_, _> = starter_carry(&Default::default())
            .proteome
            .enzymes
            .keys()
            .map(|k| (*k, 0))
            .collect();
        enzymes.extend(carry.proteome.enzymes.iter().map(|(k, n)| (*k, *n)));
        Self {
            stage,
            seed,
            spawn: Some(position.to_array()),
            genes: carry.genome.table.iter().map(|(k, s)| (*k, s.clone())).collect(),
            enzymes,
            membrane: carry.proteome.membrane.iter().map(|(k, n)| (*k, *n)).collect(),
            actions: Some(carry.action_bar.slots.to_vec()),
            pools: pools.pools.iter().map(|(k, v)| (*k, *v)).collect(),
            codex: Vec::new(),
            invulnerable,
            level: None,
        }
    }
}

/// The action bar of a list of slots. Extra entries are dropped; missing ones
/// are empty.
pub fn action_bar_from(actions: &[Option<ActionKind>]) -> ActionBar {
    let mut bar = ActionBar::default();
    for (slot, action) in bar.slots.iter_mut().zip(actions.iter().take(SLOT_COUNT)) {
        *slot = *action;
    }
    bar
}

/// The scenario name from the command line (`--scenario x` or `--scenario=x`),
/// else from the environment value. The command line wins.
pub fn scenario_name(args: &[String], env: Option<&str>) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == SCENARIO_ARG {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix("--scenario=") {
            return Some(value.to_string());
        }
    }
    env.filter(|v| !v.is_empty()).map(str::to_string)
}

/// The file of a scenario name. A bare name is a preset in `assets_dir`;
/// a name with a separator or a `.json` ending is a path.
pub fn resolve_path(name: &str, assets_dir: &Path) -> PathBuf {
    if name.contains('/') || name.contains('\\') || name.ends_with(".json") {
        PathBuf::from(name)
    } else {
        assets_dir.join(PRESET_DIR).join(format!("{name}.json"))
    }
}

/// The assets folder: `CARGO_MANIFEST_DIR/assets` if cargo runs the game, else
/// `assets` in the working directory.
pub fn assets_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(|d| PathBuf::from(d).join("assets"))
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// Reads and parses a scenario file. The error text names the file.
pub fn load(path: &Path) -> Result<Scenario, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read scenario {}: {e}", path.display()))?;
    Scenario::from_json(&text).map_err(|e| format!("cannot parse scenario {}: {e}", path.display()))
}

/// The scenario that the command line or the environment asks for. `None` if
/// none is asked for. Panics with the error if the file is bad: a scenario run
/// must never fall back to the normal start.
///
/// `--level x` (or `METABOLISTIC_LEVEL=x`) alone asks for the default scenario
/// on that level; with a scenario it replaces the scenario's level.
pub fn requested() -> Option<Scenario> {
    use crate::level_file::{LEVEL_VAR, level_name};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = std::env::var(SCENARIO_VAR).ok();
    let level = level_name(&args, std::env::var(LEVEL_VAR).ok().as_deref());
    let mut scenario = match scenario_name(&args, env.as_deref()) {
        Some(name) => match load(&resolve_path(&name, &assets_dir())) {
            Ok(scenario) => scenario,
            Err(err) => panic!("{err}"),
        },
        None if level.is_some() => Scenario::default(),
        None => return None,
    };
    if level.is_some() {
        scenario.level = level;
    }
    Some(scenario)
}

/// The scenario that the game starts with. Present from start up to the end of
/// loading.
#[derive(Resource, Debug, Clone)]
pub struct ScenarioLaunch(pub Scenario);

/// Pool values and flags that the first cell of the scenario still needs. The
/// cell does not exist until the world is built, so `apply_pending_cell` waits
/// for it.
#[derive(Resource, Debug, Clone)]
pub struct PendingScenarioCell {
    pub pools: CurrencyPools,
    pub invulnerable: bool,
}

/// Begins the run of a scenario. Called once, by the Loading screen. The codex
/// stays in memory.
pub fn start(
    scenario: &Scenario,
    commands: &mut Commands,
    codex: &mut Codex,
    store: &mut CodexStore,
) {
    info!("scenario: starting a run at {:?}, seed {}", scenario.stage, scenario.seed);
    store.path = None;
    codex.bank(&scenario.codex);
    let mut run = scenario.begin_run();
    // A bad level file stops the game, like a bad scenario file.
    match scenario.level_file(&assets_dir()) {
        Ok(Some((stage, def))) => {
            info!("scenario: level {:?} ({})", scenario.level, def.name);
            run.stage_index = STAGES.iter().position(|s| *s == stage).unwrap_or(0);
            run.level = Some(def);
        }
        Ok(None) => {}
        Err(err) => panic!("{err}"),
    }
    commands.insert_resource(run);
    commands.insert_resource(PendingScenarioCell {
        pools: scenario.start_pools(),
        invulnerable: scenario.invulnerable,
    });
}

/// Applies the pool values and the flags to the new player cell, once.
pub fn apply_pending_cell(
    mut commands: Commands,
    pending: Res<PendingScenarioCell>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
) {
    let Ok(controlled) = players.single() else {
        return;
    };
    let mut cell = commands.entity(controlled.0);
    cell.insert(pending.pools.clone());
    if pending.invulnerable {
        cell.insert(crate::engulf::Invulnerable);
    }
    commands.remove_resource::<PendingScenarioCell>();
}

pub struct ScenarioPlugin;

impl Plugin for ScenarioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_pending_cell.run_if(resource_exists::<PendingScenarioCell>));
    }
}

/// Writes a scenario to a file, and makes the directory if needed.
pub fn save(scenario: &Scenario, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = scenario.to_json().map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::starter_action_bar;
    use crate::codex::test_dir::TempDir;
    use crate::blocks::genome::create_starter_genome;
    use crate::proteins::{ProteinTarget, Proteome};

    fn preset_files() -> Vec<PathBuf> {
        let dir = assets_dir().join(PRESET_DIR);
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn a_short_file_takes_the_starter_values() {
        let scenario = Scenario::from_json(r#"{"stage":"Vent","membrane":{"Flagellum":3}}"#).unwrap();
        assert_eq!(scenario.stage, Stage::Vents);
        assert_eq!(scenario.seed, DEFAULT_SEED);
        let carry = scenario.carry();
        assert_eq!(carry.proteome.count(ProteinTarget::Membrane(MembraneProtein::Flagellum)), 3);
        assert_eq!(carry.proteome.enzymes, crate::shared::starter_proteome().enzymes);
        assert_eq!(carry.action_bar, starter_action_bar());
        assert_eq!(carry.genome.table.iter().map(|(k, s)| (*k, s.clone())).collect::<BTreeMap<_, _>>(), create_starter_genome().table.into_iter().collect());
    }

    #[test]
    fn an_empty_object_is_the_default_scenario() {
        assert_eq!(Scenario::from_json("{}").unwrap(), Scenario::default());
    }

    #[test]
    fn a_bad_file_gives_an_error_that_names_the_problem() {
        for text in ["", "not json", r#"{"stage":"Moon"}"#, r#"{"membran":{"Flagellum":1}}"#, r#"{"membrane":{"Wing":1}}"#] {
            assert!(Scenario::from_json(text).is_err(), "{text:?}");
        }
        let dir = TempDir::new();
        let path = dir.0.join("bad.json");
        std::fs::write(&path, r#"{"stage":"Moon"}"#).unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("bad.json") && err.contains("Moon"), "{err}");
        assert!(load(&dir.0.join("none.json")).unwrap_err().contains("cannot read"));
    }

    #[test]
    fn carry_round_trips_through_json() {
        let carry = Scenario::from_json(
            r#"{"genes":{"Catalase":"Expressed","Fermentation":"Mutated"},"enzymes":{"Catalase":2},
                "membrane":{"Flagellum":2,"Toxin":1},"actions":["FlagellumBurst","Toxin"]}"#,
        )
        .unwrap()
        .carry();
        let text = serde_json::to_string(&carry).unwrap();
        assert_eq!(serde_json::from_str::<Carry>(&text).unwrap(), carry);
    }

    #[test]
    fn serde_types_round_trip() {
        let mut proteome = Proteome::with_enzymes(&[(BlockKind::Catalase, 2)]);
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Lytic));
        let text = serde_json::to_string(&proteome).unwrap();
        assert_eq!(serde_json::from_str::<Proteome>(&text).unwrap(), proteome);

        let bar = ActionBar {
            slots: [Some(ActionKind::Lyse), None, Some(ActionKind::Pilus), Some(ActionKind::InduceMutation)],
        };
        let text = serde_json::to_string(&bar).unwrap();
        assert_eq!(serde_json::from_str::<ActionBar>(&text).unwrap(), bar);

        let pools = CurrencyPools::with_defaults();
        let text = serde_json::to_string(&pools).unwrap();
        assert_eq!(serde_json::from_str::<CurrencyPools>(&text).unwrap().pools, pools.pools);
    }

    #[test]
    fn a_dumped_state_loads_back_to_the_same_cell() {
        let mut scenario = Scenario::from_json(
            r#"{"stage":"Land","genes":{"Catalase":"Expressed"},"enzymes":{"Catalase":1,"Chemosynthesis":0},
                "membrane":{"Flagellum":3},"pools":{"ATP":321},"actions":["FlagellumBurst"]}"#,
        )
        .unwrap();
        scenario.invulnerable = true;
        let carry = scenario.carry();
        let dumped = Scenario::from_state(scenario.stage, 7, &carry, &scenario.start_pools(), Vec2::new(12.0, -3.0), true);
        let back = Scenario::from_json(&dumped.to_json().unwrap()).unwrap();
        assert_eq!(back, dumped);
        assert_eq!(back.carry(), carry);
        assert_eq!(back.start_pools().pools, scenario.start_pools().pools);
        assert_eq!(back.begin_run().spawn, Some(Vec2::new(12.0, -3.0)));
    }

    #[test]
    fn begin_run_sets_stage_seed_and_cell() {
        let run = Scenario::from_json(r#"{"stage":"Land","seed":5}"#).unwrap().begin_run();
        assert!(run.active);
        assert_eq!(run.stage(), Stage::Land);
        assert_eq!(run.seed, 5);
        assert!(run.carry.is_some());
    }

    #[test]
    fn actions_pad_and_cut_to_the_slot_count() {
        let bar = action_bar_from(&[Some(ActionKind::Lyse)]);
        assert_eq!(bar.slots, [Some(ActionKind::Lyse), None, None, None]);
        let many = vec![Some(ActionKind::Toxin); SLOT_COUNT + 3];
        assert_eq!(action_bar_from(&many).slots, [Some(ActionKind::Toxin); SLOT_COUNT]);
    }

    #[test]
    fn names_resolve_to_presets_or_paths() {
        let assets = Path::new("/game/assets");
        assert_eq!(resolve_path("vent_motile", assets), Path::new("/game/assets/scenarios/vent_motile.json"));
        assert_eq!(resolve_path("mine.json", assets), Path::new("mine.json"));
        assert_eq!(resolve_path("/tmp/x", assets), Path::new("/tmp/x"));
        assert_eq!(resolve_path("dir/x", assets), Path::new("dir/x"));
    }

    #[test]
    fn the_command_line_wins_over_the_environment() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(scenario_name(&args(&["--scenario", "a"]), Some("b")), Some("a".into()));
        assert_eq!(scenario_name(&args(&["--scenario=a"]), None), Some("a".into()));
        assert_eq!(scenario_name(&args(&[]), Some("b")), Some("b".into()));
        assert_eq!(scenario_name(&args(&[]), Some("")), None);
        assert_eq!(scenario_name(&args(&["--other"]), None), None);
    }

    #[test]
    fn every_preset_loads_and_builds_a_cell() {
        let files = preset_files();
        for name in ["vent_motile", "vent_hunter", "shallow_start", "land_start"] {
            assert!(files.iter().any(|f| f.file_stem().unwrap() == name), "preset {name} is missing");
        }
        for path in files {
            let scenario = load(&path).unwrap_or_else(|e| panic!("{e}"));
            let carry = scenario.carry();
            let mut app = crate::MetabolisticApp::new_headless();
            let (_, cell) = crate::shared::spawn_cell_from_carry(&mut app.world_mut().commands(), Vec2::ZERO, carry.clone());
            app.update();
            let proteome = app.world().get::<Proteome>(cell).unwrap();
            assert_eq!(*proteome, carry.proteome, "{}", path.display());
            for (protein, count) in &scenario.membrane {
                assert_eq!(proteome.count(ProteinTarget::Membrane(*protein)), *count, "{}", path.display());
            }
            assert!(scenario.begin_run().level_def().is_some(), "{}", path.display());
        }
    }

    #[test]
    fn motile_preset_has_three_flagella() {
        let scenario = load(&resolve_path("vent_motile", &assets_dir())).unwrap();
        assert_eq!(scenario.carry().proteome.count(ProteinTarget::Membrane(MembraneProtein::Flagellum)), 3);
        // The burst is on Shift, not in a slot.
        assert!(!scenario.carry().action_bar.slots.contains(&Some(ActionKind::FlagellumBurst)));
    }

    #[test]
    fn save_makes_the_directory() {
        let dir = TempDir::new();
        let path = dir.0.join("nested").join(DUMP_FILE);
        save(&Scenario::default(), &path).unwrap();
        assert_eq!(load(&path).unwrap(), Scenario::default());
    }
}
