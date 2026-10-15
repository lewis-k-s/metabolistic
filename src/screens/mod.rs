//! The top-level screens: `Loading`, `Menu`, `Run` and `Manage` (see
//! `crate::GameState`). Each screen tags the entities it spawns with
//! `DespawnOnExit`, so leaving a screen removes them.

use bevy::prelude::*;

use crate::GameState;

pub mod loading;
pub mod manage;
pub mod menu;
pub mod run;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((loading::plugin, menu::plugin, run::plugin, manage::plugin))
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, navigation_input);
}

/// One 2D camera for every screen.
fn spawn_camera(mut commands: Commands) {
    commands.spawn((Name::new("Camera"), Camera2d));
}

/// Escape goes back to the menu. Tab switches between Run and Manage.
fn navigation_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let nav = if keys.just_pressed(KeyCode::Escape) {
        crate::shared::Navigation::Back
    } else if keys.just_pressed(KeyCode::Tab) {
        crate::shared::Navigation::ToggleManage
    } else {
        return;
    };
    if let Some(target) = crate::shared::navigate(state.get(), nav) {
        next_state.set(target);
    }
}

/// Full-window column that centers its children.
pub(crate) fn screen_root() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(16.0),
        ..default()
    }
}

/// Text of the given size in the default color.
pub(crate) fn label(text: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.9, 0.9)),
    )
}
