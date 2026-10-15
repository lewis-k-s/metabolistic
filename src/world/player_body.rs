//! # The player's cell as a body
//!
//! The local player's `Cell` gets the physics body: a circle collider, mass and
//! drag from `CellSize`, and a thrust force from `MoveInput` and its flagella.
//! The sprite is a child of the cell. It plays the baked sheets of
//! `sprites` (spawn, idle, move in 8 directions). The body deform is in the
//! frames (art rule 9), so the game never scales or rotates the sprite. World
//! units are sprite pixels; the camera scale makes each one `PIXEL_SCALE`
//! screen pixels.
//!
//! The formulas are in `movement`. The systems here fetch data and hand it to
//! avian2d.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::cell::{Cell, CellSize};
use crate::movement::{
    MoveInput, ThrustEffort, body_drag, body_mass, thrust_capacity, thrust_force, wasd_direction,
    world_radius,
};
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::Proteome;
use crate::sprites::{Animator, Sheets, SpriteSystems, clip, direction_row};
use crate::{GameState, InRun};

/// How fast the camera catches up with the player, per second.
const CAMERA_FOLLOW_RATE: f32 = 6.0;
/// Speed above which the cell plays `move` and takes the row of its heading,
/// in pixels per second.
const MOVE_SPEED_THRESHOLD: f32 = 8.0;

/// The child sprite of a cell that plays the sprite sheets.
#[derive(Component)]
pub struct BlobSprite;

/// The smoothed camera position, before it is rounded to a whole pixel.
#[derive(Resource, Default)]
struct CameraTrack(Option<Vec2>);

pub fn plugin(app: &mut App) {
    app.init_resource::<CameraTrack>()
        .add_systems(
            Update,
            (read_move_input, drive_body_sprite)
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
    .add_systems(OnExit(InRun), reset_camera);
}

/// Adds the body and the sprite to the player's cell.
pub fn attach_body(commands: &mut Commands, sheets: &Sheets, cell: Entity) {
    let size = CellSize::default();
    let radius = world_radius(&size);
    commands.entity(cell).insert((
        RigidBody::Dynamic,
        Collider::circle(radius),
        Mass(body_mass(&size)),
        LinearDamping(body_drag(&size)),
        LockedAxes::ROTATION_LOCKED,
        ConstantForce::default(),
        Visibility::default(),
        DespawnOnExit(InRun),
    ));
    let body = sheets
        .bundle("player_cell", clip::SPAWN)
        .expect("the manifest has player_cell/spawn");
    commands
        .entity(cell)
        .with_child((Name::new("Cell Sprite"), BlobSprite, body, Transform::default()));
}

/// WASD sets the wanted direction on the Run screen. In Manage, time runs but
/// the keys are for the editor, so the cell stops pushing.
fn read_move_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut inputs: Query<&mut MoveInput, With<Cell>>,
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
        if let Ok(mut input) = inputs.get_mut(controlled.0) {
            input.0 = direction;
        }
    }
}

/// Sets the thrust force of each cell with a body, and adds the thrust used to
/// its `ThrustEffort`. The metabolic tick pays ATP for it.
fn apply_thrust(
    time: Res<Time>,
    mut cells: Query<
        (
            &MoveInput,
            &Proteome,
            &mut ConstantForce,
            &mut ThrustEffort,
        ),
        With<Cell>,
    >,
) {
    for (input, proteome, mut force, mut effort) in &mut cells {
        let thrust = thrust_force(input.0, thrust_capacity(proteome));
        force.0 = thrust;
        effort.0 += thrust.length() * time.delta_secs();
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
    mut sprites: Query<(&ChildOf, &mut Animator), With<BlobSprite>>,
) {
    for (parent, mut animator) in &mut sprites {
        let Ok(velocity) = cells.get(parent.parent()) else {
            continue;
        };
        animator.row = heading_row(velocity.0, animator.row);
        if animator.clip != clip::SPAWN || animator.finished() {
            let moving = velocity.0.length() > MOVE_SPEED_THRESHOLD;
            animator.play(if moving { clip::MOVE } else { clip::IDLE }, &sheets);
        }
    }
}

/// Puts the body sprite on a whole world pixel, so its pixels stay on the
/// screen pixel grid while the physics body moves smoothly.
fn snap_body_sprite(
    cells: Query<&Transform, (With<Cell>, Without<BlobSprite>)>,
    mut sprites: Query<(&ChildOf, &mut Transform), With<BlobSprite>>,
) {
    for (parent, mut transform) in &mut sprites {
        let Ok(cell) = cells.get(parent.parent()) else {
            continue;
        };
        let position = cell.translation.truncate();
        transform.translation = (snap_offset(position)).extend(0.0);
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
