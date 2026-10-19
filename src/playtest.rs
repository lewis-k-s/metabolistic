//! # Scripted playtest of the secretions (full build)
//!
//! `METABOLISTIC_PLAYTEST=1` together with a scenario launch
//! (`METABOLISTIC_SCENARIO=vent_chemist`) runs a fixed script on the Run
//! screen and exits:
//!
//! | Time (s) | Step |
//! |---|---|
//! | 1 | screenshot `0-start` |
//! | 1.5 | press Q (the real key path: `ButtonInput`) |
//! | 6 | screenshot `1-q-on` |
//! | 6.5 | press E |
//! | 11 | screenshot `2-q-e-on` |
//! | 11 to 14 | hold W (move up) |
//! | 14 | screenshot `3-moving` |
//! | 14.5 | press Q and E: both off |
//! | 19 | screenshot `4-after` |
//! | 20 | exit |
//!
//! Each second it logs the pools of the cell, the grid total of each secreted
//! channel, the cell's `Clump`, and the NPCs within 300 px (kind, distance,
//! membrane). Screenshots go to `METABOLISTIC_PLAYTEST_DIR` (default
//! `target/playtest`). A panic fails the run.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};

use crate::GameState;
use crate::cell::Cell;
use crate::envgrid::EnvGrid;
use crate::environment::Channel;
use crate::membrane::Membrane;
use crate::metabolism::CurrencyPools;
use crate::molecules::Currency;
use crate::npc::Npc;
use crate::player::{ControlledCell, LocalPlayer};
use crate::secretion::autoinducer::Clump;
use crate::secretion::{Secreting, Secretion, SecretionBar};

/// True if `METABOLISTIC_PLAYTEST` asks for the script.
pub fn requested() -> bool {
    std::env::var_os("METABOLISTIC_PLAYTEST").is_some_and(|v| !v.is_empty() && v != "0")
}

#[derive(Resource)]
struct Script {
    dir: PathBuf,
    /// Seconds on the Run screen.
    time: f32,
    /// Index of the next step.
    next: usize,
    /// Seconds until the next log line.
    log_in: f32,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Shot(&'static str),
    Press(KeyCode),
    Release(KeyCode),
    Exit,
}

const STEPS: [(f32, Step); 16] = [
    (1.0, Step::Shot("0-start")),
    (1.5, Step::Press(KeyCode::KeyQ)),
    (1.6, Step::Release(KeyCode::KeyQ)),
    (6.0, Step::Shot("1-q-on")),
    (6.5, Step::Press(KeyCode::KeyE)),
    (6.6, Step::Release(KeyCode::KeyE)),
    (11.0, Step::Shot("2-q-e-on")),
    (11.0, Step::Press(KeyCode::KeyW)),
    (14.0, Step::Shot("3-moving")),
    (14.0, Step::Release(KeyCode::KeyW)),
    (14.5, Step::Press(KeyCode::KeyQ)),
    (14.5, Step::Press(KeyCode::KeyE)),
    (14.6, Step::Release(KeyCode::KeyQ)),
    (14.6, Step::Release(KeyCode::KeyE)),
    (19.0, Step::Shot("4-after")),
    (20.0, Step::Exit),
];

pub fn plugin(app: &mut App) {
    let dir = std::env::var_os("METABOLISTIC_PLAYTEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/playtest"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("playtest: cannot create {}: {e}", dir.display());
    }
    app.insert_resource(Script { dir, time: 0.0, next: 0, log_in: 0.0 })
        // After the input systems: a key pressed here is `just_pressed` for
        // the Update systems of this frame, like a real key.
        .add_systems(
            PreUpdate,
            run_script
                .after(bevy::input::InputSystems)
                .run_if(in_state(GameState::Run)),
        )
        .add_systems(OnEnter(GameState::RunOver), stop_on_run_over);
}

/// The run ended before the script did (the cell died): log it and exit.
fn stop_on_run_over(mut exit: MessageWriter<AppExit>) {
    warn!("playtest: the run ended before the script");
    exit.write(AppExit::from_code(2));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_script(
    time: Res<Time>,
    mut script: ResMut<Script>,
    grid: Option<Res<EnvGrid>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Transform, &CurrencyPools, &SecretionBar, &Secreting, &Clump), (With<Cell>, Without<Npc>)>,
    npcs: Query<(&Npc, &Transform, &Membrane), With<Cell>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let Some(cell) = players.iter().next().map(|c| c.0) else {
        return;
    };
    let Ok((transform, pools, bar, secreting, clump)) = cells.get(cell) else {
        return;
    };
    script.time += time.delta_secs();
    let position = transform.translation.truncate();

    script.log_in -= time.delta_secs();
    if script.log_in <= 0.0 {
        script.log_in = 1.0;
        let mut clouds = String::new();
        if let Some(grid) = grid.as_deref() {
            for channel in Channel::SECRETED {
                clouds += &format!(
                    " {channel:?} {:.1}{} (here {:.2})",
                    grid.total(channel),
                    if grid.is_dormant(channel) { "z" } else { "" },
                    grid.sample(channel, position),
                );
            }
        }
        info!(
            "playtest t={:.1} pos=({:.0},{:.0}) keys={:?} on={:?} ATP {:.1} AA {:.1} C {:.1} Metals {:.2} clump {:.2} |{clouds}",
            script.time,
            position.x,
            position.y,
            bar.keys,
            Secretion::ALL.into_iter().filter(|s| secreting.is_on(*s)).collect::<Vec<_>>(),
            pools.get(Currency::ATP),
            pools.get(Currency::AminoAcids),
            pools.get(Currency::CarbonSkeletons),
            pools.get(Currency::Metals),
            clump.0,
        );
        let mut near: Vec<String> = npcs
            .iter()
            .filter(|(_, t, _)| t.translation.truncate().distance(position) < 300.0)
            .map(|(npc, t, m)| {
                let at = t.translation.truncate();
                let ai2 = grid.as_deref().map_or(0.0, |g| g.sample(Channel::Autoinducer, at));
                let microcin = grid.as_deref().map_or(0.0, |g| g.sample(Channel::Microcin, at));
                format!(
                    "{:?} d={:.0} mem={:.0} mic={microcin:.2} ai2={ai2:.2}",
                    npc.archetype,
                    at.distance(position),
                    m.integrity
                )
            })
            .collect();
        near.sort();
        info!("playtest near: {}", near.join(", "));
    }

    while let Some(&(at, step)) = STEPS.get(script.next) {
        if script.time < at {
            break;
        }
        script.next += 1;
        match step {
            Step::Shot(name) => {
                let path = script.dir.join(format!("{name}.png"));
                info!("playtest: screenshot {}", path.display());
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Step::Press(key) => {
                info!("playtest: press {key:?}");
                keys.press(key);
            }
            Step::Release(key) => keys.release(key),
            Step::Exit => {
                info!("playtest: done");
                exit.write(AppExit::Success);
            }
        }
    }
}
