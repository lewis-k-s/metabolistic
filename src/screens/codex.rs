//! Codex screen: a browser for the catalog of genes and membrane proteins
//! (`crate::codex_catalog`).
//!
//! The screen is an overlay, not a `GameState`. The Manage screen must keep its
//! world, and a state change would remove the run. So the overlay opens over
//! the menu, the Prepare screen or the Manage screen, and the screen under it
//! decides what the detail panel can do (`EditMode`):
//!
//! - Menu: browse only.
//! - Prepare: add a gene to the loadout, or remove it (`prepare::toggle`).
//! - Manage: queue an enzyme or a membrane protein (`QueueProtein`).
//!
//! Keys: C opens and closes it on the Manage screen. Esc closes it. Leaving the
//! screen below closes it too.
//!
//! The layout: a filter row; cards in groups by category; a detail panel. A
//! card shows the icon, the name and a badge. Hover or click on a card shows the
//! entry in the detail panel, and the panel stays on that entry. The card grid
//! is built again only when the data or the filter changes, so cards keep their
//! hover state. The text of the detail panel comes from a pure function
//! (`detail_lines`), so it has tests.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, GlobalZIndex};
use std::collections::BTreeSet;

use super::{label, navigation_input};
use crate::GameState;
use crate::cell::{Cell, CellSize, membrane_slots};
use crate::codex::Codex;
use crate::codex_catalog::{
    Badge, Category, CodexContext, CodexEntry, Edit, EditMode, EntryId, EntryState, Filter, Icon, badge,
    edit_options, entry, entry_state, grouped,
};
use crate::blocks::genome::Genome;
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::prepare::{PrepareChoice, START_BUDGET, toggle};
use crate::proteins::{Proteome, QueueProtein, SynthesisQueue};
use crate::sprites::Sheets;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<CodexUi>()
        .init_resource::<CodexShown>()
        .init_resource::<CodexData>()
        .add_systems(
            Update,
            (
                codex_keys.after(navigation_input),
                close_on_state_change,
                update_data,
                lifecycle,
                show_filter,
                redraw_grid,
                redraw_detail,
                hover_cards,
                highlight_cards,
                scroll_grid,
                codex_buttons,
            )
                .chain(),
        );
}

/// Is the overlay open, and which filter does it use?
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct CodexUi {
    pub open: bool,
    pub filter: Filter,
}

/// The entry that the detail panel shows.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct CodexShown(pub Option<EntryId>);

/// The data that the screen draws, copied from the ECS.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
struct CodexData(CodexContext);

#[derive(Component)]
struct CodexRoot;
#[derive(Component)]
struct FilterLabel;
#[derive(Component)]
struct GridBox;
#[derive(Component)]
struct DetailBox;
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct CodexCard(EntryId);
#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum CodexAction {
    Filter(Filter),
    Close,
    Edit(Edit),
}

// --- Colours ---

const BG: Color = Color::srgb(0.02, 0.03, 0.07);
const CARD_BG: Color = Color::srgb(0.1, 0.12, 0.16);
const CARD_BG_UNKNOWN: Color = Color::srgb(0.06, 0.06, 0.08);
const BUTTON_BG: Color = Color::srgb(0.15, 0.15, 0.15);
const TEXT: Color = Color::srgb(0.9, 0.9, 0.9);
const DIM: Color = Color::srgb(0.55, 0.58, 0.62);
const GOOD: Color = Color::srgb(0.45, 0.85, 0.5);
const BAD: Color = Color::srgb(0.9, 0.4, 0.35);
const GOLD: Color = Color::srgb(0.95, 0.8, 0.35);
const CARD_BG_PLANNED: Color = Color::srgb(0.09, 0.08, 0.12);
const CARD_SELECTED: Color = Color::srgb(0.2, 0.26, 0.36);
const OUTLINE_UNKNOWN: Color = Color::srgba(0.55, 0.58, 0.62, 0.35);
const PLANNED_EDGE: Color = Color::srgb(0.5, 0.42, 0.7);
const CARD_W: f32 = 132.0;
const CARD_H: f32 = 156.0;
/// The icon is drawn at a whole multiple of its 16 px art, so pixels stay crisp.
const CARD_ICON: f32 = 80.0;
const DETAIL_ICON: f32 = 96.0;

fn badge_color(badge: Badge) -> Color {
    match badge {
        Badge::Unknown => DIM,
        Badge::InCodex => Color::srgb(0.55, 0.7, 0.95),
        Badge::Expressed => GOOD,
        Badge::Enhanced => GOLD,
        Badge::Mutated => BAD,
        Badge::Silent => Color::srgb(0.75, 0.65, 0.45),
        Badge::Protein => Color::srgb(0.4, 0.8, 0.8),
    }
}

// --- Pure text ---

/// The name on a card: "?" for an unknown gene.
pub fn card_name(entry: &CodexEntry, state: &EntryState) -> String {
    if state.known { entry.name.to_string() } else { "?".to_string() }
}

/// The badge text on a card, with the count of enzymes or proteins.
pub fn badge_text(entry: &CodexEntry, state: &EntryState) -> String {
    let b = badge(entry.id, state);
    match (b, state.count) {
        (Badge::Protein, n) => format!("x{n}"),
        (Badge::Unknown | Badge::InCodex, _) => b.text().to_string(),
        (_, 0) => b.text().to_string(),
        (_, n) => format!("{} x{n}", b.text()),
    }
}

/// How a line of the detail panel looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Title,
    Normal,
    Dim,
    Good,
    Bad,
}

fn rate_list(items: &[(Currency, f32)], sign: f32) -> String {
    items
        .iter()
        .map(|(c, r)| format!("{c:?} {:.1}", r * sign))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The lines of the detail panel for an entry. An unknown entry shows no name
/// and no numbers.
pub fn detail_lines(entry: &CodexEntry, state: &EntryState) -> Vec<(String, Tone)> {
    use Tone::*;
    let mut out = Vec::new();
    let b = badge(entry.id, state);
    if !state.known {
        out.push(("?".to_string(), Title));
        out.push((format!("{} (unknown)", entry.category.name()), Dim));
        out.push(("You have not found this gene yet. Take it in a run to add it to the codex.".to_string(), Normal));
        return out;
    }
    out.push((entry.name.to_string(), Title));
    let mut place = entry.category.name().to_string();
    if let Some(stage) = entry.stage {
        place.push_str(&format!("  |  first in: {}", stage.name()));
    }
    out.push((place, Dim));
    let sources: Vec<_> = entry.sources.iter().map(|s| s.name()).collect();
    out.push((format!("Source: {}", sources.join(", ")), Dim));
    let tone = match b {
        Badge::Expressed | Badge::Enhanced | Badge::Protein => Good,
        Badge::Mutated => Bad,
        _ => Normal,
    };
    let mut status = match b {
        Badge::Protein => "Membrane protein".to_string(),
        other => format!("State: {}", other.text()),
    };
    if state.count > 0 || state.queued > 0 {
        let what = if entry.is_gene() { "enzymes" } else { "on this cell" };
        status.push_str(&format!("  |  {} {what}", state.count));
        if state.queued > 0 {
            status.push_str(&format!(", {} queued", state.queued));
        }
    }
    out.push((status, tone));
    out.push((entry.effect.clone(), Normal));
    let used: Vec<_> = entry.flows.iter().copied().filter(|(_, r)| *r < 0.0).collect();
    let made: Vec<_> = entry.flows.iter().copied().filter(|(_, r)| *r > 0.0).collect();
    if !used.is_empty() {
        out.push((format!("Uses per second: {}", rate_list(&used, -1.0)), Normal));
    }
    if !made.is_empty() {
        out.push((format!("Makes per second: {}", rate_list(&made, 1.0)), Normal));
    }
    let mut needs: Vec<String> = entry.cofactors.iter().map(|c| format!("{c:?} (cofactor)")).collect();
    needs.extend(entry.outside.iter().map(|c| format!("{c:?} (outside)")));
    if !needs.is_empty() {
        out.push((format!("Needs: {}", needs.join(", ")), Dim));
    }
    let cost = &entry.cost;
    let what = if entry.is_gene() { "one enzyme" } else { "one protein" };
    out.push((
        format!(
            "Build {what}: {} protein units ({} amino acids, {} ATP)",
            cost.build_units, cost.build_amino, cost.build_atp
        ),
        Dim,
    ));
    if let Some(points) = cost.prepare_points {
        out.push((format!("Prepare: {points} point (budget {START_BUDGET})"), Dim));
    }
    if !cost.per_use.is_empty() {
        out.push((format!("Each use: {}", rate_list(&cost.per_use, 1.0)), Dim));
    }
    if !cost.per_second.is_empty() {
        out.push((format!("While held, per second: {}", rate_list(&cost.per_second, 1.0)), Dim));
    }
    out.push((format!("Biology: {}", entry.note), Normal));
    out
}

// --- Systems ---

/// C opens and closes the overlay on the Manage screen. Esc closes it. This runs
/// after `navigation_input`, which does nothing while the overlay is open.
fn codex_keys(keys: Res<ButtonInput<KeyCode>>, state: Res<State<GameState>>, mut ui: ResMut<CodexUi>) {
    if keys.just_pressed(KeyCode::Escape) && ui.open {
        ui.open = false;
    } else if keys.just_pressed(KeyCode::KeyC) && *state.get() == GameState::Manage {
        ui.open = !ui.open;
    }
}

/// The overlay belongs to one screen: it closes when the screen changes.
fn close_on_state_change(state: Res<State<GameState>>, mut ui: ResMut<CodexUi>) {
    if state.is_changed() && ui.open {
        ui.open = false;
    }
}

/// Copies the data that the overlay needs while it is open. It assigns only
/// when the data differ, so that change detection stays useful.
#[allow(clippy::too_many_arguments)]
fn update_data(
    ui: Res<CodexUi>,
    state: Res<State<GameState>>,
    codex: Res<Codex>,
    choice: Res<PrepareChoice>,
    player: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Genome, &Proteome, &SynthesisQueue, &CellSize), With<Cell>>,
    mut data: ResMut<CodexData>,
) {
    if !ui.open {
        return;
    }
    let genes: BTreeSet<_> = codex.genes.clone();
    let cell = player.single().ok().and_then(|c| cells.get(c.0).ok());
    let ctx = match (state.get(), cell) {
        (GameState::Manage, Some((genome, proteome, queue, size))) => {
            CodexContext::of_cell(genes, genome, proteome, queue, membrane_slots(size))
        }
        (GameState::Prepare, _) => CodexContext::without_run(EditMode::Prepare, genes, choice.0.clone()),
        _ => CodexContext::without_run(EditMode::Browse, genes, BTreeSet::new()),
    };
    if data.0 != ctx {
        data.0 = ctx;
    }
}

/// Spawns the overlay when it opens and removes it when it closes.
fn lifecycle(ui: Res<CodexUi>, roots: Query<Entity, With<CodexRoot>>, mut commands: Commands) {
    if !ui.open {
        for root in &roots {
            commands.entity(root).despawn();
        }
        return;
    }
    if !roots.is_empty() {
        return;
    }
    commands
        .spawn((
            Name::new("Codex Screen"),
            CodexRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(14.0)),
                ..default()
            },
            BackgroundColor(BG),
            GlobalZIndex(50),
            // The overlay takes the clicks, so nothing under it reacts.
            Interaction::default(),
            FocusPolicy::Block,
        ))
        .with_children(|root| {
            root.spawn(Node {
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|row| {
                row.spawn(label("Codex", 32.0));
                for filter in Filter::ALL {
                    row.spawn(text_button(filter.name(), CodexAction::Filter(filter), true));
                }
                row.spawn((FilterLabel, Text::new(""), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(GOLD)));
                row.spawn(text_button("Close (Esc)", CodexAction::Close, true));
            });
            root.spawn(Node {
                flex_grow: 1.0,
                column_gap: Val::Px(14.0),
                min_height: Val::Px(0.0),
                ..default()
            })
            .with_children(|body| {
                body.spawn((
                    GridBox,
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                ));
                body.spawn((
                    DetailBox,
                    Node {
                        width: Val::Px(360.0),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        padding: UiRect::all(Val::Px(12.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.06, 0.08, 0.12)),
                ));
            });
        });
}

fn text_button(text: &str, action: CodexAction, enabled: bool) -> impl Bundle {
    (
        Button,
        Node {
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON_BG),
        action,
        children![(
            Text::new(text),
            TextFont { font_size: FontSize::Px(18.0), ..default() },
            TextColor(if enabled { TEXT } else { DIM }),
        )],
    )
}

/// The image of an icon. `None` if there is no art, or the sheet is not loaded.
fn icon_image(icon: Icon, server: &AssetServer, sheets: Option<&Sheets>) -> Option<ImageNode> {
    match icon {
        Icon::Image(path) => Some(ImageNode::new(server.load(path.to_string()))),
        Icon::Overlay(name) => {
            let sheet = sheets?.sheet(name, "idle")?;
            Some(ImageNode::from_atlas_image(
                sheet.image.clone(),
                TextureAtlas { layout: sheet.layout.clone(), index: 0 },
            ))
        }
        Icon::Missing => None,
    }
}

/// An icon box: the image, a dim outline with "?" (unknown), or a labelled
/// tile (no art). `size` is a whole multiple of the art, so pixels stay crisp.
fn spawn_icon(
    parent: &mut ChildSpawnerCommands,
    entry: &CodexEntry,
    known: bool,
    size: f32,
    server: &AssetServer,
    sheets: Option<&Sheets>,
) {
    let node = Node { width: Val::Px(size), height: Val::Px(size), ..default() };
    let planned = entry.category == Category::Planned;
    let image = if known { icon_image(entry.icon, server, sheets) } else { None };
    match image {
        Some(image) => {
            parent.spawn((node, image));
        }
        None => {
            let (text, edge, color) = if !known {
                ("?".to_string(), OUTLINE_UNKNOWN, OUTLINE_UNKNOWN)
            } else if planned {
                (entry.name.chars().take(3).collect::<String>().to_uppercase(), PLANNED_EDGE, PLANNED_EDGE)
            } else {
                (entry.name.chars().take(3).collect::<String>().to_uppercase(), DIM, DIM)
            };
            parent.spawn((
                Node {
                    border: UiRect::all(Val::Px(2.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..node
                },
                BorderColor::all(edge),
                BackgroundColor(Color::NONE),
                children![(
                    Text::new(text),
                    TextFont { font_size: FontSize::Px(size * 0.4), ..default() },
                    TextColor(color),
                )],
            ));
        }
    }
}

/// Names the filter that is on.
fn show_filter(ui: Res<CodexUi>, mut labels: Query<&mut Text, With<FilterLabel>>) {
    for mut text in &mut labels {
        let want = format!("Showing: {}", ui.filter.name());
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Builds the card grid again when the data or the filter changes.
fn redraw_grid(
    ui: Res<CodexUi>,
    data: Res<CodexData>,
    grids: Query<Entity, With<GridBox>>,
    fresh: Query<(), Added<GridBox>>,
    server: Res<AssetServer>,
    sheets: Option<Res<Sheets>>,
    mut commands: Commands,
) {
    if !(ui.is_changed() || data.is_changed() || !fresh.is_empty()) {
        return;
    }
    for grid in &grids {
        commands.entity(grid).despawn_children().with_children(|grid| {
            for (category, entries) in grouped(ui.filter, &data.0) {
                grid.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() })
                .with_children(|group| {
                    group.spawn((
                        Text::new(category.name()),
                        TextFont { font_size: FontSize::Px(16.0), ..default() },
                        TextColor(GOLD),
                    ));
                    group
                        .spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), ..default() })
                        .with_children(|row| {
                            for e in &entries {
                                let state = entry_state(e.id, &data.0);
                                spawn_card(row, e, &state, &server, sheets.as_deref());
                            }
                        });
                });
            }
        });
    }
}

fn spawn_card(
    row: &mut ChildSpawnerCommands,
    entry: &CodexEntry,
    state: &EntryState,
    server: &AssetServer,
    sheets: Option<&Sheets>,
) {
    let b = badge(entry.id, state);
    row.spawn((
        Button,
        CodexCard(entry.id),
        Node {
            width: Val::Px(CARD_W),
            height: Val::Px(CARD_H),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::all(Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(card_color(entry, state.known, false)),
        BorderColor::all(if entry.category == Category::Planned { PLANNED_EDGE } else { Color::NONE }),
    ))
    .with_children(|card| {
        spawn_icon(card, entry, state.known, CARD_ICON, server, sheets);
        card.spawn((
            Text::new(card_name(entry, state)),
            TextFont { font_size: FontSize::Px(14.0), ..default() },
            TextColor(if state.known { TEXT } else { DIM }),
            TextLayout::justify(Justify::Center),
        ));
        card.spawn((
            Text::new(badge_text(entry, state)),
            TextFont { font_size: FontSize::Px(14.0), ..default() },
            TextColor(badge_color(b)),
        ));
    });
}

fn card_color(entry: &CodexEntry, known: bool, selected: bool) -> Color {
    if selected {
        CARD_SELECTED
    } else if entry.category == Category::Planned {
        CARD_BG_PLANNED
    } else if known {
        CARD_BG
    } else {
        CARD_BG_UNKNOWN
    }
}

/// The mouse wheel scrolls the card grid.
fn scroll_grid(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut grids: Query<(&mut ScrollPosition, &ComputedNode), With<GridBox>>,
) {
    use bevy::input::mouse::MouseScrollUnit;
    let dy: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y * 40.0,
            MouseScrollUnit::Pixel => w.y,
        })
        .sum();
    if dy == 0.0 {
        return;
    }
    for (mut pos, node) in &mut grids {
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        pos.y = (pos.y - dy).clamp(0.0, max);
    }
}

/// The shown card gets a light fill and a gold border.
fn highlight_cards(
    shown: Res<CodexShown>,
    data: Res<CodexData>,
    mut cards: Query<(&CodexCard, &mut BackgroundColor, &mut BorderColor)>,
) {
    for (card, mut bg, mut border) in &mut cards {
        let e = entry(card.0);
        let selected = shown.0 == Some(card.0);
        let known = entry_state(card.0, &data.0).known;
        bg.0 = card_color(&e, known, selected);
        *border = BorderColor::all(if selected {
            GOLD
        } else if e.category == Category::Planned {
            PLANNED_EDGE
        } else {
            Color::NONE
        });
    }
}

/// Builds the detail panel again when the entry or the data changes.
#[allow(clippy::too_many_arguments)]
fn redraw_detail(
    ui: Res<CodexUi>,
    data: Res<CodexData>,
    shown: Res<CodexShown>,
    boxes: Query<Entity, With<DetailBox>>,
    fresh: Query<(), Added<DetailBox>>,
    server: Res<AssetServer>,
    sheets: Option<Res<Sheets>>,
    mut commands: Commands,
) {
    if !(ui.is_changed() || data.is_changed() || shown.is_changed() || !fresh.is_empty()) {
        return;
    }
    for panel in &boxes {
        commands.entity(panel).despawn_children().with_children(|panel| {
            let Some(id) = shown.0 else {
                panel.spawn(label("Point at a card to read about it.", 18.0));
                return;
            };
            let e = entry(id);
            let state = entry_state(id, &data.0);
            spawn_icon(panel, &e, state.known, DETAIL_ICON, &server, sheets.as_deref());
            for (text, tone) in detail_lines(&e, &state) {
                let (size, color) = match tone {
                    Tone::Title => (26.0, TEXT),
                    Tone::Normal => (15.0, TEXT),
                    Tone::Dim => (14.0, DIM),
                    Tone::Good => (15.0, GOOD),
                    Tone::Bad => (15.0, BAD),
                };
                panel.spawn((Text::new(text), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color)));
            }
            if state.known {
                for option in edit_options(id, &data.0) {
                    panel.spawn(text_button(&option.label, CodexAction::Edit(option.edit), option.enabled));
                    if let Some(note) = option.note {
                        panel.spawn((Text::new(note), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(DIM)));
                    }
                }
            }
        });
    }
}

/// A hovered or clicked card becomes the shown entry.
fn hover_cards(
    cards: Query<(&Interaction, &CodexCard), Changed<Interaction>>,
    mut shown: ResMut<CodexShown>,
) {
    for (interaction, card) in &cards {
        if *interaction != Interaction::None && shown.0 != Some(card.0) {
            shown.0 = Some(card.0);
        }
    }
}

/// Filter, close and edit buttons. Edit buttons whose option is off do nothing.
fn codex_buttons(
    buttons: Query<(&Interaction, &CodexAction), Changed<Interaction>>,
    mut ui: ResMut<CodexUi>,
    data: Res<CodexData>,
    shown: Res<CodexShown>,
    player: Query<&ControlledCell, With<LocalPlayer>>,
    mut choice: ResMut<PrepareChoice>,
    codex: Res<Codex>,
    mut queue: MessageWriter<QueueProtein>,
) {
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            CodexAction::Filter(filter) => {
                if ui.filter != filter {
                    ui.filter = filter;
                }
            }
            CodexAction::Close => ui.open = false,
            CodexAction::Edit(edit) => {
                // Only an edit that is on for the shown entry goes through.
                let allowed = shown
                    .0
                    .is_some_and(|id| edit_options(id, &data.0).iter().any(|o| o.edit == edit && o.enabled));
                if !allowed {
                    continue;
                }
                match edit {
                    Edit::Queue(target) => {
                        if let Ok(cell) = player.single() {
                            queue.write(QueueProtein { cell: cell.0, target });
                        }
                    }
                    Edit::ToggleLoadout(kind) => {
                        toggle(&mut choice.0, kind, &codex, START_BUDGET);
                    }
                }
            }
        }
    }
}

// --- Smoke test hooks ---

/// What the smoke test can see of the overlay.
#[derive(Debug, Default)]
pub struct CodexProbe {
    pub open: bool,
    pub cards: usize,
    pub shown: Option<EntryId>,
    pub edit_buttons: Vec<Edit>,
}

pub fn smoke_open(world: &mut World, open: bool) {
    world.resource_mut::<CodexUi>().open = open;
}

pub fn smoke_show(world: &mut World, id: EntryId) {
    world.resource_mut::<CodexShown>().0 = Some(id);
}

pub fn smoke_probe(world: &mut World) -> CodexProbe {
    let mut probe = CodexProbe {
        open: world.resource::<CodexUi>().open,
        shown: world.resource::<CodexShown>().0,
        ..default()
    };
    probe.cards = world.query::<&CodexCard>().iter(world).count();
    probe.edit_buttons = world
        .query::<&CodexAction>()
        .iter(world)
        .filter_map(|a| if let CodexAction::Edit(e) = a { Some(*e) } else { None })
        .collect();
    probe
}

/// Presses an edit button for one frame. Call it after the UI focus system.
pub fn smoke_press_edit(world: &mut World, edit: Edit) -> bool {
    let mut found = false;
    let mut buttons = world.query::<(&CodexAction, &mut Interaction)>();
    for (action, mut interaction) in buttons.iter_mut(world) {
        if *action == CodexAction::Edit(edit) {
            *interaction = Interaction::Pressed;
            found = true;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::BlockKind;
    use crate::proteins::MembraneProtein;

    fn lines(id: EntryId, ctx: &CodexContext) -> Vec<String> {
        detail_lines(&entry(id), &entry_state(id, ctx)).into_iter().map(|(t, _)| t).collect()
    }

    #[test]
    fn an_unknown_gene_shows_a_question_mark_and_no_numbers() {
        let ctx = CodexContext::without_run(EditMode::Browse, BTreeSet::new(), BTreeSet::new());
        let id = EntryId::Gene(BlockKind::Cuticle);
        let e = entry(id);
        let s = entry_state(id, &ctx);
        assert_eq!(card_name(&e, &s), "?");
        let text = lines(id, &ctx).join("\n");
        assert!(!text.contains("Cuticle") && !text.contains("Biology"));
    }

    #[test]
    fn a_known_gene_shows_flows_cost_and_biology() {
        let ctx = CodexContext::without_run(EditMode::Browse, BTreeSet::new(), BTreeSet::new());
        let text = lines(EntryId::Gene(BlockKind::Chemosynthesis), &ctx).join("\n");
        assert!(text.contains("Makes per second: ATP 2.0"), "{text}");
        assert!(text.contains("Metals (cofactor)") && text.contains("Heat (outside)"), "{text}");
        assert!(text.contains("Build one enzyme: 10 protein units"), "{text}");
        assert!(text.contains("Riftia"), "{text}");
    }

    #[test]
    fn protein_cards_show_the_count_and_costs_of_use() {
        let mut ctx = CodexContext::without_run(EditMode::Run, BTreeSet::new(), BTreeSet::new());
        ctx.proteome.add(crate::proteins::ProteinTarget::Membrane(MembraneProtein::Toxin));
        let id = EntryId::Protein(MembraneProtein::Toxin);
        assert_eq!(badge_text(&entry(id), &entry_state(id, &ctx)), "x1");
        let text = lines(id, &ctx).join("\n");
        assert!(text.contains("Each use: ATP 2.0, Metals 1.0"), "{text}");
    }
}
