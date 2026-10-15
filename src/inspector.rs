use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContext, EguiPlugin};
use bevy_inspector_egui::DefaultInspectorConfigPlugin;
use crate::metabolism::FlowmapTimingConfig;

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
            // Metabolic Timing Controls
            egui::CollapsingHeader::new("Metabolic Timing").show(ui, |ui| {
                if let Some(mut timing_config) = world.get_resource_mut::<FlowmapTimingConfig>() {
                    ui.label("Flowmap and Metabolic Block Update Rate");
                    
                    let mut rate = timing_config.updates_per_second;
                    ui.add(egui::Slider::new(&mut rate, 0.1..=60.0)
                        .text("Updates per Second")
                        .logarithmic(true));
                    
                    if rate != timing_config.updates_per_second {
                        timing_config.set_rate(rate);
                        info!("Updated metabolic timing rate to {:.2} Hz", rate);
                    }
                    
                    ui.separator();
                    ui.label(format!("Current Rate: {:.2} Hz", timing_config.updates_per_second));
                    ui.label(format!("Target Delta: {:.3}s", timing_config.target_delta()));
                    ui.label(format!("Accumulator: {:.3}s", timing_config.accumulator));
                    
                    // Quick preset buttons
                    ui.horizontal(|ui| {
                        if ui.button("1 Hz").clicked() {
                            timing_config.set_rate(1.0);
                        }
                        if ui.button("5 Hz").clicked() {
                            timing_config.set_rate(5.0);
                        }
                        if ui.button("10 Hz").clicked() {
                            timing_config.set_rate(10.0);
                        }
                        if ui.button("30 Hz").clicked() {
                            timing_config.set_rate(30.0);
                        }
                    });
                } else {
                    ui.label("FlowmapTimingConfig resource not found");
                }
            });
            
            ui.separator();
            
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
