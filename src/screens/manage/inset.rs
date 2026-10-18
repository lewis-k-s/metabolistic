//! The live inset of the run. While Manage is open, time still runs outside a
//! shelter, so the player must see the danger. The run camera (the one of the
//! Run screen) draws into a small corner viewport above the map camera, with no
//! run HUD (the HUD is Bevy UI and is removed when the Run screen closes).
//!
//! The viewport follows the `InsetFrame` UI node in the right column, so the
//! layout of the window decides its place. The frame is also the border of the
//! inset: it flashes red when the cell takes damage (the membrane or the
//! genome loses integrity).

use bevy::camera::{ClearColorConfig, Viewport};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::GameState;
use crate::cell::Cell;
use crate::genome_damage::GenomeIntegrity;
use crate::membrane::Membrane;
use crate::player::{ControlledCell, LocalPlayer};
use crate::screens::RunCamera;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SavedRunCamera>()
        .init_resource::<DamageFlash>()
        .add_systems(OnEnter(GameState::Manage), take_run_camera)
        .add_systems(OnExit(GameState::Manage), give_back_run_camera)
        .add_systems(
            Update,
            (follow_frame, damage_flash)
                .chain()
                .run_if(in_state(GameState::Manage)),
        );
}

/// Screen pixels (logical) in the inset for each world pixel.
const INSET_ZOOM: f32 = 2.0;
const FRAME_WIDTH: f32 = 3.0;
const FRAME_CALM: Color = Color::srgb(0.35, 0.4, 0.5);
const FRAME_DAMAGE: Color = Color::srgb(1.0, 0.15, 0.1);
/// How fast the flash fades, per second.
const FLASH_DECAY: f32 = 1.6;

/// The node whose rectangle is the inset.
#[derive(Component)]
pub(super) struct InsetFrame;

/// A node for the right column: the frame of the inset.
pub(super) fn frame_bundle(height: f32) -> impl Bundle {
    (
        InsetFrame,
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(height),
            border: UiRect::all(Val::Px(FRAME_WIDTH)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(FRAME_CALM),
    )
}

/// What the run camera was before the inset took it.
#[derive(Resource, Default)]
struct SavedRunCamera(Option<(isize, ClearColorConfig, Projection)>);

fn take_run_camera(
    mut saved: ResMut<SavedRunCamera>,
    mut cameras: Query<(&mut Camera, &mut Projection), With<RunCamera>>,
) {
    for (mut camera, mut projection) in &mut cameras {
        saved.0 = Some((camera.order, camera.clear_color.clone(), projection.clone()));
        // Above the map camera, drawing on top of it: no clearing.
        camera.order = 1;
        camera.clear_color = ClearColorConfig::None;
        // Not active until the frame has a size.
        camera.is_active = false;
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = 1.0 / INSET_ZOOM;
        }
    }
}

fn give_back_run_camera(
    mut saved: ResMut<SavedRunCamera>,
    mut cameras: Query<(&mut Camera, &mut Projection), With<RunCamera>>,
) {
    let Some((order, clear, original)) = saved.0.take() else {
        return;
    };
    for (mut camera, mut projection) in &mut cameras {
        camera.order = order;
        camera.clear_color = clear.clone();
        camera.viewport = None;
        camera.is_active = true;
        *projection = original.clone();
    }
}

/// Puts the run camera's viewport on the visible frame node. The Codex hides
/// the right column and shows a frame of its own in the same place.
fn follow_frame(
    window: Query<&Window, With<PrimaryWindow>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<InsetFrame>>,
    mut cameras: Query<&mut Camera, With<RunCamera>>,
) {
    let Ok(window) = window.single() else {
        return;
    };
    let Some((node, transform, _)) = frames.iter().find(|(_, _, visible)| visible.get()) else {
        return;
    };
    let border = FRAME_WIDTH * window.scale_factor();
    let size = (node.size() - Vec2::splat(border * 2.0)).max(Vec2::ZERO);
    let center = transform.translation;
    let target = window.physical_size();
    for mut camera in &mut cameras {
        if size.x < 8.0 || size.y < 8.0 {
            camera.is_active = false;
            continue;
        }
        let top_left = (center - size / 2.0).max(Vec2::ZERO);
        let max = target.as_vec2();
        let size = size.min(max - top_left);
        camera.is_active = size.x >= 1.0 && size.y >= 1.0;
        camera.viewport = Some(Viewport {
            physical_position: top_left.as_uvec2(),
            physical_size: size.as_uvec2().max(UVec2::ONE),
            ..default()
        });
    }
}

/// The red flash of the frame.
#[derive(Resource, Default)]
struct DamageFlash {
    last: Option<(f32, f32)>,
    level: f32,
}

/// True if either integrity fell between two frames.
pub fn took_damage(last: (f32, f32), now: (f32, f32)) -> bool {
    now.0 < last.0 - 1e-4 || now.1 < last.1 - 1e-4
}

fn damage_flash(
    time: Res<Time<Real>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Membrane, &GenomeIntegrity), With<Cell>>,
    mut flash: ResMut<DamageFlash>,
    mut frame: Query<&mut BorderColor, With<InsetFrame>>,
) {
    if let Some((membrane, genome)) = super::local_cell(&players).and_then(|c| cells.get(c).ok()) {
        let now = (membrane.integrity, genome.0);
        if let Some(last) = flash.last
            && took_damage(last, now)
        {
            flash.level = 1.0;
        }
        flash.last = Some(now);
    }
    flash.level = (flash.level - FLASH_DECAY * time.delta_secs()).max(0.0);
    let color = FRAME_CALM.mix(&FRAME_DAMAGE, flash.level);
    for mut border in &mut frame {
        *border = BorderColor::all(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_is_a_fall_of_membrane_or_genome() {
        assert!(took_damage((100.0, 1.0), (99.0, 1.0)));
        assert!(took_damage((100.0, 1.0), (100.0, 0.9)));
        assert!(!took_damage((100.0, 1.0), (100.0, 1.0)));
        // Repair is not damage.
        assert!(!took_damage((90.0, 0.9), (95.0, 1.0)));
    }
}
