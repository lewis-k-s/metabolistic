//! # Actions in the world
//!
//! The parts of the action bar that need graphics:
//!
//! - **Bindings** on the Run screen: the slots are on the mouse buttons (left,
//!   right, back, forward; `SLOT_LABELS`), with keys 1-4 as a second binding.
//!   `SlotHeld` follows them, for actions that need a hold (the pilus). Shift
//!   fires the flagellum burst (`UseAction`), so WASD and the burst stay under
//!   the left hand. The mouse wheel reels a held pilus: scroll down pulls in,
//!   scroll up pays out (`ReelPilus`, `scroll_reel_pixels`).
//! - **Mouse aim**: the aim of the player's `ActionState` points from the cell
//!   to the cursor (`aim_toward`). Without a cursor in the window, the old aim
//!   (the last move direction) stays: the fallback for keyboard and gamepad.
//! - **Aim line**: a thin line along the aim while a slot that needs aim is held.
//! - **Membrane protein overlays** (`docs/art-direction.md`): one 56x56 sprite
//!   for each protein, up to the caps of the display mode (`CellDisplay`), mounted on the rim at 0.95 x the radius and drawn above the cell. The
//!   baked rows give the direction, so the game never turns or scales them.
//!   A flagellum trails the heading (the move direction). The proteins of aimed
//!   actions sit in a cluster round the aim: the last used one (`AimOrder`)
//!   points exactly at the cursor, the others one row (45 degrees) further to
//!   each side in turn (`cluster_row`). They play `idle`; `use` once on the
//!   action; the adhesin loops `hold`.
//! - **Shelters**: a darker, calm disc on the water. It uses the water colours
//!   of the art doc; the art owner may replace it with a sprite.

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::actions::*;
use crate::cell::{Cell, CellSize};
use crate::interior::{CellDisplay, OverlayCaps};
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::pilus::{PILUS_RANGE, PilusTether, REEL_PIXELS_PER_NOTCH, ReelPilus};
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};
use crate::shelter::Shelter;
use crate::sprites::{Animator, Sheets, SpriteSystems, clip, direction_row, opposite_row, row_direction};
use crate::world::player_body::snap_offset;
use crate::{GameState, InRun};

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
            slot_hotkeys,
            scroll_reel.run_if(in_state(GameState::Run)),
            mouse_aim
                .after(crate::world::player_body::read_move_input)
                .run_if(in_state(GameState::Run)),
            draw_aim_line.run_if(in_state(GameState::Run)),
            (sync_overlays, drive_overlays)
                .chain()
                .before(SpriteSystems::Animate),
        )
            .run_if(in_state(InRun)),
    );
}

// --- Bindings ---

/// The mouse button of each slot (see `SLOT_LABELS`).
const SLOT_BUTTONS: [MouseButton; SLOT_COUNT] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Back,
    MouseButton::Forward,
];

/// The second binding of each slot.
const SLOT_KEYS: [KeyCode; SLOT_COUNT] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
];

/// The keys of the flagellum burst.
const BURST_KEYS: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];

/// The secretion keys (see `secretion::KEY_LABELS`): each press toggles.
const SECRETION_KEYS: [KeyCode; crate::secretion::KEY_COUNT] = [KeyCode::KeyQ, KeyCode::KeyE];

/// Presses and holds of the slot buttons, and the burst key. Only the Run
/// screen takes them: in Manage the mouse and keys are for the editor, so
/// nothing is held.
fn slot_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut held: Query<&mut SlotHeld, With<Cell>>,
    mut orders: MessageWriter<UseSlot>,
    mut direct: MessageWriter<UseAction>,
    mut toggles: MessageWriter<crate::secretion::ToggleSecretion>,
) {
    let on_run = *state.get() == GameState::Run;
    for controlled in &players {
        let mut now = [false; SLOT_COUNT];
        for slot in 0..SLOT_COUNT {
            let (button, key) = (SLOT_BUTTONS[slot], SLOT_KEYS[slot]);
            now[slot] = on_run && (mouse.pressed(button) || keys.pressed(key));
            if on_run && (mouse.just_pressed(button) || keys.just_pressed(key)) {
                orders.write(UseSlot {
                    cell: controlled.0,
                    slot,
                });
            }
        }
        if on_run && keys.any_just_pressed(BURST_KEYS) {
            direct.write(UseAction {
                cell: controlled.0,
                kind: ActionKind::FlagellumBurst,
            });
        }
        for (key, code) in SECRETION_KEYS.iter().enumerate() {
            if on_run && keys.just_pressed(*code) {
                toggles.write(crate::secretion::ToggleSecretion {
                    cell: controlled.0,
                    key,
                });
            }
        }
        if let Ok(mut held) = held.get_mut(controlled.0)
            && held.0 != now
        {
            held.0 = now;
        }
    }
}

/// Scroll pixels (trackpads, smooth wheels) that count as one notch.
const PIXELS_PER_NOTCH: f32 = 16.0;
/// The wheel direction that reels in: scroll down (a negative delta) pulls in,
/// scroll up pays out. Flip the sign to swap the directions.
const REEL_IN_SIGN: f32 = -1.0;

/// The reel order for a wheel delta: negative pixels reel in.
pub fn scroll_reel_pixels(delta_y: f32, unit: MouseScrollUnit) -> f32 {
    let notches = match unit {
        MouseScrollUnit::Line => delta_y,
        MouseScrollUnit::Pixel => delta_y / PIXELS_PER_NOTCH,
    };
    -REEL_IN_SIGN * notches * REEL_PIXELS_PER_NOTCH
}

/// The mouse wheel reels the player's pilus while it is attached.
fn scroll_reel(
    scroll: Res<AccumulatedMouseScroll>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    tethers: Query<&PilusTether, With<Cell>>,
    mut orders: MessageWriter<ReelPilus>,
) {
    if scroll.delta.y == 0.0 {
        return;
    }
    let pixels = scroll_reel_pixels(scroll.delta.y, scroll.unit);
    for controlled in &players {
        if tethers.get(controlled.0).is_ok_and(|t| t.0.is_some()) {
            orders.write(ReelPilus { cell: controlled.0, pixels });
        }
    }
}

// --- Mouse aim and the aim line ---

/// The cursor in world space, if the window has a cursor and the camera sees it.
fn cursor_world(
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<crate::screens::RunCamera>>,
    canvas: &crate::world::shader_cells::Canvas,
) -> Option<Vec2> {
    let window = windows.single().ok()?;
    let (camera, transform) = cameras.single().ok()?;
    if canvas.is_open() {
        return crate::world::shader_cells::canvas_cursor_world(window, transform);
    }
    camera.viewport_to_world_2d(transform, window.cursor_position()?).ok()
}

/// The window cursor (logical pixels) that points at `world`: the inverse of
/// `cursor_world`. Scripts use it to aim (see `agent_script`).
pub fn world_to_cursor(
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    canvas: &crate::world::shader_cells::Canvas,
    world: Vec2,
) -> Option<Vec2> {
    let (camera, transform) = camera;
    if canvas.is_open() {
        return Some(crate::world::shader_cells::canvas_world_to_cursor(
            window.size(),
            transform,
            world,
        ));
    }
    camera.world_to_viewport(transform, world.extend(0.0)).ok()
}

/// Points the aim of the player's cell at the cursor.
fn mouse_aim(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::screens::RunCamera>>,
    canvas: Res<crate::world::shader_cells::Canvas>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&Transform, &mut ActionState), With<Cell>>,
) {
    let Some(cursor) = cursor_world(&windows, &cameras, &canvas) else {
        return;
    };
    for controlled in &players {
        if let Ok((transform, mut state)) = cells.get_mut(controlled.0) {
            state.aim_toward(transform.translation.truncate(), cursor);
        }
    }
}

/// Colour of the aim line.
const AIM_LINE_COLOR: Color = Color::srgba(0.85, 1.0, 0.85, 0.55);
/// Length of the aim line for an action with no range of its own, in pixels.
const AIM_LINE_LENGTH: f32 = 70.0;

/// The length of the aim line for a held action.
pub fn aim_line_length(kind: ActionKind) -> f32 {
    match kind {
        ActionKind::Pilus => PILUS_RANGE,
        _ => AIM_LINE_LENGTH,
    }
}

/// Draws a thin line from the rim along the aim while a slot that needs aim is
/// held. The line has the length of the longest held action.
fn draw_aim_line(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Transform, &CellSize, &ActionState, &ActionBar, &SlotHeld), With<Cell>>,
    mut gizmos: Gizmos,
) {
    for controlled in &players {
        let Ok((transform, size, state, bar, held)) = cells.get(controlled.0) else {
            continue;
        };
        let length = bar
            .slots
            .iter()
            .zip(held.0)
            .filter_map(|(kind, held)| kind.filter(|k| held && k.needs_aim()))
            .map(aim_line_length)
            .fold(0.0_f32, f32::max);
        if length <= 0.0 {
            continue;
        }
        let start = transform.translation.truncate() + state.aim * world_radius(size);
        gizmos.line_2d(start, start + state.aim * length, AIM_LINE_COLOR);
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

/// Does the overlay of a protein point at the cursor? These are the proteins
/// of the aimed actions (`ActionKind::needs_aim`).
pub fn tracks_aim(protein: MembraneProtein) -> bool {
    matches!(
        protein,
        MembraneProtein::Toxin | MembraneProtein::Pilus | MembraneProtein::Injector | MembraneProtein::Lytic
    )
}

/// Most overlays of one protein on a cell, under the caps of the display mode.
/// An overlay that points along the aim has one place, so a second one would
/// lie on the first.
pub fn max_overlays_of(protein: MembraneProtein, caps: OverlayCaps) -> u32 {
    if tracks_aim(protein) {
        caps.per_protein.min(1)
    } else {
        caps.per_protein
    }
}

/// The overlays a proteome shows, in a fixed order: up to `caps.per_protein` of
/// each protein that has art (`max_overlays_of`), and `caps.total` in all. A new
/// protein is one more entry in the list.
pub fn overlay_plan(proteome: &Proteome, caps: OverlayCaps) -> Vec<MembraneProtein> {
    let mut plan = Vec::new();
    for protein in [
        MembraneProtein::Flagellum,
        MembraneProtein::Adhesin,
        MembraneProtein::Toxin,
        MembraneProtein::Pilus,
        MembraneProtein::Injector,
        MembraneProtein::Lytic,
        MembraneProtein::Transporter,
    ] {
        let count = proteome
            .count(ProteinTarget::Membrane(protein))
            .min(max_overlays_of(protein, caps));
        for _ in 0..count {
            if plan.len() < caps.total {
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
        // The pilus extends once, then pulls taut while it reels.
        MembraneProtein::Pilus if state.pilus_flash > 0.0 => clip::USE,
        MembraneProtein::Pilus if state.pilus_taut => clip::HOLD,
        MembraneProtein::Injector if state.injector_flash > 0.0 => clip::USE,
        _ => clip::IDLE,
    }
}

/// The row of place `rank` in the aim cluster round `aim_row`: the aim row
/// itself, then one row to the one side, one to the other, two to the one
/// side, and so on.
pub fn cluster_row(aim_row: usize, rank: usize) -> usize {
    let step = rank.div_ceil(2) as i32;
    let offset = if rank % 2 == 1 { step } else { -step };
    (aim_row as i32 + offset).rem_euclid(8) as usize
}

/// The row of an overlay. A flagellum trails the heading (it points away from
/// where the cell goes). The proteins of aimed actions (`tracks_aim`) sit in
/// the aim cluster at place `aim_rank` (0 points at the cursor). The others
/// are spaced round the rim.
pub fn overlay_row(
    protein: MembraneProtein,
    index: usize,
    count: usize,
    aim_rank: usize,
    state: &ActionState,
) -> usize {
    match protein {
        MembraneProtein::Flagellum => opposite_row(direction_row(state.heading)),
        p if tracks_aim(p) => cluster_row(direction_row(state.aim), aim_rank),
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
    display: Res<CellDisplay>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Proteome, Option<&OverlayPlan>, Option<&Children>), With<Cell>>,
    overlays: Query<(), With<Overlay>>,
    mut commands: Commands,
) {
    for controlled in &players {
        let Ok((proteome, current, children)) = cells.get(controlled.0) else {
            continue;
        };
        let plan = overlay_plan(proteome, display.mode.overlay_caps());
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
#[allow(clippy::type_complexity)]
fn drive_overlays(
    sheets: Res<Sheets>,
    cells: Query<
        (&Transform, &CellSize, &ActionState, &AimOrder, &OverlayPlan),
        (With<Cell>, Without<Overlay>),
    >,
    mut overlays: Query<(&ChildOf, &Overlay, &mut Animator, &mut Transform)>,
) {
    for (parent, overlay, mut animator, mut transform) in &mut overlays {
        let Ok((cell, size, state, aim_order, plan)) = cells.get(parent.parent()) else {
            continue;
        };
        let aimed: Vec<MembraneProtein> = plan.0.iter().copied().filter(|p| tracks_aim(*p)).collect();
        let rank = aim_order.rank(&aimed, overlay.protein).unwrap_or(0);
        let row = overlay_row(overlay.protein, overlay.index, plan.0.len(), rank, state);
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
    use crate::interior::DisplayMode;

    const CAPS: OverlayCaps = OverlayCaps { per_protein: 2, total: 6 };

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
        assert!(overlay_plan(&Proteome::default(), CAPS).is_empty());
        let plan = overlay_plan(&proteome(&[
            (MembraneProtein::Flagellum, 5),
            (MembraneProtein::Toxin, 1),
            (MembraneProtein::Receptor, 2),
        ]), CAPS);
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
        ]), CAPS);
        assert_eq!(many.len(), CAPS.total);
        // The mode sets the caps: Full allows more than Standard.
        let more = overlay_plan(&proteome(&[(MembraneProtein::Flagellum, 5)]), DisplayMode::Full.overlay_caps());
        assert_eq!(more.len(), 3);
    }

    #[test]
    fn overlays_are_spaced_round_the_rim() {
        for count in 1..=CAPS.total {
            let mut rows: Vec<_> = (0..count).map(|i| spaced_row(i, count)).collect();
            rows.sort();
            rows.dedup();
            assert_eq!(rows.len(), count, "count {count}");
        }
    }

    /// A state that moves east and aims north.
    fn east_aim_north() -> ActionState {
        ActionState {
            heading: Vec2::X,
            aim: Vec2::Y,
            ..default()
        }
    }

    #[test]
    fn a_flagellum_trails_the_heading_not_the_cursor() {
        // Heading east (row 0): the flagellum points west (row 4).
        let state = east_aim_north();
        assert_eq!(overlay_row(MembraneProtein::Flagellum, 0, 1, 0, &state), 4);
        assert_eq!(overlay_row(MembraneProtein::Adhesin, 0, 1, 0, &state), spaced_row(0, 1));
    }

    #[test]
    fn aimed_proteins_point_at_the_cursor() {
        let state = east_aim_north();
        for protein in [
            MembraneProtein::Toxin,
            MembraneProtein::Pilus,
            MembraneProtein::Injector,
            MembraneProtein::Lytic,
        ] {
            assert_eq!(overlay_row(protein, 1, 3, 0, &state), direction_row(Vec2::Y), "{protein:?}");
            assert_eq!(max_overlays_of(protein, CAPS), 1, "{protein:?}");
        }
        assert!(!tracks_aim(MembraneProtein::Flagellum));
        assert_eq!(max_overlays_of(MembraneProtein::Flagellum, CAPS), CAPS.per_protein);
    }

    #[test]
    fn the_aim_cluster_puts_the_leader_on_the_aim_and_the_rest_beside_it() {
        assert_eq!(cluster_row(2, 0), 2);
        assert_eq!(cluster_row(2, 1), 3);
        assert_eq!(cluster_row(2, 2), 1);
        assert_eq!(cluster_row(2, 3), 4);
        // It wraps round the eight rows.
        assert_eq!(cluster_row(0, 2), 7);
        assert_eq!(cluster_row(7, 1), 0);
        // Four aimed proteins get four rows.
        let mut rows: Vec<_> = (0..4).map(|r| cluster_row(5, r)).collect();
        rows.sort();
        rows.dedup();
        assert_eq!(rows.len(), 4);
        let state = ActionState { aim: Vec2::Y, ..default() };
        let north = direction_row(Vec2::Y);
        assert_eq!(overlay_row(MembraneProtein::Pilus, 0, 2, 1, &state), cluster_row(north, 1));
        assert_ne!(cluster_row(north, 1), north);
    }

    #[test]
    fn the_wheel_reels_in_one_way_and_out_the_other() {
        let line = scroll_reel_pixels(1.0, MouseScrollUnit::Line);
        assert_eq!(line, REEL_PIXELS_PER_NOTCH, "scroll up pays out");
        assert!(scroll_reel_pixels(-1.0, MouseScrollUnit::Line) < 0.0, "scroll down reels in");
        assert_eq!(scroll_reel_pixels(-1.0, MouseScrollUnit::Line), -line);
        assert_eq!(scroll_reel_pixels(PIXELS_PER_NOTCH, MouseScrollUnit::Pixel), line);
        assert_eq!(scroll_reel_pixels(0.0, MouseScrollUnit::Line), 0.0);
    }

    #[test]
    fn the_slot_bindings_have_one_label_each() {
        assert_eq!(SLOT_BUTTONS.len(), SLOT_LABELS.len());
        assert_eq!(SLOT_BUTTONS[0], MouseButton::Left);
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

    #[test]
    fn the_pilus_overlay_extends_then_pulls_taut() {
        let mut state = ActionState::default();
        assert_eq!(overlay_clip(MembraneProtein::Pilus, &state), clip::IDLE);
        state.pilus_flash = 0.3;
        state.pilus_taut = true;
        assert_eq!(overlay_clip(MembraneProtein::Pilus, &state), clip::USE);
        state.pilus_flash = 0.0;
        assert_eq!(overlay_clip(MembraneProtein::Pilus, &state), clip::HOLD);
        state.pilus_taut = false;
        state.injector_flash = 0.3;
        assert_eq!(overlay_clip(MembraneProtein::Injector, &state), clip::USE);
    }

    #[test]
    fn a_pilus_and_an_injector_point_along_the_aim_with_one_overlay_each() {
        let north = ActionState { aim: Vec2::Y, ..default() };
        let west = ActionState { aim: Vec2::NEG_X, ..default() };
        assert_eq!(overlay_row(MembraneProtein::Pilus, 0, 3, 0, &north), direction_row(Vec2::Y));
        assert_eq!(overlay_row(MembraneProtein::Injector, 2, 3, 0, &west), direction_row(Vec2::NEG_X));
        let plan = overlay_plan(&proteome(&[
            (MembraneProtein::Pilus, 3),
            (MembraneProtein::Injector, 2),
        ]), DisplayMode::Full.overlay_caps());
        assert_eq!(plan, vec![MembraneProtein::Pilus, MembraneProtein::Injector]);
        assert!(overlay_sprite(MembraneProtein::Pilus).is_some());
        assert_eq!(aim_line_length(ActionKind::Pilus), PILUS_RANGE);
        assert!(aim_line_length(ActionKind::Injector) < PILUS_RANGE);
    }
}
