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
    commands
        .entity(cell)
        .with_child((Name::new("Cell Sprite"), BlobSprite, body, Transform::default()));
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
    mut sprites: Query<(&ChildOf, &mut Sprite), With<BlobSprite>>,
) {
    for (parent, mut sprite) in &mut sprites {
        let Ok(pools) = cells.get(parent.parent()) else {
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
