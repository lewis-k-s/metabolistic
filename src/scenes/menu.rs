use bevy::prelude::*;

use crate::GameState;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::MainMenu), setup_menu)
            .add_systems(Update, button_system.run_if(in_state(GameState::MainMenu)))
            .add_systems(OnExit(GameState::MainMenu), cleanup_menu);
    }
}

#[derive(Component)]
struct MenuUIRoot;

#[derive(Component)]
enum MenuButton {
    Scene3D,
    Scene2D,
    GenomeEditing,
}

fn setup_menu(mut commands: Commands) {
    commands.spawn((Camera2d, MenuUIRoot));

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            MenuUIRoot,
        ))
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("Metabolistic3D"),
                TextFont {
                    font_size: 80.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            // Spacer
            parent.spawn(Node {
                height: Val::Px(64.0),
                ..default()
            });

            // Scene buttons
            create_button(parent, "3D Scene", MenuButton::Scene3D);
            create_button(parent, "2D Scene", MenuButton::Scene2D);
            create_button(parent, "Genome Editor", MenuButton::GenomeEditing);
        });
}

fn create_button(parent: &mut ChildBuilder, text: &str, button_type: MenuButton) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(250.0),
                height: Val::Px(65.0),
                margin: UiRect::all(Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
            button_type,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(text),
                TextFont {
                    font_size: 40.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.9, 0.9)),
            ));
        });
}

fn button_system(
    mut interaction_query: Query<
        (&Interaction, &MenuButton, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for (interaction, menu_button, mut color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = Color::srgb(0.35, 0.75, 0.35).into();
                match menu_button {
                    MenuButton::Scene3D => next_state.set(GameState::Scene3D),
                    MenuButton::Scene2D => next_state.set(GameState::Scene2D),
                    MenuButton::GenomeEditing => next_state.set(GameState::GenomeEditing),
                }
            }
            Interaction::Hovered => {
                *color = Color::srgb(0.25, 0.25, 0.25).into();
            }
            Interaction::None => {
                *color = Color::srgb(0.15, 0.15, 0.15).into();
            }
        }
    }
}

fn cleanup_menu(mut commands: Commands, query: Query<Entity, With<MenuUIRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}
