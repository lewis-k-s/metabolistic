//! The membrane band at the top of the Manage screen: the lipid bilayer
//! (`wall_membrane`, tiled sideways) with a socket for each membrane slot.
//!
//! - A used slot shows the overlay sheet of its protein, row `OUT_ROW` (the
//!   protein points out of the cell), and plays the `use` or `hold` clip while
//!   the action is on (from `ActionState`, with the same rule as the Run
//!   screen: `overlay_clip`).
//! - A queued membrane protein is a faint ghost of its overlay in the next
//!   free slot, with a small bar for the build progress.
//! - An empty slot is an empty socket.
//!
//! The protein-to-sheet mapping is `proteins::overlay_sheet`, shared with the
//! Run screen. The slots are spawned again only when their number changes (the
//! cell grew); the overlays change only when a slot changes its content.

use bevy::prelude::*;

use super::layout;
use super::{BAD, MapEntity, MapLayout, Pick, make_pickable, map_layer};
use crate::GameState;
use crate::actions::ActionState;
use crate::cell::{Cell, CellSize, membrane_slots};
use crate::membrane::Membrane;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{
    MembraneProtein, ProteinTarget, Proteome, SynthesisQueue, UNITS_PER_PROTEIN, overlay_sheet,
};
use crate::sprites::{Animator, Sheets};
use crate::world::actions_view::overlay_clip;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            spawn_band.run_if(resource_exists_and_changed::<MapLayout>),
            sync_slots,
            update_slots,
            tint_band,
        )
            .chain()
            .after(super::fit_map)
            .run_if(in_state(GameState::Manage)),
    );
}

/// The row of an overlay sheet that points up (north), out of the cell.
pub const OUT_ROW: usize = 6;

const Z_BAND: f32 = 0.0;
const Z_SOCKET: f32 = 3.0;
const Z_OVERLAY: f32 = 4.0;

/// What a socket holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlotContent {
    Empty,
    /// A finished protein.
    Used(MembraneProtein),
    /// A protein in the queue. `progress` is the build progress in [0, 1] for
    /// the first order of the queue, and 0 for the others.
    Queued {
        protein: MembraneProtein,
        progress: f32,
    },
}

impl SlotContent {
    /// The same thing in the same state, for deciding if the overlay changes.
    fn same_kind(&self, other: &SlotContent) -> bool {
        match (self, other) {
            (SlotContent::Empty, SlotContent::Empty) => true,
            (SlotContent::Used(a), SlotContent::Used(b)) => a == b,
            (SlotContent::Queued { protein: a, .. }, SlotContent::Queued { protein: b, .. }) => {
                a == b
            }
            _ => false,
        }
    }
}

/// What each of `slots` sockets holds: the proteins of the proteome in the
/// order of `MembraneProtein::ALL`, then the queued membrane proteins, then
/// empty sockets. Proteins beyond the slots are dropped.
pub fn slot_contents(proteome: &Proteome, queue: &SynthesisQueue, slots: usize) -> Vec<SlotContent> {
    let mut out = Vec::new();
    for protein in MembraneProtein::ALL {
        for _ in 0..proteome.count(ProteinTarget::Membrane(protein)) {
            out.push(SlotContent::Used(protein));
        }
    }
    for (i, order) in queue.orders.iter().enumerate() {
        if let ProteinTarget::Membrane(protein) = order {
            let progress = if i == 0 {
                (queue.progress / UNITS_PER_PROTEIN).clamp(0.0, 1.0)
            } else {
                0.0
            };
            out.push(SlotContent::Queued {
                protein: *protein,
                progress,
            });
        }
    }
    out.truncate(slots);
    out.resize(slots, SlotContent::Empty);
    out
}

#[derive(Component)]
struct BandTile;

/// One socket. `shown` is what its overlay shows now.
#[derive(Component)]
struct Slot {
    index: usize,
    shown: SlotContent,
    overlay: Option<Entity>,
}

#[derive(Component)]
struct SlotBar(usize);

fn spawn_band(mut commands: Commands, server: Res<AssetServer>, layout: Res<MapLayout>) {
    let mut x = layout.left() + layout::BAND_TILE / 2.0;
    while x < layout.right() + layout::BAND_TILE / 2.0 {
        commands.spawn((
            BandTile,
            MapEntity,
            Sprite {
                image: server.load("sprites/wall_membrane.png"),
                ..default()
            },
            Transform::from_xyz(x, layout.band_y, Z_BAND),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
        x += layout::BAND_TILE;
    }
}

/// The colour of the band from the membrane integrity: it reddens as the
/// membrane breaks (art rule 6).
fn tint_band(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&Membrane, With<Cell>>,
    mut tiles: Query<&mut Sprite, With<BandTile>>,
) {
    let Some(membrane) = super::local_cell(&players).and_then(|c| cells.get(c).ok()) else {
        return;
    };
    let health = if membrane.max > 0.0 {
        (membrane.integrity / membrane.max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let color = Color::WHITE.mix(&BAD, (1.0 - health) * 0.8);
    for mut sprite in &mut tiles {
        if sprite.color != color {
            sprite.color = color;
        }
    }
}

/// Spawns the sockets again when their number changes.
#[allow(clippy::type_complexity)]
fn sync_slots(
    mut commands: Commands,
    server: Res<AssetServer>,
    layout: Res<MapLayout>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&CellSize, With<Cell>>,
    slots: Query<(Entity, &Slot)>,
    bars: Query<Entity, With<SlotBar>>,
) {
    let Some(size) = super::local_cell(&players).and_then(|c| cells.get(c).ok()) else {
        return;
    };
    let wanted = layout.slot_positions(membrane_slots(size) as usize);
    if slots.iter().count() == wanted.len() && !slots.is_empty() {
        return;
    }
    if wanted.is_empty() && slots.is_empty() {
        return;
    }
    for (entity, _) in &slots {
        commands.entity(entity).despawn();
    }
    for entity in &bars {
        commands.entity(entity).despawn();
    }
    for (index, pos) in wanted.into_iter().enumerate() {
        let mut socket = commands.spawn((
            Name::new(format!("Slot {index}")),
            MapEntity,
            Slot {
                index,
                shown: SlotContent::Empty,
                overlay: None,
            },
            Sprite {
                image: server.load("sprites/slot_socket.png"),
                ..default()
            },
            Transform::from_translation(pos.extend(Z_SOCKET)),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
        make_pickable(&mut socket, Pick::Slot(index));
        // The progress bar of a queued protein, under the band.
        commands.spawn((
            SlotBar(index),
            MapEntity,
            Sprite::from_color(Color::srgb(0.45, 0.6, 1.0), Vec2::new(0.0, 2.0)),
            bevy::sprite::Anchor::CENTER_LEFT,
            Transform::from_xyz(pos.x - 10.0, layout.band_bottom() - 3.0, Z_OVERLAY),
            Visibility::Hidden,
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn update_slots(
    mut commands: Commands,
    sheets: Res<Sheets>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Proteome, &SynthesisQueue, &ActionState), With<Cell>>,
    mut slots: Query<(Entity, &mut Slot, &mut Sprite)>,
    mut overlays: Query<(&mut Animator, &mut Sprite), Without<Slot>>,
    mut bars: Query<(&SlotBar, &mut Sprite, &mut Visibility), (Without<Slot>, Without<Animator>)>,
) {
    let Some((proteome, queue, state)) =
        super::local_cell(&players).and_then(|c| cells.get(c).ok())
    else {
        return;
    };
    let count = slots.iter().count();
    let contents = slot_contents(proteome, queue, count);
    for (slot_entity, mut slot, mut socket) in &mut slots {
        let content = contents[slot.index];
        // The socket looks empty-dark when it holds nothing.
        socket.color = match content {
            SlotContent::Empty => Color::srgb(0.75, 0.75, 0.8),
            _ => Color::WHITE,
        };
        if !slot.shown.same_kind(&content) {
            if let Some(old) = slot.overlay.take() {
                commands.entity(old).despawn();
            }
            let protein = match content {
                SlotContent::Used(p) | SlotContent::Queued { protein: p, .. } => Some(p),
                SlotContent::Empty => None,
            };
            if let Some(protein) = protein
                && let Some(bundle) = sheets.bundle_row(overlay_sheet(protein), "idle", OUT_ROW)
            {
                let ghost = matches!(content, SlotContent::Queued { .. });
                let mut overlay = commands.spawn((
                    bundle,
                    Transform::from_xyz(0.0, 0.0, Z_OVERLAY - Z_SOCKET),
                    map_layer(),
                ));
                if ghost {
                    overlay.insert(Pickable::IGNORE);
                }
                let id = overlay.id();
                commands.entity(slot_entity).add_child(id);
                slot.overlay = Some(id);
            }
        }
        slot.shown = content;
        if let Some(overlay) = slot.overlay
            && let Ok((mut animator, mut sprite)) = overlays.get_mut(overlay)
        {
            match content {
                SlotContent::Used(protein) => {
                    animator.play(overlay_clip(protein, state), &sheets);
                    sprite.color = Color::WHITE;
                }
                SlotContent::Queued { .. } => {
                    // A ghost: faint and blue, in the idle pose.
                    animator.play("idle", &sheets);
                    sprite.color = Color::srgba(0.6, 0.7, 1.0, 0.4);
                }
                SlotContent::Empty => {}
            }
        }
    }
    for (bar, mut sprite, mut visibility) in &mut bars {
        match contents.get(bar.0) {
            Some(SlotContent::Queued { progress, .. }) => {
                *visibility = Visibility::Inherited;
                sprite.custom_size = Some(Vec2::new(20.0 * progress.max(0.05), 2.0));
            }
            _ => *visibility = Visibility::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn proteome(items: &[(MembraneProtein, u32)]) -> Proteome {
        let mut p = Proteome::default();
        for (protein, n) in items {
            p.membrane.insert(*protein, *n);
        }
        p
    }

    #[test]
    fn slots_hold_proteins_then_queued_ghosts_then_nothing() {
        let proteome = proteome(&[(MembraneProtein::Flagellum, 2), (MembraneProtein::Toxin, 1)]);
        let queue = SynthesisQueue {
            orders: VecDeque::from([
                ProteinTarget::Enzyme(crate::blocks::genome::BlockKind::Respiration),
                ProteinTarget::Membrane(MembraneProtein::Adhesin),
                ProteinTarget::Membrane(MembraneProtein::Lytic),
            ]),
            progress: 5.0,
        };
        let slots = slot_contents(&proteome, &queue, 7);
        assert_eq!(slots.len(), 7);
        assert_eq!(slots[0], SlotContent::Used(MembraneProtein::Flagellum));
        assert_eq!(slots[1], SlotContent::Used(MembraneProtein::Flagellum));
        assert_eq!(slots[2], SlotContent::Used(MembraneProtein::Toxin));
        // The enzyme is first in the queue, so it gets the progress; the
        // membrane proteins wait with 0.
        assert_eq!(
            slots[3],
            SlotContent::Queued {
                protein: MembraneProtein::Adhesin,
                progress: 0.0
            }
        );
        assert!(matches!(slots[4], SlotContent::Queued { .. }));
        assert_eq!(slots[5], SlotContent::Empty);
    }

    #[test]
    fn a_first_membrane_order_shows_its_progress() {
        let queue = SynthesisQueue {
            orders: VecDeque::from([ProteinTarget::Membrane(MembraneProtein::Toxin)]),
            progress: UNITS_PER_PROTEIN / 2.0,
        };
        let slots = slot_contents(&Proteome::default(), &queue, 3);
        assert_eq!(
            slots[0],
            SlotContent::Queued {
                protein: MembraneProtein::Toxin,
                progress: 0.5
            }
        );
    }

    #[test]
    fn too_many_proteins_are_cut_to_the_slots() {
        let proteome = proteome(&[(MembraneProtein::Flagellum, 9)]);
        assert_eq!(slot_contents(&proteome, &SynthesisQueue::default(), 4).len(), 4);
        assert!(slot_contents(&proteome, &SynthesisQueue::default(), 0).is_empty());
    }

    #[test]
    fn every_protein_has_a_sheet_in_the_manifest() {
        let manifest =
            crate::sprites::parse_manifest(include_str!("../../../assets/sprites/anim/manifest.json"))
                .unwrap();
        for protein in MembraneProtein::ALL {
            let sheet = manifest.get(overlay_sheet(protein));
            let sheet = sheet.unwrap_or_else(|| panic!("{protein:?}: {}", overlay_sheet(protein)));
            assert!(sheet.clips.contains_key("idle"));
            // Row OUT_ROW exists.
            assert!(sheet.clips["idle"].rows as usize > OUT_ROW);
        }
    }
}
