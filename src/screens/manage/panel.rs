//! The side panel and the tooltips (Bevy UI).
//!
//! A click on a plate or a socket (`Selection`) opens the panel in the right
//! column, under the run inset:
//!
//! - a pathway: its gene state, the level buttons Off / Low / High
//!   (`SetPathwayLevel`), enzymes with + and - (`QueueProtein` and
//!   `RecycleProtein`), the rates in and out, and the limit in words;
//! - a socket: the membrane proteins with + and -, and the slots used;
//! - a pool: its level and the flows in and out.
//!
//! Hover (`Hovered`) shows a short tooltip at the pointer. The text comes from
//! pure functions of the view (`build_content`, `tooltip_text`), so they have
//! tests. The panel is rebuilt only when its content changes, so buttons keep
//! their hover and press state.

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;
use bevy::window::PrimaryWindow;

use super::diagram::{pool_name, source_name, spaced_name};
use super::inset;
use super::{BAD, DIM, GOLD, GOOD, Hovered, ManageAction, Pick, Selection};
use crate::GameState;
use crate::actions::{HIGH_LEVEL, LEVEL_CHANGE_ATP, LOW_LEVEL, Level, PathwayLevels};
use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellSize, membrane_slots};
use crate::environment::Channel;
use crate::metabolism::BlockStatus;
use crate::metabolism::solver::Limit;
use crate::metabolism::view::{EdgeKind, LocalMetabolismView, MetabolismView, Node as FlowNode, net_rate};
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{
    MembraneProtein, ProteinTarget, Proteome, SynthesisQueue, UNITS_PER_PROTEIN,
    membrane_slot_usage,
};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PanelState>()
        .add_systems(OnEnter(GameState::Manage), (spawn_column, spawn_tooltip))
        .add_systems(OnExit(GameState::Manage), |mut s: ResMut<PanelState>| {
            *s = PanelState::default()
        })
        .add_systems(
            Update,
            (layout_column, refresh_panel, update_tooltip)
                .chain()
                .run_if(in_state(GameState::Manage)),
        );
}

const PANEL_BG: Color = Color::srgba(0.02, 0.04, 0.09, 0.94);
const TEXT: Color = Color::srgb(0.9, 0.9, 0.9);
const BUTTON_BG: BackgroundColor = BackgroundColor(Color::srgb(0.15, 0.15, 0.15));
/// Height of the inset frame in logical pixels, from the column width.
const INSET_ASPECT: f32 = 0.7;

// --- Words ---

/// The name of a protein target.
pub(super) fn protein_name(target: ProteinTarget) -> String {
    match target {
        ProteinTarget::Enzyme(kind) => format!("{} enzyme", spaced_name(kind)),
        ProteinTarget::Membrane(p) => format!("{p:?}"),
    }
}

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Heat => "heat",
        Channel::Minerals => "vent minerals",
        Channel::Light => "light",
        Channel::O2 => "oxygen",
        Channel::Water => "water",
    }
}


/// The limit of a pathway in words.
pub fn limit_words(limit: Limit) -> String {
    match limit {
        Limit::None => "Running at full rate".into(),
        Limit::Off => "Off: the gene is silent or the valve is closed".into(),
        Limit::NoEnzymes => "No enzymes: queue some with +".into(),
        Limit::External(channel) => format!("Waiting for {}", channel_name(channel)),
        Limit::Input(c) => format!("Short of {}", pool_name(c)),
        Limit::Cofactor(c) => format!("Short of {} (a cofactor: needed, not used up)", pool_name(c)),
        Limit::Shared(c) => format!("{} is shared with other pathways", pool_name(c)),
    }
}

fn status_text(state: Option<BlockStatus>) -> (&'static str, Color) {
    match state {
        Some(BlockStatus::Active) => ("Expressed", GOOD),
        Some(BlockStatus::Silent) => ("Silent", DIM),
        Some(BlockStatus::Mutated) => ("Mutated", BAD),
        Some(BlockStatus::Enhanced) => ("Enhanced x1.25", GOLD),
        None => ("Not in the genome", DIM),
    }
}

fn valve_words(throttle: f32) -> &'static str {
    if throttle < 0.05 {
        "closed"
    } else if throttle < (LOW_LEVEL + HIGH_LEVEL) / 2.0 + 0.01 {
        "low"
    } else {
        "open"
    }
}

fn rate(amount: f32) -> String {
    format!("{amount:.2}/s")
}

// --- Content ---

#[derive(Debug, Clone, PartialEq)]
pub struct Line(pub String, pub Color);

/// A protein row: name, count, + and -.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub target: ProteinTarget,
    pub count: u32,
    pub can_queue: bool,
    pub note: Option<String>,
}

/// What the panel shows. A new value is drawn only when it differs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Content {
    pub title: String,
    pub lines: Vec<Line>,
    /// The level buttons of an expressed pathway, and the level set now.
    pub level: Option<(BlockKind, Level)>,
    pub rows: Vec<Row>,
}

/// The cell data that the panel needs besides the view.
pub struct CellData<'a> {
    pub proteome: &'a Proteome,
    pub queue: &'a SynthesisQueue,
    pub slots: u32,
    pub levels: &'a PathwayLevels,
}

fn line(text: impl Into<String>, color: Color) -> Line {
    Line(text.into(), color)
}

fn queue_lines(cell: &CellData) -> Vec<Line> {
    let mut out = Vec::new();
    if cell.queue.is_empty() {
        out.push(line("Queue: nothing", DIM));
        return out;
    }
    for (i, order) in cell.queue.orders.iter().enumerate() {
        let text = if i == 0 {
            format!(
                "Queue 1: {}  {:.0}/{}",
                protein_name(*order),
                cell.queue.progress,
                UNITS_PER_PROTEIN
            )
        } else {
            format!("Queue {}: {}", i + 1, protein_name(*order))
        };
        out.push(line(text, TEXT));
    }
    out
}

/// The flows in and out of a pool, as lines.
fn pool_flows(view: &MetabolismView, currency: Currency) -> (Vec<Line>, Vec<Line>) {
    let mut ins = Vec::new();
    let mut outs = Vec::new();
    for e in &view.edges {
        if e.currency != Some(currency) {
            continue;
        }
        match (e.kind, e.from, e.to) {
            (EdgeKind::Output, FlowNode::Pathway(k), FlowNode::Pool(_)) => {
                ins.push(line(format!("  {} {}", spaced_name(k), rate(e.flux)), TEXT));
            }
            (EdgeKind::Input, FlowNode::Pool(_), FlowNode::Pathway(k)) => {
                outs.push(line(format!("  {} {}", spaced_name(k), rate(e.flux)), TEXT));
            }
            (EdgeKind::Other, FlowNode::Source(s), FlowNode::Pool(_)) if e.flux > 0.0 => {
                ins.push(line(format!("  {} {}", source_name(s), rate(e.flux)), TEXT));
            }
            (EdgeKind::Other, FlowNode::Pool(_), FlowNode::Source(s)) if e.flux > 0.0 => {
                outs.push(line(format!("  {} {}", source_name(s), rate(e.flux)), TEXT));
            }
            _ => {}
        }
    }
    (ins, outs)
}

/// The panel content for a selection.
pub fn build_content(pick: Option<Pick>, view: &MetabolismView, cell: &CellData) -> Content {
    match pick {
        None => Content {
            title: "Cell map".into(),
            lines: vec![
                line(
                    "Click a pathway plate for its rates, enzymes and level, or a socket in the membrane to add proteins. Click the valve on a plate to turn the pathway Off, Low or High. Keys 1-4 put active skills in the action slots.",
                    TEXT,
                ),
                line(
                    "The small tags on the left of a pool are flows that are not pathways: U upkeep, M movement, A actions, R membrane repair, G genome repair, F food, E engulfed cell, C recycled protein.",
                    DIM,
                ),
            ],
            ..default()
        },
        Some(Pick::Pathway(kind)) => pathway_content(kind, view, cell),
        Some(Pick::Slot(index)) => slot_content(index, cell),
        Some(Pick::Pool(currency)) => {
            let level = view.pool(currency).map_or(0.0, |p| p.level);
            let net = net_rate(view, currency);
            let (ins, outs) = pool_flows(view, currency);
            let mut lines = vec![
                line(format!("Level {level:.1}"), TEXT),
                line(
                    format!("Net {}{}", if net >= 0.0 { "+" } else { "" }, rate(net)),
                    if net < -0.005 { BAD } else { GOOD },
                ),
                line("In:", DIM),
            ];
            if ins.is_empty() {
                lines.push(line("  nothing makes it", DIM));
            }
            lines.extend(ins);
            lines.push(line("Out:", DIM));
            if outs.is_empty() {
                lines.push(line("  nothing uses it", DIM));
            }
            lines.extend(outs);
            Content {
                title: pool_name(currency).to_string(),
                lines,
                ..default()
            }
        }
    }
}

fn pathway_content(kind: BlockKind, view: &MetabolismView, cell: &CellData) -> Content {
    let Some(p) = view.pathway(kind) else {
        return Content {
            title: spaced_name(kind),
            ..default()
        };
    };
    let title = if p.known { spaced_name(kind) } else { "Unknown pathway".to_string() };
    if !p.owned {
        let mut lines = vec![line("The cell does not have this gene.", DIM)];
        if p.known {
            lines.push(line("Find it in the world: a gate cell", DIM));
            lines.push(line("gives one gene at the end of a stage.", DIM));
        } else {
            lines.push(line("The codex does not know it yet.", DIM));
        }
        return Content { title, lines, ..default() };
    }
    let (state, color) = status_text(p.state);
    let mut lines = vec![
        line(format!("Gene: {state}"), color),
        line(
            format!("Activity {:.0}%   valve {}", p.activity * 100.0, valve_words(p.throttle)),
            TEXT,
        ),
    ];
    let mut ins = Vec::new();
    let mut outs = Vec::new();
    let mut needs = Vec::new();
    for e in &view.edges {
        let to_this = e.to == FlowNode::Pathway(kind);
        let from_this = e.from == FlowNode::Pathway(kind);
        match e.kind {
            EdgeKind::Input if to_this => ins.push(line(
                format!("  {} {}", pool_name(e.currency.unwrap_or(Currency::ATP)), rate(e.flux)),
                TEXT,
            )),
            EdgeKind::Output if from_this => outs.push(line(
                format!("  {} {}", pool_name(e.currency.unwrap_or(Currency::ATP)), rate(e.flux)),
                TEXT,
            )),
            EdgeKind::Cofactor if to_this => needs.push(line(
                format!("  {} (cofactor)", pool_name(e.currency.unwrap_or(Currency::ATP))),
                DIM,
            )),
            EdgeKind::Outside if to_this => {
                if let FlowNode::Outside(channel) = e.from {
                    needs.push(line(format!("  {} (outside)", channel_name(channel)), DIM));
                }
            }
            _ => {}
        }
    }
    if !ins.is_empty() {
        lines.push(line("Uses:", DIM));
        lines.extend(ins);
    }
    if !outs.is_empty() {
        lines.push(line("Makes:", DIM));
        lines.extend(outs);
    }
    if !needs.is_empty() {
        lines.push(line("Needs:", DIM));
        lines.extend(needs);
    }
    lines.push(line(
        format!("Limit: {}", limit_words(p.limit)),
        if matches!(p.limit, Limit::None) { GOOD } else { BAD },
    ));
    lines.extend(queue_lines(cell));
    let expressed = matches!(p.state, Some(BlockStatus::Active | BlockStatus::Enhanced));
    Content {
        title,
        lines,
        level: expressed.then(|| (kind, cell.levels.level(kind))),
        rows: vec![Row {
            target: ProteinTarget::Enzyme(kind),
            count: p.enzymes,
            can_queue: true,
            note: None,
        }],
    }
}

fn slot_content(index: usize, cell: &CellData) -> Content {
    let (used, total) = membrane_slot_usage(cell.queue, cell.proteome, cell.slots);
    let contents = super::membrane_view::slot_contents(cell.proteome, cell.queue, cell.slots as usize);
    let here = contents.get(index).copied();
    let mut lines = vec![
        line(format!("Slots used {used}/{total}"), if used < total { TEXT } else { BAD }),
        line(
            match here {
                Some(super::membrane_view::SlotContent::Used(p)) => format!("This socket holds {p:?}."),
                Some(super::membrane_view::SlotContent::Queued { protein, .. }) => {
                    format!("{protein:?} is being built for this socket.")
                }
                _ => "This socket is empty.".to_string(),
            },
            TEXT,
        ),
    ];
    lines.extend(queue_lines(cell));
    let rows = MembraneProtein::ALL
        .into_iter()
        .map(|p| Row {
            target: ProteinTarget::Membrane(p),
            count: cell.proteome.count(ProteinTarget::Membrane(p)),
            can_queue: used < total,
            note: None,
        })
        .collect();
    Content {
        title: format!("Membrane socket {}", index + 1),
        lines,
        level: None,
        rows,
    }
}

/// A short line for the tooltip.
pub fn tooltip_text(pick: Pick, view: &MetabolismView, cell: &CellData) -> String {
    match pick {
        Pick::Pool(currency) => {
            let level = view.pool(currency).map_or(0.0, |p| p.level);
            let net = net_rate(view, currency);
            format!("{}: {level:.1}  ({}{})", pool_name(currency), if net >= 0.0 { "+" } else { "" }, rate(net))
        }
        Pick::Pathway(kind) => match view.pathway(kind) {
            Some(p) if !p.owned => {
                if p.known {
                    format!("{} (no gene)", spaced_name(kind))
                } else {
                    "? (unknown pathway)".to_string()
                }
            }
            Some(p) => {
                let mut tip = format!(
                    "{}: {} enzymes, {:.0}%\n{}",
                    spaced_name(kind),
                    p.enzymes,
                    p.activity * 100.0,
                    limit_words(p.limit)
                );
                if matches!(p.state, Some(BlockStatus::Active | BlockStatus::Enhanced)) {
                    tip.push_str(&format!("\nLevel {} (click the valve)", cell.levels.level(kind).name()));
                }
                tip
            }
            None => spaced_name(kind),
        },
        Pick::Slot(index) => {
            let contents = super::membrane_view::slot_contents(cell.proteome, cell.queue, cell.slots as usize);
            match contents.get(index) {
                Some(super::membrane_view::SlotContent::Used(p)) => format!("Socket {}: {p:?}", index + 1),
                Some(super::membrane_view::SlotContent::Queued { protein, .. }) => {
                    format!("Socket {}: {protein:?} (building)", index + 1)
                }
                _ => format!("Socket {}: empty", index + 1),
            }
        }
    }
}

// --- UI ---

#[derive(Component)]
struct RightColumn;

#[derive(Component)]
struct InsetHolder;

#[derive(Component)]
struct PanelBody;

#[derive(Component)]
struct Tooltip;

#[derive(Component)]
struct TooltipText;

#[derive(Resource, Default)]
struct PanelState {
    shown: Option<Content>,
}

fn spawn_column(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Right Column"),
            RightColumn,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(340.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            DespawnOnExit(GameState::Manage),
        ))
        .with_children(|column| {
            column.spawn((InsetHolder, inset::frame_bundle(220.0)));
            column.spawn((
                PanelBody,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
            ));
        });
}

fn spawn_tooltip(mut commands: Commands) {
    commands.spawn((
        Name::new("Tooltip"),
        Tooltip,
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.05, 0.92)),
        GlobalZIndex(10),
        Visibility::Hidden,
        DespawnOnExit(GameState::Manage),
        children![(TooltipText, super::label("", 15.0))],
    ));
}

/// Sets the width of the right column from the map: the column is the part of
/// the window that the map leaves free. The inset keeps its aspect.
fn layout_column(
    window: Query<&Window, With<PrimaryWindow>>,
    mut column: Query<&mut Node, (With<RightColumn>, Without<InsetHolder>)>,
    mut holder: Query<&mut Node, (With<InsetHolder>, Without<RightColumn>)>,
) {
    let (Ok(window), Ok(mut column), Ok(mut holder)) =
        (window.single(), column.single_mut(), holder.single_mut())
    else {
        return;
    };
    let width = super::right_column_width(window.width());
    column.width = Val::Px(width);
    holder.height = Val::Px(((width - 16.0) * INSET_ASPECT).round());
}

#[allow(clippy::too_many_arguments)]
fn refresh_panel(
    mut commands: Commands,
    selection: Res<Selection>,
    view: Res<LocalMetabolismView>,
    mut state: ResMut<PanelState>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Proteome, &SynthesisQueue, &CellSize, &PathwayLevels), With<Cell>>,
    body: Query<(Entity, Option<&Children>), With<PanelBody>>,
) {
    let Some(view) = &view.0 else { return };
    let Some((proteome, queue, size, levels)) = super::local_cell(&players).and_then(|c| cells.get(c).ok())
    else {
        return;
    };
    let cell = CellData {
        proteome,
        queue,
        slots: membrane_slots(size),
        levels,
    };
    let content = build_content(selection.0, view, &cell);
    if state.shown.as_ref() == Some(&content) {
        return;
    }
    let Ok((body, children)) = body.single() else { return };
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }
    commands.entity(body).with_children(|panel| {
        panel.spawn(super::label(content.title.clone(), 22.0));
        for Line(text, color) in &content.lines {
            panel.spawn(text_line(text, *color));
        }
        if let Some((kind, level)) = content.level {
            spawn_level_row(panel, kind, level);
        }
        for row in &content.rows {
            spawn_row(panel, row);
        }
    });
    state.shown = Some(content);
}

fn text_line(text: &str, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(color),
    )
}

fn spawn_row(panel: &mut ChildSpawnerCommands, row: &Row) {
    panel
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        })
        .with_children(|r| {
            r.spawn((
                Node {
                    width: Val::Px(170.0),
                    ..default()
                },
                children![text_line(&format!("{} x{}", protein_name(row.target), row.count), TEXT)],
            ));
            r.spawn(small_button("+", row.can_queue.then_some(ManageAction::Queue(row.target))));
            r.spawn(small_button("-", (row.count > 0).then_some(ManageAction::Recycle(row.target))));
            if let Some(note) = &row.note {
                r.spawn(text_line(note, DIM));
            }
        });
}

/// The row of level buttons: the level set now is lit.
fn spawn_level_row(panel: &mut ChildSpawnerCommands, kind: BlockKind, current: Level) {
    panel
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        })
        .with_children(|r| {
            r.spawn(text_line("Level", TEXT));
            for level in [Level::Off, Level::Low, Level::High] {
                let lit = level == current;
                r.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                        ..default()
                    },
                    if lit {
                        BackgroundColor(Color::srgb(0.2, 0.42, 0.25))
                    } else {
                        BUTTON_BG
                    },
                    ManageAction::SetLevel(kind, level),
                    children![text_line(level.name(), if lit { Color::WHITE } else { DIM })],
                ));
            }
            r.spawn(text_line(&format!("{LEVEL_CHANGE_ATP} ATP"), DIM));
        });
}

fn small_button(text: &str, action: Option<ManageAction>) -> impl Bundle {
    let enabled = action.is_some();
    (
        Button,
        Node {
            width: Val::Px(30.0),
            height: Val::Px(26.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BUTTON_BG,
        // A button with no action looks the same but does nothing.
        action.unwrap_or(ManageAction::CycleSlot(usize::MAX)),
        children![(
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(if enabled { Color::WHITE } else { DIM }),
        )],
    )
}

/// Shows the tooltip at the pointer for the hovered thing.
#[allow(clippy::too_many_arguments)]
fn update_tooltip(
    hovered: Res<Hovered>,
    view: Res<LocalMetabolismView>,
    window: Query<&Window, With<PrimaryWindow>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Proteome, &SynthesisQueue, &CellSize, &PathwayLevels), With<Cell>>,
    mut tooltip: Query<(&mut Node, &mut Visibility), With<Tooltip>>,
    mut text: Query<&mut Text, With<TooltipText>>,
) {
    let (Ok((mut node, mut visibility)), Ok(mut text)) = (tooltip.single_mut(), text.single_mut()) else {
        return;
    };
    let (Some(pick), Some(view), Ok(window)) = (hovered.0, &view.0, window.single()) else {
        *visibility = Visibility::Hidden;
        return;
    };
    // Without a cursor (a test sends the hover), the tooltip sits in the middle.
    let cursor = window
        .cursor_position()
        .unwrap_or(Vec2::new(window.width() * 0.3, window.height() * 0.4));
    let (cursor, Some((proteome, queue, size, levels))) = (
        cursor,
        super::local_cell(&players).and_then(|c| cells.get(c).ok()),
    ) else {
        *visibility = Visibility::Hidden;
        return;
    };
    let cell = CellData {
        proteome,
        queue,
        slots: membrane_slots(size),
        levels,
    };
    let line = tooltip_text(pick, view, &cell);
    if text.0 != line {
        text.0 = line;
    }
    node.left = Val::Px((cursor.x + 14.0).min(window.width() - 330.0).max(0.0));
    node.top = Val::Px(cursor.y + 16.0);
    *visibility = Visibility::Inherited;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metabolism::view::{EdgeView, PathwayView, PoolView};

    fn pathway(kind: BlockKind, owned: bool, limit: Limit) -> PathwayView {
        PathwayView {
            kind,
            owned,
            known: owned,
            state: owned.then_some(BlockStatus::Active),
            enzymes: if owned { 2 } else { 0 },
            activity: 0.5,
            throttle: 1.0,
            limit,
        }
    }

    fn view() -> MetabolismView {
        MetabolismView {
            pools: vec![
                PoolView { currency: Currency::Pyruvate, level: 12.0 },
                PoolView { currency: Currency::ATP, level: 40.0 },
            ],
            pathways: vec![
                pathway(BlockKind::Fermentation, true, Limit::Input(Currency::Pyruvate)),
                pathway(BlockKind::Respiration, false, Limit::None),
            ],
            edges: vec![
                EdgeView {
                    from: FlowNode::Pool(Currency::Pyruvate),
                    to: FlowNode::Pathway(BlockKind::Fermentation),
                    currency: Some(Currency::Pyruvate),
                    flux: 0.4,
                    kind: EdgeKind::Input,
                },
                EdgeView {
                    from: FlowNode::Pathway(BlockKind::Fermentation),
                    to: FlowNode::Pool(Currency::ATP),
                    currency: Some(Currency::ATP),
                    flux: 0.4,
                    kind: EdgeKind::Output,
                },
            ],
        }
    }

    fn cell<'a>(proteome: &'a Proteome, queue: &'a SynthesisQueue) -> CellData<'a> {
        let levels: &'static PathwayLevels = Box::leak(Box::default());
        CellData { proteome, queue, slots: 6, levels }
    }

    #[test]
    fn every_limit_has_its_own_words() {
        let limits = [
            Limit::None,
            Limit::Off,
            Limit::NoEnzymes,
            Limit::External(Channel::Heat),
            Limit::Input(Currency::Pyruvate),
            Limit::Cofactor(Currency::Metals),
            Limit::Shared(Currency::ATP),
        ];
        let mut words: Vec<_> = limits.iter().map(|l| limit_words(*l)).collect();
        words.sort();
        words.dedup();
        assert_eq!(words.len(), limits.len());
        assert!(limit_words(Limit::Input(Currency::Pyruvate)).contains("Pyruvate"));
        assert!(limit_words(Limit::External(Channel::Heat)).contains("heat"));
    }

    #[test]
    fn a_pathway_panel_has_rates_limit_and_an_enzyme_row() {
        let (proteome, queue) = (Proteome::default(), SynthesisQueue::default());
        let content = build_content(Some(Pick::Pathway(BlockKind::Fermentation)), &view(), &cell(&proteome, &queue));
        let text: Vec<&str> = content.lines.iter().map(|l| l.0.as_str()).collect();
        assert!(text.iter().any(|l| l.contains("Pyruvate 0.40/s")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("ATP 0.40/s")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("Short of Pyruvate")), "{text:?}");
        assert_eq!(content.rows.len(), 1);
        assert_eq!(content.rows[0].target, ProteinTarget::Enzyme(BlockKind::Fermentation));
        assert_eq!(content.rows[0].count, 2);
        assert_eq!(content.level, Some((BlockKind::Fermentation, Level::High)));
    }

    #[test]
    fn a_ghost_pathway_has_no_rows_and_hides_its_name() {
        let (proteome, queue) = (Proteome::default(), SynthesisQueue::default());
        let content = build_content(Some(Pick::Pathway(BlockKind::Respiration)), &view(), &cell(&proteome, &queue));
        assert!(content.rows.is_empty());
        assert_eq!(content.level, None, "no gene, no level buttons");
        assert_eq!(content.title, "Unknown pathway");
    }

    #[test]
    fn a_socket_panel_lists_every_membrane_protein() {
        let (proteome, queue) = (Proteome::default(), SynthesisQueue::default());
        let content = build_content(Some(Pick::Slot(2)), &view(), &cell(&proteome, &queue));
        assert_eq!(content.rows.len(), MembraneProtein::ALL.len());
        assert!(content.rows.iter().all(|r| r.can_queue), "6 free slots");
        assert_eq!(content.title, "Membrane socket 3");
    }

    #[test]
    fn a_full_membrane_turns_the_plus_buttons_off() {
        let mut proteome = Proteome::default();
        proteome.membrane.insert(MembraneProtein::Flagellum, 6);
        let queue = SynthesisQueue::default();
        let content = build_content(Some(Pick::Slot(0)), &view(), &cell(&proteome, &queue));
        assert!(content.rows.iter().all(|r| !r.can_queue));
    }

    #[test]
    fn a_pool_panel_lists_who_makes_and_who_uses_it() {
        let (proteome, queue) = (Proteome::default(), SynthesisQueue::default());
        let content = build_content(Some(Pick::Pool(Currency::ATP)), &view(), &cell(&proteome, &queue));
        let text: Vec<&str> = content.lines.iter().map(|l| l.0.as_str()).collect();
        assert!(text.iter().any(|l| l.contains("Fermentation 0.40/s")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("nothing uses it")));
    }

    #[test]
    fn the_tooltip_says_the_limit() {
        let (proteome, queue) = (Proteome::default(), SynthesisQueue::default());
        let tip = tooltip_text(Pick::Pathway(BlockKind::Fermentation), &view(), &cell(&proteome, &queue));
        assert!(tip.contains("Short of Pyruvate"), "{tip}");
        let tip = tooltip_text(Pick::Pathway(BlockKind::Respiration), &view(), &cell(&proteome, &queue));
        assert!(tip.contains("unknown"), "{tip}");
    }
}

/// The title of the panel now, for the smoke test.
pub(super) fn shown_title(world: &World) -> Option<String> {
    world
        .resource::<PanelState>()
        .shown
        .as_ref()
        .map(|c| c.title.clone())
}

/// Whether the tooltip is visible, and its text, for the smoke test.
pub(super) fn tooltip_state(world: &mut World) -> (bool, String) {
    let visible = world
        .query_filtered::<&Visibility, With<Tooltip>>()
        .iter(world)
        .any(|v| *v != Visibility::Hidden);
    let text = world
        .query_filtered::<&Text, With<TooltipText>>()
        .iter(world)
        .next()
        .map_or(String::new(), |t| t.0.clone());
    (visible, text)
}
