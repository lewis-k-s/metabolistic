//! Manage screen: the metabolism editor. It reads the local player's cell and
//! shows:
//!
//! - the genome: each gene with its state (Silent / Expressed / Mutated /
//!   Enhanced);
//! - the enzymes of each expressed block, with + (queue one) and - (recycle one,
//!   half of the amino acids come back);
//! - the membrane slots (used / total) and each membrane protein, with + and -.
//!   The + button is off when no slot is free;
//! - the synthesis queue, the progress of the first order and the time to
//!   finish at the current synthesis rate;
//! - the four action slots. A click (or the keys 1-4) gives the slot the next
//!   thing the cell can do.
//!
//! Game time keeps running here, except in a shelter (see `shelter`). The edit
//! rules are in `proteins` and `actions`; this screen only sends messages and
//! changes the bar. The content is built again a few times a second, and
//! right after a click.

use bevy::prelude::*;

use super::label;
use crate::actions::{
    ActionBar, ActionState, PathwayLevels, SLOT_COUNT, Throttle, available_actions,
    next_assignment, slot_status,
};
use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::blocks::protein_synthesis::potential_rate;
use crate::cell::{Cell, CellBlocks, CellSize, membrane_slots};
use crate::genome_damage::GenomeIntegrity;
use crate::metabolism::{BlockStatus, BlockTurnover, CurrencyPools, FlowmapTimingConfig};
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{
    MembraneProtein, ProteinTarget, Proteome, QueueProtein, RecycleProtein, SynthesisQueue,
    UNITS_PER_PROTEIN, first_order_eta, membrane_slot_usage, synthesis_eta,
};
use crate::shelter::{InShelter, sim_paused};
use crate::GameState;

/// Seconds between rebuilds of the content (real time).
const REFRESH_SECONDS: f32 = 0.25;
const GOOD: Color = Color::srgb(0.45, 0.9, 0.5);
const DIM: Color = Color::srgb(0.5, 0.5, 0.55);
const BAD: Color = Color::srgb(0.95, 0.4, 0.35);
const GOLD: Color = Color::srgb(0.95, 0.8, 0.3);
const PANEL: Color = Color::srgba(0.02, 0.04, 0.09, 0.92);
const SMALL: BackgroundColor = BackgroundColor(Color::srgb(0.15, 0.15, 0.15));

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<RefreshTimer>()
        .add_systems(OnEnter(GameState::Manage), spawn_manage_screen)
        .add_systems(
            Update,
            (manage_buttons, manage_keys, refresh_manage)
                .chain()
                .run_if(in_state(GameState::Manage)),
        );
}

/// What a button of the screen does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ManageAction {
    Queue(ProteinTarget),
    Recycle(ProteinTarget),
    CycleSlot(usize),
}

/// The container that is built again.
#[derive(Component)]
struct ManageBody;

#[derive(Resource, Default)]
struct RefreshTimer {
    seconds: f32,
    /// What the screen showed at the last rebuild. The content is built again
    /// only when this changes, so buttons keep their hover and press state.
    shown: String,
    /// Build again now (a click changed something).
    dirty: bool,
}

fn spawn_manage_screen(mut commands: Commands, mut timer: ResMut<RefreshTimer>) {
    timer.dirty = true;
    commands.spawn((
        Name::new("Manage Screen"),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(16.0)),
            row_gap: Val::Px(8.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        DespawnOnExit(GameState::Manage),
        children![(
            ManageBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
        )],
    ));
}

// --- Input ---

#[allow(clippy::type_complexity)]
fn manage_buttons(
    buttons: Query<(&Interaction, &ManageAction), Changed<Interaction>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&mut ActionBar, &Genome, &Proteome), With<Cell>>,
    mut queue: MessageWriter<QueueProtein>,
    mut recycle: MessageWriter<RecycleProtein>,
    mut timer: ResMut<RefreshTimer>,
) {
    let Some(cell) = players.iter().next().map(|c| c.0) else {
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
        timer.dirty = true;
    }
}

/// Keys 1-4 give the slot the next thing the cell can do.
fn manage_keys(
    keys: Res<ButtonInput<KeyCode>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&mut ActionBar, &Genome, &Proteome), With<Cell>>,
    mut timer: ResMut<RefreshTimer>,
) {
    const SLOT_KEYS: [KeyCode; SLOT_COUNT] =
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];
    let Some(cell) = players.iter().next().map(|c| c.0) else {
        return;
    };
    for (slot, key) in SLOT_KEYS.iter().enumerate() {
        if keys.just_pressed(*key)
            && let Ok((mut bar, genome, proteome)) = cells.get_mut(cell)
        {
            cycle_slot(&mut bar, slot, genome, proteome);
            timer.dirty = true;
        }
    }
}

fn cycle_slot(bar: &mut ActionBar, slot: usize, genome: &Genome, proteome: &Proteome) {
    if let Some(content) = bar.slots.get_mut(slot) {
        *content = next_assignment(*content, &available_actions(genome, proteome));
    }
}

// --- Content ---

/// Colour and name of a gene state.
pub fn gene_state_text(state: &GeneState) -> (&'static str, Color) {
    match state {
        GeneState::Expressed => ("Expressed", GOOD),
        GeneState::Silent => ("Silent", DIM),
        GeneState::Mutated => ("Mutated", BAD),
        GeneState::Enhanced => ("Enhanced x1.25", GOLD),
    }
}

/// Membrane proteins that the editor lists.
const LISTED_PROTEINS: [MembraneProtein; 5] = [
    MembraneProtein::Transporter,
    MembraneProtein::Flagellum,
    MembraneProtein::Adhesin,
    MembraneProtein::Toxin,
    MembraneProtein::Receptor,
];

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
        SMALL,
        // A button with no action still has its look, but does nothing.
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

fn coloured(text: impl Into<String>, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(color),
    )
}

fn panel(title: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            padding: UiRect::all(Val::Px(10.0)),
            min_width: Val::Px(300.0),
            ..default()
        },
        BackgroundColor(PANEL),
        children![label(title, 22.0)],
    )
}

fn protein_name(target: ProteinTarget) -> String {
    match target {
        ProteinTarget::Enzyme(kind) => format!("{kind:?}"),
        ProteinTarget::Membrane(p) => format!("{p:?}"),
    }
}

/// A row: a name, a count, and the + and - buttons.
fn protein_row(
    commands: &mut Commands,
    parent: Entity,
    target: ProteinTarget,
    count: u32,
    can_queue: bool,
    note: Option<(String, Color)>,
) {
    let row = commands
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .id();
    commands.entity(parent).add_child(row);
    commands.entity(row).with_children(|r| {
        r.spawn(Node {
            width: Val::Px(300.0),
            ..default()
        })
        .with_child(coloured(
            format!("{} x{count}", protein_name(target)),
            Color::srgb(0.9, 0.9, 0.9),
        ));
        r.spawn(small_button("+", can_queue.then_some(ManageAction::Queue(target))));
        r.spawn(small_button("-", (count > 0).then_some(ManageAction::Recycle(target))));
        if let Some((text, color)) = note {
            r.spawn(coloured(text, color));
        }
    });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn refresh_manage(
    time: Res<Time<Real>>,
    mut timer: ResMut<RefreshTimer>,
    timing: Res<FlowmapTimingConfig>,
    body: Query<(Entity, Option<&Children>), With<ManageBody>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<
        (
            &Genome,
            &Proteome,
            &SynthesisQueue,
            &CellSize,
            &CurrencyPools,
            &CellBlocks,
            &ActionBar,
            &PathwayLevels,
            &ActionState,
            &GenomeIntegrity,
            &InShelter,
        ),
        With<Cell>,
    >,
    blocks: Query<(&crate::metabolism::MetabolicNode, &Throttle, &BlockTurnover)>,
    mut commands: Commands,
) {
    timer.seconds += time.delta_secs();
    if !timer.dirty && timer.seconds < REFRESH_SECONDS {
        return;
    }
    timer.seconds = 0.0;
    let Ok((body, children)) = body.single() else {
        return;
    };
    let Some((genome, proteome, queue, size, pools, cell_blocks, bar, levels, actions, integrity, shelter)) =
        players.iter().find_map(|c| cells.get(c.0).ok())
    else {
        return;
    };
    let mut genes_key: Vec<_> = genome.table.iter().collect();
    genes_key.sort_by_key(|(kind, _)| **kind);
    let mut enzymes_key: Vec<_> = proteome.enzymes.iter().collect();
    enzymes_key.sort();
    let mut membrane_key: Vec<_> = proteome.membrane.iter().collect();
    membrane_key.sort();
    let key = format!(
        "{genes_key:?}{enzymes_key:?}{membrane_key:?}{:?}{:.0}{:.0}{:.0}{:.0}{}{}{:?}",
        queue.orders,
        queue.progress,
        pools.get(Currency::ATP),
        pools.get(Currency::AminoAcids),
        integrity.0 * 100.0,
        shelter.0,
        (0..SLOT_COUNT)
            .map(|s| slot_status(bar.slots[s], levels, actions, proteome))
            .collect::<Vec<_>>()
            .join("|"),
        size.radius,
    );
    if !timer.dirty && timer.shown == key {
        return;
    }
    timer.dirty = false;
    timer.shown = key;
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }

    // Header.
    let header = commands.spawn(Node { column_gap: Val::Px(24.0), align_items: AlignItems::Center, ..default() }).id();
    commands.entity(body).add_child(header);
    let paused = sim_paused(shelter.0, true);
    commands.entity(header).with_children(|h| {
        h.spawn(label("Manage", 40.0));
        h.spawn(coloured(
            if paused {
                "In a shelter: time is stopped"
            } else {
                "Time keeps running"
            },
            if paused { GOOD } else { BAD },
        ));
        h.spawn(coloured(
            format!(
                "ATP {:.0}   AminoAcids {:.0}   Metals {:.0}",
                pools.get(Currency::ATP),
                pools.get(Currency::AminoAcids),
                pools.get(Currency::Metals)
            ),
            Color::srgb(0.9, 0.9, 0.9),
        ));
        h.spawn(coloured("Tab: back to run    Esc: menu", DIM));
    });

    let columns = commands
        .spawn(Node {
            column_gap: Val::Px(12.0),
            align_items: AlignItems::FlexStart,
            ..default()
        })
        .id();
    commands.entity(body).add_child(columns);

    // Genome.
    let genome_panel = commands.spawn(panel("Genome")).id();
    commands.entity(columns).add_child(genome_panel);
    let mut genes: Vec<_> = genome.table.iter().collect();
    genes.sort_by_key(|(kind, _)| **kind);
    for (kind, state) in genes {
        let (text, color) = gene_state_text(state);
        let row = commands.spawn(coloured(format!("{kind:?}: {text}"), color)).id();
        commands.entity(genome_panel).add_child(row);
    }
    let note = commands
        .spawn(coloured(
            format!("Integrity {:.0}%", integrity.0 * 100.0),
            Color::srgb(0.9, 0.9, 0.9),
        ))
        .id();
    commands.entity(genome_panel).add_child(note);

    // Enzymes of expressed blocks (an enhanced block counts).
    let enzyme_panel = commands.spawn(panel("Enzymes (+ queue, - recycle)")).id();
    commands.entity(columns).add_child(enzyme_panel);
    let mut expressed: Vec<BlockKind> = genome
        .table
        .iter()
        .filter(|(kind, state)| {
            crate::blocks::block_spec(**kind).is_some()
                && matches!(state, GeneState::Expressed | GeneState::Enhanced | GeneState::Mutated)
        })
        .map(|(kind, _)| *kind)
        .collect();
    expressed.sort();
    for kind in expressed {
        let target = ProteinTarget::Enzyme(kind);
        protein_row(&mut commands, enzyme_panel, target, proteome.count(target), true, None);
    }

    // Membrane.
    let (used, total) = membrane_slot_usage(queue, proteome, membrane_slots(size));
    let membrane_panel = commands
        .spawn(panel(&format!("Membrane slots {used}/{total}")))
        .id();
    commands.entity(columns).add_child(membrane_panel);
    for protein in LISTED_PROTEINS {
        let target = ProteinTarget::Membrane(protein);
        protein_row(
            &mut commands,
            membrane_panel,
            target,
            proteome.count(target),
            used < total,
            None,
        );
    }

    // Synthesis rate: what the ribosomes made last tick, or what they could make.
    let ribosomes = proteome.enzymes(BlockKind::ProteinSynthesis);
    let block = cell_blocks
        .get(BlockKind::ProteinSynthesis)
        .and_then(|e| blocks.get(e).ok());
    let observed = block.map_or(0.0, |(_, _, turnover)| turnover.0 / timing.target_delta());
    let rate = if observed > 0.0 {
        observed
    } else {
        block.map_or(0.0, |(node, throttle, _)| {
            let status: BlockStatus = node.status;
            potential_rate(ribosomes, status, throttle.0, integrity.0)
        })
    };

    // Queue.
    let queue_panel = commands.spawn(panel("Synthesis queue")).id();
    commands.entity(body).add_child(queue_panel);
    if queue.is_empty() {
        let row = commands.spawn(coloured("Nothing queued", DIM)).id();
        commands.entity(queue_panel).add_child(row);
    }
    for (i, order) in queue.orders.iter().enumerate() {
        let text = if i == 0 {
            let eta = first_order_eta(queue, rate)
                .map_or("no synthesis".to_string(), |s| format!("{s:.0}s"));
            format!(
                "1. {}  {:.0}/{} units  ({eta})",
                protein_name(*order),
                queue.progress,
                UNITS_PER_PROTEIN
            )
        } else {
            format!("{}. {}", i + 1, protein_name(*order))
        };
        let row = commands.spawn(coloured(text, Color::srgb(0.9, 0.9, 0.9))).id();
        commands.entity(queue_panel).add_child(row);
    }
    if let Some(eta) = synthesis_eta(queue, rate) {
        let row = commands
            .spawn(coloured(
                format!("All done in about {eta:.0}s (rate {rate:.2} units/s)"),
                GOOD,
            ))
            .id();
        commands.entity(queue_panel).add_child(row);
    }

    // Action slots.
    let slot_panel = commands
        .spawn(panel("Action slots (click or press 1-4 to change)"))
        .id();
    commands.entity(body).add_child(slot_panel);
    for slot in 0..SLOT_COUNT {
        let text = format!(
            "[{}] {}",
            slot + 1,
            slot_status(bar.slots[slot], levels, actions, proteome)
        );
        let button = commands
            .spawn((
                Button,
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                    align_self: AlignSelf::FlexStart,
                    ..default()
                },
                SMALL,
                ManageAction::CycleSlot(slot),
                children![label(text, 18.0)],
            ))
            .id();
        commands.entity(slot_panel).add_child(button);
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
