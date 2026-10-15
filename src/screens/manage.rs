//! Manage screen: the metabolism editor. The player views the metabolic
//! diagram of their cell and edits it. Placeholder for now.

use bevy::prelude::*;

use super::{label, screen_root};
use crate::GameState;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Manage), spawn_manage_screen);
}

fn spawn_manage_screen(mut commands: Commands) {
    commands.spawn((
        Name::new("Manage Screen"),
        screen_root(),
        DespawnOnExit(GameState::Manage),
        children![
            label("Manage", 60.0),
            label("Metabolism editor", 24.0),
            label("Tab: run    Esc: menu", 18.0),
        ],
    ));
}
