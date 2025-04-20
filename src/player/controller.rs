use crate::player::Player;
use avian3d::{math::*, prelude::*};
use bevy::color::palettes::basic::{BLUE, GREEN, RED, YELLOW};
use bevy::color::LinearRgba;
use bevy::gizmos::gizmos::Gizmos;
use bevy::{ecs::query::Has, prelude::*};
use leafwing_input_manager::prelude::*;
use std::f32::consts::PI;

pub struct CharacterControllerPlugin;

#[derive(Actionlike, Clone, Debug, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum Action {
    Jump,
    #[actionlike(DualAxis)]
    Move,
    #[actionlike(DualAxis)]
    Pan,
}

impl Action {
    pub fn input_map() -> InputMap<Self> {
        let dpad = VirtualDPad::new(KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD);

        InputMap::new([(Action::Jump, KeyCode::Space)])
            .with_dual_axis(Action::Move, dpad)
            .with_dual_axis(Action::Pan, MouseMove::default())
    }
}

impl Plugin for CharacterControllerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<MovementAction>()
            .add_plugins(InputManagerPlugin::<Action>::default())
            .add_systems(Update, (movement_input, update_grounded, movement).chain());
    }
}

/// An event sent for a movement input action.
#[derive(Event)]
pub enum MovementAction {
    Move(Vector2),
    Jump,
}

/// A marker component indicating that an entity is using a character controller.
#[derive(Component)]
pub struct CharacterController;

/// A marker component indicating that an entity is on the ground.
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct Grounded;
/// The acceleration used for character movement.
#[derive(Component)]
pub struct MovementAcceleration(Scalar);

/// The strength of a jump.
#[derive(Component)]
pub struct JumpImpulse(Scalar);

/// The maximum angle a slope can have for a character controller
#[derive(Component)]
pub struct MaxSlopeAngle(Scalar);

/// kinematic character controller.
#[derive(Bundle)]
pub struct CharacterControllerBundle {
    character_controller: CharacterController,
    rigid_body: RigidBody,
    collider: Collider,
    friction: Friction,
    external_torque: ExternalTorque,
    ground_caster: ShapeCaster,
    locked_axes: LockedAxes,
    movement: MovementBundle,
}

/// character movement.
#[derive(Bundle)]
pub struct MovementBundle {
    acceleration: MovementAcceleration,
    damping: AngularDamping,
    jump_impulse: JumpImpulse,
    max_slope_angle: MaxSlopeAngle,
}

impl MovementBundle {
    pub const fn new(
        acceleration: Scalar,
        damping: Scalar,
        jump_impulse: Scalar,
        max_slope_angle: Scalar,
    ) -> Self {
        Self {
            acceleration: MovementAcceleration(acceleration),
            damping: AngularDamping(damping),
            jump_impulse: JumpImpulse(jump_impulse),
            max_slope_angle: MaxSlopeAngle(max_slope_angle),
        }
    }
}

impl Default for MovementBundle {
    fn default() -> Self {
        Self::new(1.0, 0.9, 7.0, PI * 0.45)
    }
}

impl CharacterControllerBundle {
    pub fn new(collider: Collider) -> Self {
        // Create shape caster as a slightly smaller version of collider
        let mut caster_shape = collider.clone();
        caster_shape.set_scale(Vector::ONE * 0.99, 10);

        Self {
            character_controller: CharacterController,
            rigid_body: RigidBody::Dynamic,
            collider,
            friction: Friction {
                static_coefficient: 0.9,
                dynamic_coefficient: 0.9,
                combine_rule: CoefficientCombine::Average,
            },
            external_torque: ExternalTorque::ZERO,
            ground_caster: ShapeCaster::new(
                caster_shape,
                Vector::ZERO,
                Quaternion::default(),
                Dir3::NEG_Y,
            )
            .with_max_distance(0.2),
            locked_axes: LockedAxes::new(),
            movement: MovementBundle::default(),
        }
    }

    pub fn with_movement(
        mut self,
        acceleration: Scalar,
        damping: Scalar,
        jump_impulse: Scalar,
        max_slope_angle: Scalar,
    ) -> Self {
        self.movement = MovementBundle::new(acceleration, damping, jump_impulse, max_slope_angle);
        self
    }
}

/// Sends [`MovementAction`] events based on keyboard input.
/// local to the player perspective because we query for camera. Then the event reader is 'global'
fn movement_input(
    mut movement_event_writer: EventWriter<MovementAction>,
    player_query: Query<(&ActionState<Action>, &GlobalTransform), With<Player>>,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    let Ok((action_state, player_transform)) = player_query.get_single() else {
        return;
    };

    let Ok(camera_transform) = camera_query.get_single() else {
        error!("No Camera3d found");
        return;
    };

    let forward = camera_transform.forward().xz().normalize_or_zero();
    let right = camera_transform.right().xz().normalize_or_zero();

    // floating camera gizmo (relative to player)
    let start = player_transform.translation() + Vec3::Y;
    let fwdcam = start + Vec3::new(forward.x, 0.0, forward.y);
    let rightcam = start + Vec3::new(right.x, 0.0, right.y);

    gizmos.circle(start, 0.1, LinearRgba::from(RED));
    gizmos.line(start, fwdcam, LinearRgba::from(RED));
    gizmos.line(start, rightcam, LinearRgba::from(GREEN));

    let input_direction = action_state.axis_pair(&Action::Move);

    // // raw input gizmo
    // let start = player_transform.translation();
    // let end = start + Vec3::new(input_direction.x, 0.0, input_direction.y) * 2.0;
    // gizmos.line(start, end, LinearRgba::from(BLUE));

    if input_direction != Vector2::ZERO {
        let move_direction =
            (forward * input_direction.y + right * input_direction.x).normalize_or_zero();

        if move_direction != Vector2::ZERO {
            movement_event_writer.send(MovementAction::Move(move_direction));

            // gizmo facing direction
            let start = player_transform.translation();
            let end = start + Vec3::new(move_direction.x, 0.0, move_direction.y) * 2.0;
            gizmos.line(start, end, LinearRgba::from(YELLOW));
        }
    }

    if action_state.just_pressed(&Action::Jump) {
        movement_event_writer.send(MovementAction::Jump);
    }
}

/// Updates the [`Grounded`] status for character controllers.
fn update_grounded(
    mut commands: Commands,
    mut query: Query<
        (Entity, &ShapeHits, &Rotation, Option<&MaxSlopeAngle>),
        With<CharacterController>,
    >,
) {
    for (entity, hits, rotation, max_slope_angle) in &mut query {
        // The character is grounded if the shape caster has a hit with a normal
        // that isn't too steep.
        let is_grounded = hits.iter().any(|hit| {
            if let Some(angle) = max_slope_angle {
                (rotation * -hit.normal2).angle_between(Vector::Y).abs() <= angle.0
            } else {
                true
            }
        });

        if is_grounded {
            commands.entity(entity).insert(Grounded);
        } else {
            commands.entity(entity).remove::<Grounded>();
        }
    }
}

/// Responds to [`MovementAction`] events and moves character controllers accordingly.
fn movement(
    mut movement_event_reader: EventReader<MovementAction>,
    mut controllers: Query<(
        &MovementAcceleration,
        &JumpImpulse,
        &mut ExternalTorque,
        &mut LinearVelocity,
        Has<Grounded>,
    )>,
) {
    let mut active_spin = false;
    for (
        movement_acceleration,
        jump_impulse,
        mut external_torque,
        mut linear_velocity,
        is_grounded,
    ) in &mut controllers
    {
        for event in movement_event_reader.read() {
            match event {
                MovementAction::Move(direction) => {
                    external_torque
                        .apply_torque(
                            Vec3::new(direction.y, 0.0, -direction.x) * movement_acceleration.0,
                        )
                        .with_persistence(false);
                    active_spin = true;
                }
                MovementAction::Jump => {
                    if is_grounded {
                        linear_velocity.y = jump_impulse.0;
                    }
                }
            }
        }
        if !active_spin {
            external_torque.set_torque(Vec3::ZERO);
        }
    }
}
