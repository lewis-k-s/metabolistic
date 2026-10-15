//! The top-level screens: `Loading`, `Menu`, `Run`, `Manage` and `RunOver` (see
//! `crate::GameState`). Each screen tags the entities it spawns with
//! `DespawnOnExit`, so leaving a screen removes them. The world of a run (the
//! vent field and the player's cell) is not a screen: see `crate::world`.

use bevy::prelude::*;

use crate::GameState;

pub mod loading;
pub mod manage;
pub mod menu;
pub mod run;
pub mod run_over;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        loading::plugin,
        menu::plugin,
        run::plugin,
        run_over::plugin,
        manage::plugin,
    ))
    .add_systems(Startup, spawn_camera)
    .add_systems(Update, (navigation_input, button_colors));
}

/// One 2D camera for every screen.
fn spawn_camera(mut commands: Commands) {
    // World units are sprite pixels. The scale makes each one a whole number
    // of screen pixels (art doc, "In the game").
    commands.spawn((
        Name::new("Camera"),
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0 / crate::sprites::PIXEL_SCALE,
            ..OrthographicProjection::default_2d()
        }),
    ));
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

const BUTTON_IDLE: Color = Color::srgb(0.15, 0.15, 0.15);
const BUTTON_HOVERED: Color = Color::srgb(0.25, 0.25, 0.25);
const BUTTON_PRESSED: Color = Color::srgb(0.35, 0.75, 0.35);

/// A menu button with a text label. `action` says what it does; a system on
/// the screen reads `Interaction` together with it.
pub(crate) fn button(text: &str, action: impl Component) -> impl Bundle {
    (
        Button,
        Node {
            min_width: Val::Px(250.0),
            height: Val::Px(65.0),
            padding: UiRect::horizontal(Val::Px(20.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON_IDLE),
        action,
        children![label(text, 36.0)],
    )
}

/// Colours every button by its interaction.
#[allow(clippy::type_complexity)]
fn button_colors(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, mut color) in &mut buttons {
        *color = match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_IDLE,
        }
        .into();
    }
}
