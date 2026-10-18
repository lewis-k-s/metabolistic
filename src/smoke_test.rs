//! Smoke test for the full game: run with `METABOLISTIC_SMOKE_TEST=1 cargo run`.
//!
//! The app waits for `Loading` to open the menu by itself, then visits every
//! screen and menu page, holds W on the Run screen, saves a screenshot per
//! step, logs the player cell's position and pools, then exits. A panic or a
//! missing resource fails the run. So do these, on the Run screen: the player
//! dies, or the player does not move while W is held. Screenshots go to
//! `METABOLISTIC_SMOKE_DIR` (default `target/smoke`).
//!
//! M8 steps: after the second StageComplete the sequence enters land (the smoke
//! test sets the run state as a won shallow stage would: a carry with
//! Respiration and its free enzyme). Three Run steps follow: the start with W
//! held (the drag on dry ground slows the cell), a dry stretch of the plain with
//! hydration 0.2 (the HUD warning), and the waxy gate chamber. The test checks
//! that the stage is land, that the cell has respiration, and that the world has
//! creatures and a gate cell.
//!
//! M7 steps: after StageComplete the sequence enters the shallow-water stage (the
//! smoke test sets the run state as a won vent stage would: the next stage, and a
//! carry with the gate gene and its free enzyme). Three Run steps follow: the
//! start with W held, the catalase prey with a high ROS level (for the tint), and
//! the gate chamber on the sea floor. The test checks that the stage is shallow
//! water, that the cell photosynthesises, and that the world has creatures and a
//! gate cell.
//!
//! M6 steps: the sequence also visits Prepare and StageComplete. The codex is
//! saved in `<smoke dir>/save` (or `METABOLISTIC_SAVE_DIR`), never in the user
//! data directory.
//!
//! M5 steps: the player cell is `Invulnerable` in smoke mode (a predator could
//! engulf it, and the run must not end by chance). On the first Run step the
//! test checks that NPCs are in the world. A new Run step moves the player into
//! the gate chamber with W released, so the screenshot shows the walls, the pore
//! and the gate cell; the test checks that the level was not won by chance.
//!
//! M4 steps (`smoke_actions`): on the first Run step the cell gets one flagellum,
//! one adhesin and one toxin, so that the overlays show. On the Manage step the
//! test queues one enzyme. Back on Run, it uses slot 1 and checks that the
//! enzyme order is in the queue and that the slot changed the pathway level.

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use std::path::PathBuf;

use avian2d::prelude::{LinearVelocity, Position};

use crate::GameState;
use crate::engulf::Invulnerable;
use crate::gate::GateProgress;
use crate::npc::{Archetype, Npc};
use crate::run::RunSummary;
use crate::actions::{Level, PathwayLevels};
use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellSize, membrane_slots};
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
const SEQUENCE: [(GameState, Option<MenuPage>); 23] = [
    (GameState::Menu, Some(MenuPage::Main)),
    (GameState::Menu, Some(MenuPage::Settings)),
    (GameState::Prepare, None),
    (GameState::Run, None),
    (GameState::Manage, None),
    (GameState::Manage, None),
    (GameState::Manage, None),
    (GameState::Manage, None),
    (GameState::Manage, None),
    (GameState::Manage, None),
    (GameState::Run, None),
    (GameState::Run, None),
    (GameState::StageComplete, None),
    (GameState::Run, None),
    (GameState::Run, None),
    (GameState::Run, None),
    (GameState::StageComplete, None),
    (GameState::Run, None),
    (GameState::Run, None),
    (GameState::Run, None),
    (GameState::RunOver, None),
    (GameState::Menu, Some(MenuPage::Main)),
    (GameState::Menu, Some(MenuPage::Main)),
];

/// Steps of the M4 checks.
const STEP_RUN: usize = 3;
const STEP_MANAGE: usize = 4;
/// The Manage checks: the side panel on a pathway, on a socket, a tooltip, and
/// a limited pathway.
const STEP_MANAGE_PANEL: usize = 5;
const STEP_MANAGE_SOCKET: usize = 6;
const STEP_MANAGE_TIP: usize = 7;
const STEP_MANAGE_LIMIT: usize = 8;
/// The Codex overlay on the Manage screen (C), and in the menu (last step).
const STEP_MANAGE_CODEX: usize = 9;
const STEP_MENU_CODEX: usize = 22;
const STEP_RUN_AGAIN: usize = 10;
/// The gate chamber view.
const STEP_GATE_VIEW: usize = 11;
/// The stage-complete screen. The run state moves to shallow water here.
const STEP_STAGE_COMPLETE: usize = 12;
/// The shallow-water steps (M7).
const STEP_SHALLOW: usize = 13;
const STEP_SHALLOW_CATALASE: usize = 14;
const STEP_SHALLOW_GATE: usize = 15;
/// The second stage-complete screen. The run state moves to land here.
const STEP_STAGE_COMPLETE_2: usize = 16;
/// The land steps (M8).
const STEP_LAND: usize = 17;
const STEP_LAND_DRY: usize = 18;
const STEP_LAND_GATE: usize = 19;

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
    // The smoke test never touches the real codex: it saves in its own
    // directory, unless METABOLISTIC_SAVE_DIR says otherwise. The codex has
    // three genes, so that the Prepare screen has something to show.
    let save_dir = if std::env::var_os(crate::codex::SAVE_DIR_VAR).is_some() {
        crate::codex::save_dir().expect("a save directory")
    } else {
        dir.join("save")
    };
    let mut codex = crate::codex::load(&crate::codex::codex_path(&save_dir));
    codex.bank(&[BlockKind::Fermentation, BlockKind::FatStorage, BlockKind::LightCapture]);
    app.insert_resource(codex).insert_resource(crate::codex::CodexStore {
        path: Some(crate::codex::codex_path(&save_dir)),
    });
    app.insert_resource(SmokeTest {
        dir,
        step: 0,
        frames_in_step: 0,
        loading_frames: 0,
        screenshot_requested_at: None,
        run_start: None,
    })
    .add_systems(Update, (drive_smoke_test, smoke_actions, smoke_shallow, smoke_land, smoke_advance_stage))
    // After the UI focus system, so that a pressed button stays pressed for the
    // screen's own systems of this frame.
    .add_systems(PreUpdate, (smoke_manage, smoke_codex).after(bevy::ui::UiSystems::Focus))
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
    lighting: Res<crate::world::lighting_view::LightingOn>,
    darkness: Query<(), With<crate::world::lighting_view::DarknessQuad>>,
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
                // The lighting is on during the Run steps, so the screenshot shows it.
                assert!(lighting.0, "smoke test: the lighting is off on the Run screen");
                assert!(darkness.iter().count() == 2 || smoke.step >= STEP_LAND, "smoke test: the darkness overlay is missing");
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
            // A new world follows the stage-complete screen: measure the move from
            // its start.
            if smoke.step == STEP_SHALLOW || smoke.step == STEP_LAND {
                smoke.run_start = None;
            }
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

/// The M4 checks: overlays, the editor queue, and the pathway level set on the
/// map (see `smoke_manage`).
#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn smoke_actions(
    smoke: Res<SmokeTest>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&mut Proteome, &SynthesisQueue, &PathwayLevels, &CurrencyPools), With<Cell>>,
    mut queue: MessageWriter<QueueProtein>,
    npcs: Query<&Npc>,
    gates: Query<&GateProgress>,
    summary: Res<RunSummary>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut done: Local<[bool; 6]>,
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
            commands.entity(cell).insert(Invulnerable);
            info!("smoke test: gave the cell a flagellum, an adhesin and a toxin; it is invulnerable");
        }
        STEP_RUN if !done[3] && smoke.frames_in_step >= 10 => {
            let mut count = std::collections::BTreeMap::new();
            for npc in &npcs {
                *count.entry(format!("{:?}", npc.archetype)).or_insert(0) += 1;
            }
            info!("smoke test: NPCs in the world: {count:?}");
            assert!(
                npcs.iter().filter(|n| n.archetype != Archetype::Gate).count() >= 8,
                "smoke test: the world has too few NPCs"
            );
            assert!(gates.iter().count() == 1, "smoke test: the world has no gate cell");
            done[3] = true;
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
            done[2] = true;
            info!("smoke test: back in the run at ATP {:.1}", pools.get(Currency::ATP));
        }
        STEP_RUN_AGAIN if done[2] && smoke.frames_in_step == 40 => {
            assert_eq!(
                levels.level(BlockKind::Chemosynthesis),
                Level::Low,
                "smoke test: the level set on the map did not stay in the run"
            );
        }
        STEP_GATE_VIEW if !done[4] && smoke.frames_in_step >= 2 => {
            // Into the chamber, away from the gate cell, with W released.
            let position = Vec2::new(880.0, 15.0);
            commands.entity(cell).insert((
                Transform::from_translation(position.extend(0.0)),
                Position(position),
                LinearVelocity(Vec2::ZERO),
            ));
            done[4] = true;
            info!("smoke test: moved the player into the gate chamber");
        }
        STEP_GATE_VIEW if done[4] => {
            keys.release(KeyCode::KeyW);
            if smoke.frames_in_step == 100 {
                let mut count = std::collections::BTreeMap::new();
                for npc in &npcs {
                    *count.entry(format!("{:?}", npc.archetype)).or_insert(0) += 1;
                }
                info!("smoke test: NPCs at the end of the run: {count:?}");
                assert!(summary.victory.is_none(), "smoke test: the level was won by chance");
                assert!(
                    gates.iter().all(|g| g.held == 0.0),
                    "smoke test: the player touched the gate cell"
                );
                done[5] = true;
            }
        }
        _ => {}
    }
}

/// On a stage-complete screen, the run moves on as it would after a win: the
/// next stage, and a carry with the gate gene and its free enzyme.
fn smoke_advance_stage(
    smoke: Res<SmokeTest>,
    state: Res<State<GameState>>,
    mut run: ResMut<crate::run::RunState>,
    mut done: Local<[bool; 2]>,
) {
    if *state.get() != GameState::StageComplete {
        return;
    }
    let (index, gene) = match smoke.step {
        STEP_STAGE_COMPLETE => (0, BlockKind::LightCapture),
        STEP_STAGE_COMPLETE_2 => (1, BlockKind::Respiration),
        _ => return,
    };
    if done[index] {
        return;
    }
    let mut carry = crate::run::starter_carry(&run.loadout);
    // The cell that won the stage before: photosynthesis, and on land also
    // catalase (the shallow key gene) and respiration (the shallow gate gene).
    let mut genes = vec![BlockKind::LightCapture];
    if gene == BlockKind::Respiration {
        genes.extend([BlockKind::Catalase, BlockKind::Respiration]);
    }
    for kind in genes {
        carry.genome.table.insert(kind, crate::blocks::genome::GeneState::Expressed);
        crate::run::grant_gate_enzyme(&mut carry, kind);
    }
    let outcome = run.complete_stage(carry, crate::stage::Stage::is_playable);
    assert_eq!(outcome, crate::run::StageOutcome::NextStage, "smoke test: no next stage");
    done[index] = true;
    info!("smoke test: the run moved on to {:?}", run.stage());
}

/// The M7 checks and views in the shallow-water stage.
#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn smoke_shallow(
    smoke: Res<SmokeTest>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&Proteome, &mut CurrencyPools, &crate::cell::CellBlocks), With<Cell>>,
    npcs: Query<&Npc>,
    gates: Query<&GateProgress>,
    run: Res<crate::run::RunState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut done: Local<[bool; 4]>,
) {
    if *state.get() != GameState::Run
        || !(STEP_SHALLOW..=STEP_SHALLOW_GATE).contains(&smoke.step)
    {
        return;
    }
    let Some(cell) = players.iter().next().map(|c| c.0) else {
        return;
    };
    let Ok((proteome, mut pools, blocks)) = cells.get_mut(cell) else {
        return;
    };
    let teleport = |commands: &mut Commands, position: Vec2| {
        commands.entity(cell).insert((
            Transform::from_translation(position.extend(0.0)),
            Position(position),
            LinearVelocity(Vec2::ZERO),
        ));
    };
    match smoke.step {
        STEP_SHALLOW if !done[0] && smoke.frames_in_step >= 5 => {
            assert_eq!(run.stage(), crate::stage::Stage::ShallowWater, "smoke test: not in shallow water");
            assert!(blocks.contains(BlockKind::LightCapture), "smoke test: no photosynthesis block");
            assert!(proteome.enzymes(BlockKind::LightCapture) >= 1, "smoke test: no free enzyme for the gate gene");
            commands.entity(cell).insert(Invulnerable);
            let mut count = std::collections::BTreeMap::new();
            for npc in &npcs {
                *count.entry(format!("{:?}", npc.archetype)).or_insert(0) += 1;
            }
            info!("smoke test: shallow water, NPCs: {count:?}");
            assert!(
                npcs.iter().filter(|n| n.archetype != Archetype::Gate).count() >= 8,
                "smoke test: the shallow world has too few NPCs"
            );
            assert!(gates.iter().count() == 1, "smoke test: the shallow world has no gate cell");
            done[0] = true;
        }
        STEP_SHALLOW if done[0] && smoke.frames_in_step == 100 => {
            info!(
                "smoke test: shallow start, O2 {:.1} ROS {:.1} ATP {:.1}",
                pools.get(Currency::O2),
                pools.get(Currency::ROS),
                pools.get(Currency::ATP),
            );
        }
        STEP_SHALLOW_CATALASE if !done[1] && smoke.frames_in_step >= 2 => {
            // Next to the catalase prey, with a lot of ROS: the tint shows.
            teleport(&mut commands, Vec2::new(-380.0, 80.0));
            pools.set(Currency::ROS, 18.0);
            pools.set(Currency::O2, 8.0);
            done[1] = true;
            info!("smoke test: moved the player next to the catalase prey, ROS 18");
        }
        STEP_SHALLOW_CATALASE if done[1] => keys.release(KeyCode::KeyW),
        STEP_SHALLOW_GATE if !done[2] && smoke.frames_in_step >= 2 => {
            // Below the chamber on the sea floor, W released.
            teleport(&mut commands, Vec2::new(800.0, -640.0));
            done[2] = true;
            info!("smoke test: moved the player below the shallow gate chamber");
        }
        STEP_SHALLOW_GATE if done[2] => {
            keys.release(KeyCode::KeyW);
            if smoke.frames_in_step == 100 && !done[3] {
                assert!(gates.iter().all(|g| g.held == 0.0), "smoke test: the player touched the gate cell");
                done[3] = true;
            }
        }
        _ => {}
    }
}

/// The M8 checks and views on land.
#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn smoke_land(
    smoke: Res<SmokeTest>,
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<
        (&Proteome, &mut CurrencyPools, &crate::cell::CellBlocks, &mut crate::desiccation::Hydration, &LocalEnvironment),
        With<Cell>,
    >,
    npcs: Query<&Npc>,
    gates: Query<&GateProgress>,
    run: Res<crate::run::RunState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut done: Local<[bool; 5]>,
) {
    if *state.get() != GameState::Run || !(STEP_LAND..=STEP_LAND_GATE).contains(&smoke.step) {
        return;
    }
    let Some(cell) = players.iter().next().map(|c| c.0) else {
        return;
    };
    let Ok((proteome, pools, blocks, mut hydration, env)) = cells.get_mut(cell) else {
        return;
    };
    let teleport = |commands: &mut Commands, position: Vec2| {
        commands.entity(cell).insert((
            Transform::from_translation(position.extend(0.0)),
            Position(position),
            LinearVelocity(Vec2::ZERO),
        ));
    };
    match smoke.step {
        STEP_LAND if !done[0] && smoke.frames_in_step >= 5 => {
            assert_eq!(run.stage(), crate::stage::Stage::Land, "smoke test: not on land");
            assert!(blocks.contains(BlockKind::Respiration), "smoke test: no respiration block");
            assert!(proteome.enzymes(BlockKind::Respiration) >= 1, "smoke test: no free enzyme for respiration");
            commands.entity(cell).insert(Invulnerable);
            let mut count = std::collections::BTreeMap::new();
            for npc in &npcs {
                *count.entry(format!("{:?}", npc.archetype)).or_insert(0) += 1;
            }
            info!("smoke test: land, NPCs: {count:?}");
            assert!(
                npcs.iter().filter(|n| n.archetype != Archetype::Gate).count() >= 8,
                "smoke test: the land world has too few NPCs"
            );
            assert!(gates.iter().count() == 1, "smoke test: the land world has no gate cell");
            done[0] = true;
        }
        STEP_LAND if done[0] && smoke.frames_in_step == 100 => {
            info!(
                "smoke test: land start, O2 {:.1} ROS {:.1} ATP {:.1} hydration {:.2} water {:.1}",
                pools.get(Currency::O2),
                pools.get(Currency::ROS),
                pools.get(Currency::ATP),
                hydration.0,
                env.get(Channel::Water),
            );
        }
        STEP_LAND_DRY if !done[1] && smoke.frames_in_step >= 2 => {
            // On the dry plain, with a dry cell: the HUD warns.
            teleport(&mut commands, Vec2::new(-330.0, -190.0));
            hydration.0 = 0.2;
            done[1] = true;
            info!("smoke test: moved the player to the dry plain, hydration 0.2");
        }
        STEP_LAND_DRY if done[1] => keys.release(KeyCode::KeyW),
        STEP_LAND_GATE if !done[2] && smoke.frames_in_step >= 2 => {
            // Below the waxy chamber, W released.
            teleport(&mut commands, Vec2::new(800.0, -110.0));
            done[2] = true;
            info!("smoke test: moved the player below the land gate chamber");
        }
        STEP_LAND_GATE if done[2] => {
            keys.release(KeyCode::KeyW);
            if smoke.frames_in_step == 100 && !done[3] {
                assert!(gates.iter().all(|g| g.held == 0.0), "smoke test: the player touched the gate cell");
                done[3] = true;
            }
        }
        _ => {}
    }
}

/// What the Manage steps remember between frames.
#[derive(Default)]
struct ManageSmoke {
    orders_before: usize,
    /// The empty socket that the socket check picked. `None`: every socket
    /// is in use (a scenario cell can fill them), so the check is skipped.
    empty_slot: Option<usize>,
    /// What the codex check queues: a toxin, or an enzyme when every socket
    /// is in use.
    codex_target: Option<ProteinTarget>,
}

/// The Codex checks. The overlay opens with the key C on the Manage screen,
/// shows one card for every gene and membrane protein, and a button press
/// queues a protein (a membrane protein, or an enzyme when every socket is in
/// use). The last step opens it from the menu.
fn smoke_codex(world: &mut World, mut memory: Local<ManageSmoke>) {
    use crate::codex_catalog::{Edit, EntryId};
    use crate::screens::codex::{smoke_open, smoke_press_edit, smoke_probe, smoke_show};
    let (step, frame) = {
        let smoke = world.resource::<SmokeTest>();
        (smoke.step, smoke.frames_in_step)
    };
    let state = world.resource::<State<GameState>>().get().clone();
    let all = BlockKind::ALL.len() + MembraneProtein::ALL.len();
    match (step, frame, state) {
        (STEP_MANAGE_CODEX, 5, GameState::Manage) => {
            // Press C, as the player does.
            world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyC);
        }
        (STEP_MANAGE_CODEX, 6, GameState::Manage) => {
            world.resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyC);
        }
        (STEP_MANAGE_CODEX, 20, GameState::Manage) => {
            let probe = smoke_probe(world);
            assert!(probe.open, "smoke test: C did not open the codex");
            assert_eq!(probe.cards, all, "smoke test: the codex has a card for every entry");
            if let Some(cell) = world.query_filtered::<&ControlledCell, With<LocalPlayer>>().iter(world).next().map(|c| c.0) {
                // A full membrane refuses a membrane protein, so queue an enzyme then.
                let (entry, target) = match first_free_socket(world, cell) {
                    Some(_) => (
                        EntryId::Protein(MembraneProtein::Toxin),
                        ProteinTarget::Membrane(MembraneProtein::Toxin),
                    ),
                    None => (
                        EntryId::Gene(BlockKind::Chemosynthesis),
                        ProteinTarget::Enzyme(BlockKind::Chemosynthesis),
                    ),
                };
                smoke_show(world, entry);
                memory.codex_target = Some(target);
                memory.orders_before = ordered(world, cell, target);
            }
        }
        (STEP_MANAGE_CODEX, 30, GameState::Manage) => {
            let target = memory.codex_target.expect("smoke test: no codex target");
            assert!(
                smoke_probe(world).edit_buttons.contains(&Edit::Queue(target)),
                "smoke test: the codex detail panel has no queue button for {target:?}"
            );
            smoke_press_edit(world, Edit::Queue(target));
        }
        (STEP_MANAGE_CODEX, 50, GameState::Manage) => {
            let cell = world
                .query_filtered::<&ControlledCell, With<LocalPlayer>>()
                .iter(world)
                .next()
                .map(|c| c.0)
                .expect("smoke test: a player cell");
            // Count the target in the queue and in the proteome: an order can
            // finish between the two checks, so the queue length alone can go down.
            let target = memory.codex_target.expect("smoke test: no codex target");
            assert!(
                ordered(world, cell, target) > memory.orders_before,
                "smoke test: the codex button queued no {target:?}"
            );
            info!("smoke test: the codex opened with C and queued a protein");
            smoke_show(world, EntryId::Gene(BlockKind::Respiration));
        }
        (STEP_MENU_CODEX, 5, GameState::Menu) => smoke_open(world, true),
        (STEP_MENU_CODEX, 20, GameState::Menu) => {
            let probe = smoke_probe(world);
            assert!(probe.open && probe.cards == all, "smoke test: the menu codex shows {} cards", probe.cards);
            smoke_show(world, EntryId::Gene(BlockKind::Fermentation));
            info!("smoke test: the codex opened from the menu with {} cards", probe.cards);
        }
        _ => {}
    }
}

/// Proteins of a target that a cell has ordered: in the synthesis queue or made.
fn ordered(world: &World, cell: Entity, target: ProteinTarget) -> usize {
    let queued = world
        .get::<SynthesisQueue>(cell)
        .map_or(0, |q| q.orders.iter().filter(|o| **o == target).count());
    let made = world.get::<Proteome>(cell).map_or(0, |p| p.count(target) as usize);
    queued + made
}

/// The first empty membrane socket of a cell. Sockets hold the proteome, then
/// the queued membrane proteins, then empty ones (`slot_contents`). `None`: all
/// sockets are in use or ordered.
fn first_free_socket(world: &World, cell: Entity) -> Option<usize> {
    let used = world.get::<Proteome>(cell).map_or(0, |p| {
        MembraneProtein::ALL
            .iter()
            .map(|m| p.count(ProteinTarget::Membrane(*m)) as usize)
            .sum::<usize>()
    });
    let queued = world.get::<SynthesisQueue>(cell).map_or(0, |q| {
        q.orders.iter().filter(|o| matches!(o, ProteinTarget::Membrane(_))).count()
    });
    let slots = world.get::<CellSize>(cell).map_or(0, |s| membrane_slots(s) as usize);
    (used + queued < slots).then_some(used + queued)
}

/// The Manage checks. The picks are what a click and a hover send (the smoke
/// test has no pointer), and the + press is a real button press, so the
/// panel, the buttons and the messages are all exercised.
fn smoke_manage(world: &mut World, mut memory: Local<ManageSmoke>) {
    use crate::screens::manage::{
        SmokePick, smoke_hover, smoke_press_level, smoke_probe, smoke_press_plus, smoke_select,
    };
    if *world.resource::<State<GameState>>().get() != GameState::Manage {
        return;
    }
    let (step, frame) = {
        let smoke = world.resource::<SmokeTest>();
        (smoke.step, smoke.frames_in_step)
    };
    if !(STEP_MANAGE_PANEL..=STEP_MANAGE_LIMIT).contains(&step) {
        return;
    }
    let Some(cell) = world
        .query_filtered::<&ControlledCell, With<LocalPlayer>>()
        .iter(world)
        .next()
        .map(|c| c.0)
    else {
        return;
    };
    let orders = |world: &World| world.get::<SynthesisQueue>(cell).map_or(0, |q| q.orders.len());
    match (step, frame) {
        (STEP_MANAGE_PANEL, 5) => {
            smoke_select(world, Some(SmokePick::Pathway(BlockKind::Chemosynthesis)));
            memory.orders_before = orders(world);
        }
        (STEP_MANAGE_PANEL, 15) => {
            let target = ProteinTarget::Enzyme(BlockKind::Chemosynthesis);
            assert!(
                smoke_probe(world).plus_buttons.contains(&target),
                "smoke test: the panel of a pathway has no + button"
            );
            smoke_press_plus(world, target);
        }
        (STEP_MANAGE_PANEL, 20) => {
            // Low, not Off: the limit check below needs the pathway to run.
            assert!(
                smoke_press_level(world, BlockKind::Chemosynthesis, Level::Low),
                "smoke test: the panel of a pathway has no level buttons"
            );
        }
        (STEP_MANAGE_PANEL, 40) => {
            let probe = smoke_probe(world);
            assert_eq!(probe.panel_title.as_deref(), Some("Chemosynthesis"), "smoke test: the panel did not open");
            assert!(orders(world) > memory.orders_before, "smoke test: + queued no enzyme");
            assert_eq!(
                world.get::<PathwayLevels>(cell).map(|l| l.level(BlockKind::Chemosynthesis)),
                Some(Level::Low),
                "smoke test: the Low button did not set the level"
            );
            info!("smoke test: the panel of a pathway opened, + queued an enzyme, Low set the level");
        }
        (STEP_MANAGE_SOCKET, 5) => {
            // A scenario cell can start with proteins, so pick the first empty
            // socket, not socket 0.
            memory.empty_slot = first_free_socket(world, cell);
            match memory.empty_slot {
                Some(slot) => smoke_select(world, Some(SmokePick::Slot(slot))),
                None => info!("smoke test: every socket is in use, the socket check is skipped"),
            }
            memory.orders_before = orders(world);
        }
        (STEP_MANAGE_SOCKET, 15 | 40) if memory.empty_slot.is_none() => {}
        (STEP_MANAGE_SOCKET, 15) => {
            let target = ProteinTarget::Membrane(MembraneProtein::Toxin);
            assert!(
                smoke_probe(world).plus_buttons.contains(&target),
                "smoke test: the panel of a socket has no + button"
            );
            smoke_press_plus(world, target);
        }
        (STEP_MANAGE_SOCKET, 40) => {
            let probe = smoke_probe(world);
            assert!(
                probe.panel_title.as_deref().is_some_and(|t| t.starts_with("Membrane socket")),
                "smoke test: the panel of a socket did not open: {:?}",
                probe.panel_title
            );
            assert!(orders(world) > memory.orders_before, "smoke test: + queued no membrane protein");
            info!("smoke test: the panel of a socket opened and + queued a protein");
        }
        (STEP_MANAGE_TIP, 5) => {
            smoke_select(world, Some(SmokePick::Pathway(BlockKind::Fermentation)));
            smoke_hover(world, Some(SmokePick::Pathway(BlockKind::Chemosynthesis)));
        }
        (STEP_MANAGE_TIP, 40) => {
            let probe = smoke_probe(world);
            assert!(probe.tooltip_visible, "smoke test: no tooltip on hover");
            assert!(
                probe.tooltip_text.contains("Chemosynthesis") || probe.tooltip_text.contains("Chemo"),
                "smoke test: the tooltip says {:?}",
                probe.tooltip_text
            );
            info!("smoke test: tooltip: {:?}", probe.tooltip_text);
        }
        (STEP_MANAGE_LIMIT, 2) => {
            smoke_hover(world, None);
            smoke_select(world, Some(SmokePick::Pathway(BlockKind::Chemosynthesis)));
        }
        (STEP_MANAGE_LIMIT, 3..=60) => {
            // No minerals: Chemosynthesis is starved.
            if let Some(mut pools) = world.get_mut::<CurrencyPools>(cell) {
                pools.set(Currency::Minerals, 0.0);
            }
            if frame == 60 {
                let probe = smoke_probe(world);
                info!("smoke test: badges {:?}, red pipes {}", probe.badges, probe.limiting_pipes);
                assert!(
                    probe.badges.contains(&BlockKind::Chemosynthesis),
                    "smoke test: no limit badge on a starved pathway"
                );
                assert!(probe.limiting_pipes >= 1, "smoke test: no red pulse on a starved pathway");
            }
        }
        _ => {}
    }
}
