//! Smoke test for the full game: run with `METABOLISTIC_SMOKE_TEST=1 cargo run`.
//!
//! The app waits for `Loading` to open the menu by itself, then visits every
//! screen and menu page, holds W on the Run screen, saves a screenshot per
//! step, logs the player cell's position and pools, then exits. A panic or a
//! missing resource fails the run. So do these, on the Run screen: the player
//! dies, or the player does not move while W is held. Screenshots go to
//! `METABOLISTIC_SMOKE_DIR` (default `target/smoke`).
//!
//! M4 steps (`smoke_actions`): on the first Run step the cell gets one flagellum,
//! one adhesin and one toxin, so that the overlays show. On the Manage step the
//! test queues one enzyme. Back on Run, it uses slot 1 and checks that the
//! enzyme order is in the queue and that the slot changed the pathway level.

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use std::path::PathBuf;

use crate::GameState;
use crate::actions::{Level, PathwayLevels, UseSlot};
use crate::blocks::genome::BlockKind;
use crate::cell::Cell;
use crate::environment::{Channel, LocalEnvironment};
use crate::genome_damage::GenomeIntegrity;
use crate::membrane::Membrane;
use crate::metabolism::CurrencyPools;
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome, QueueProtein, SynthesisQueue};
use crate::screens::menu::MenuPage;

/// Frames to stay in each state before the screenshot. The first frames compile
/// shaders, so the first state gets more.
const FRAMES_PER_STATE: u32 = 120;
const FIRST_STATE_EXTRA_FRAMES: u32 = 120;
/// Frames to wait after a screenshot request, so it is written before we move on.
const SCREENSHOT_SETTLE_FRAMES: u32 = 10;

/// Frames that `Loading` may take before it counts as stuck.
const MAX_LOADING_FRAMES: u32 = 600;

/// Each step: a screen, and for the menu, the page.
const SEQUENCE: [(GameState, Option<MenuPage>); 7] = [
    (GameState::Menu, Some(MenuPage::Main)),
    (GameState::Menu, Some(MenuPage::Settings)),
    (GameState::Run, None),
    (GameState::Manage, None),
    (GameState::Run, None),
    (GameState::RunOver, None),
    (GameState::Menu, Some(MenuPage::Main)),
];

/// Steps of the M4 checks.
const STEP_RUN: usize = 2;
const STEP_MANAGE: usize = 3;
const STEP_RUN_AGAIN: usize = 4;

/// The player must move at least this far (pixels) while W is held on the
/// Run screen.
const MIN_RUN_DISTANCE: f32 = 40.0;

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
        run_start: None,
    })
    .add_systems(Update, (drive_smoke_test, smoke_actions))
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
    /// Where the player cell was when the Run screen settled.
    run_start: Option<Vec2>,
}

#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn drive_smoke_test(
    mut smoke: ResMut<SmokeTest>,
    state: Res<State<GameState>>,
    page: Option<Res<State<MenuPage>>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut next_page: ResMut<NextState<MenuPage>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<
        (&Transform, &CurrencyPools, &Membrane, &LocalEnvironment, &GenomeIntegrity),
        With<Cell>,
    >,
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
    // The sequence moves to `RunOver` by itself. Reaching it earlier means the
    // player died.
    let run_over_step = SEQUENCE
        .iter()
        .position(|(state, _)| *state == GameState::RunOver)
        .expect("the sequence visits RunOver");
    assert!(
        *state.get() != GameState::RunOver || smoke.step >= run_over_step,
        "smoke test: the player died (step {}, {:?})",
        smoke.step,
        target
    );
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
    // The start of the Run step: the world has just spawned. Skip the first
    // frames, so that physics has the cell in place.
    if target == GameState::Run && smoke.run_start.is_none() && smoke.frames_in_step >= 5 {
        smoke.run_start = players
            .iter()
            .find_map(|controlled| cells.get(controlled.0).ok())
            .map(|(transform, ..)| transform.translation.truncate());
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
            for (transform, pools, membrane, env, genome) in players
                .iter()
                .filter_map(|controlled| cells.get(controlled.0).ok())
            {
                info!(
                    "smoke test: {name} player at {:.1?} ATP {:.1} membrane {:.1} minerals {:.1} heat {:.1} light {:.1} genome {:.2}",
                    transform.translation.truncate(),
                    pools.get(Currency::ATP),
                    membrane.integrity,
                    pools.get(Currency::Minerals),
                    env.get(Channel::Heat),
                    env.get(Channel::Light),
                    genome.0,
                );
                if target == GameState::Run {
                    let start = smoke.run_start.expect("smoke test: no player cell on the Run screen");
                    let moved = start.distance(transform.translation.truncate());
                    info!("smoke test: the player moved {moved:.1} px while W was held");
                    assert!(
                        moved >= MIN_RUN_DISTANCE,
                        "smoke test: the player moved only {moved:.1} px with W held"
                    );
                }
            }
            if target == GameState::Run {
                assert!(
                    players
                        .iter()
                        .any(|controlled| cells.get(controlled.0).is_ok()),
                    "smoke test: the Run screen has no player cell"
                );
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

/// The M4 checks: overlays, the editor queue, and a slot use.
#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn smoke_actions(
    smoke: Res<SmokeTest>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&mut Proteome, &SynthesisQueue, &PathwayLevels, &CurrencyPools), With<Cell>>,
    mut queue: MessageWriter<QueueProtein>,
    mut slots: MessageWriter<UseSlot>,
    mut done: Local<[bool; 3]>,
) {
    let target = SEQUENCE.get(smoke.step).map(|(state, _)| state.clone());
    if target.as_ref() != Some(state.get()) {
        return;
    }
    let Some(cell) = players.iter().next().map(|c| c.0) else {
        return;
    };
    let Ok((mut proteome, order_queue, levels, pools)) = cells.get_mut(cell) else {
        return;
    };
    match smoke.step {
        STEP_RUN if !done[0] && smoke.frames_in_step >= 5 => {
            for protein in [
                MembraneProtein::Flagellum,
                MembraneProtein::Adhesin,
                MembraneProtein::Toxin,
            ] {
                proteome.add(ProteinTarget::Membrane(protein));
            }
            done[0] = true;
            info!("smoke test: gave the cell a flagellum, an adhesin and a toxin");
        }
        STEP_MANAGE if !done[1] && smoke.frames_in_step >= 10 => {
            queue.write(QueueProtein {
                cell,
                target: ProteinTarget::Enzyme(BlockKind::Chemosynthesis),
            });
            done[1] = true;
            info!("smoke test: queued one Chemosynthesis enzyme in the editor");
        }
        STEP_RUN_AGAIN if !done[2] && smoke.frames_in_step >= 20 => {
            assert!(
                !order_queue.is_empty() || proteome.enzymes(BlockKind::Chemosynthesis) > 2,
                "smoke test: the queued enzyme is neither in the queue nor built"
            );
            let atp = pools.get(Currency::ATP);
            slots.write(UseSlot { cell, slot: 0 });
            done[2] = true;
            info!("smoke test: used slot 1 at ATP {atp:.1}");
        }
        STEP_RUN_AGAIN if done[2] && smoke.frames_in_step == 40 => {
            assert_eq!(
                levels.level(BlockKind::Chemosynthesis),
                Level::Off,
                "smoke test: slot 1 did not switch the pathway off"
            );
        }
        _ => {}
    }
}
