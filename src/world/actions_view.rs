//! # Actions in the world
//!
//! The parts of the action bar that need graphics:
//!
//! - **Hotkeys** 1-4 use the slots on the Run screen.
//! - **Membrane protein overlays** (`docs/art-direction.md`): one 56x56 sprite
//!   for each protein, up to `MAX_PER_PROTEIN` of a kind and `MAX_OVERLAYS` in
//!   all, mounted on the rim at 0.95 x the radius and drawn above the cell. The
//!   baked rows give the direction, so the game never turns or scales them.
//!   They play `idle`; `use` once on the action; the adhesin loops `hold`.
//! - **Shelters**: a darker, calm disc on the water. It uses the water colours
//!   of the art doc; the art owner may replace it with a sprite.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::actions::*;
use crate::cell::{Cell, CellSize};
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};
use crate::shelter::Shelter;
use crate::sprites::{Animator, Sheets, SpriteSystems, clip, direction_row, opposite_row, row_direction};
use crate::world::player_body::snap_offset;
use crate::{GameState, InRun};

/// Most overlays of one protein kind on a cell.
pub const MAX_PER_PROTEIN: u32 = 2;
/// Most overlays on a cell.
pub const MAX_OVERLAYS: usize = 6;
/// Mount distance from the cell centre, as a fraction of the radius.
pub const MOUNT_FRACTION: f32 = 0.95;
/// Z of an overlay above its cell.
const OVERLAY_Z: f32 = 0.5;
/// Z of the shelter disc: above the grid (-10), below the particles (-8).
const SHELTER_Z: f32 = -9.0;

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            slot_hotkeys.run_if(in_state(GameState::Run)),
            (sync_overlays, drive_overlays)
                .chain()
                .before(SpriteSystems::Animate),
        )
            .run_if(in_state(InRun)),
    );
}

// --- Hotkeys ---

const SLOT_KEYS: [KeyCode; SLOT_COUNT] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
];

fn slot_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut orders: MessageWriter<UseSlot>,
) {
    for controlled in &players {
        for (slot, key) in SLOT_KEYS.iter().enumerate() {
            if keys.just_pressed(*key) {
                orders.write(UseSlot {
                    cell: controlled.0,
                    slot,
                });
            }
        }
    }
}

// --- Overlays ---

/// The sprite name of the overlay for a protein, if the art has one.
pub fn overlay_sprite(protein: MembraneProtein) -> Option<&'static str> {
    match protein {
        // The Run screen does not mount a receptor yet.
        MembraneProtein::Receptor => None,
        _ => Some(crate::proteins::overlay_sheet(protein)),
    }
}

/// The overlays a proteome shows, in a fixed order: up to `MAX_PER_PROTEIN` of
/// each protein that has art, and `MAX_OVERLAYS` in all.
pub fn overlay_plan(proteome: &Proteome) -> Vec<MembraneProtein> {
    let mut plan = Vec::new();
    for protein in [
        MembraneProtein::Flagellum,
        MembraneProtein::Adhesin,
        MembraneProtein::Toxin,
        MembraneProtein::Lytic,
        MembraneProtein::Transporter,
    ] {
        let count = proteome
            .count(ProteinTarget::Membrane(protein))
            .min(MAX_PER_PROTEIN);
        for _ in 0..count {
            if plan.len() < MAX_OVERLAYS {
                plan.push(protein);
            }
        }
    }
    plan
}

/// The row (direction) of overlay `index` of `count`, spaced round the rim.
pub fn spaced_row(index: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    // Start at the south, so that the first overlay is below the cell.
    ((index as f32 * 8.0 / count as f32).round() as usize + 2) % 8
}

/// The clip of an overlay from the action state.
pub fn overlay_clip(protein: MembraneProtein, state: &ActionState) -> &'static str {
    match protein {
        MembraneProtein::Flagellum if state.bursting() => clip::USE,
        MembraneProtein::Adhesin if state.adhesin_held => {
            if state.held_for < ADHESIN_GRAB_SECONDS {
                clip::USE
            } else {
                clip::HOLD
            }
        }
        MembraneProtein::Toxin if state.toxin_flash > 0.0 => clip::USE,
        _ => clip::IDLE,
    }
}

/// The row of an overlay. A flagellum points away from where the cell goes.
pub fn overlay_row(protein: MembraneProtein, index: usize, count: usize, aim: Vec2) -> usize {
    match protein {
        MembraneProtein::Flagellum => opposite_row(direction_row(aim)),
        _ => spaced_row(index, count),
    }
}

/// An overlay on a cell. `index` is its place in `overlay_plan`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Overlay {
    pub protein: MembraneProtein,
    pub index: usize,
}

/// The plan that a cell's overlay children were built from.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct OverlayPlan(Vec<MembraneProtein>);

fn sync_overlays(
    sheets: Res<Sheets>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Proteome, Option<&OverlayPlan>, Option<&Children>), With<Cell>>,
    overlays: Query<(), With<Overlay>>,
    mut commands: Commands,
) {
    for controlled in &players {
        let Ok((proteome, current, children)) = cells.get(controlled.0) else {
            continue;
        };
        let plan = overlay_plan(proteome);
        if current.is_some_and(|c| c.0 == plan) {
            continue;
        }
        for child in children.into_iter().flatten() {
            if overlays.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        for (index, protein) in plan.iter().enumerate() {
            let Some(name) = overlay_sprite(*protein) else {
                continue;
            };
            let Some(sprite) = sheets.bundle(name, clip::IDLE) else {
                continue;
            };
            commands.entity(controlled.0).with_child((
                Name::new(format!("{name} {index}")),
                Overlay {
                    protein: *protein,
                    index,
                },
                sprite,
                Transform::from_translation(Vec3::new(0.0, 0.0, OVERLAY_Z)),
            ));
        }
        commands.entity(controlled.0).insert(OverlayPlan(plan));
    }
}

/// Picks the clip, the row and the place of each overlay.
fn drive_overlays(
    sheets: Res<Sheets>,
    cells: Query<(&Transform, &CellSize, &ActionState, &OverlayPlan), (With<Cell>, Without<Overlay>)>,
    mut overlays: Query<(&ChildOf, &Overlay, &mut Animator, &mut Transform)>,
) {
    for (parent, overlay, mut animator, mut transform) in &mut overlays {
        let Ok((cell, size, state, plan)) = cells.get(parent.parent()) else {
            continue;
        };
        let row = overlay_row(overlay.protein, overlay.index, plan.0.len(), state.aim);
        animator.row = row;
        animator.play(overlay_clip(overlay.protein, state), &sheets);
        let mount = (row_direction(row) * MOUNT_FRACTION * world_radius(size)).round();
        let offset = snap_offset(cell.translation.truncate());
        transform.translation = (mount + offset).extend(OVERLAY_Z);
    }
}

// --- Shelters ---

/// Colours of the shelter disc: darker than the water, with a lighter rim.
const SHELTER_FILL: [u8; 4] = [4, 8, 20, 150];
const SHELTER_RIM: [u8; 4] = [40, 70, 110, 170];
/// Width of the rim in world pixels.
const SHELTER_RIM_WIDTH: f32 = 2.0;

/// RGBA pixels of a shelter disc, `2r x 2r`. Hard edges: no partial alpha, so
/// the pixels stay on the pixel grid.
pub fn shelter_pixels(radius: f32) -> (u32, Vec<u8>) {
    let side = (radius * 2.0).ceil() as u32;
    let centre = side as f32 * 0.5;
    let mut data = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let d = Vec2::new(x as f32 + 0.5 - centre, y as f32 + 0.5 - centre).length();
            let pixel = if d > radius {
                [0, 0, 0, 0]
            } else if d > radius - SHELTER_RIM_WIDTH {
                SHELTER_RIM
            } else {
                SHELTER_FILL
            };
            data.extend_from_slice(&pixel);
        }
    }
    (side, data)
}

/// Spawns the sprite of a shelter.
pub fn spawn_shelter(commands: &mut Commands, images: &mut Assets<Image>, shelter: Shelter) {
    let (side, data) = shelter_pixels(shelter.radius);
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    commands.spawn((
        Name::new("Shelter"),
        Sprite::from_image(images.add(image)),
        Transform::from_translation(shelter.position.round().extend(SHELTER_Z)),
        DespawnOnExit(InRun),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proteome(counts: &[(MembraneProtein, u32)]) -> Proteome {
        let mut proteome = Proteome::default();
        for &(protein, n) in counts {
            for _ in 0..n {
                proteome.add(ProteinTarget::Membrane(protein));
            }
        }
        proteome
    }

    #[test]
    fn one_overlay_for_each_protein_up_to_the_caps() {
        assert!(overlay_plan(&Proteome::default()).is_empty());
        let plan = overlay_plan(&proteome(&[
            (MembraneProtein::Flagellum, 5),
            (MembraneProtein::Toxin, 1),
            (MembraneProtein::Receptor, 2),
        ]));
        assert_eq!(
            plan,
            vec![
                MembraneProtein::Flagellum,
                MembraneProtein::Flagellum,
                MembraneProtein::Toxin
            ],
            "two flagella at most, and no art for receptors"
        );
        let many = overlay_plan(&proteome(&[
            (MembraneProtein::Flagellum, 2),
            (MembraneProtein::Adhesin, 2),
            (MembraneProtein::Toxin, 2),
            (MembraneProtein::Transporter, 2),
        ]));
        assert_eq!(many.len(), MAX_OVERLAYS);
    }

    #[test]
    fn overlays_are_spaced_round_the_rim() {
        for count in 1..=MAX_OVERLAYS {
            let mut rows: Vec<_> = (0..count).map(|i| spaced_row(i, count)).collect();
            rows.sort();
            rows.dedup();
            assert_eq!(rows.len(), count, "count {count}");
        }
    }

    #[test]
    fn a_flagellum_points_away_from_the_aim() {
        // Aim east (row 0): the flagellum points west (row 4).
        assert_eq!(overlay_row(MembraneProtein::Flagellum, 0, 1, Vec2::X), 4);
        assert_eq!(overlay_row(MembraneProtein::Toxin, 0, 1, Vec2::X), spaced_row(0, 1));
    }

    #[test]
    fn overlay_clips_follow_the_action_state() {
        let mut state = ActionState::default();
        assert_eq!(overlay_clip(MembraneProtein::Adhesin, &state), clip::IDLE);
        state.adhesin_held = true;
        assert_eq!(overlay_clip(MembraneProtein::Adhesin, &state), clip::USE);
        state.held_for = ADHESIN_GRAB_SECONDS + 0.1;
        assert_eq!(overlay_clip(MembraneProtein::Adhesin, &state), clip::HOLD);
        state.burst_left = 1.0;
        assert_eq!(overlay_clip(MembraneProtein::Flagellum, &state), clip::USE);
        assert_eq!(overlay_clip(MembraneProtein::Toxin, &state), clip::IDLE);
        state.toxin_flash = 0.3;
        assert_eq!(overlay_clip(MembraneProtein::Toxin, &state), clip::USE);
    }

    #[test]
    fn the_shelter_disc_has_hard_edges_and_a_clear_corner() {
        let (side, data) = shelter_pixels(48.0);
        assert_eq!(side, 96);
        let alpha = |x: u32, y: u32| data[((y * side + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(48, 48), SHELTER_FILL[3]);
        assert_eq!(alpha(48, 0), SHELTER_RIM[3]);
    }
}
