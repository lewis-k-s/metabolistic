//! Stage-complete screen: the gate cell was engulfed and another stage follows.
//! It shows the gene that the player took and the next stage. Continue builds the
//! world of the next stage with the same cell (see `crate::run`).

use bevy::prelude::*;

use super::{button, label, screen_root};
use crate::GameState;
use crate::run::{RunState, RunSummary};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::StageComplete), spawn_screen)
        .add_systems(Update, buttons.run_if(in_state(GameState::StageComplete)));
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum StageCompleteAction {
    Continue,
    Menu,
}

/// "You took the gene X." for the gene of the stage that was won.
pub fn gene_line(summary: &RunSummary) -> String {
    match summary.victory {
        Some(gene) => format!("You engulfed the gate cell and took its gene: {gene:?}."),
        None => "You engulfed the gate cell.".to_string(),
    }
}

fn spawn_screen(mut commands: Commands, summary: Res<RunSummary>, run: Res<RunState>) {
    // The run is already on the next stage; the stage that was won is the one before.
    let won = crate::stage::STAGES[run.stage_index.saturating_sub(1)];
    commands.spawn((
        Name::new("Stage Complete Screen"),
        screen_root(),
        DespawnOnExit(GameState::StageComplete),
        children![
            label("Stage complete", 80.0),
            label(won.name(), 36.0),
            label(gene_line(&summary), 28.0),
            label(format!("Next: {}    seed {}", run.stage().name(), run.seed), 24.0),
            button("Continue", StageCompleteAction::Continue),
            button("Menu", StageCompleteAction::Menu),
        ],
    ));
}

fn buttons(
    buttons: Query<(&Interaction, &StageCompleteAction), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for (interaction, action) in &buttons {
        if *interaction == Interaction::Pressed {
            next_state.set(match action {
                StageCompleteAction::Continue => GameState::Run,
                StageCompleteAction::Menu => GameState::Menu,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::BlockKind;

    #[test]
    fn the_screen_names_the_gene() {
        let summary = RunSummary { victory: Some(BlockKind::LightCapture), ..default() };
        assert!(gene_line(&summary).contains("LightCapture"));
        assert!(!gene_line(&RunSummary::default()).contains("gene:"));
    }
}
