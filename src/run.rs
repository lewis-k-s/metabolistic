//! # Run end
//!
//! A run ends when the player's cell dies (see `cell::death_cause`). While
//! `InRun` is active, `detect_death_system` watches the controlled cell. When it
//! dies, the system writes the cause to `RunSummary` and moves to
//! `GameState::RunOver`. That state leaves `InRun`, so the world is removed.
//! A restart enters `Run` again and spawns a new world.
//!
//! A run can also be **won**: the gate cell gives its gene (see `gate`). The
//! summary then holds `victory = Some(gene)` and the run-over screen shows
//! "Victory". A player cell that a bigger cell engulfed dies with the cause
//! `DeathCause::Engulfed`.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::cell::{DamageSource, DamageTaken, DeathCause, death_cause};
use crate::engulf::{Engulfed, Invulnerable};
use crate::membrane::Membrane;
use crate::metabolism::CurrencyPools;
use crate::player::{ControlledCell, LocalPlayer};
use crate::{GameState, InRun};

/// How the last run ended. `None` before the first death.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct RunSummary {
    pub cause: Option<DeathCause>,
    /// The main source of membrane damage, for a broken membrane.
    pub source: Option<DamageSource>,
    /// The gene that the gate cell gave, if the run was won.
    pub victory: Option<BlockKind>,
}

/// The run is won and the screen waits for the victory clip. The player cannot
/// die in this time.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PendingVictory(pub f32);

/// The title of the run-over screen.
pub fn run_over_title(summary: &RunSummary) -> &'static str {
    if summary.victory.is_some() { "Victory" } else { "Run over" }
}

/// The line under the title: the cause of death, or the gene that was taken.
pub fn run_over_message(summary: &RunSummary) -> String {
    match summary.victory {
        Some(gene) => format!("You engulfed the gate cell and took its gene: {gene:?}."),
        None => death_message(summary.cause, summary.source),
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

/// A new run starts with an empty summary.
fn start_run(mut summary: ResMut<RunSummary>) {
    *summary = RunSummary::default();
}

/// Counts down the victory clip, then shows the run-over screen.
pub fn victory_countdown_system(
    time: Res<Time>,
    pending: Option<ResMut<PendingVictory>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(mut pending) = pending else { return };
    pending.0 -= time.delta_secs();
    if pending.0 <= 0.0 {
        next_state.set(GameState::RunOver);
    }
}

pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RunSummary>()
            .add_systems(OnEnter(InRun), start_run)
            .add_systems(OnExit(InRun), |mut commands: Commands| {
                commands.remove_resource::<PendingVictory>();
            })
            .add_systems(
                Update,
                (detect_death_system, victory_countdown_system).run_if(in_state(InRun)),
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
        assert_ne!(starved, death_message(None, None));
    }
}
