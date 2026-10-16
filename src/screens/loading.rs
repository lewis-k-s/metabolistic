//! Loading screen: the first state. It waits for the sprite sheets, then opens
//! the menu. A sheet that fails to load is logged, and does not block the game.

use bevy::prelude::*;

use super::{label, screen_root};
use crate::GameState;
use crate::codex::{Codex, CodexStore};
use crate::scenario::{self, ScenarioLaunch};
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

#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn finish_loading(
    mut commands: Commands,
    sheets: Option<Res<Sheets>>,
    server: Res<AssetServer>,
    scenario: Option<Res<ScenarioLaunch>>,
    mut codex: ResMut<Codex>,
    mut store: ResMut<CodexStore>,
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
    // A scenario skips the menu and Prepare: the run starts at once.
    if let Some(scenario) = scenario {
        scenario::start(&scenario.0, &mut commands, &mut codex, &mut store);
        commands.remove_resource::<ScenarioLaunch>();
        next_state.set(GameState::Run);
        return;
    }
    next_state.set(GameState::Menu);
}
