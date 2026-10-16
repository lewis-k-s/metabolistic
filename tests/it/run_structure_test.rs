//! # Milestone M6: run structure (headless)
//!
//! Stage transitions keep the genome and proteome and reset the rest. The run is
//! complete when no stage follows. The codex gets the genes of a run when it
//! ends, in a temp directory (never the real save file).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use metabolistic3d::actions::{ActionBar, ActionKind};
use metabolistic3d::blocks::genome::{BlockKind, GeneState, Genome};
use metabolistic3d::cell::DeathCause;
use metabolistic3d::codex::{Codex, CodexStore, codex_path, load};
use metabolistic3d::engulf::EngulfEvent;
use metabolistic3d::genome_damage::GenomeIntegrity;
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::Currency;
use metabolistic3d::npc::{Archetype, spawn_npc};
use metabolistic3d::player::ControlledCell;
use metabolistic3d::proteins::{ProteinTarget, Proteome};
use metabolistic3d::run::{
    RunState, RunSummary, StageOutcome, carry_over, starter_carry,
};
use metabolistic3d::shared::{spawn_cell_from_carry, spawn_starter_cell};
use metabolistic3d::gate::GateProgress;
use metabolistic3d::stage::Stage;
use metabolistic3d::{GameState, MetabolisticApp};
use std::collections::BTreeSet;
use std::path::PathBuf;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("metabolistic3d-run-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app(dir: &TempDir) -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
    app.insert_resource(CodexStore { path: Some(codex_path(&dir.0)) });
    app.update();
    app
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn go(app: &mut App, state: GameState) {
    app.world_mut().resource_mut::<NextState<GameState>>().set(state);
    frames(app, 2);
}

fn state(app: &App) -> GameState {
    app.world().resource::<State<GameState>>().get().clone()
}

fn spawn_player(app: &mut App, carry: metabolistic3d::run::Carry, at: Vec2) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_cell_from_carry(&mut commands, at, carry).1
    };
    app.world_mut().flush();
    cell
}

#[test]
fn a_stage_transition_keeps_genome_and_proteome_and_resets_the_rest() {
    let dir = TempDir::new("carry");
    let mut app = app(&dir);
    // A cell at the end of a stage: a taken gene, an extra enzyme, a mutated
    // gene, drained pools, a damaged membrane and genome.
    let old = spawn_player(&mut app, starter_carry(&BTreeSet::new()), Vec2::ZERO);
    {
        let mut world = app.world_mut().entity_mut(old);
        let mut genome = world.get_mut::<Genome>().unwrap();
        genome.table.insert(BlockKind::SecondaryMetabolites, GeneState::Expressed);
        genome.table.insert(BlockKind::Chemosynthesis, GeneState::Mutated);
        let mut proteome = world.get_mut::<Proteome>().unwrap();
        proteome.add(ProteinTarget::Enzyme(BlockKind::SecondaryMetabolites));
        proteome.add(ProteinTarget::Enzyme(BlockKind::Chemosynthesis));
        let mut pools = world.get_mut::<CurrencyPools>().unwrap();
        pools.set(Currency::ATP, 3.0);
        pools.set(Currency::Minerals, 0.0);
        world.get_mut::<Membrane>().unwrap().integrity = 12.0;
        world.get_mut::<GenomeIntegrity>().unwrap().0 = 0.2;
    }
    let carry = {
        let world = app.world();
        carry_over(
            world.get::<Genome>(old).unwrap(),
            world.get::<Proteome>(old).unwrap(),
            world.get::<ActionBar>(old).unwrap(),
        )
    };
    let mut run = RunState::begin(1, BTreeSet::new());
    // With a playable next stage, the run goes on and holds the carry.
    assert_eq!(run.complete_stage(carry, |_| true), StageOutcome::NextStage);
    assert_eq!(run.stage_index, 1);
    let next = spawn_player(&mut app, run.cell_carry(), Vec2::new(5.0, 5.0));

    let world = app.world();
    let genome = world.get::<Genome>(next).unwrap();
    assert_eq!(genome.get_gene_state(&BlockKind::SecondaryMetabolites), Some(&GeneState::Expressed));
    assert_eq!(genome.get_gene_state(&BlockKind::Chemosynthesis), Some(&GeneState::Expressed), "repaired");
    let proteome = world.get::<Proteome>(next).unwrap();
    assert_eq!(proteome.count(ProteinTarget::Enzyme(BlockKind::SecondaryMetabolites)), 1);
    assert_eq!(proteome.count(ProteinTarget::Enzyme(BlockKind::Chemosynthesis)), 3);
    assert!(world.get::<ActionBar>(next).unwrap().slots.contains(&Some(ActionKind::InduceMutation)));
    // Everything else is new.
    let fresh = CurrencyPools::with_defaults();
    let pools = world.get::<CurrencyPools>(next).unwrap();
    assert_eq!(pools.get(Currency::ATP), fresh.get(Currency::ATP));
    assert_eq!(pools.get(Currency::Minerals), fresh.get(Currency::Minerals));
    assert_eq!(world.get::<Membrane>(next).unwrap().integrity, Membrane::default().integrity);
    assert_eq!(world.get::<GenomeIntegrity>(next).unwrap().0, GenomeIntegrity::default().0);
}

#[test]
fn the_run_is_complete_when_no_next_stage_has_a_level() {
    let mut run = RunState::begin(9, BTreeSet::new());
    assert_eq!(run.stage(), Stage::Vents);
    assert!(run.level_def().is_some());
    let outcome = run.complete_stage(starter_carry(&BTreeSet::new()), Stage::is_playable);
    assert_eq!(outcome, StageOutcome::RunComplete);
    assert_eq!(run.stage_index, 0, "the run stays on its last stage");
    assert!(run.carry.is_none());
}

#[test]
fn winning_the_last_built_stage_ends_the_run_and_banks_the_gate_gene() {
    let dir = TempDir::new("win");
    let mut app = app(&dir);
    *app.world_mut().resource_mut::<RunState>() = RunState::begin(4, BTreeSet::new());
    go(&mut app, GameState::Run);
    let player = spawn_player(&mut app, starter_carry(&BTreeSet::new()), Vec2::ZERO);
    app.world_mut().commands().spawn((metabolistic3d::player::Player { client_id: 0 }, metabolistic3d::player::LocalPlayer, ControlledCell(player)));
    app.world_mut().flush();
    let gate = {
        let mut commands = app.world_mut().commands();
        spawn_npc(&mut commands, Archetype::Gate, Vec2::new(47.0, 0.0), None, 0)
    };
    app.world_mut().flush();
    app.world_mut().entity_mut(gate).insert(GateProgress::default());
    frames(&mut app, 60); // 3 s of contact + 1.6 s clip
    assert_eq!(state(&app), GameState::RunOver);
    let summary = *app.world().resource::<RunSummary>();
    assert!(summary.run_complete);
    assert_eq!(summary.victory, Some(BlockKind::LightCapture));
    let run = app.world().resource::<RunState>();
    assert!(!run.active, "the run is finished");
    assert_eq!(run.new_genes, vec![BlockKind::LightCapture]);
    assert!(app.world().resource::<Codex>().genes.contains(&BlockKind::LightCapture));
    assert!(load(&codex_path(&dir.0)).genes.contains(&BlockKind::LightCapture), "saved in the temp dir");
}

#[test]
fn a_lost_run_banks_the_genes_it_took() {
    let dir = TempDir::new("loss");
    let mut app = app(&dir);
    *app.world_mut().resource_mut::<RunState>() = RunState::begin(5, BTreeSet::new());
    go(&mut app, GameState::Run);
    let (player, cell) = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO)
    };
    app.world_mut().flush();
    let prey = app.world_mut().spawn_empty().id();
    app.world_mut().resource_mut::<Messages<EngulfEvent>>().write(EngulfEvent {
        engulfer: cell,
        target: prey,
        gene: Some(BlockKind::VesicleExport),
        target_archetype: None,
        direction: Vec2::X,
        position: Vec2::ZERO,
    });
    // An NPC that engulfs something is not the player: its gene does not count.
    let other = app.world_mut().spawn_empty().id();
    app.world_mut().resource_mut::<Messages<EngulfEvent>>().write(EngulfEvent {
        engulfer: other,
        target: prey,
        gene: Some(BlockKind::FatStorage),
        target_archetype: None,
        direction: Vec2::X,
        position: Vec2::ZERO,
    });
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<RunState>().genes_taken, vec![BlockKind::VesicleExport]);
    // The cell starves.
    app.world_mut().get_mut::<CurrencyPools>(cell).unwrap().set(Currency::ATP, 0.0);
    frames(&mut app, 3);
    let _ = player;
    assert_eq!(state(&app), GameState::RunOver);
    assert_eq!(app.world().resource::<RunSummary>().cause, Some(DeathCause::Starved));
    assert_eq!(app.world().resource::<RunState>().new_genes, vec![BlockKind::VesicleExport]);
    let saved = load(&codex_path(&dir.0));
    assert!(saved.genes.contains(&BlockKind::VesicleExport) && !saved.genes.contains(&BlockKind::FatStorage));
}

#[test]
fn the_headless_app_keeps_the_codex_in_memory() {
    let app = MetabolisticApp::new_headless();
    assert_eq!(app.world().resource::<CodexStore>().path, None);
}

#[test]
fn closing_the_game_during_a_run_banks_the_genes_once() {
    let dir = TempDir::new("exit");
    let mut app = app(&dir);
    let mut run = RunState::begin(6, BTreeSet::new());
    run.record_gene(BlockKind::Fermentation);
    *app.world_mut().resource_mut::<RunState>() = run;
    app.world_mut().resource_mut::<Messages<AppExit>>().write(AppExit::Success);
    frames(&mut app, 2);
    assert!(load(&codex_path(&dir.0)).genes.contains(&BlockKind::Fermentation));
    assert!(!app.world().resource::<RunState>().active);
    assert_eq!(app.world().resource::<RunState>().new_genes, vec![BlockKind::Fermentation]);
}
