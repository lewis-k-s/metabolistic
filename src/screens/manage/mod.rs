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

use bevy::camera::{ClearColorConfig, ScalingMode};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;

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
mod outside;
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
            outside::plugin,
            panel::plugin,
            inset::plugin,
        ))
        .add_systems(
            OnEnter(GameState::Manage),
            (spawn_map_camera, spawn_header_and_bar),
        )
        .add_systems(OnExit(GameState::Manage), (clear_picks, forget_layout))
        .add_systems(
            Update,
            fit_map.run_if(in_state(GameState::Manage)),
        )
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
/// draws the inset above it (see `inset`). `fit_map` sets its scale and place.
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
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_2d()
        }),
        map_layer(),
        // The map camera shows the UI of this screen.
        IsDefaultUiCamera,
        DespawnOnExit(GameState::Manage),
    ));
}

/// The layout of the map for the current window. A new value (the window
/// changed) makes the map systems spawn their sprites again.
#[derive(Resource, Deref)]
pub(super) struct MapLayout(pub layout::Layout);

/// Marks a sprite of the map that is spawned for one layout.
#[derive(Component)]
pub(super) struct MapEntity;

/// Space in the window that the map leaves for the header and the action bar.
const TOP_MARGIN: f32 = 40.0;
const BOTTOM_MARGIN: f32 = 58.0;

/// Width of the right column (the inset and the side panel), in logical
/// pixels.
pub(super) fn right_column_width(window_width: f32) -> f32 {
    (window_width * 0.27).clamp(300.0, 440.0)
}

/// The scale of the map, in physical pixels for each sprite pixel: a whole
/// number from 2 up (the pixel grid stays even), a quarter step below.
pub fn map_scale(available: Vec2, scale_factor: f32) -> f32 {
    let fit = (available * scale_factor / layout::MIN_SIZE).min_element();
    if fit >= 2.0 {
        (fit + 0.05).floor()
    } else {
        ((fit * 4.0 + 0.2).floor() / 4.0).max(0.5)
    }
}

/// Fits the map to the window: the camera scale and place, and the layout. The
/// map fills the window left of the right column, between the header and the
/// action bar.
fn fit_map(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    current: Option<Res<MapLayout>>,
    mut camera: Query<(&mut Projection, &mut Transform), With<MapCamera>>,
    old: Query<Entity, With<MapEntity>>,
) {
    let (Ok(window), Ok((mut projection, mut transform))) = (window.single(), camera.single_mut())
    else {
        return;
    };
    let (width, height) = (window.width(), window.height());
    let left = (width - right_column_width(width) - 8.0).max(200.0);
    let area = Vec2::new(left, (height - TOP_MARGIN - BOTTOM_MARGIN).max(200.0));
    let scale = map_scale(area, window.scale_factor()) / window.scale_factor();
    if let Projection::Orthographic(ortho) = &mut *projection
        && ortho.scale != 1.0 / scale
    {
        ortho.scale = 1.0 / scale;
    }
    // The map is centred in `area`; the camera looks at the window centre.
    let centre = Vec2::new(
        (width / 2.0 - left / 2.0) / scale,
        (TOP_MARGIN + area.y / 2.0 - height / 2.0) / scale,
    )
    .round();
    transform.translation.x = centre.x;
    transform.translation.y = centre.y;
    let wanted = layout::Layout::new((area / scale).floor());
    if current.is_none_or(|c| c.0 != wanted) {
        for entity in &old {
            commands.entity(entity).despawn();
        }
        commands.insert_resource(MapLayout(wanted));
    }
}

fn forget_layout(mut commands: Commands) {
    commands.remove_resource::<MapLayout>();
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


// --- Hooks for the smoke test ---

/// A thing on the map, for the smoke test to point at (the same picks that a
/// click or a hover sets).
#[derive(Debug, Clone, Copy)]
pub enum SmokePick {
    Pathway(BlockKind),
    Slot(usize),
    Pool(Currency),
}

impl From<SmokePick> for Pick {
    fn from(pick: SmokePick) -> Pick {
        match pick {
            SmokePick::Pathway(k) => Pick::Pathway(k),
            SmokePick::Slot(i) => Pick::Slot(i),
            SmokePick::Pool(c) => Pick::Pool(c),
        }
    }
}

/// What the smoke test reads from the screen.
#[derive(Debug, Default)]
pub struct SmokeProbe {
    /// The title of the side panel.
    pub panel_title: Option<String>,
    pub tooltip_visible: bool,
    pub tooltip_text: String,
    /// The pathways that show the "!" badge.
    pub badges: Vec<BlockKind>,
    /// Pipes that show the red pulse.
    pub limiting_pipes: usize,
    /// The + buttons of the panel.
    pub plus_buttons: Vec<ProteinTarget>,
}

/// Sends what a click on the thing sends: the panel opens (or closes, for
/// `None`).
pub fn smoke_select(world: &mut World, pick: Option<SmokePick>) {
    world.resource_mut::<Selection>().0 = pick.map(Pick::from);
}

/// Sends what the pointer sends when it is over the thing.
pub fn smoke_hover(world: &mut World, pick: Option<SmokePick>) {
    world.resource_mut::<Hovered>().0 = pick.map(Pick::from);
}

/// Presses the + button of a protein in the panel, for one frame. Call it
/// after the UI focus system (`PreUpdate`), so the press is kept. Returns
/// false if the panel has no such button.
pub fn smoke_press_plus(world: &mut World, target: ProteinTarget) -> bool {
    let mut found = false;
    let mut buttons = world.query::<(&ManageAction, &mut Interaction)>();
    for (action, mut interaction) in buttons.iter_mut(world) {
        if *action == ManageAction::Queue(target) {
            *interaction = Interaction::Pressed;
            found = true;
        }
    }
    found
}

pub fn smoke_probe(world: &mut World) -> SmokeProbe {
    let mut probe = SmokeProbe {
        panel_title: panel::shown_title(world),
        ..default()
    };
    let (visible, text) = panel::tooltip_state(world);
    probe.tooltip_visible = visible;
    probe.tooltip_text = text;
    let (badges, limiting) = diagram::probe(world);
    probe.badges = badges;
    probe.limiting_pipes = limiting;
    let mut buttons = world.query::<&ManageAction>();
    probe.plus_buttons = buttons
        .iter(world)
        .filter_map(|a| match a {
            ManageAction::Queue(target) => Some(*target),
            _ => None,
        })
        .collect();
    probe
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
