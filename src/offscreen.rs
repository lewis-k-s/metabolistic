//! # Offscreen mode (full build): the game renders to an image, never to an
//! OS window
//!
//! This is the default when an agent starts the game, so that sessions do not
//! open windows and keep working while the screen is locked. The rules
//! (`wants_offscreen`):
//!
//! - `METABOLISTIC_OFFSCREEN=1` turns the mode on, `METABOLISTIC_OFFSCREEN=0`
//!   turns it off (a window, also for an agent).
//! - Otherwise the mode is on when an agent variable is set: `CLAUDECODE`,
//!   `AI_AGENT`, `CODEX_SANDBOX` or `CODEX_THREAD_ID`.
//!
//! How it works: winit is off, so the primary `Window` entity has no OS window
//! (the renderer skips a window without a raw handle). Window size queries
//! still work. Each camera that targets the primary window renders to one
//! image (`OffscreenTarget`) instead, and each `Screenshot::primary_window()`
//! captures that image. So the smoke test and the playtest need no changes.
//!
//! The cursor: `VirtualCursor` (logical pixels, `None` = no cursor) sets the
//! cursor of the primary window and sends `CursorMoved`, so aim, hover and
//! picking work as with a mouse. Scripts set it (see `agent_script`).
//!
//! Time: each frame is exactly 1/60 s (`TimeUpdateStrategy::ManualDuration`),
//! for virtual, fixed and real time. A slow frame makes the game slower, not
//! less precise. With the same seed and script, the events, positions and aim
//! repeat; pools can differ a little (the NPC and gene RNGs use entropy, and
//! the number of loading frames changes the phase of the metabolic tick).
//!
//! Assets: no hot reload, so edits by other sessions do not reach a running
//! game.
//!
//! A launch with no script (no smoke test, no playtest, no agent script) saves one frame every
//! 2 s to `METABOLISTIC_OFFSCREEN_DIR` (default `target/offscreen`) and exits
//! after `METABOLISTIC_OFFSCREEN_SECS` (default 20). Thus an agent launch never
//! runs forever.
//!
//! The pattern comes from `examples/offscreen_spike.rs`, tested with the screen
//! locked.

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::{PluginGroupBuilder, ScheduleRunnerPlugin};
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{IsDefaultUiCamera, UiSystems};
use bevy::window::{CursorMoved, PrimaryWindow, WindowRef, WindowResolution};
use bevy::winit::WinitPlugin;

/// The length of one frame in offscreen mode.
pub const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// The size of the image, and of the primary `Window` entity.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

const AGENT_VARS: [&str; 4] = ["CLAUDECODE", "AI_AGENT", "CODEX_SANDBOX", "CODEX_THREAD_ID"];

/// True when the game must render offscreen (see the module doc).
pub fn requested() -> bool {
    wants_offscreen(|name| std::env::var(name).ok())
}

/// The rule of `requested`, with the environment as a parameter.
pub fn wants_offscreen(var: impl Fn(&str) -> Option<String>) -> bool {
    match var("METABOLISTIC_OFFSCREEN").as_deref() {
        Some("0") | Some("false") => false,
        Some(v) if !v.is_empty() => true,
        _ => AGENT_VARS
            .iter()
            .any(|name| var(name).is_some_and(|v| !v.is_empty() && v != "0")),
    }
}

/// The agent variable that turned the mode on, for the log.
fn reason() -> String {
    if std::env::var_os("METABOLISTIC_OFFSCREEN").is_some() {
        return "METABOLISTIC_OFFSCREEN".into();
    }
    AGENT_VARS
        .iter()
        .find(|name| std::env::var_os(name).is_some())
        .map(|name| format!("agent variable {name}"))
        .unwrap_or_default()
}

/// The primary window of offscreen mode: a fixed size, no OS window.
pub fn window(title: &str) -> Window {
    Window {
        title: title.into(),
        resolution: WindowResolution::new(WIDTH, HEIGHT),
        ..default()
    }
}

/// Removes winit from the default plugins, runs the loop at 60 Hz, and turns
/// off asset hot reload: another session can edit a shader while this run
/// draws, and a run must use the files as they were at start (a half-edited
/// `lighting.wgsl` froze one run).
pub fn configure(plugins: PluginGroupBuilder) -> PluginGroupBuilder {
    plugins
        .disable::<WinitPlugin>()
        .set(AssetPlugin {
            watch_for_changes_override: Some(false),
            ..default()
        })
        .add(ScheduleRunnerPlugin::run_loop(FRAME))
}

/// The image that replaces the primary window.
#[derive(Resource, Clone)]
pub struct OffscreenTarget(pub Handle<Image>);

/// A camera whose window target this module changed to the image.
#[derive(Component)]
struct Retargeted;

/// The `IsDefaultUiCamera` that this module added (see `pick_ui_camera`).
#[derive(Component)]
struct OffscreenUi;

pub fn plugin(app: &mut App) {
    let image = target_image();
    let handle = app
        .world_mut()
        .get_resource_mut::<Assets<Image>>()
        .expect("offscreen::plugin needs the default plugins first")
        .add(image);
    app.insert_resource(OffscreenTarget(handle))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .add_observer(retarget_camera)
        .add_observer(retarget_screenshot)
        .add_systems(PostUpdate, pick_ui_camera.before(UiSystems::Prepare));
    cursor_plugin(app);

    let scripted = crate::smoke_test::requested()
        || crate::agent_script::requested()
        || std::env::var_os("METABOLISTIC_PLAYTEST").is_some_and(|v| !v.is_empty() && v != "0");
    if !scripted {
        app.insert_resource(AutoFrames::from_env())
            .add_systems(Update, auto_frames);
    }
    info!(
        "Offscreen mode ({}): no window, {WIDTH}x{HEIGHT} image. \
         METABOLISTIC_OFFSCREEN=0 opens a window.",
        reason()
    );
}

/// The cursor of the primary window in offscreen mode, in logical pixels.
/// `None` is no cursor. Scripts set it; `apply_virtual_cursor` copies it to the
/// window at the start of the next frame.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct VirtualCursor(pub Option<Vec2>);

/// Adds `VirtualCursor` once (the offscreen plugin and scripts both call it).
/// Call it as a function: a function plugin can be added only once.
pub fn cursor_plugin(app: &mut App) {
    if app.world().contains_resource::<VirtualCursor>() {
        return;
    }
    app.init_resource::<VirtualCursor>()
        .add_systems(First, apply_virtual_cursor);
}

/// Sets the window cursor and, when it moved, sends `CursorMoved` (picking
/// reads the messages, the game reads the window).
fn apply_virtual_cursor(
    cursor: Res<VirtualCursor>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut moved: MessageWriter<CursorMoved>,
    mut last: Local<Option<Vec2>>,
) {
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    if window.cursor_position() != cursor.0 {
        window.set_cursor_position(cursor.0);
    }
    if *last != cursor.0 {
        if let Some(position) = cursor.0 {
            moved.write(CursorMoved {
                window: entity,
                position,
                delta: last.map(|old| position - old),
            });
        }
        *last = cursor.0;
    }
}

fn target_image() -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    image
}

fn is_primary(target: &RenderTarget, primary: &Query<(), With<PrimaryWindow>>) -> bool {
    match target {
        RenderTarget::Window(WindowRef::Primary) => true,
        RenderTarget::Window(WindowRef::Entity(e)) => primary.contains(*e),
        _ => false,
    }
}

/// A camera that targets the primary window renders to the image instead.
fn retarget_camera(
    insert: On<Insert, RenderTarget>,
    targets: Query<&RenderTarget, With<Camera>>,
    primary: Query<(), With<PrimaryWindow>>,
    image: Res<OffscreenTarget>,
    mut commands: Commands,
) {
    let Ok(target) = targets.get(insert.entity) else {
        return;
    };
    if is_primary(target, &primary) {
        commands
            .entity(insert.entity)
            .insert((RenderTarget::Image(image.0.clone().into()), Retargeted));
    }
}

/// A screenshot of the primary window captures the image instead.
fn retarget_screenshot(
    add: On<Add, Screenshot>,
    shots: Query<&Screenshot>,
    primary: Query<(), With<PrimaryWindow>>,
    image: Res<OffscreenTarget>,
    mut commands: Commands,
) {
    let Ok(shot) = shots.get(add.entity) else {
        return;
    };
    if is_primary(&shot.0, &primary) {
        commands
            .entity(add.entity)
            .insert(Screenshot::image(image.0.clone()));
    }
}

/// Bevy UI falls back to the top camera of the primary window when no camera
/// has `IsDefaultUiCamera`. With the cameras on the image, this system does the
/// same: the top camera on the image gets the marker, while the game has no
/// marker of its own.
#[allow(clippy::type_complexity)]
fn pick_ui_camera(
    cameras: Query<(Entity, &Camera, &RenderTarget, Has<OffscreenUi>), With<Retargeted>>,
    game_defaults: Query<(), (With<IsDefaultUiCamera>, Without<OffscreenUi>)>,
    image: Res<OffscreenTarget>,
    mut commands: Commands,
) {
    let on_image =
        |target: &RenderTarget| matches!(target, RenderTarget::Image(t) if t.handle == image.0);
    let best = if game_defaults.is_empty() {
        cameras
            .iter()
            .filter(|(_, _, target, _)| on_image(target))
            .max_by_key(|(e, camera, _, _)| (camera.order, *e))
            .map(|(e, ..)| e)
    } else {
        None
    };
    for (entity, _, _, has) in &cameras {
        let want = best == Some(entity);
        if want && !has {
            commands
                .entity(entity)
                .insert((IsDefaultUiCamera, OffscreenUi));
        } else if !want && has {
            commands
                .entity(entity)
                .remove::<(IsDefaultUiCamera, OffscreenUi)>();
        }
    }
}

/// Frames and the time limit of a launch with no script.
#[derive(Resource)]
struct AutoFrames {
    dir: PathBuf,
    limit: Duration,
    interval: Duration,
    next: Duration,
    count: u32,
}

impl AutoFrames {
    fn from_env() -> Self {
        let dir = std::env::var_os("METABOLISTIC_OFFSCREEN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/offscreen"));
        let secs = std::env::var("METABOLISTIC_OFFSCREEN_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(20);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            error!("Offscreen: cannot create {}: {e}", dir.display());
        }
        info!(
            "Offscreen: no script, so one frame every 2 s to {} and exit after {secs} s",
            dir.display()
        );
        Self {
            dir,
            limit: Duration::from_secs(secs),
            interval: Duration::from_secs(2),
            next: Duration::from_secs(2),
            count: 0,
        }
    }
}

fn auto_frames(
    time: Res<Time<Real>>,
    mut frames: ResMut<AutoFrames>,
    image: Res<OffscreenTarget>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = time.elapsed();
    if elapsed >= frames.limit + Duration::from_millis(500) {
        info!("Offscreen: time limit, {} frames saved", frames.count);
        exit.write(AppExit::Success);
        return;
    }
    if elapsed < frames.next || elapsed > frames.limit {
        return;
    }
    let interval = frames.interval;
    frames.next += interval;
    frames.count += 1;
    let path = frames.dir.join(format!("frame-{:03}.png", frames.count));
    commands
        .spawn(Screenshot::image(image.0.clone()))
        .observe(save_to_disk(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn a_window_for_a_person() {
        assert!(!wants_offscreen(env(&[])));
    }

    #[test]
    fn offscreen_for_an_agent() {
        assert!(wants_offscreen(env(&[("CLAUDECODE", "1")])));
        assert!(wants_offscreen(env(&[("AI_AGENT", "claude-code_2")])));
        assert!(!wants_offscreen(env(&[("CLAUDECODE", "0")])));
    }

    #[test]
    fn the_explicit_variable_wins() {
        assert!(!wants_offscreen(env(&[
            ("CLAUDECODE", "1"),
            ("METABOLISTIC_OFFSCREEN", "0")
        ])));
        assert!(wants_offscreen(env(&[("METABOLISTIC_OFFSCREEN", "1")])));
    }
}
