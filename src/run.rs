//! # Run
//!
//! A run is a sequence of stages (see `stage`). `RunState` holds the seed, the
//! stage index, the genes taken so far and the cell that carries over.
//!
//! - **Start**: the Prepare screen calls `RunState::begin` with the seed and the
//!   starting genes. Entering `InRun` with no run (the menu's Manage button,
//!   the smoke test) begins a default run.
//! - **Stage won** (the gate cell): after the victory clip, `victory_countdown_system`
//!   copies the genome, proteome and action bar (`Carry`) and calls
//!   `RunState::complete_stage`. If the next stage has a `LevelDef`, the game
//!   goes to `GameState::StageComplete`, then back to `Run` for the new stage
//!   (`InRun` is left and entered again, so the world is rebuilt from the new
//!   `LevelDef`). Pools reset to the starter values; membrane and genome
//!   integrity are full; mutated genes are repaired. If there is no next stage,
//!   the run is complete: `RunOver` with `RunSummary::run_complete`.
//! - **Death** (`cell::death_cause`): `detect_death_system` writes the cause to
//!   `RunSummary` and goes to `RunOver`. That state leaves `InRun`.
//! - **End**: on `RunOver`, when the player leaves for the menu, or when the game
//!   closes, the genes
//!   that the run took go into the codex (`finish_run_system`) and the screen
//!   lists the new ones.
//!
//! A player cell that a bigger cell engulfed dies with `DeathCause::Engulfed`.

use bevy::prelude::*;
use rand::prelude::*;

use std::collections::BTreeSet;

use crate::actions::{ActionBar, starter_action_bar};
use crate::blocks::genome::{BlockKind, GeneState, Genome, create_starter_genome};
use crate::cell::{Cell, DamageSource, DamageTaken, DeathCause, death_cause};
use crate::codex::{Codex, CodexStore, bank_and_save};
use crate::engulf::{EngulfEvent, Engulfed, Invulnerable};
use crate::gate::GateWon;
use crate::level::LevelDef;
use crate::membrane::Membrane;
use crate::metabolism::CurrencyPools;
use crate::particles::VirusInfection;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{ProteinTarget, Proteome};
use crate::stage::{STAGES, Stage, next_stage_index};
use crate::{GameState, InRun};

/// How the last run ended. `None` before the first death.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct RunSummary {
    pub cause: Option<DeathCause>,
    /// The main source of membrane damage, for a broken membrane.
    pub source: Option<DamageSource>,
    /// The gene that the gate cell gave, if the run was won.
    pub victory: Option<BlockKind>,
    /// The last stage was won and there is no stage after it.
    pub run_complete: bool,
}

/// What the cell takes from one stage to the next: its genome and proteome (and
/// the action bar that goes with the proteome). Everything else starts again.
#[derive(Debug, Clone, Default)]
pub struct Carry {
    pub genome: Genome,
    pub proteome: Proteome,
    pub action_bar: ActionBar,
}

/// The cell of a new run: the starter genome and proteome, plus the genes that
/// the player chose in Prepare.
pub fn starter_carry(loadout: &BTreeSet<BlockKind>) -> Carry {
    let mut genome = create_starter_genome();
    let mut proteome = crate::shared::starter_proteome();
    crate::prepare::apply_loadout(&mut genome, &mut proteome, loadout);
    Carry {
        genome,
        proteome,
        action_bar: starter_action_bar(),
    }
}

/// The carry of a cell that won a stage. Mutated genes are repaired, because
/// the genome integrity resets to full.
pub fn carry_over(genome: &Genome, proteome: &Proteome, action_bar: &ActionBar) -> Carry {
    let mut genome = genome.clone();
    for state in genome.table.values_mut() {
        if *state == GeneState::Mutated {
            *state = GeneState::Expressed;
        }
    }
    Carry {
        genome,
        proteome: proteome.clone(),
        action_bar: action_bar.clone(),
    }
}

/// The reward of a gate cell: if its gene has a block, the carry gets one free
/// enzyme for it, so the next stage starts with the pathway working, slowly.
/// A gene with no block (Multicellularity) gets nothing. The pathway
/// also goes into the first empty action slot (if one is free), so the player
/// can turn it Off or Low at once: photosynthesis makes ROS.
pub fn grant_gate_enzyme(carry: &mut Carry, gene: BlockKind) {
    if crate::blocks::block_spec(gene).is_some() {
        carry.proteome.add(ProteinTarget::Enzyme(gene));
        let action = crate::actions::ActionKind::Pathway(gene);
        if !carry.action_bar.slots.contains(&Some(action))
            && let Some(slot) = carry.action_bar.slots.iter_mut().find(|s| s.is_none())
        {
            *slot = Some(action);
        }
    }
}

/// What follows a won stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOutcome {
    NextStage,
    RunComplete,
}

/// The state of the run in progress.
#[derive(Resource, Debug, Clone, Default)]
pub struct RunState {
    /// True from `begin` until the genes go to the codex.
    pub active: bool,
    pub seed: u64,
    /// Index into `STAGES`.
    pub stage_index: usize,
    /// The genes chosen in Prepare.
    pub loadout: BTreeSet<BlockKind>,
    /// Genes taken in this run (engulf, virus, gate), in order, once each.
    pub genes_taken: Vec<BlockKind>,
    /// The cell that goes into the next stage. `None` in the first stage.
    pub carry: Option<Carry>,
    /// The genes that were new to the codex when the run ended.
    pub new_genes: Vec<BlockKind>,
}

impl RunState {
    /// Starts a run at the first stage.
    pub fn begin(seed: u64, loadout: BTreeSet<BlockKind>) -> Self {
        Self {
            active: true,
            seed,
            loadout,
            ..default()
        }
    }

    pub fn stage(&self) -> Stage {
        STAGES[self.stage_index.min(STAGES.len() - 1)]
    }

    /// The level of the current stage.
    pub fn level_def(&self) -> Option<LevelDef> {
        self.stage().level_def(self.seed)
    }

    pub fn record_gene(&mut self, kind: BlockKind) {
        if !self.genes_taken.contains(&kind) {
            self.genes_taken.push(kind);
        }
    }

    /// The cell that the current stage starts with.
    pub fn cell_carry(&self) -> Carry {
        self.carry.clone().unwrap_or_else(|| starter_carry(&self.loadout))
    }

    /// The current stage was won with this cell. Moves to the next stage if
    /// `playable` says that it has a level.
    pub fn complete_stage(&mut self, carry: Carry, playable: impl Fn(Stage) -> bool) -> StageOutcome {
        match next_stage_index(self.stage_index, playable) {
            Some(next) => {
                self.stage_index = next;
                self.carry = Some(carry);
                StageOutcome::NextStage
            }
            None => StageOutcome::RunComplete,
        }
    }
}

/// The stage is won and the screen waits for the victory clip. The player
/// cannot die in this time.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PendingVictory(pub f32);

/// The title of the run-over screen.
pub fn run_over_title(summary: &RunSummary) -> &'static str {
    if summary.victory.is_some() { "Run complete" } else { "Run over" }
}

/// The line under the title: the cause of death, or the gene that was taken.
pub fn run_over_message(summary: &RunSummary) -> String {
    match summary.victory {
        Some(gene) if summary.run_complete => format!(
            "You engulfed the last gate cell and took its gene: {gene:?}. You crossed from the vents to the land."
        ),
        Some(gene) => format!("You engulfed the gate cell and took its gene: {gene:?}."),
        None => death_message(summary.cause, summary.source),
    }
}

/// The line about new codex genes on the run-over screen.
pub fn new_genes_message(new_genes: &[BlockKind]) -> String {
    if new_genes.is_empty() {
        "No new genes for the codex.".to_string()
    } else {
        let names: Vec<_> = new_genes.iter().map(|g| format!("{g:?}")).collect();
        format!("New in the codex: {}", names.join(", "))
    }
}

/// The text for the run-over screen.
pub fn death_message(cause: Option<DeathCause>, source: Option<DamageSource>) -> String {
    match cause {
        Some(DeathCause::Starved) => "You starved: ATP ran out.".to_string(),
        Some(DeathCause::Engulfed) => "A bigger cell engulfed you.".to_string(),
        Some(DeathCause::MembraneBroken) => match source {
            Some(DamageSource::Heat) => "Your membrane failed. Main damage: heat.".to_string(),
            Some(DamageSource::Uv) => "Your membrane failed. Main damage: UV light.".to_string(),
            Some(DamageSource::Ros) => {
                "Your membrane failed. Main damage: reactive oxygen species. You poisoned yourself with your own oxygen.".to_string()
            }
            Some(DamageSource::Desiccation) => {
                "Your membrane failed. Main damage: desiccation. You dried out away from water.".to_string()
            }
            None => "Your membrane failed.".to_string(),
        },
        None => "The run ended.".to_string(),
    }
}

/// Moves to `RunOver` when the local player's cell dies.
pub fn detect_death_system(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&CurrencyPools, &Membrane, &DamageTaken, Has<Engulfed>, Has<Invulnerable>)>,
    pending: Option<Res<PendingVictory>>,
    mut summary: ResMut<RunSummary>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if pending.is_some() {
        return;
    }
    for controlled in &players {
        let Ok((pools, membrane, taken, engulfed, invulnerable)) = cells.get(controlled.0) else {
            continue;
        };
        if invulnerable {
            continue;
        }
        let cause = if engulfed {
            Some(DeathCause::Engulfed)
        } else {
            death_cause(pools, membrane)
        };
        if let Some(cause) = cause {
            summary.cause = Some(cause);
            summary.source = taken.main_source();
            summary.victory = None;
            next_state.set(GameState::RunOver);
        }
    }
}

/// Every stage starts with an empty summary. Entering a run that nobody began
/// (the Manage button, the smoke test) begins a default one.
fn start_stage(mut summary: ResMut<RunSummary>, mut run: ResMut<RunState>) {
    *summary = RunSummary::default();
    if !run.active {
        *run = RunState::begin(rand::rng().random::<u64>(), BTreeSet::new());
    }
}

/// Records the genes that the player's cell takes.
pub fn track_genes_system(
    mut engulfs: MessageReader<EngulfEvent>,
    mut infections: MessageReader<VirusInfection>,
    mut won: MessageReader<GateWon>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut run: ResMut<RunState>,
) {
    let cells: Vec<Entity> = players.iter().map(|c| c.0).collect();
    for event in engulfs.read() {
        if let (true, Some(gene)) = (cells.contains(&event.engulfer), event.gene) {
            run.record_gene(gene);
        }
    }
    for event in infections.read() {
        if let (true, Some(gene)) = (cells.contains(&event.cell), event.gene) {
            run.record_gene(gene);
        }
    }
    for event in won.read() {
        run.record_gene(event.gene);
    }
}

/// Ends the run in progress: its genes go into the codex. Once per run.
fn finish_run(run: &mut RunState, codex: &mut Codex, store: &CodexStore) {
    if !run.active {
        return;
    }
    run.active = false;
    let genes = run.genes_taken.clone();
    run.new_genes = bank_and_save(codex, store, &genes);
}

/// Ends the run when the run-over screen or the menu opens.
pub fn finish_run_system(
    mut run: ResMut<RunState>,
    mut codex: ResMut<Codex>,
    store: Res<CodexStore>,
) {
    finish_run(&mut run, &mut codex, &store);
}

/// Closing the game during a run keeps the genes taken so far.
pub fn finish_run_on_exit_system(
    mut exits: MessageReader<AppExit>,
    mut run: ResMut<RunState>,
    mut codex: ResMut<Codex>,
    store: Res<CodexStore>,
) {
    if exits.read().next().is_some() {
        finish_run(&mut run, &mut codex, &store);
    }
}

/// Counts down the victory clip, then goes to the next stage or ends the run.
#[allow(clippy::type_complexity)]
pub fn victory_countdown_system(
    time: Res<Time>,
    pending: Option<ResMut<PendingVictory>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Genome, &Proteome, &ActionBar), With<Cell>>,
    mut run: ResMut<RunState>,
    mut summary: ResMut<RunSummary>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(mut pending) = pending else { return };
    pending.0 -= time.delta_secs();
    if pending.0 > 0.0 {
        return;
    }
    let mut carry = players
        .iter()
        .find_map(|c| cells.get(c.0).ok())
        .map(|(genome, proteome, bar)| carry_over(genome, proteome, bar))
        .unwrap_or_else(|| run.cell_carry());
    if let Some(gene) = summary.victory {
        grant_gate_enzyme(&mut carry, gene);
    }
    match run.complete_stage(carry, Stage::is_playable) {
        StageOutcome::NextStage => next_state.set(GameState::StageComplete),
        StageOutcome::RunComplete => {
            summary.run_complete = true;
            next_state.set(GameState::RunOver);
        }
    }
}

pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RunSummary>()
            .init_resource::<RunState>()
            .init_resource::<Codex>()
            .init_resource::<CodexStore>()
            .add_systems(OnEnter(InRun), start_stage)
            .add_systems(Last, finish_run_on_exit_system)
            .add_systems(OnEnter(GameState::RunOver), finish_run_system)
            .add_systems(OnEnter(GameState::Menu), finish_run_system)
            .add_systems(OnExit(InRun), |mut commands: Commands| {
                commands.remove_resource::<PendingVictory>();
            })
            .add_systems(
                Update,
                (track_genes_system, detect_death_system, victory_countdown_system)
                    .chain()
                    .run_if(in_state(InRun)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cause_has_its_own_message() {
        let starved = death_message(Some(DeathCause::Starved), None);
        let broken = death_message(Some(DeathCause::MembraneBroken), None);
        let engulfed = death_message(Some(DeathCause::Engulfed), None);
        assert!(engulfed.contains("engulfed") && engulfed != starved);
        let burnt = death_message(Some(DeathCause::MembraneBroken), Some(DamageSource::Heat));
        assert_ne!(starved, broken);
        assert_ne!(broken, burnt);
        let uv = death_message(Some(DeathCause::MembraneBroken), Some(DamageSource::Uv));
        assert!(uv.contains("UV") && uv != burnt);
        assert!(burnt.contains("heat"));
        let ros = death_message(Some(DeathCause::MembraneBroken), Some(DamageSource::Ros));
        assert!(ros.contains("oxygen") && ros != uv && ros != burnt);
        let dry = death_message(Some(DeathCause::MembraneBroken), Some(DamageSource::Desiccation));
        assert!(dry.contains("desiccation") && dry != ros && dry != uv);
        assert_ne!(starved, death_message(None, None));
    }
}
