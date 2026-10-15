//! Loading screen: the first state. It waits for assets, then opens the menu.
//! There are no assets to wait for yet, so it moves on at once.

use bevy::prelude::*;

use super::{label, screen_root};
use crate::GameState;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Loading), spawn_loading_screen)
        .add_systems(Update, finish_loading.run_if(in_state(GameState::Loading)));
}

fn spawn_loading_screen(mut commands: Commands) {
    commands.spawn((
        Name::new("Loading Screen"),
        screen_root(),
        DespawnOnExit(GameState::Loading),
        children![label("Loading…", 40.0)],
    ));
}

fn finish_loading(mut next_state: ResMut<NextState<GameState>>) {
    next_state.set(GameState::Menu);
}
