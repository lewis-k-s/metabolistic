//! Prepare screen: before a run, choose starting genes from the codex. The
//! starter genome is free; each codex gene costs points (see `crate::prepare`).
//! Genes with no block yet are listed in grey and cannot be chosen. Start begins
//! a run with a new seed.

use bevy::prelude::*;
use rand::prelude::*;

use super::{button, label, screen_root};
use crate::GameState;
use crate::blocks::genome::BlockKind;
use crate::codex::Codex;
use crate::prepare::{PrepareChoice, START_BUDGET, codex_list, gene_cost, spent, toggle};
use crate::run::RunState;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PrepareChoice>()
        .add_systems(OnEnter(GameState::Prepare), (reset_choice, spawn_prepare_screen).chain())
        .add_systems(
            Update,
            (prepare_buttons, refresh_list)
                .chain()
                .run_if(in_state(GameState::Prepare)),
        );
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PrepareAction {
    Toggle(BlockKind),
    Start,
    Back,
}

/// The column that holds the gene list and the points line.
#[derive(Component)]
struct GeneList;

fn reset_choice(mut choice: ResMut<PrepareChoice>) {
    choice.0.clear();
}

fn spawn_prepare_screen(mut commands: Commands, codex: Res<Codex>, choice: Res<PrepareChoice>) {
    commands
        .spawn((
            Name::new("Prepare Screen"),
            screen_root(),
            DespawnOnExit(GameState::Prepare),
            children![
                label("Prepare", 70.0),
                label(
                    "The starter genome is free. Spend points on genes from your codex.",
                    24.0
                ),
            ],
        ))
        .with_children(|root| {
            root.spawn((
                GeneList,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(8.0),
                    ..default()
                },
            ))
            .with_children(|list| fill_list(list, &codex, &choice));
            root.spawn(button("Start run", PrepareAction::Start));
            root.spawn(button("Back", PrepareAction::Back));
        });
}

/// Points line and one row for each codex gene.
fn fill_list(list: &mut ChildSpawnerCommands, codex: &Codex, choice: &PrepareChoice) {
    list.spawn(label(points_line(choice), 30.0));
    if codex.genes.is_empty() {
        list.spawn(label("The codex is empty. Genes that you take in a run go here.", 22.0));
    }
    for (kind, selectable) in codex_list(codex) {
        if selectable {
            let mark = if choice.0.contains(&kind) { "[x]" } else { "[ ]" };
            list.spawn(button(
                &format!("{mark} {kind:?} ({})", gene_cost(kind)),
                PrepareAction::Toggle(kind),
            ));
        } else {
            list.spawn((
                Text::new(format!("{kind:?} (no block yet)")),
                TextFont { font_size: FontSize::Px(24.0), ..default() },
                TextColor(Color::srgb(0.4, 0.4, 0.4)),
            ));
        }
    }
}

/// "Points: 1/2".
pub fn points_line(choice: &PrepareChoice) -> String {
    format!("Points: {}/{}", spent(&choice.0), START_BUDGET)
}

fn prepare_buttons(
    buttons: Query<(&Interaction, &PrepareAction), Changed<Interaction>>,
    codex: Res<Codex>,
    mut choice: ResMut<PrepareChoice>,
    mut run: ResMut<RunState>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            PrepareAction::Toggle(kind) => {
                toggle(&mut choice.0, kind, &codex, START_BUDGET);
            }
            PrepareAction::Start => {
                *run = RunState::begin(rand::rng().random::<u64>(), choice.0.clone());
                next_state.set(GameState::Run);
            }
            PrepareAction::Back => next_state.set(GameState::Menu),
        }
    }
}

/// Builds the list again when the choice changes.
fn refresh_list(
    choice: Res<PrepareChoice>,
    codex: Res<Codex>,
    list: Query<Entity, With<GeneList>>,
    mut commands: Commands,
) {
    if !choice.is_changed() || choice.is_added() {
        return;
    }
    for entity in &list {
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|parent| fill_list(parent, &codex, &choice));
    }
}
