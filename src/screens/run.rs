//! Run screen: the main loop. The world (the vent field and the player's cell)
//! is in `crate::world`; it stays when the player switches to Manage. This
//! screen only adds the controls hint. The status bars are in `world::hud`.

use bevy::prelude::*;

use super::label;
use crate::GameState;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Run), spawn_run_hint);
}

fn spawn_run_hint(mut commands: Commands) {
    commands.spawn((
        Name::new("Run Hint"),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        DespawnOnExit(GameState::Run),
        children![label("WASD: move    1-4: action slots    Tab: manage    Esc: menu", 18.0)],
    ));
}
