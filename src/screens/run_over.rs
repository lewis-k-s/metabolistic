//! Run-over screen: the cell died, or the last stage was won. It shows the
//! cause (or the gene taken), the stage and the seed, and the genes that are new
//! in the codex. Restart goes to Prepare; the other button goes to the menu.

use bevy::prelude::*;

use super::{button, label, screen_root};
use crate::GameState;
use crate::run::{RunState, RunSummary, new_genes_message, run_over_message, run_over_title};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::RunOver), spawn_run_over_screen)
        .add_systems(Update, run_over_buttons.run_if(in_state(GameState::RunOver)));
}

/// What a run-over button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum RunOverAction {
    Restart,
    Menu,
}

impl RunOverAction {
    fn target(self) -> GameState {
        match self {
            RunOverAction::Restart => GameState::Prepare,
            RunOverAction::Menu => GameState::Menu,
        }
    }
}

fn spawn_run_over_screen(mut commands: Commands, summary: Res<RunSummary>, run: Res<RunState>) {
    commands.spawn((
        Name::new("Run Over Screen"),
        screen_root(),
        DespawnOnExit(GameState::RunOver),
        children![
            label(run_over_title(&summary), 80.0),
            label(&run_over_message(&summary), 28.0),
            label(format!("Stage {}    seed {}", run.stage().name(), run.seed), 22.0),
            label(new_genes_message(&run.new_genes), 28.0),
            button("New run", RunOverAction::Restart),
            button("Menu", RunOverAction::Menu),
        ],
    ));
}

fn run_over_buttons(
    buttons: Query<(&Interaction, &RunOverAction), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for (interaction, action) in &buttons {
        if *interaction == Interaction::Pressed {
            next_state.set(action.target());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_starts_a_new_run_and_menu_leaves() {
        assert_eq!(RunOverAction::Restart.target(), GameState::Prepare);
        assert_eq!(RunOverAction::Menu.target(), GameState::Menu);
    }
}
