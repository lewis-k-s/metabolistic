use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContext, EguiPlugin};
use bevy_inspector_egui::DefaultInspectorConfigPlugin;

/// Plugin for adding inspector functionality to debug builds
pub(crate) fn plugin(app: &mut App) {
    // Make sure we have egui available
    app.add_plugins(EguiPlugin);

    // Add the default inspector configuration
    app.add_plugins(DefaultInspectorConfigPlugin);

    // Add the world inspector system
    app.add_systems(Update, inspector_ui.run_if(input_toggle_active));
}

fn inspector_ui(world: &mut World) {
    let Ok(egui_context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single(world)
    else {
        return;
    };
    let mut egui_context = egui_context.clone();

    egui::Window::new("UI").show(egui_context.get_mut(), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            // equivalent to `WorldInspectorPlugin`
            bevy_inspector_egui::bevy_inspector::ui_for_world(world, ui);

            egui::CollapsingHeader::new("Materials").show(ui, |ui| {
                bevy_inspector_egui::bevy_inspector::ui_for_assets::<StandardMaterial>(world, ui);
            });

            ui.heading("Entities");
            bevy_inspector_egui::bevy_inspector::ui_for_entities(world, ui);
        });
    });
}

/// System to toggle the inspector with the F1 key
fn input_toggle_active(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut inspector_active: Local<bool>,
) -> bool {
    if keyboard.just_pressed(KeyCode::F1) {
        *inspector_active = !*inspector_active;
    }
    *inspector_active
}
