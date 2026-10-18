//! The zoom between the Run screen and the Manage screen.
//!
//! Manage is the inside of the player's cell, so Tab zooms in to it and out of
//! it. A zoom has two parts:
//!
//! 1. **Out**: the camera of the screen that closes zooms (into the cell, or
//!    away from the map) while a cytoplasm-coloured cover fades in.
//! 2. **In**: the state changes under the full cover. The camera of the new
//!    screen starts at the other end of the zoom and settles, while the cover
//!    fades out.
//!
//! Both parts move the same way (in, or out), so the two cameras read as one
//! move. The run camera keeps its scale in `Zoom::base` and gets it back at the
//! end. The map camera needs no copy: `fit_map` sets it again each frame. The
//! Codex has its own move from the map to the genome (`codex::genome_view`).
//!
//! The smoke test sets the state directly, so it skips the zoom.

use bevy::camera::CameraUpdateSystems;
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, GlobalZIndex};

use super::RunCamera;
use super::manage::MapCamera;
use crate::GameState;

/// Seconds of each part.
const OUT_SECONDS: f32 = 0.24;
const IN_SECONDS: f32 = 0.34;
/// Camera scale at the end of the out part and at the start of the in part,
/// for a zoom into the cell (a smaller scale shows less of the world). A zoom
/// out of the cell uses the inverse.
const INTO_OUT_SCALE: f32 = 0.22;
const INTO_IN_SCALE: f32 = 2.6;
/// The colour of the cover: the cytoplasm of the Manage map.
const COVER: Color = Color::srgb(0.05, 0.07, 0.12);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ScreenZoom>()
        .add_systems(PostUpdate, run_zoom.before(CameraUpdateSystems).before(TransformSystems::Propagate));
}

/// The zoom that is on, if any.
#[derive(Resource, Default, Debug)]
pub struct ScreenZoom(Option<Zoom>);

#[derive(Debug, Clone, PartialEq)]
struct Zoom {
    from: GameState,
    to: GameState,
    /// Seconds since the start.
    t: f32,
    /// The state change was asked for.
    switched: bool,
    /// The scale of the run camera before the zoom.
    base: Option<f32>,
}

impl ScreenZoom {
    /// A zoom is on: the navigation keys wait.
    pub fn busy(&self) -> bool {
        self.0.is_some()
    }

    /// Starts a zoom from `from` to `to`. Does nothing while a zoom is on.
    pub fn start(&mut self, from: &GameState, to: GameState) {
        if self.0.is_none() {
            self.0 = Some(Zoom { from: from.clone(), to, t: 0.0, switched: false, base: None });
        }
    }
}

/// True if a move from `from` to `to` goes into the cell.
pub fn inward(to: &GameState) -> bool {
    *to == GameState::Manage
}

/// The camera scale factor and the cover's opacity at `t` seconds into a
/// zoom.
pub fn zoom_frame(inward: bool, t: f32) -> (f32, f32) {
    let (out_end, in_start) = if inward {
        (INTO_OUT_SCALE, INTO_IN_SCALE)
    } else {
        (1.0 / INTO_OUT_SCALE, 1.0 / INTO_IN_SCALE)
    };
    let log_lerp = |a: f32, b: f32, e: f32| (a.ln() + (b.ln() - a.ln()) * e).exp();
    if t < OUT_SECONDS {
        let u = (t / OUT_SECONDS).clamp(0.0, 1.0);
        // Faster and faster, and the cover comes in the second half.
        (log_lerp(1.0, out_end, u * u), smoothstep(0.35, 1.0, u))
    } else {
        let u = ((t - OUT_SECONDS) / IN_SECONDS).clamp(0.0, 1.0);
        // Slower and slower: the new view settles.
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        (log_lerp(in_start, 1.0, e), 1.0 - smoothstep(0.0, 0.6, u))
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// The cover over the screen while a zoom is on.
#[derive(Component)]
struct ZoomCover;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn run_zoom(
    time: Res<Time<Real>>,
    mut zoom: ResMut<ScreenZoom>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut run_cameras: Query<&mut Projection, (With<RunCamera>, Without<MapCamera>)>,
    mut map_cameras: Query<&mut Projection, (With<MapCamera>, Without<RunCamera>)>,
    mut covers: Query<(Entity, &mut BackgroundColor), With<ZoomCover>>,
    mut commands: Commands,
) {
    let Some(z) = zoom.0.as_mut() else {
        for (entity, _) in &covers {
            commands.entity(entity).despawn();
        }
        return;
    };
    let now = state.get();
    // Something else changed the screen (Esc, death): stop.
    let lost = if z.switched { *now != z.to && *now != z.from } else { *now != z.from };
    if lost {
        restore(&mut run_cameras, z.base.take());
        zoom.0 = None;
        return;
    }
    if covers.is_empty() {
        commands.spawn((
            Name::new("Zoom Cover"),
            ZoomCover,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(COVER.with_alpha(0.0)),
            GlobalZIndex(200),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ));
    }
    let waiting = z.switched && *now != z.to;
    if !waiting {
        z.t += time.delta_secs();
    }
    let t = if z.switched { z.t.max(OUT_SECONDS) } else { z.t.min(OUT_SECONDS) };
    let (factor, alpha) = zoom_frame(inward(&z.to), t);
    for (_, mut color) in &mut covers {
        color.0 = COVER.with_alpha(alpha);
    }
    if !z.switched && z.t >= OUT_SECONDS {
        // Under the full cover: the run camera gets its scale back before the
        // Manage screen takes it for the inset.
        restore(&mut run_cameras, z.base.take());
        next.set(z.to.clone());
        z.switched = true;
        return;
    }
    if waiting {
        return;
    }
    if z.t >= OUT_SECONDS + IN_SECONDS {
        restore(&mut run_cameras, z.base.take());
        zoom.0 = None;
        return;
    }
    match now {
        GameState::Run => {
            for mut projection in &mut run_cameras {
                if let Projection::Orthographic(ortho) = &mut *projection {
                    let base = *z.base.get_or_insert(ortho.scale);
                    ortho.scale = base * factor;
                }
            }
        }
        GameState::Manage => {
            // `fit_map` sets the scale again each frame.
            for mut projection in &mut map_cameras {
                if let Projection::Orthographic(ortho) = &mut *projection {
                    ortho.scale *= factor;
                }
            }
        }
        _ => {}
    }
}

/// Gives the run camera its scale from before the zoom.
fn restore(
    run_cameras: &mut Query<&mut Projection, (With<RunCamera>, Without<MapCamera>)>,
    base: Option<f32>,
) {
    let Some(base) = base else {
        return;
    };
    for mut projection in run_cameras.iter_mut() {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = base;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_starts_and_ends_at_the_normal_scale_with_no_cover() {
        for inward in [true, false] {
            let (start, cover) = zoom_frame(inward, 0.0);
            assert!((start - 1.0).abs() < 1e-5 && cover == 0.0);
            let (end, cover) = zoom_frame(inward, OUT_SECONDS + IN_SECONDS);
            assert!((end - 1.0).abs() < 1e-5 && cover == 0.0);
        }
    }

    #[test]
    fn the_cover_is_full_at_the_switch() {
        for inward in [true, false] {
            let (_, before) = zoom_frame(inward, OUT_SECONDS - 1e-4);
            let (_, after) = zoom_frame(inward, OUT_SECONDS);
            assert!(before > 0.99 && after > 0.99);
        }
    }

    #[test]
    fn both_parts_move_the_same_way() {
        // Into the cell, the scale falls in both parts (the world grows).
        let samples = |inward| -> Vec<f32> {
            (0..=20).map(|i| zoom_frame(inward, (OUT_SECONDS + IN_SECONDS) * i as f32 / 20.0).0).collect()
        };
        let into = samples(true);
        let (out_part, in_part) = into.split_at(into.iter().position(|f| *f > 1.0).unwrap());
        assert!(out_part.windows(2).all(|w| w[1] <= w[0] + 1e-6));
        assert!(in_part.windows(2).all(|w| w[1] <= w[0] + 1e-6));
        // Out of the cell, it rises in both parts.
        let away = samples(false);
        let (out_part, in_part) = away.split_at(away.iter().position(|f| *f < 1.0).unwrap());
        assert!(out_part.windows(2).all(|w| w[1] >= w[0] - 1e-6));
        assert!(in_part.windows(2).all(|w| w[1] >= w[0] - 1e-6));
    }

    #[test]
    fn a_second_start_waits_for_the_first() {
        let mut zoom = ScreenZoom::default();
        zoom.start(&GameState::Run, GameState::Manage);
        zoom.start(&GameState::Manage, GameState::Run);
        assert!(zoom.busy());
        assert_eq!(zoom.0.as_ref().unwrap().to, GameState::Manage);
    }
}
