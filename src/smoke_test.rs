//! Smoke test for the full game: run with `METABOLISTIC_SMOKE_TEST=1 cargo run`.
//!
//! The app waits for `Loading` to open the menu by itself, then visits every
//! screen and menu page, holds W on the Run screen, saves a screenshot per
//! step, logs the run player's position, then exits. W is held for the first
//! frames of the Run screen only, so the screenshot shows the cell near the
//! centre, not off screen. A panic or a missing
//! resource fails the run. Screenshots go to `METABOLISTIC_SMOKE_DIR`
//! (default `target/smoke`).

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use std::path::PathBuf;

use crate::GameState;
use crate::screens::menu::MenuPage;
use crate::screens::run::RunPlayer;

/// Frames to stay in each state before the screenshot. The first frames compile
/// shaders, so the first state gets more.
const FRAMES_PER_STATE: u32 = 120;
const FIRST_STATE_EXTRA_FRAMES: u32 = 120;
/// Frames to wait after a screenshot request, so it is written before we move on.
const SCREENSHOT_SETTLE_FRAMES: u32 = 10;

/// Frames to hold W on the Run screen. At 300 px/s the cell moves about 150 px.
const HOLD_FORWARD_FRAMES: u32 = 30;

/// Frames that `Loading` may take before it counts as stuck.
const MAX_LOADING_FRAMES: u32 = 600;

/// Each step: a screen, and for the menu, the page.
const SEQUENCE: [(GameState, Option<MenuPage>); 5] = [
    (GameState::Menu, Some(MenuPage::Main)),
    (GameState::Menu, Some(MenuPage::Settings)),
    (GameState::Run, None),
    (GameState::Manage, None),
    (GameState::Menu, Some(MenuPage::Main)),
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
        loading_frames: 0,
        screenshot_requested_at: None,
    })
    .add_systems(Update, drive_smoke_test)
    .add_systems(OnEnter(GameState::Run), hold_forward)
    .add_systems(OnExit(GameState::Run), release_forward);
}

#[derive(Resource)]
struct SmokeTest {
    dir: PathBuf,
    step: usize,
    frames_in_step: u32,
    loading_frames: u32,
    screenshot_requested_at: Option<u32>,
}

#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn drive_smoke_test(
    mut smoke: ResMut<SmokeTest>,
    state: Res<State<GameState>>,
    page: Option<Res<State<MenuPage>>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut next_page: ResMut<NextState<MenuPage>>,
    players: Query<&Transform, With<RunPlayer>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    // Loading must open the menu without help.
    if *state.get() == GameState::Loading {
        smoke.loading_frames += 1;
        assert!(
            smoke.loading_frames < MAX_LOADING_FRAMES,
            "smoke test: stuck in Loading"
        );
        return;
    }
    let (target, target_page) = SEQUENCE[smoke.step].clone();
    if *state.get() != target {
        next_state.set(target);
        return;
    }
    if let Some(target_page) = target_page
        && page.is_some_and(|page| *page.get() != target_page)
    {
        next_page.set(target_page);
        return;
    }
    let name = match &target_page {
        Some(page) => format!("{target:?}-{page:?}"),
        None => format!("{target:?}"),
    };
    smoke.frames_in_step += 1;
    if target == GameState::Run && smoke.frames_in_step == HOLD_FORWARD_FRAMES {
        keys.release(KeyCode::KeyW);
    }

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
            for transform in &players {
                info!("smoke test: {name} player at {:.1?}", transform.translation);
            }
            let path = smoke.dir.join(format!("{}-{name}.png", smoke.step));
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

/// Hold W so the run player moves; this exercises input and movement.
fn hold_forward(mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.press(KeyCode::KeyW);
}

fn release_forward(mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.release(KeyCode::KeyW);
}
