//! Loading screen: the first state. It waits for the sprite sheets, then opens
//! the menu. A sheet that fails to load is logged, and does not block the game.

use bevy::prelude::*;

use super::{label, screen_root};
use crate::GameState;
use crate::sprites::Sheets;

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

fn finish_loading(
    sheets: Option<Res<Sheets>>,
    server: Res<AssetServer>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    // The sheets are inserted at startup, so on the first frame they may be missing.
    let Some(sheets) = sheets else {
        return;
    };
    let progress = sheets.progress(&server);
    if progress.pending > 0 {
        return;
    }
    if progress.failed > 0 {
        error!("{} sprite sheets failed to load", progress.failed);
    }
    next_state.set(GameState::Menu);
}
