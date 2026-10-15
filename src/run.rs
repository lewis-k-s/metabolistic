//! # Run end
//!
//! A run ends when the player's cell dies (see `cell::death_cause`). While
//! `InRun` is active, `detect_death_system` watches the controlled cell. When it
//! dies, the system writes the cause to `RunSummary` and moves to
//! `GameState::RunOver`. That state leaves `InRun`, so the world is removed.
//! A restart enters `Run` again and spawns a new world.

use bevy::prelude::*;

use crate::cell::{DamageSource, DamageTaken, DeathCause, death_cause};
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
}

/// The text for the run-over screen.
pub fn death_message(cause: Option<DeathCause>, source: Option<DamageSource>) -> String {
    match cause {
        Some(DeathCause::Starved) => "You starved: ATP ran out.".to_string(),
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
    cells: Query<(&CurrencyPools, &Membrane, &DamageTaken)>,
    mut summary: ResMut<RunSummary>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for controlled in &players {
        let Ok((pools, membrane, taken)) = cells.get(controlled.0) else {
            continue;
        };
        if let Some(cause) = death_cause(pools, membrane) {
            summary.cause = Some(cause);
            summary.source = taken.main_source();
            next_state.set(GameState::RunOver);
        }
    }
}

pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RunSummary>()
            .add_systems(Update, detect_death_system.run_if(in_state(InRun)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cause_has_its_own_message() {
        let starved = death_message(Some(DeathCause::Starved), None);
        let broken = death_message(Some(DeathCause::MembraneBroken), None);
        let burnt = death_message(Some(DeathCause::MembraneBroken), Some(DamageSource::Heat));
        assert_ne!(starved, broken);
        assert_ne!(broken, burnt);
        let uv = death_message(Some(DeathCause::MembraneBroken), Some(DamageSource::Uv));
        assert!(uv.contains("UV") && uv != burnt);
        assert!(burnt.contains("heat"));
        assert_ne!(starved, death_message(None, None));
    }
}
