//! Development tools for the game. This plugin is only enabled in dev builds.

use bevy::{
    input::common_conditions::input_just_pressed, prelude::*, ui_render::GlobalUiDebugOptions,
};

pub(crate) fn plugin(app: &mut App) {
    // Toggle the debug overlay for UI.
    // The UI debug overlay comes with the `bevy_ui_debug` feature.
    app.add_systems(
        Update,
        toggle_debug_ui.run_if(input_just_pressed(TOGGLE_KEY)),
    );
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;

fn toggle_debug_ui(mut options: ResMut<GlobalUiDebugOptions>) {
    options.toggle();
}
