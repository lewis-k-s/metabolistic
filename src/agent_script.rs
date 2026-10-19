//! # Agent scripts (full build): drive a run from a JSON file
//!
//! `METABOLISTIC_SCRIPT=pilus_aim cargo run` runs `assets/scripts/pilus_aim.json`
//! on the Run screen and exits. A bare name is a file in `assets/scripts/`; a
//! name with `/` or `.json` is a path. Use it in offscreen mode (the default for
//! agents, see `offscreen`): with fixed 1/60 s frames, the events and positions
//! of a script with a fixed seed repeat (pools can differ a little).
//!
//! The file:
//!
//! ```json
//! {
//!   "scenario": "vent_predator",
//!   "log_every": 0.5,
//!   "steps": [
//!     { "at": 0.5, "shot": "start" },
//!     { "at": 1.0, "aim": [120, 0] },
//!     { "at": 1.2, "press": "Left" },
//!     { "at": 1.3, "release": "Left" },
//!     { "at": 2.0, "expect": { "pilus": "attached" } },
//!     { "at": 3.0, "exit": 0 }
//!   ]
//! }
//! ```
//!
//! - `scenario` (optional): the scenario to start, as `METABOLISTIC_SCENARIO`.
//!   `METABOLISTIC_SCENARIO` wins if both are set.
//! - `log_every` (optional, s): a state line (position, ATP, aim, cursor,
//!   pilus) at this interval. Default 1.
//! - `at`: seconds of game time on the Run screen.
//! - `shot`: a screenshot `<name>.png` in `METABOLISTIC_SCRIPT_DIR` (default
//!   `target/script/<script name>`).
//! - `aim`: points the cursor at this offset (world pixels, y up) from the
//!   player cell. `aim_screen`: the cursor at this window position (logical
//!   pixels, y down). `aim_nearest_cell`: the cursor on the nearest other
//!   cell (`true`). `no_cursor`: removes the cursor.
//! - `press` / `release`: a `MouseButton` (`Left`, `Right`, `Back`, `Forward`)
//!   or a `KeyCode` (`KeyW`, `ShiftLeft`, `Digit1`, `Space`, ...) by its Rust
//!   name.
//! - `wheel`: mouse wheel lines (negative = scroll down, reels the pilus in).
//! - `log`: a log line with this text and the state.
//! - `expect`: checks of the player cell. `"pilus"`: `"attached"` or `"in"`.
//!   `"aim"`: `[x, y]`, the aim must point this way (within 0.05). A failed
//!   check logs `agent script: FAIL` and makes the exit code 3.
//! - `exit`: ends the run with this code (3 instead of 0 if a check failed).
//!
//! The run ends with code 2 if the cell dies before the script ends. A bad
//! file stops the game with the parse error.

use std::path::{Path, PathBuf};

use bevy::input::InputSystems;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::prelude::*;
use bevy::reflect::enums::{DynamicEnum, DynamicVariant};
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;
use serde::Deserialize;

use crate::GameState;
use crate::actions::ActionState;
use crate::metabolism::CurrencyPools;
use crate::molecules::Currency;
use crate::offscreen::VirtualCursor;
use crate::pilus::PilusTether;
use crate::player::{ControlledCell, LocalPlayer};
use crate::screens::RunCamera;
use crate::world::shader_cells::Canvas;

const SCRIPT_VAR: &str = "METABOLISTIC_SCRIPT";
const SCRIPT_DIR: &str = "scripts";

/// A script file (see the module doc).
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default)]
    pub scenario: Option<String>,
    #[serde(default = "one_second")]
    pub log_every: f32,
    pub steps: Vec<Step>,
}

fn one_second() -> f32 {
    1.0
}

/// One step: a time and an action.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Step {
    pub at: f32,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Shot(String),
    Aim([f32; 2]),
    AimScreen([f32; 2]),
    AimNearestCell(bool),
    NoCursor(bool),
    Press(String),
    Release(String),
    Wheel(f32),
    Log(String),
    Expect(Expect),
    Exit(i32),
}

/// A check of the player cell.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// `"attached"` or `"in"`.
    #[serde(default)]
    pub pilus: Option<String>,
    /// The direction that the aim must point (any length).
    #[serde(default)]
    pub aim: Option<[f32; 2]>,
}

/// A mouse button or a key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Button {
    Mouse(MouseButton),
    Key(KeyCode),
}

/// The button with this Rust variant name (`Left`, `KeyW`, ...).
pub fn parse_button(name: &str) -> Result<Button, String> {
    let unit = || DynamicEnum::new(name, DynamicVariant::Unit);
    if let Some(button) = MouseButton::from_reflect(&unit()) {
        return Ok(Button::Mouse(button));
    }
    if let Some(key) = KeyCode::from_reflect(&unit()) {
        return Ok(Button::Key(key));
    }
    Err(format!(
        "unknown button or key {name:?} (use a MouseButton or KeyCode name)"
    ))
}

impl Script {
    /// Parses a script and checks its button names and step order.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let script: Script = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut last = f32::NEG_INFINITY;
        for step in &script.steps {
            if step.at < last {
                return Err(format!("step at {} comes after a step at {last}", step.at));
            }
            last = step.at;
            if let Action::Press(name) | Action::Release(name) = &step.action {
                parse_button(name)?;
            }
            if let Action::Expect(Expect { pilus: Some(p), .. }) = &step.action
                && p != "attached"
                && p != "in"
            {
                return Err(format!("expect pilus {p:?}: use \"attached\" or \"in\""));
            }
        }
        Ok(script)
    }
}

/// The result of a check of the player cell.
pub fn check(expect: &Expect, tether: &PilusTether, aim: Vec2) -> Result<(), String> {
    match expect.pilus.as_deref() {
        Some("attached") if tether.0.is_none() => {
            return Err("pilus is in, expected attached".into());
        }
        Some("in") if tether.0.is_some() => return Err("pilus is attached, expected in".into()),
        _ => {}
    }
    if let Some([x, y]) = expect.aim {
        let want = Vec2::new(x, y).normalize_or_zero();
        if aim.distance(want) > 0.05 {
            return Err(format!(
                "aim is ({:.2},{:.2}), expected ({:.2},{:.2})",
                aim.x, aim.y, want.x, want.y
            ));
        }
    }
    Ok(())
}

/// The script name from the environment, if any.
pub fn requested() -> bool {
    std::env::var(SCRIPT_VAR).is_ok_and(|v| !v.is_empty())
}

/// The file of a script name (see `scenario::resolve_path`).
pub fn resolve_path(name: &str, assets_dir: &Path) -> PathBuf {
    if name.contains('/') || name.contains('\\') || name.ends_with(".json") {
        PathBuf::from(name)
    } else {
        assets_dir.join(SCRIPT_DIR).join(format!("{name}.json"))
    }
}

/// The running script.
#[derive(Resource)]
struct Runner {
    script: Script,
    dir: PathBuf,
    time: f32,
    next: usize,
    log_in: f32,
    failures: u32,
}

/// Adds the script runner. Panics with the error if the file is bad.
pub fn plugin(app: &mut App) {
    let name = std::env::var(SCRIPT_VAR).unwrap_or_default();
    let path = resolve_path(&name, &crate::scenario::assets_dir());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read script {}: {e}", path.display()));
    let script = Script::from_json(&text)
        .unwrap_or_else(|e| panic!("cannot parse script {}: {e}", path.display()));

    if let Some(scenario) = &script.scenario
        && !app
            .world()
            .contains_resource::<crate::scenario::ScenarioLaunch>()
    {
        let file = crate::scenario::resolve_path(scenario, &crate::scenario::assets_dir());
        let scenario = crate::scenario::load(&file).unwrap_or_else(|e| panic!("{e}"));
        app.insert_resource(crate::scenario::ScenarioLaunch(scenario));
    }

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = std::env::var_os("METABOLISTIC_SCRIPT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/script").join(&stem));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        error!("agent script: cannot create {}: {e}", dir.display());
    }
    info!(
        "agent script: {} ({} steps), output in {}",
        path.display(),
        script.steps.len(),
        dir.display()
    );
    if !crate::offscreen::requested() {
        warn!("agent script: not in offscreen mode, so a window opens and time is not fixed");
    }

    crate::offscreen::cursor_plugin(app);
    app.insert_resource(Runner {
        log_in: 0.0,
        script,
        dir,
        time: 0.0,
        next: 0,
        failures: 0,
    })
    .add_systems(
        PreUpdate,
        run_script
            .after(InputSystems)
            .run_if(in_state(GameState::Run)),
    )
    .add_systems(OnEnter(GameState::RunOver), stop_on_run_over);
}

fn stop_on_run_over(mut exit: MessageWriter<AppExit>) {
    warn!("agent script: the run ended before the script (the cell died)");
    exit.write(AppExit::from_code(2));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_script(
    time: Res<Time>,
    mut runner: ResMut<Runner>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(&Transform, &CurrencyPools, &ActionState, &PilusTether)>,
    others: Query<(Entity, &Transform), With<crate::cell::Cell>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<RunCamera>>,
    canvas: Res<Canvas>,
    mut cursor: ResMut<VirtualCursor>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: MessageWriter<MouseWheel>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    let Some(cell) = players.iter().next().map(|c| c.0) else {
        return;
    };
    let Ok((transform, pools, action, tether)) = cells.get(cell) else {
        return;
    };
    let position = transform.translation.truncate();
    runner.time += time.delta_secs();
    let state = |cursor: Option<Vec2>| {
        let pilus = match tether.0 {
            Some(t) => format!(
                "attached(anchor=({:.0},{:.0}) len={:.0} target={})",
                t.anchor.x,
                t.anchor.y,
                t.length,
                if t.target.is_some() { "cell" } else { "wall" }
            ),
            None => "in".to_string(),
        };
        format!(
            "pos=({:.0},{:.0}) ATP={:.1} aim=({:.2},{:.2}) cursor={:?} pilus={pilus}",
            position.x,
            position.y,
            pools.get(Currency::ATP),
            action.aim.x,
            action.aim.y,
            cursor.map(|c| (c.x.round(), c.y.round())),
        )
    };

    runner.log_in -= time.delta_secs();
    if runner.log_in <= 0.0 {
        runner.log_in = runner.script.log_every;
        info!("agent script t={:.2} {}", runner.time, state(cursor.0));
    }

    while let Some(step) = runner.script.steps.get(runner.next).cloned() {
        if runner.time < step.at {
            break;
        }
        runner.next += 1;
        let t = runner.time;
        match step.action {
            Action::Shot(name) => {
                let path = runner.dir.join(format!("{name}.png"));
                info!("agent script t={t:.2} shot {}", path.display());
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            Action::Aim([dx, dy]) => {
                let target = position + Vec2::new(dx, dy);
                let point = windows.single().ok().zip(cameras.single().ok()).and_then(
                    |((_, window), camera)| {
                        crate::world::actions_view::world_to_cursor(window, camera, &canvas, target)
                    },
                );
                info!(
                    "agent script t={t:.2} aim at world ({:.0},{:.0}) -> cursor {point:?}",
                    target.x, target.y
                );
                cursor.0 = point;
            }
            Action::AimScreen([x, y]) => cursor.0 = Some(Vec2::new(x, y)),
            Action::AimNearestCell(_) => {
                let nearest = others
                    .iter()
                    .filter(|(e, _)| *e != cell)
                    .map(|(_, t)| t.translation.truncate())
                    .min_by(|a, b| a.distance(position).total_cmp(&b.distance(position)));
                let point = nearest.and_then(|target| {
                    let ((_, window), camera) = windows.single().ok().zip(cameras.single().ok())?;
                    crate::world::actions_view::world_to_cursor(window, camera, &canvas, target)
                });
                info!(
                    "agent script t={t:.2} aim at nearest cell {:?} (offset {:?}) -> cursor {point:?}",
                    nearest.map(|n| (n.x.round(), n.y.round())),
                    nearest.map(|n| ((n.x - position.x).round(), (n.y - position.y).round())),
                );
                cursor.0 = point;
            }
            Action::NoCursor(_) => cursor.0 = None,
            Action::Press(name) => {
                info!("agent script t={t:.2} press {name}");
                match parse_button(&name) {
                    Ok(Button::Mouse(b)) => mouse.press(b),
                    Ok(Button::Key(k)) => keys.press(k),
                    Err(e) => error!("agent script: {e}"),
                }
            }
            Action::Release(name) => match parse_button(&name) {
                Ok(Button::Mouse(b)) => mouse.release(b),
                Ok(Button::Key(k)) => keys.release(k),
                Err(e) => error!("agent script: {e}"),
            },
            Action::Wheel(lines) => {
                if let Ok((window, _)) = windows.single() {
                    wheel.write(MouseWheel {
                        unit: MouseScrollUnit::Line,
                        x: 0.0,
                        y: lines,
                        window,
                        phase: TouchPhase::Moved,
                    });
                }
            }
            Action::Log(text) => info!("agent script t={t:.2} {text}: {}", state(cursor.0)),
            Action::Expect(expect) => match check(&expect, tether, action.aim) {
                Ok(()) => info!("agent script t={t:.2} PASS {expect:?}"),
                Err(e) => {
                    runner.failures += 1;
                    error!("agent script t={t:.2} FAIL {e}: {}", state(cursor.0));
                }
            },
            Action::Exit(code) => {
                let code = if runner.failures > 0 && code == 0 {
                    3
                } else {
                    code
                };
                info!(
                    "agent script: done, {} failed checks, exit code {code}",
                    runner.failures
                );
                exit.write(AppExit::from_code(code as u8));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pilus::{Pull, Tether};

    #[test]
    fn buttons_parse_by_their_rust_names() {
        assert_eq!(parse_button("Left"), Ok(Button::Mouse(MouseButton::Left)));
        assert_eq!(parse_button("KeyW"), Ok(Button::Key(KeyCode::KeyW)));
        assert_eq!(
            parse_button("ShiftLeft"),
            Ok(Button::Key(KeyCode::ShiftLeft))
        );
        assert!(parse_button("W").is_err());
    }

    #[test]
    fn a_script_parses_and_bad_ones_do_not() {
        let ok = r#"{"steps":[{"at":0,"aim":[1,2]},{"at":1,"press":"Left"},{"at":2,"exit":0}]}"#;
        let script = Script::from_json(ok).unwrap();
        assert_eq!(script.steps[0].action, Action::Aim([1.0, 2.0]));
        assert_eq!(script.log_every, 1.0);
        assert!(Script::from_json(r#"{"steps":[{"at":0,"press":"Nope"}]}"#).is_err());
        assert!(Script::from_json(r#"{"steps":[{"at":2,"log":"a"},{"at":1,"log":"b"}]}"#).is_err());
        assert!(Script::from_json(r#"{"steps":[{"at":0,"expect":{"pilus":"out"}}]}"#).is_err());
    }

    #[test]
    fn checks_test_the_pilus() {
        let attached = PilusTether(Some(Tether {
            target: None,
            anchor: Vec2::ZERO,
            length: 50.0,
            min_length: 10.0,
            max_length: 100.0,
            wanted: 50.0,
            pull: Pull::OwnerToTarget,
        }));
        let want = |p: &str| Expect {
            pilus: Some(p.into()),
            aim: None,
        };
        assert!(check(&want("attached"), &attached, Vec2::X).is_ok());
        assert!(check(&want("in"), &attached, Vec2::X).is_err());
        assert!(check(&want("attached"), &PilusTether(None), Vec2::X).is_err());
        let aim = Expect {
            pilus: None,
            aim: Some([2.0, 0.0]),
        };
        assert!(check(&aim, &PilusTether(None), Vec2::X).is_ok());
        assert!(check(&aim, &PilusTether(None), Vec2::Y).is_err());
    }

    #[test]
    fn every_preset_script_parses() {
        let dir = crate::scenario::assets_dir().join(SCRIPT_DIR);
        let mut count = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path).unwrap();
                let script =
                    Script::from_json(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                if let Some(name) = &script.scenario {
                    let file = crate::scenario::resolve_path(name, &crate::scenario::assets_dir());
                    crate::scenario::load(&file).unwrap();
                }
                count += 1;
            }
        }
        assert!(count > 0, "no scripts in {}", dir.display());
    }
}
