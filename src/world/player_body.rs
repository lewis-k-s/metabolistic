//! # The player's cell as a body
//!
//! The local player's `Cell` gets the physics body: a circle collider, mass and
//! drag from `CellSize`, and a thrust force from `MoveInput` and its flagella.
//! The sprite is a child of the cell, so the blob look (squash and wobble from
//! velocity) changes only the sprite's scale and rotation, never the collider.
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
use crate::{GameState, InRun};

/// How fast the camera catches up with the player, per second.
const CAMERA_FOLLOW_RATE: f32 = 6.0;
/// Speed at which the blob is fully stretched, in pixels per second.
const BLOB_REFERENCE_SPEED: f32 = 200.0;
/// Size of `player_cell.png` in pixels.
const PLAYER_SPRITE_PIXELS: f32 = 48.0;

/// The child sprite of a cell that squashes and wobbles.
#[derive(Component)]
pub struct BlobSprite;

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (read_move_input, animate_blob, camera_follow)
            .chain()
            .run_if(in_state(InRun)),
    )
    // Thrust is a force: set it before avian's fixed-step physics.
    .add_systems(FixedUpdate, apply_thrust.run_if(in_state(InRun)))
    .add_systems(OnExit(InRun), reset_camera);
}

/// Adds the body and the sprite to the player's cell.
pub fn attach_body(commands: &mut Commands, asset_server: &AssetServer, cell: Entity) {
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
    let sprite_scale = 2.0 * radius / PLAYER_SPRITE_PIXELS;
    commands.entity(cell).with_child((
        Name::new("Cell Sprite"),
        BlobSprite,
        Sprite::from_image(asset_server.load("sprites/player_cell.png")),
        Transform::from_scale(Vec3::splat(sprite_scale)),
    ));
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

/// Visual only: squash and wobble the sprite from the velocity of its cell.
fn animate_blob(
    time: Res<Time>,
    cells: Query<&LinearVelocity, With<Cell>>,
    mut sprites: Query<(&ChildOf, &mut Transform), With<BlobSprite>>,
) {
    let rest_scale = 2.0 * world_radius(&CellSize::default()) / PLAYER_SPRITE_PIXELS;
    for (parent, mut transform) in &mut sprites {
        let Ok(velocity) = cells.get(parent.parent()) else {
            continue;
        };
        let (scale, angle) = blob_shape(velocity.0, time.elapsed_secs());
        transform.scale = (scale * rest_scale).extend(1.0);
        transform.rotation = Quat::from_rotation_z(angle);
    }
}

/// The squash scale and the tilt (radians) of a blob for a velocity and a time.
/// It stretches along the dominant axis of motion and keeps its area, and it
/// wobbles a little, more when it moves fast. Everything is bounded.
pub fn blob_shape(velocity: Vec2, time: f32) -> (Vec2, f32) {
    let speed = velocity.length();
    let motion = (speed / BLOB_REFERENCE_SPEED).clamp(0.0, 1.0);
    let direction = velocity.normalize_or_zero();

    // Stretch along x when moving sideways, along y when moving up or down.
    let axis_bias = direction.x.abs() - direction.y.abs();
    let stretch = 1.0 + 0.18 * motion * axis_bias;
    // A slow breathing wobble, faster and larger when moving.
    let wobble = (0.02 + 0.04 * motion) * (time * (3.0 + 5.0 * motion)).sin();
    let sx = (stretch + wobble).max(0.5);
    let sy = (1.0 / stretch - wobble).max(0.5);

    // Lean into sideways motion, with a small wobble.
    let tilt = -0.12 * motion * direction.x + 0.05 * motion * (time * 7.0).sin();
    (Vec2::new(sx, sy), tilt)
}

/// One smoothing step of the camera towards a target. Exact for any `dt`.
pub fn follow_step(current: Vec2, target: Vec2, dt: f32, rate: f32) -> Vec2 {
    current.lerp(target, 1.0 - (-rate * dt).exp())
}

fn camera_follow(
    time: Res<Time>,
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
        let next = follow_step(
            camera.translation.truncate(),
            target,
            time.delta_secs(),
            CAMERA_FOLLOW_RATE,
        );
        camera.translation = next.extend(camera.translation.z);
    }
}

/// Puts the camera back at the origin for the menu screens.
fn reset_camera(mut cameras: Query<&mut Transform, With<Camera2d>>) {
    for mut camera in &mut cameras {
        camera.translation.x = 0.0;
        camera.translation.y = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blob_at_rest_is_nearly_round() {
        for t in [0.0, 0.7, 1.9, 5.0] {
            let (scale, angle) = blob_shape(Vec2::ZERO, t);
            assert!((scale.x - 1.0).abs() < 0.03 && (scale.y - 1.0).abs() < 0.03);
            assert!(angle.abs() < 0.01);
        }
    }

    #[test]
    fn a_fast_blob_stretches_along_its_motion_and_keeps_its_area() {
        let (side, _) = blob_shape(Vec2::new(400.0, 0.0), 0.0);
        assert!(side.x > 1.1 && side.y < 0.95, "sideways: {side}");
        let (up, _) = blob_shape(Vec2::new(0.0, 400.0), 0.0);
        assert!(up.y > up.x, "upwards: {up}");
        assert!((side.x * side.y - 1.0).abs() < 0.05, "area {}", side.x * side.y);
    }

    #[test]
    fn the_blob_shape_is_bounded() {
        for speed in [0.0, 50.0, 1e3, 1e6] {
            for angle in 0..8 {
                let v = Vec2::from_angle(angle as f32) * speed;
                for t in 0..20 {
                    let (scale, tilt) = blob_shape(v, t as f32 * 0.37);
                    assert!(scale.x > 0.5 && scale.x < 1.4, "{scale}");
                    assert!(scale.y > 0.5 && scale.y < 1.4, "{scale}");
                    assert!(tilt.abs() < 0.25);
                }
            }
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
