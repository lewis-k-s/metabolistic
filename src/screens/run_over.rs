//! Run-over screen: the player's cell died. The player can restart (a new
//! world and a new starter cell) or go back to the menu.

use bevy::prelude::*;

use super::{button, label, screen_root};
use crate::GameState;
use crate::run::{RunSummary, run_over_message, run_over_title};

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
            RunOverAction::Restart => GameState::Run,
            RunOverAction::Menu => GameState::Menu,
        }
    }
}

fn spawn_run_over_screen(mut commands: Commands, summary: Res<RunSummary>) {
    commands.spawn((
        Name::new("Run Over Screen"),
        screen_root(),
        DespawnOnExit(GameState::RunOver),
        children![
            label(run_over_title(&summary), 80.0),
            label(&run_over_message(&summary), 28.0),
            button("Restart", RunOverAction::Restart),
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
        assert_eq!(RunOverAction::Restart.target(), GameState::Run);
        assert_eq!(RunOverAction::Menu.target(), GameState::Menu);
    }
}
