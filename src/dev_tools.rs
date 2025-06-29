//! Development tools for the game. This plugin is only enabled in dev builds.

use crate::player::{controller::MovementAction, Player}; // Import MovementAction
use avian3d::prelude::{AngularVelocity, ExternalTorque};
use bevy::{
    dev_tools::ui_debug_overlay::{DebugUiPlugin, UiDebugOptions},
    ecs::event::EventReader,
    input::common_conditions::input_just_pressed,
    prelude::*,
    window::PrimaryWindow,
};
use bevy_egui::{egui, EguiContext}; // Import egui and EguiContext

// Resource to store the latest movement info for debugging
#[derive(Resource, Default, Debug)]
struct DebugMovementInfo {
    last_move_direction: Option<Vec2>,
    jumped_this_frame: bool,
    external_torque: Option<Vec3>,
    angular_velocity: Option<Vec3>,
}

pub(crate) fn plugin(app: &mut App) {
    let toggle_system = toggle_debug_ui.run_if(input_just_pressed(TOGGLE_KEY));

    // Toggle the debug overlay for UI.
    app.add_plugins(DebugUiPlugin)
        .init_resource::<DebugMovementInfo>() // Initialize the resource
        .add_systems(
            Update,
            (
                toggle_system,
                read_movement_actions,
                read_angular_movement_info,
                inspector_ui.run_if(is_debug_ui_enabled),
            ),
        );
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;

fn toggle_debug_ui(mut options: ResMut<UiDebugOptions>) {
    println!("Toggling debug UI");
    options.toggle();
}

// Run condition that checks if the debug UI is enabled
fn is_debug_ui_enabled(debug_options: Res<UiDebugOptions>) -> bool {
    debug_options.enabled
}

// System to read MovementAction events and update the DebugMovementInfo resource
fn read_movement_actions(
    mut reader: EventReader<MovementAction>,
    mut move_info: ResMut<DebugMovementInfo>,
) {
    // Reset jump state for the new frame
    move_info.jumped_this_frame = false;
    let mut latest_move = None;

    for event in reader.read() {
        match event {
            MovementAction::Move(direction) => {
                latest_move = Some(*direction);
            }
            MovementAction::Jump => {
                move_info.jumped_this_frame = true;
            }
        }
    }
    // Store the last move direction encountered this frame
    move_info.last_move_direction = latest_move;
}

fn read_angular_movement_info(
    mut move_info: ResMut<DebugMovementInfo>,
    query: Query<(&ExternalTorque, &AngularVelocity), With<Player>>,
) {
    if let Ok((torque, angular_velocity)) = query.get_single() {
        move_info.external_torque = Some(**torque);
        move_info.angular_velocity = Some(**angular_velocity);
    }
}

fn inspector_ui(world: &mut World) {
    // Fetch DebugMovementInfo first
    // Use query to avoid borrowing the whole world if DebugMovementInfo doesn't exist yet
    let move_info_exists = world.contains_resource::<DebugMovementInfo>();
    let (last_move_direction, jumped_this_frame, external_torque, angular_velocity) =
        if move_info_exists {
            let move_info = world.resource::<DebugMovementInfo>();
            (
                move_info.last_move_direction,
                move_info.jumped_this_frame,
                move_info.external_torque,
                move_info.angular_velocity,
            )
        } else {
            (None, false, None, None)
        };

    let Ok(egui_context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single_mut(world)
    else {
        return;
    };

    let mut egui_context = egui_context.clone();

    egui::Window::new("Debug Info").show(egui_context.get_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Display Movement Info (using fetched values)
            ui.heading("Movement Actions");
            if let Some(dir) = last_move_direction {
                ui.label(format!("Move Direction: {:.2}, {:.2}", dir.x, dir.y));
            } else {
                ui.label("Move Direction: None");
            }
            ui.label(format!("Jumped: {}", jumped_this_frame));
            ui.separator();

            if let Some(torque) = external_torque {
                ui.label(format!(
                    "External Torque: {:.2}, {:.2}, {:.2}",
                    torque.x, torque.y, torque.z
                ));
            } else {
                ui.label("External Torque: None");
            }
            if let Some(angular_velocity) = angular_velocity {
                ui.label(format!(
                    "Angular Velocity: {:.2}, {:.2}, {:.2}",
                    angular_velocity.x, angular_velocity.y, angular_velocity.z
                ));
            } else {
                ui.label("Angular Velocity: None");
            }
        });
    });
}
