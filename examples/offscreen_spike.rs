//! Spike: render without a window and save PNGs, so an agent can iterate
//! while the screen is locked.
//!
//! `cargo run --example offscreen_spike` saves a few frames and exits.
//! `cargo run --example offscreen_spike -- --soak 180` runs for 180 s and
//! saves one frame every 3 s. Each sample logs the macOS screen lock state,
//! the frame count, a pixel hash (it must change, the scene moves) and the
//! mean brightness (it must not be zero).
//!
//! The scene has a 2D camera that renders to an `Image`, a moving sprite from
//! the game assets, a mesh, and a UI text node (UI follows the camera target).
//! Frames go to `target/offscreen_spike/` (or `OFFSCREEN_SPIKE_DIR`).

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

#[derive(Resource)]
struct Spike {
    target: Handle<Image>,
    dir: PathBuf,
    start: Instant,
    soak: Option<Duration>,
    interval: Duration,
    next_shot: Duration,
    shots: u32,
    last_hash: Option<u64>,
}

#[derive(Component)]
struct Spinner;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let soak = args
        .iter()
        .position(|a| a == "--soak")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs);
    let dir = std::env::var("OFFSCREEN_SPIKE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target/offscreen_spike"));
    std::fs::create_dir_all(&dir).expect("create the output dir");

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            .disable::<WinitPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
        1.0 / 60.0,
    )))
    .insert_resource(SpikeArgs { dir, soak })
    .add_systems(Startup, setup)
    .add_systems(Update, (spin, sample));
    app.run();
}

#[derive(Resource)]
struct SpikeArgs {
    dir: PathBuf,
    soak: Option<Duration>,
}

fn setup(
    mut commands: Commands,
    args: Res<SpikeArgs>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    assets: Res<AssetServer>,
) {
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
    let target = images.add(image);

    let camera = commands
        .spawn((
            Camera2d,
            RenderTarget::Image(target.clone().into()),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.04, 0.08, 0.14)),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        Mesh2d(meshes.add(Circle::new(60.0))),
        MeshMaterial2d(materials.add(Color::srgb(0.9, 0.4, 0.2))),
        Transform::from_xyz(-150.0, 0.0, 0.0),
    ));
    commands.spawn((
        Sprite::from_image(assets.load("sprites/enzyme.png")),
        Transform::from_xyz(150.0, 0.0, 1.0).with_scale(Vec3::splat(4.0)),
        Spinner,
    ));
    commands.spawn((
        Text::new("offscreen spike"),
        TextFont {
            font_size: FontSize::Px(28.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        UiTargetCamera(camera),
    ));

    let soak = args.soak;
    commands.insert_resource(Spike {
        target,
        dir: args.dir.clone(),
        start: Instant::now(),
        soak,
        interval: if soak.is_some() {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(500)
        },
        next_shot: Duration::from_millis(500),
        shots: 0,
        last_hash: None,
    });
    info!("offscreen spike: soak = {soak:?}");
}

fn spin(time: Res<Time>, mut q: Query<&mut Transform, With<Spinner>>) {
    for mut t in &mut q {
        t.rotation = Quat::from_rotation_z(time.elapsed_secs());
        t.translation.y = 80.0 * time.elapsed_secs().sin();
    }
}

fn sample(mut commands: Commands, mut spike: ResMut<Spike>, mut exit: MessageWriter<AppExit>) {
    let elapsed = spike.start.elapsed();
    let limit = spike.soak.unwrap_or(Duration::from_secs(3));
    if elapsed > limit + Duration::from_secs(1) {
        info!("offscreen spike: done, {} shots", spike.shots);
        exit.write(AppExit::Success);
        return;
    }
    if elapsed < spike.next_shot || elapsed > limit {
        return;
    }
    let interval = spike.interval;
    spike.next_shot += interval;
    spike.shots += 1;
    let path = spike.dir.join(format!("frame-{:03}.png", spike.shots));
    let t = elapsed.as_secs_f32();
    commands
        .spawn(Screenshot::image(spike.target.clone()))
        .observe(save_to_disk(path))
        .observe(
            move |captured: On<ScreenshotCaptured>, mut spike: ResMut<Spike>| {
                report(&captured.image, t, &mut spike);
            },
        );
}

fn report(image: &Image, t: f32, spike: &mut Spike) {
    let data = image.data.as_deref().unwrap_or(&[]);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();
    let mut sum = 0u64;
    for px in data.chunks_exact(4) {
        sum += (px[0] as u64 + px[1] as u64 + px[2] as u64) / 3;
    }
    let mean = sum as f64 / (data.len() / 4).max(1) as f64;
    let changed = spike.last_hash.map(|h| h != hash);
    spike.last_hash = Some(hash);
    info!(
        "SAMPLE t={t:6.1}s locked={} bytes={} mean={mean:5.1} hash={hash:016x} changed={changed:?}",
        screen_locked(),
        data.len(),
    );
}

/// The macOS screen lock state, from `ioreg` (`CGSSessionScreenIsLocked`).
fn screen_locked() -> &'static str {
    let Ok(out) = std::process::Command::new("ioreg")
        .args(["-n", "Root", "-d1", "-a"])
        .output()
    else {
        return "unknown";
    };
    let text = String::from_utf8_lossy(&out.stdout);
    match text.find("<key>CGSSessionScreenIsLocked</key>") {
        Some(i)
            if text[i..]
                .trim_start_matches("<key>CGSSessionScreenIsLocked</key>")
                .trim_start()
                .starts_with("<true/>") =>
        {
            "yes"
        }
        _ => "no",
    }
}
