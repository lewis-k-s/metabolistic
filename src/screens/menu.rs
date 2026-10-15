//! Menu screen: the main menu and its submenus. Each submenu is a `MenuPage`,
//! a sub-state that exists only while `GameState::Menu` is active.

use bevy::prelude::*;

use super::{button, label, screen_root};
use crate::GameState;

pub(super) fn plugin(app: &mut App) {
    app.add_sub_state::<MenuPage>()
        .add_systems(OnEnter(MenuPage::Main), spawn_main_page)
        .add_systems(OnEnter(MenuPage::Settings), spawn_settings_page)
        .add_systems(Update, menu_buttons.run_if(in_state(GameState::Menu)));
}

/// The page shown while in `GameState::Menu`.
#[derive(SubStates, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[source(GameState = GameState::Menu)]
pub enum MenuPage {
    #[default]
    Main,
    Settings,
}

/// What a menu button does when pressed.
#[derive(Component, Debug, Clone)]
enum MenuAction {
    Open(GameState),
    Page(MenuPage),
    Quit,
}

fn spawn_main_page(mut commands: Commands) {
    commands.spawn((
        Name::new("Main Menu"),
        screen_root(),
        DespawnOnExit(MenuPage::Main),
        children![
            label("Metabolistic", 80.0),
            button("Start", MenuAction::Open(GameState::Prepare)),
            button("Manage", MenuAction::Open(GameState::Manage)),
            button("Settings", MenuAction::Page(MenuPage::Settings)),
            button("Quit", MenuAction::Quit),
        ],
    ));
}

fn spawn_settings_page(mut commands: Commands) {
    commands.spawn((
        Name::new("Settings Menu"),
        screen_root(),
        DespawnOnExit(MenuPage::Settings),
        children![
            label("Settings", 60.0),
            button("Back", MenuAction::Page(MenuPage::Main)),
        ],
    ));
}

fn menu_buttons(
    buttons: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut next_page: ResMut<NextState<MenuPage>>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            MenuAction::Open(state) => next_state.set(state.clone()),
            MenuAction::Page(page) => next_page.set(*page),
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}
