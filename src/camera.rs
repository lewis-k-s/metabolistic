use bevy::prelude::*;
use std::f32::consts::PI;
use leafwing_input_manager::prelude::*; // Added for ActionState
use crate::player::controller::Action; // Added for Action enum
use crate::player::Player; // Import Player marker component

// Resource to store the last known focus point for the camera
#[derive(Resource, Default)]
struct LastCameraFocus(Vec3);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastCameraFocus>() // Initialize the resource
           .add_systems(Startup, spawn_camera)
           .add_systems(Update, (pan_camera_input.before(follow_player), follow_player)); // Ensure pan runs before follow
    }
}

// Define components for camera behavior
#[derive(Component)]
pub struct FollowCamera {
    pub distance: f32,
    pub target: Option<Entity>, // Optionally follow a target entity
    pub target_focus_offset: Vec3, // Offset from target's center to look at
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self {
            distance: 4.272,
            target: None,
            target_focus_offset: Vec3::Y * 0.5,
        }
    }
}

// Startup system to spawn the camera
fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.5, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        FollowCamera::default(),
        InputManagerBundle::with_map(Action::input_map()), // Add input map for pan
    ));
}

// System to handle camera panning based on mouse input
fn pan_camera_input(
    player_query: Query<&GlobalTransform, With<Player>>, // Added player query
    mut camera_query: Query<(&mut Transform, &FollowCamera, &ActionState<Action>), With<Camera3d>>,
    last_focus: Res<LastCameraFocus>, // Get the resource
) {
    const CAMERA_ROTATE_RATE: f32 = 0.005;

    for (mut transform, follow_camera, action_state) in camera_query.iter_mut() {
        let camera_pan_vector = action_state.axis_pair(&Action::Pan);

        if camera_pan_vector.length_squared() > 0.0 {
            let delta = camera_pan_vector * CAMERA_ROTATE_RATE;

            // Determine the focus point for rotation
            let focus_point = if let Some(target_entity) = follow_camera.target {
                // If following a target, try to get its position
                player_query.get(target_entity)
                    .map(|tf| tf.translation() + follow_camera.target_focus_offset)
                    .unwrap_or_else(|_| last_focus.0) // Fallback to last known focus if target exists but query fails
            } else {
                last_focus.0 // Default to last known focus if no target
            };

            // Rotate around the focus point for yaw
            transform.rotate_around(focus_point, Quat::from_rotation_y(-delta.x));

            // Rotate locally around the camera's right axis for pitch
            let current_pitch = transform.rotation.to_euler(EulerRot::YXZ).1;
            let max_pitch = PI / 2.0 - 0.01;
            let min_pitch = -PI / 2.0 + 0.01;

            let pitch_change = -delta.y;
            let desired_pitch = current_pitch + pitch_change;
            let clamped_pitch = desired_pitch.clamp(min_pitch, max_pitch);
            let actual_pitch_rotation = Quat::from_rotation_x(clamped_pitch - current_pitch);

            transform.rotate_local(actual_pitch_rotation);
        }
    }
}

// System to make the camera follow the player entity
fn follow_player(
    player_query: Query<(Entity, &GlobalTransform), With<Player>>,
    mut camera_query: Query<(&mut Transform, &mut FollowCamera), With<Camera3d>>,
    time: Res<Time>, // Optional: For smooth camera movement (lerp)
    mut last_focus: ResMut<LastCameraFocus>, // Get the resource mutably
) {
    let follow_speed = 10.0; // Adjust for desired camera follow smoothness

    // Try to find the player entity and its transform
    let player_entity_and_transform = player_query.get_single().ok();

    for (mut camera_transform, mut follow_camera) in camera_query.iter_mut() {
        // Assign the player entity to the camera's target if it's not already set
        if follow_camera.target.is_none() {
            if let Some((player_entity, _)) = player_entity_and_transform {
                follow_camera.target = Some(player_entity);
            }
        }

        // If the camera has a target (the player)
        if let Some(target_entity) = follow_camera.target {
            // Get the target's current transform (if it still exists)
            if let Ok((_, target_transform)) = player_query.get(target_entity) {
                let target_position = target_transform.translation();
                let focus_point = target_position + follow_camera.target_focus_offset;
                last_focus.0 = focus_point; // Update the last known focus point

                // Calculate the desired camera position based on its current rotation (from panning)
                // and the required distance from the focus point.
                let desired_position = focus_point + (camera_transform.back() * follow_camera.distance);

                // Smoothly interpolate (lerp) the camera's position towards the desired position
                let lerped_position = camera_transform.translation.lerp(
                    desired_position,
                    (follow_speed * time.delta_secs()).clamp(0.0, 1.0),
                );

                // Update camera transform to look at the focus point from the new position
                camera_transform.translation = lerped_position;
                camera_transform.look_at(focus_point, Vec3::Y);
            }
        }
    }
} 