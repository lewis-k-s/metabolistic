//! Manage screen: the metabolism editor, drawn as a cross-section of the cell.
//!
//! - the membrane band at the top, with a socket for each membrane slot
//!   (`membrane_view`);
//! - the metabolic map in the middle: pools, pathway plates with their
//!   enzymes, and pipes with molecules that flow at the live rate
//!   (`diagram`, placed by `layout`);
//! - the ribosome at the bottom, with the synthesis queue;
//! - a small live inset of the run in the top right corner (`inset`);
//! - the side panel and the tooltips (`panel`), and the action bar (keys 1-4).
//!
//! The map is world-space sprites on their own render layer and camera
//! (`MAP_LAYER`); Bevy UI draws only the header, the side panel, the tooltip
//! and the action bar. The sprites are spawned once and then updated from
//! `LocalMetabolismView` each frame. The screen never computes a flow: the
//! data layer in `metabolism::view` does. A click on a plate or a socket
//! opens the side panel, which sends `QueueProtein` and `RecycleProtein`.
//!
//! Game time keeps running here, except in a shelter (see `shelter`). The edit
//! rules are in `proteins` and `actions`; this screen only sends messages and
//! changes the bar.

use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;

use super::label;
use crate::GameState;
use crate::actions::{ActionBar, ActionState, PathwayLevels, SLOT_COUNT, available_actions, next_assignment, slot_status};
use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::Cell;
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{Proteome, ProteinTarget, QueueProtein, RecycleProtein};
use crate::shelter::{InShelter, sim_paused};

mod diagram;
mod inset;
pub mod layout;
mod membrane_view;
mod panel;

const GOOD: Color = Color::srgb(0.45, 0.9, 0.5);
const DIM: Color = Color::srgb(0.5, 0.5, 0.55);
const BAD: Color = Color::srgb(0.95, 0.4, 0.35);
const GOLD: Color = Color::srgb(0.95, 0.8, 0.3);
/// The cytoplasm: the background of the map.
const CYTOPLASM: Color = Color::srgb(0.05, 0.07, 0.12);

/// The render layer of the map sprites. The run world stays on layer 0.
pub(super) fn map_layer() -> RenderLayers {
    RenderLayers::layer(1)
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Selection>()
        .init_resource::<Hovered>()
        .add_plugins((
            diagram::plugin,
            membrane_view::plugin,
            panel::plugin,
            inset::plugin,
        ))
        .add_systems(
            OnEnter(GameState::Manage),
            (spawn_map_camera, spawn_header_and_bar),
        )
        .add_systems(OnExit(GameState::Manage), clear_picks)
        .add_systems(
            Update,
            (manage_buttons, manage_keys, update_header, update_action_bar)
                .chain()
                .run_if(in_state(GameState::Manage)),
        );
}

/// Something on the map that the player can point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Pick {
    Pathway(BlockKind),
    Slot(usize),
    Pool(Currency),
}

/// The thing the side panel shows (a click).
#[derive(Resource, Default, Debug)]
pub(super) struct Selection(pub Option<Pick>);

/// The thing under the pointer (the tooltip).
#[derive(Resource, Default, Debug)]
pub(super) struct Hovered(pub Option<Pick>);

fn clear_picks(mut selection: ResMut<Selection>, mut hovered: ResMut<Hovered>) {
    selection.0 = None;
    hovered.0 = None;
}

/// Adds picking to a map sprite: hover sets `Hovered`, a left click sets
/// `Selection`.
pub(super) fn make_pickable(entity: &mut EntityCommands, pick: Pick) {
    entity
        .insert(Pickable::default())
        .observe(move |_: On<Pointer<Over>>, mut hovered: ResMut<Hovered>| {
            hovered.0 = Some(pick);
        })
        .observe(
            move |_: On<Pointer<Out>>, mut hovered: ResMut<Hovered>| {
                if hovered.0 == Some(pick) {
                    hovered.0 = None;
                }
            },
        )
        .observe(
            move |click: On<Pointer<Click>>, mut selection: ResMut<Selection>| {
                if click.button == PointerButton::Primary {
                    selection.0 = Some(pick);
                }
            },
        );
}

/// The camera of the map. It draws first, on the whole window; the run camera
/// draws the inset above it (see `inset`).
#[derive(Component)]
struct MapCamera;

fn spawn_map_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("Map Camera"),
        MapCamera,
        Camera2d,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(CYTOPLASM),
            ..default()
        },
        // Whole pixels for each sprite pixel: 2 screen pixels at 1280x720.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: layout::VIEW_SIZE.x,
                min_height: layout::VIEW_SIZE.y,
            },
            ..OrthographicProjection::default_2d()
        }),
        map_layer(),
        // The map camera shows the UI of this screen.
        IsDefaultUiCamera,
        DespawnOnExit(GameState::Manage),
    ));
}

/// The local player's cell.
pub(super) fn local_cell(players: &Query<&ControlledCell, With<LocalPlayer>>) -> Option<Entity> {
    players.iter().next().map(|c| c.0)
}

// --- Buttons and keys ---

/// What a button of the screen does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManageAction {
    Queue(ProteinTarget),
    Recycle(ProteinTarget),
    CycleSlot(usize),
}

#[allow(clippy::type_complexity)]
fn manage_buttons(
    buttons: Query<(&Interaction, &ManageAction), Changed<Interaction>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&mut ActionBar, &Genome, &Proteome), With<Cell>>,
    mut queue: MessageWriter<QueueProtein>,
    mut recycle: MessageWriter<RecycleProtein>,
) {
    let Some(cell) = local_cell(&players) else {
        return;
    };
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            ManageAction::Queue(target) => {
                queue.write(QueueProtein { cell, target });
            }
            ManageAction::Recycle(target) => {
                recycle.write(RecycleProtein { cell, target });
            }
            ManageAction::CycleSlot(slot) => {
                if let Ok((mut bar, genome, proteome)) = cells.get_mut(cell) {
                    cycle_slot(&mut bar, slot, genome, proteome);
                }
            }
        }
    }
}

/// Keys 1-4 give the slot the next thing the cell can do.
fn manage_keys(
    keys: Res<ButtonInput<KeyCode>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&mut ActionBar, &Genome, &Proteome), With<Cell>>,
) {
    const SLOT_KEYS: [KeyCode; SLOT_COUNT] =
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];
    let Some(cell) = local_cell(&players) else {
        return;
    };
    for (slot, key) in SLOT_KEYS.iter().enumerate() {
        if keys.just_pressed(*key)
            && let Ok((mut bar, genome, proteome)) = cells.get_mut(cell)
        {
            cycle_slot(&mut bar, slot, genome, proteome);
        }
    }
}

fn cycle_slot(bar: &mut ActionBar, slot: usize, genome: &Genome, proteome: &Proteome) {
    if let Some(content) = bar.slots.get_mut(slot) {
        *content = next_assignment(*content, &available_actions(genome, proteome));
    }
}

/// Colour and name of a gene state.
pub fn gene_state_text(state: &GeneState) -> (&'static str, Color) {
    match state {
        GeneState::Expressed => ("Expressed", GOOD),
        GeneState::Silent => ("Silent", DIM),
        GeneState::Mutated => ("Mutated", BAD),
        GeneState::Enhanced => ("Enhanced x1.25", GOLD),
    }
}

// --- Header and action bar (Bevy UI) ---

#[derive(Component)]
struct HeaderText;

#[derive(Component)]
struct ActionSlotText(usize);

fn spawn_header_and_bar(mut commands: Commands) {
    commands.spawn((
        Name::new("Manage Header"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            left: Val::Px(10.0),
            ..default()
        },
        DespawnOnExit(GameState::Manage),
        children![(HeaderText, label("Manage", 20.0))],
    ));
    commands
        .spawn((
            Name::new("Action Bar"),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(8.0),
                left: Val::Px(10.0),
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                ..default()
            },
            DespawnOnExit(GameState::Manage),
        ))
        .with_children(|bar| {
            for slot in 0..SLOT_COUNT {
                bar.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
                    ManageAction::CycleSlot(slot),
                    children![(ActionSlotText(slot), label("", 16.0))],
                ));
            }
        });
}

fn update_header(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&InShelter, With<Cell>>,
    mut header: Query<&mut Text, With<HeaderText>>,
) {
    let Some(shelter) = local_cell(&players).and_then(|c| cells.get(c).ok()) else {
        return;
    };
    let Ok(mut text) = header.single_mut() else {
        return;
    };
    let paused = sim_paused(shelter.0, true);
    let line = format!(
        "Manage  -  {}    Tab: back to run   Esc: menu",
        if paused {
            "in a shelter: time is stopped"
        } else {
            "time keeps running"
        }
    );
    if text.0 != line {
        text.0 = line;
    }
}

fn update_action_bar(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&ActionBar, &PathwayLevels, &ActionState, &Proteome), With<Cell>>,
    mut texts: Query<(&ActionSlotText, &mut Text)>,
) {
    let Some((bar, levels, state, proteome)) = local_cell(&players).and_then(|c| cells.get(c).ok())
    else {
        return;
    };
    for (slot, mut text) in &mut texts {
        let line = format!("[{}] {}", slot.0 + 1, slot_status(bar.slots[slot.0], levels, state, proteome));
        if text.0 != line {
            text.0 = line;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::create_starter_genome;

    #[test]
    fn every_gene_state_has_its_own_text() {
        let states = [
            GeneState::Silent,
            GeneState::Expressed,
            GeneState::Mutated,
            GeneState::Enhanced,
        ];
        let mut names: Vec<_> = states.iter().map(|s| gene_state_text(s).0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn cycling_a_slot_walks_the_options() {
        let genome = create_starter_genome();
        let proteome = Proteome::default();
        let mut bar = ActionBar::default();
        let options = available_actions(&genome, &proteome);
        for expected in &options {
            cycle_slot(&mut bar, 2, &genome, &proteome);
            assert_eq!(bar.slots[2], Some(*expected));
        }
        cycle_slot(&mut bar, 2, &genome, &proteome);
        assert_eq!(bar.slots[2], None);
        // A slot out of range does nothing.
        cycle_slot(&mut bar, 9, &genome, &proteome);
    }
}
