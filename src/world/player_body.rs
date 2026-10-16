//! # The player's cell as a body
//!
//! The local player's `Cell` gets the physics body: a circle collider, mass and
//! drag from `CellSize`, and a thrust force from `MoveInput` and its flagella.
//! The sprite is a grandchild of the cell: cell -> `BodyDeform` pivot -> sprite.
//! It plays the baked sheets of `sprites` (spawn, idle, move in 8 directions).
//! The frames carry the shape of each clip (art rule 9). On top of them, the
//! pivot stretches the sprite along its motion while the cell pushes in that
//! direction (`move_stretch`): a fast cell that the player drives stretches, a
//! cell that a blow or a current carries does not. The sprite turns back by the
//! pivot angle, so the frame itself never rotates. World units are sprite
//! pixels; the camera scale makes each one `PIXEL_SCALE` screen pixels.
//!
//! The formulas are in `movement`. The systems here fetch data and hand it to
//! avian2d.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::actions::{ActionState, action_damping, action_thrust};
use crate::desiccation::CuticleShield;
use crate::envgrid::EnvGrid;
use crate::environment::Channel;
use crate::level::CurrentLevel;
use crate::walls::{MAT_CRAWL_SPEED, WallMap, crawl_force, crawl_force_at, in_mat};
use crate::world::cell_layers;
use crate::cell::{Cell, CellSize};
use crate::movement::{
    MoveInput, ThrustEffort, body_drag, drag_in_field, body_mass, thrust_capacity, wasd_direction, world_radius,
};
use crate::player::{ControlledCell, LocalPlayer};
use crate::metabolism::CurrencyPools;
use crate::molecules::Currency;
use crate::proteins::Proteome;
use crate::sprites::{Animator, Sheets, SpriteSystems, clip, direction_row};
use crate::{GameState, InRun};

/// How fast the camera catches up with the player, per second.
const CAMERA_FOLLOW_RATE: f32 = 6.0;
/// Speed above which the cell plays `move` and takes the row of its heading,
/// in pixels per second.
const MOVE_SPEED_THRESHOLD: f32 = 8.0;

/// Most stretch of a body sprite along its motion, at full speed with a full
/// push in that direction. The long axis is `1 + MAX_MOVE_STRETCH`. **Playtest.**
pub const MAX_MOVE_STRETCH: f32 = 0.15;
/// How fast the stretch follows its target, per second.
const DEFORM_RATE: f32 = 12.0;
/// Below this stretch the pivot is the identity, so the still sprite stays on
/// the pixel grid.
const MIN_VISIBLE_STRETCH: f32 = 1e-3;

/// The sprite of a cell that plays the sprite sheets. `cell` is the cell that
/// it shows (the sprite is a grandchild of it, under the `BodyDeform` pivot).
#[derive(Component)]
pub struct BlobSprite {
    pub cell: Entity,
}

/// The pivot between a cell and its sprite. It carries the smoothed stretch
/// along `angle` (radians, the direction of motion).
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct BodyDeform {
    pub stretch: f32,
    pub angle: f32,
}

/// The smoothed camera position, before it is rounded to a whole pixel.
#[derive(Resource, Default)]
struct CameraTrack(Option<Vec2>);

pub fn plugin(app: &mut App) {
    app.init_resource::<CameraTrack>()
        .add_systems(
            Update,
            (read_move_input, drive_body_sprite, deform_body_sprite)
                .chain()
                .before(SpriteSystems::Animate)
                .run_if(in_state(InRun)),
        )
        .add_systems(
            PostUpdate,
            (snap_body_sprite, camera_follow)
                .chain()
                .before(TransformSystems::Propagate)
                .run_if(in_state(InRun)),
        )
    // Thrust is a force: set it before avian's fixed-step physics.
    .add_systems(FixedUpdate, apply_thrust.run_if(in_state(InRun)))
    .add_systems(Update, (update_cell_layers, ros_tint_system).run_if(in_state(InRun)))
    .add_systems(OnExit(InRun), reset_camera);
}

/// Adds the body and the sprite to the player's cell.
pub fn attach_body(commands: &mut Commands, sheets: &Sheets, cell: Entity) {
    attach_body_for(commands, sheets, cell, &CellSize::default(), "player_cell", clip::SPAWN, false);
}

/// Adds a body and a sprite to a cell of any size. `sprite` is the sheet name
/// and `first_clip` the clip that it starts with. A `fixed` body does not move
/// (the gate cell).
pub fn attach_body_for(
    commands: &mut Commands,
    sheets: &Sheets,
    cell: Entity,
    size: &CellSize,
    sprite: &'static str,
    first_clip: &'static str,
    fixed: bool,
) {
    let radius = world_radius(size);
    commands.entity(cell).insert((
        if fixed { RigidBody::Static } else { RigidBody::Dynamic },
        Collider::circle(radius),
        Mass(body_mass(size)),
        LinearDamping(body_drag(size)),
        LockedAxes::ROTATION_LOCKED,
        ConstantForce::default(),
        Visibility::default(),
        // The real layers follow the proteome (see `update_cell_layers`).
        cell_layers(&Proteome::default(), false),
        DespawnOnExit(InRun),
    ));
    let Some(body) = sheets.bundle(sprite, first_clip) else {
        warn!("the manifest has no {sprite}/{first_clip}");
        return;
    };
    commands.entity(cell).with_child((
        Name::new("Cell Deform"),
        BodyDeform::default(),
        Transform::default(),
        Visibility::default(),
        children![(Name::new("Cell Sprite"), BlobSprite { cell }, body, Transform::default())],
    ));
}

/// WASD sets the wanted direction on the Run screen, and the aim of the actions.
/// In Manage, time may run but the keys are for the editor, so the cell stops
/// pushing.
fn read_move_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut inputs: Query<(&mut MoveInput, &mut ActionState), With<Cell>>,
) {
    let direction = if *state.get() == GameState::Run {
        wasd_direction(
            keys.pressed(KeyCode::KeyW),
            keys.pressed(KeyCode::KeyA),
            keys.pressed(KeyCode::KeyS),
            keys.pressed(KeyCode::KeyD),
        )
    } else {
        Vec2::ZERO
    };
    for controlled in &players {
        if let Ok((mut input, mut actions)) = inputs.get_mut(controlled.0) {
            input.0 = direction;
            actions.aim_at(direction);
        }
    }
}

/// Sets the thrust force of each cell with a body, and adds the thrust used to
/// its `ThrustEffort`. The metabolic tick pays ATP for it.
fn apply_thrust(
    time: Res<Time>,
    walls: Res<WallMap>,
    grid: Option<Res<EnvGrid>>,
    level: Option<Res<CurrentLevel>>,
    mut cells: Query<
        (
            &Transform,
            &MoveInput,
            &Proteome,
            &CellSize,
            &ActionState,
            &CuticleShield,
            &mut ConstantForce,
            &mut LinearDamping,
            &mut ThrustEffort,
        ),
        With<Cell>,
    >,
) {
    let field = level.as_ref().map(|l| l.0.field);
    for (transform, input, proteome, size, actions, shield, mut force, mut damping, mut effort) in &mut cells {
        // The field sets the drag at the cell's position (dry ground on land is
        // much heavier than water); a cuticle adds a little.
        let water = grid.as_deref().map_or(0.0, |g| g.sample(Channel::Water, transform.translation.truncate()));
        let multiplier = field.map_or(1.0, |f| f.drag_multiplier(water));
        let drag = drag_in_field(action_damping(body_drag(size), actions), multiplier, shield.0);
        let on_mat = grid
            .as_deref()
            .is_some_and(|g| in_mat(&walls, g.origin(), transform.translation.truncate()));
        // A held adhesin on a wall crawls through it at a slow speed. So does
        // a cell in a microbial mat, with no adhesin.
        let thrust = if actions.ghosting() {
            crawl_force(input.0, body_mass(size), drag)
        } else if on_mat {
            crawl_force_at(input.0, body_mass(size), drag, MAT_CRAWL_SPEED)
        } else {
            action_thrust(input.0, thrust_capacity(proteome), actions)
        };
        force.0 = thrust;
        damping.0 = drag;
        effort.0 += thrust.length() * time.delta_secs();
    }
}

/// Sets which walls each body collides with: a pore opens for a transporter, and
/// an adhesin crawl ghosts through walls and pores.
fn update_cell_layers(
    cells: Query<(Entity, &Proteome, &ActionState, &CollisionLayers), With<Cell>>,
    mut commands: Commands,
) {
    for (entity, proteome, actions, layers) in &cells {
        let wanted = cell_layers(proteome, actions.ghosting());
        if *layers != wanted {
            commands.entity(entity).insert(wanted);
        }
    }
}

/// The colour multiplier of a cell sprite at a ROS tint (0 to 1): from white to
/// a sickly green-grey. A flat colour only: the sprite is never scaled or
/// rotated. The art has no ROS look yet: **art owner** should bake a "stressed"
/// clip.
pub fn ros_tint_color(tint: f32) -> Color {
    let t = tint.clamp(0.0, 1.0);
    Color::srgb(1.0 - 0.35 * t, 1.0 - 0.05 * t, 1.0 - 0.55 * t)
}

/// Tints the sprite of each cell with its ROS: the body-horror look.
fn ros_tint_system(
    cells: Query<&CurrencyPools, With<Cell>>,
    mut sprites: Query<(&BlobSprite, &mut Sprite)>,
) {
    for (blob, mut sprite) in &mut sprites {
        let Ok(pools) = cells.get(blob.cell) else {
            continue;
        };
        let color = ros_tint_color(crate::oxygen::ros_tint(pools.get(Currency::ROS)));
        if sprite.color != color {
            sprite.color = color;
        }
    }
}

/// The sprite row for a velocity: the row of the heading when the cell moves,
/// and the current row when it is (nearly) still.
pub fn heading_row(velocity: Vec2, current: usize) -> usize {
    if velocity.length() > MOVE_SPEED_THRESHOLD {
        direction_row(velocity)
    } else {
        current
    }
}

/// Picks the clip and the row of the body sprite. It plays the one-shot `spawn`,
/// then `move` while the cell moves and `idle` when it is still.
fn drive_body_sprite(
    sheets: Res<Sheets>,
    cells: Query<&LinearVelocity, With<Cell>>,
    mut sprites: Query<(&BlobSprite, &mut Animator)>,
) {
    for (blob, mut animator) in &mut sprites {
        let Ok(velocity) = cells.get(blob.cell) else {
            continue;
        };
        animator.row = heading_row(velocity.0, animator.row);
        if animator.clip != clip::SPAWN || animator.finished() {
            let moving = velocity.0.length() > MOVE_SPEED_THRESHOLD;
            animator.play(if moving { clip::MOVE } else { clip::IDLE }, &sheets);
        }
    }
}

/// The target stretch of a body sprite: `MAX_MOVE_STRETCH` times the pace
/// (speed over `top_speed`, at most 1) times the intent (the part of the input
/// along the motion, 0 to 1). A cell that does not push, or pushes against its
/// motion, does not stretch however fast it goes.
pub fn move_stretch(velocity: Vec2, input: Vec2, top_speed: f32) -> f32 {
    let speed = velocity.length();
    if speed <= MOVE_SPEED_THRESHOLD || top_speed <= 0.0 {
        return 0.0;
    }
    let pace = (speed / top_speed).min(1.0);
    let intent = input.clamp_length_max(1.0).dot(velocity / speed).max(0.0);
    MAX_MOVE_STRETCH * pace * intent
}

/// The scale of a stretch along (x) and across (y) the motion. The area stays
/// the same.
pub fn stretch_scale(stretch: f32) -> Vec2 {
    let along = 1.0 + stretch.max(0.0);
    Vec2::new(along, 1.0 / along)
}

/// One smoothing step of the stretch towards its target, and the new angle:
/// the direction of motion while the cell moves, else the last angle (so the
/// stretch relaxes along the axis it had). Exact for any `dt`.
pub fn deform_step(deform: BodyDeform, target: f32, velocity: Vec2, dt: f32) -> BodyDeform {
    let stretch = deform.stretch + (target - deform.stretch) * (1.0 - (-DEFORM_RATE * dt).exp());
    let angle = if velocity.length() > MOVE_SPEED_THRESHOLD {
        velocity.to_angle()
    } else {
        deform.angle
    };
    BodyDeform { stretch, angle }
}

/// The transform of the pivot at a snap offset, and the rotation of the sprite
/// under it. The pivot turns to the motion and stretches; the sprite turns back,
/// so only the shape changes. A stretch too small to see gives the identity.
pub fn deform_transforms(snap: Vec2, deform: BodyDeform) -> (Transform, Quat) {
    let pivot = Transform::from_translation(snap.extend(0.0));
    if deform.stretch < MIN_VISIBLE_STRETCH {
        return (pivot, Quat::IDENTITY);
    }
    let pivot = pivot
        .with_rotation(Quat::from_rotation_z(deform.angle))
        .with_scale(stretch_scale(deform.stretch).extend(1.0));
    (pivot, Quat::from_rotation_z(-deform.angle))
}

/// Moves the stretch of each body sprite towards the target of its cell. The
/// top speed is the terminal speed of the full thrust in the current drag.
fn deform_body_sprite(
    time: Res<Time>,
    cells: Query<(&LinearVelocity, &MoveInput, &Proteome, &CellSize, &LinearDamping), With<Cell>>,
    mut pivots: Query<(&ChildOf, &mut BodyDeform)>,
) {
    for (parent, mut deform) in &mut pivots {
        let Ok((velocity, input, proteome, size, damping)) = cells.get(parent.parent()) else {
            continue;
        };
        let top_speed = thrust_capacity(proteome) / (body_mass(size) * damping.0.max(f32::EPSILON));
        let target = move_stretch(velocity.0, input.0, top_speed);
        let next = deform_step(*deform, target, velocity.0, time.delta_secs());
        if *deform != next {
            *deform = next;
        }
    }
}

/// Puts the body sprite on a whole world pixel, so its pixels stay on the
/// screen pixel grid while the physics body moves smoothly, and applies the
/// stretch of the pivot.
fn snap_body_sprite(
    cells: Query<&Transform, (With<Cell>, Without<BodyDeform>, Without<BlobSprite>)>,
    mut pivots: Query<(&ChildOf, &Children, &BodyDeform, &mut Transform), Without<BlobSprite>>,
    mut sprites: Query<&mut Transform, (With<BlobSprite>, Without<BodyDeform>)>,
) {
    for (parent, children, deform, mut pivot) in &mut pivots {
        let Ok(cell) = cells.get(parent.parent()) else {
            continue;
        };
        let (pivot_transform, rotation) = deform_transforms(snap_offset(cell.translation.truncate()), *deform);
        *pivot = pivot_transform;
        let mut children = sprites.iter_many_mut(children);
        while let Some(mut sprite) = children.fetch_next() {
            sprite.rotation = rotation;
        }
    }
}

/// The offset that moves `position` to the nearest whole pixel.
pub fn snap_offset(position: Vec2) -> Vec2 {
    position.round() - position
}

/// One smoothing step of the camera towards a target. Exact for any `dt`.
pub fn follow_step(current: Vec2, target: Vec2, dt: f32, rate: f32) -> Vec2 {
    current.lerp(target, 1.0 - (-rate * dt).exp())
}

fn camera_follow(
    time: Res<Time>,
    mut track: ResMut<CameraTrack>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&Transform, (With<Cell>, Without<Camera2d>)>,
    mut cameras: Query<&mut Transform, With<Camera2d>>,
) {
    let Some(target) = players
        .iter()
        .find_map(|controlled| cells.get(controlled.0).ok())
        .map(|t| t.translation.truncate())
    else {
        return;
    };
    for mut camera in &mut cameras {
        let current = track.0.unwrap_or(camera.translation.truncate());
        let next = follow_step(current, target, time.delta_secs(), CAMERA_FOLLOW_RATE);
        track.0 = Some(next);
        // A whole pixel keeps the sprite pixels on the screen pixel grid.
        camera.translation = next.round().extend(camera.translation.z);
    }
}

/// Puts the camera back at the origin for the menu screens.
fn reset_camera(
    mut track: ResMut<CameraTrack>,
    mut cameras: Query<&mut Transform, With<Camera2d>>,
) {
    track.0 = None;
    for mut camera in &mut cameras {
        camera.translation.x = 0.0;
        camera.translation.y = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heading_row_follows_fast_motion_and_holds_when_still() {
        assert_eq!(heading_row(Vec2::new(100.0, 0.0), 3), 0);
        assert_eq!(heading_row(Vec2::new(0.0, 100.0), 3), 6);
        assert_eq!(heading_row(Vec2::new(1.0, 1.0), 3), 3);
        assert_eq!(heading_row(Vec2::ZERO, 5), 5);
    }

    #[test]
    fn the_stretch_needs_both_speed_and_a_push_along_the_motion() {
        let top = 100.0;
        // Full speed, full push along the motion: the full stretch.
        assert!((move_stretch(Vec2::new(100.0, 0.0), Vec2::X, top) - MAX_MOVE_STRETCH).abs() < 1e-6);
        // Half the speed: half the stretch.
        assert!((move_stretch(Vec2::new(50.0, 0.0), Vec2::X, top) - MAX_MOVE_STRETCH / 2.0).abs() < 1e-6);
        // Faster than the top speed (a dash, a current) does not stretch more.
        assert!((move_stretch(Vec2::new(300.0, 0.0), Vec2::X, top) - MAX_MOVE_STRETCH).abs() < 1e-6);
        // Carried with no push, or pushing against the motion: no stretch.
        assert_eq!(move_stretch(Vec2::new(100.0, 0.0), Vec2::ZERO, top), 0.0);
        assert_eq!(move_stretch(Vec2::new(100.0, 0.0), -Vec2::X, top), 0.0);
        // A push across the motion gives no stretch, a diagonal push a part.
        assert!(move_stretch(Vec2::new(100.0, 0.0), Vec2::Y, top).abs() < 1e-6);
        let diagonal = move_stretch(Vec2::new(100.0, 0.0), Vec2::ONE.normalize(), top);
        assert!(diagonal > 0.0 && diagonal < MAX_MOVE_STRETCH);
        // Still, or no top speed: no stretch.
        assert_eq!(move_stretch(Vec2::new(1.0, 0.0), Vec2::X, top), 0.0);
        assert_eq!(move_stretch(Vec2::new(100.0, 0.0), Vec2::X, 0.0), 0.0);
    }

    #[test]
    fn the_stretch_keeps_the_area() {
        for stretch in [0.0, 0.05, MAX_MOVE_STRETCH, 1.0] {
            let scale = stretch_scale(stretch);
            assert!((scale.x * scale.y - 1.0).abs() < 1e-6);
            assert!(scale.x >= 1.0);
        }
        assert_eq!(stretch_scale(-1.0), Vec2::ONE);
    }

    #[test]
    fn the_deform_step_does_not_depend_on_the_frame_length() {
        let start = BodyDeform::default();
        let v = Vec2::new(0.0, 80.0);
        let one = deform_step(start, 0.1, v, 0.1);
        let two = deform_step(deform_step(start, 0.1, v, 0.05), 0.1, v, 0.05);
        assert!((one.stretch - two.stretch).abs() < 1e-6);
        assert!((one.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // When the cell stops, the stretch relaxes along the last axis.
        let relaxed = deform_step(one, 0.0, Vec2::ZERO, 10.0);
        assert!(relaxed.stretch < 1e-6);
        assert_eq!(relaxed.angle, one.angle);
    }

    #[test]
    fn the_pivot_stretches_along_the_motion_and_the_frame_does_not_turn() {
        // No stretch: the identity, so the still sprite stays on the pixel grid.
        let (pivot, rotation) = deform_transforms(Vec2::new(0.25, -0.5), BodyDeform { stretch: 0.0, angle: 1.0 });
        assert_eq!(pivot, Transform::from_xyz(0.25, -0.5, 0.0));
        assert_eq!(rotation, Quat::IDENTITY);
        // A stretch along +Y (up): the sprite is taller and thinner, not turned.
        let deform = BodyDeform { stretch: 0.2, angle: std::f32::consts::FRAC_PI_2 };
        let (pivot, rotation) = deform_transforms(Vec2::ZERO, deform);
        let affine = pivot.to_matrix() * Transform::from_rotation(rotation).to_matrix();
        let up = affine.transform_vector3(Vec3::Y);
        let right = affine.transform_vector3(Vec3::X);
        assert!((up - Vec3::Y * 1.2).length() < 1e-5, "{up}");
        assert!((right - Vec3::X / 1.2).length() < 1e-5, "{right}");
    }

    #[test]
    fn the_ros_tint_goes_from_white_to_sickly_green() {
        assert_eq!(ros_tint_color(0.0).to_srgba(), Srgba::WHITE);
        let full = ros_tint_color(1.0).to_srgba();
        assert!(full.red < 0.7 && full.blue < 0.5 && full.green > 0.9, "{full:?}");
        assert_eq!(ros_tint_color(5.0), ros_tint_color(1.0));
    }

    #[test]
    fn the_snap_offset_lands_on_a_whole_pixel() {
        for p in [Vec2::new(10.3, -4.7), Vec2::new(-0.5, 99.99), Vec2::ZERO] {
            let snapped = p + snap_offset(p);
            assert_eq!(snapped, snapped.round());
            assert!(snap_offset(p).abs().max_element() <= 0.5);
        }
    }

    #[test]
    fn camera_follow_does_not_depend_on_the_frame_length() {
        let target = Vec2::new(100.0, -40.0);
        let one = follow_step(Vec2::ZERO, target, 0.1, 6.0);
        let two = follow_step(follow_step(Vec2::ZERO, target, 0.05, 6.0), target, 0.05, 6.0);
        assert!(one.distance(two) < 1e-3);
        assert!(follow_step(Vec2::ZERO, target, 100.0, 6.0).distance(target) < 1e-3);
    }
}
