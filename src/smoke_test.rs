//! Smoke test for the full game: run with `METABOLISTIC_SMOKE_TEST=1 cargo run`.
//!
//! The app visits every `GameState`, holds W in the 3D scene to drive the ball,
//! saves a screenshot per state, logs the player ball's motion, then exits.
//! A panic or a missing resource fails the run. Screenshots go to
//! `METABOLISTIC_SMOKE_DIR` (default `target/smoke`).

use avian3d::prelude::AngularVelocity;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use std::path::PathBuf;

use crate::GameState;
use crate::player::Player;

/// Frames to stay in each state before the screenshot. The first frames compile
/// shaders, so the first state gets more.
const FRAMES_PER_STATE: u32 = 120;
const FIRST_STATE_EXTRA_FRAMES: u32 = 120;
/// Frames to wait after a screenshot request, so it is written before we move on.
const SCREENSHOT_SETTLE_FRAMES: u32 = 10;

const SEQUENCE: [GameState; 5] = [
    GameState::MainMenu,
    GameState::Scene3D,
    GameState::Scene2D,
    GameState::GenomeEditing,
    GameState::MainMenu,
];

/// True when the smoke test is requested by environment variable.
pub fn requested() -> bool {
    std::env::var_os("METABOLISTIC_SMOKE_TEST").is_some()
}

pub fn plugin(app: &mut App) {
    let dir = std::env::var_os("METABOLISTIC_SMOKE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/smoke"));
    if let Err(err) = std::fs::create_dir_all(&dir) {
        warn!("smoke test: cannot create {}: {err}", dir.display());
    }
    app.insert_resource(SmokeTest {
        dir,
        step: 0,
        frames_in_step: 0,
        screenshot_requested_at: None,
    })
    .add_systems(Update, drive_smoke_test)
    .add_systems(OnEnter(GameState::Scene3D), hold_forward)
    .add_systems(OnExit(GameState::Scene3D), release_forward);
}

#[derive(Resource)]
struct SmokeTest {
    dir: PathBuf,
    step: usize,
    frames_in_step: u32,
    screenshot_requested_at: Option<u32>,
}

fn drive_smoke_test(
    mut smoke: ResMut<SmokeTest>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
    players: Query<(&Transform, Option<&AngularVelocity>), With<Player>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let target = SEQUENCE[smoke.step].clone();
    if *state.get() != target {
        next_state.set(target);
        return;
    }
    smoke.frames_in_step += 1;

    let hold = FRAMES_PER_STATE
        + if smoke.step == 0 {
            FIRST_STATE_EXTRA_FRAMES
        } else {
            0
        };
    if smoke.frames_in_step < hold {
        return;
    }

    match smoke.screenshot_requested_at {
        None => {
            for (transform, angular_velocity) in &players {
                info!(
                    "smoke test: {target:?} player at {:.2?}, angular velocity {:.2?}",
                    transform.translation,
                    angular_velocity.map(|v| v.0)
                );
            }
            let path = smoke.dir.join(format!("{}-{target:?}.png", smoke.step));
            info!("smoke test: screenshot {}", path.display());
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
            smoke.screenshot_requested_at = Some(smoke.frames_in_step);
        }
        Some(at) if smoke.frames_in_step >= at + SCREENSHOT_SETTLE_FRAMES => {
            smoke.step += 1;
            smoke.frames_in_step = 0;
            smoke.screenshot_requested_at = None;
            if smoke.step == SEQUENCE.len() {
                info!("smoke test: all states visited, exiting");
                exit.write(AppExit::Success);
            }
        }
        Some(_) => {}
    }
}

/// Hold W so the player ball rolls; this exercises input, controller and physics.
fn hold_forward(mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.press(KeyCode::KeyW);
}

fn release_forward(mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.release(KeyCode::KeyW);
}
