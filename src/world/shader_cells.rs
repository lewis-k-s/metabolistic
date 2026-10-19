//! # Spike: shader cells on a low-res canvas
//!
//! An experiment, off by default. It tests whether cell bodies can be drawn by
//! a signed-distance-field shader instead of baked sprite sheets, so a cell can
//! grow, stretch and turn without new art.
//!
//! - **Low-res canvas.** When shader cells are on (Run screen only), the run
//!   camera draws into an image at one texel per world pixel. A second camera
//!   draws that image `PIXEL_SCALE` x larger with nearest filtering, and the UI
//!   on top. Every world object is then on the same pixel grid.
//! - **Shader body.** The player's cell gets a quad with `CellMaterial`
//!   (`assets/shaders/cell.wgsl`, a port of `lit_body` in `art/gen_sprites.py`).
//!   Its sprite is hidden. The radius comes from `CellSize`, the stretch and
//!   the angle from `BodyDeform`.
//!
//! Keys (Run screen): **F7** turns shader cells on and off. **F8** cycles the
//! player's radius through 1.0, 1.25 and 1.5 (a visual test: the collider does
//! not change).
//!
//! `METABOLISTIC_SHADER_CELLS=1` starts with shader cells on.
//! `METABOLISTIC_SPIKE_CAPTURE=<dir>` runs a short capture on the first Run
//! screen: screenshots of the sprite and the shader at each radius and while
//! turning, the mean frame time of each mode, then it exits. Use it with a
//! scenario launch.

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;
use std::path::PathBuf;

use crate::cell::{Cell, CellSize};
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::screens::RunCamera;
use crate::sprites::PIXEL_SCALE;
use crate::world::player_body::{BlobSprite, BodyDeform, snap_offset};
use crate::{GameState, InRun};

const SHADER_PATH: &str = "shaders/cell.wgsl";
/// The layer of the scaled-up canvas and its camera.
const CANVAS_LAYER: usize = 7;
/// Seconds of one idle loop of the sprite (8 frames at 6 fps).
const IDLE_LOOP_SECONDS: f32 = 8.0 / 6.0;
/// The player's ramp (`RAMPS["player"]` of the generator).
const PLAYER_RAMP: [&str; 5] = ["#17352f", "#3c8a6e", "#78d49a", "#b4f0b8", "#f0ffe0"];
/// The radii that F8 cycles through, in size units.
const TEST_RADII: [f32; 3] = [1.0, 1.25, 1.5];

pub fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<CellMaterial>::default())
        .insert_resource(ShaderCells(std::env::var("METABOLISTIC_SHADER_CELLS").is_ok_and(|v| v == "1")))
        .init_resource::<Canvas>()
        .add_systems(
            Update,
            (
                (toggle_keys, sync_canvas, fit_canvas).chain().run_if(in_state(GameState::Run)),
                (attach_shader_body, update_shader_body).chain(),
            )
                .run_if(in_state(InRun)),
        )
        .add_systems(
            PostUpdate,
            shader_body_visibility
                .before(VisibilitySystems::VisibilityPropagate)
                .run_if(in_state(InRun)),
        )
        .add_systems(OnExit(GameState::Run), close_canvas);
    if let Ok(dir) = std::env::var("METABOLISTIC_SPIKE_CAPTURE") {
        app.insert_resource(Capture { dir: PathBuf::from(dir), frame: 0, samples: Vec::new() })
            .add_systems(Update, capture.run_if(in_state(GameState::Run)).before(toggle_keys));
    }
}

/// Whether cell bodies are drawn by the shader.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShaderCells(pub bool);

/// The low-res canvas, while it is open.
#[derive(Resource, Default)]
pub struct Canvas {
    image: Option<Handle<Image>>,
    size: UVec2,
}

impl Canvas {
    pub fn is_open(&self) -> bool {
        self.image.is_some()
    }
}

/// The camera that draws the scaled-up canvas and the UI.
#[derive(Component)]
struct CanvasCamera;

/// The sprite that shows the canvas.
#[derive(Component)]
struct CanvasSprite;

/// The shader quad of a cell body.
#[derive(Component)]
struct ShaderBody {
    cell: Entity,
}

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct CellParams {
    pub tones: [Vec4; 5],
    pub radius: f32,
    pub stretch: f32,
    pub angle: f32,
    pub phase: f32,
    pub band: f32,
    pub depth_width: f32,
    pub half_extent: f32,
    pub wobble: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CellMaterial {
    #[uniform(0)]
    pub params: CellParams,
}

impl Material2d for CellMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Mask(0.5)
    }
}

/// The ramp as linear colours, for the uniform.
fn ramp(codes: [&str; 5]) -> [Vec4; 5] {
    codes.map(|code| {
        let srgb = Srgba::hex(code).unwrap_or(Srgba::WHITE);
        Vec4::from_array(LinearRgba::from(srgb).to_f32_array())
    })
}

/// The shader inputs of a body of `radius` pixels. The rim and the shading
/// depth scale with the radius as in `player_cell` (2.6 and 15 at 20 px).
pub fn cell_params(radius: f32, deform: BodyDeform, phase: f32) -> CellParams {
    CellParams {
        tones: ramp(PLAYER_RAMP),
        radius,
        stretch: deform.stretch,
        angle: deform.angle,
        phase,
        band: 2.6,
        depth_width: 0.75 * radius,
        half_extent: quad_half_extent(radius),
        wobble: 1.0,
    }
}

/// Half the side of the quad: the radius with the largest wobble and stretch,
/// plus the outline.
pub fn quad_half_extent(radius: f32) -> f32 {
    (radius * 1.12 * 1.2 + 3.0).ceil()
}

/// The size of the canvas, in texels, for a window of `logical` size: one
/// texel per world pixel, rounded up to an even size (so the centre of the
/// canvas is on a texel edge, like the window centre).
pub fn canvas_size(logical: Vec2) -> UVec2 {
    let texels = (logical / PIXEL_SCALE).ceil().as_uvec2();
    (texels + UVec2::ONE) / 2 * 2
}

/// The cursor in world space while the canvas is open. The run camera draws
/// into the image, so `viewport_to_world_2d` does not work with the window
/// cursor.
pub fn canvas_cursor_world(window: &Window, camera: &GlobalTransform) -> Option<Vec2> {
    let cursor = window.cursor_position()?;
    let offset = cursor - window.size() / 2.0;
    Some(camera.translation().truncate() + Vec2::new(offset.x, -offset.y) / PIXEL_SCALE)
}

/// The inverse of `canvas_cursor_world`: the window cursor (logical pixels)
/// that points at `world` while the canvas is open.
pub fn canvas_world_to_cursor(window_size: Vec2, camera: &GlobalTransform, world: Vec2) -> Vec2 {
    let offset = (world - camera.translation().truncate()) * PIXEL_SCALE;
    window_size / 2.0 + Vec2::new(offset.x, -offset.y)
}

fn toggle_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut shader: ResMut<ShaderCells>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut sizes: Query<&mut CellSize, With<Cell>>,
) {
    if keys.just_pressed(KeyCode::F7) {
        shader.0 = !shader.0;
        info!("shader cells: {}", shader.0);
    }
    if keys.just_pressed(KeyCode::F8) {
        for controlled in &players {
            if let Ok(mut size) = sizes.get_mut(controlled.0) {
                size.radius = next_radius(size.radius);
                info!("player radius: {} px", world_radius(&size));
            }
        }
    }
}

/// The test radius after `radius`.
fn next_radius(radius: f32) -> f32 {
    let i = TEST_RADII.iter().position(|r| (r - radius).abs() < 1e-3).map_or(0, |i| i + 1);
    TEST_RADII[i % TEST_RADII.len()]
}

/// Opens the canvas when shader cells are on, and closes it when they are off.
fn sync_canvas(
    shader: Res<ShaderCells>,
    canvas: Res<Canvas>,
    mut commands: Commands,
) {
    if shader.0 != canvas.is_open() {
        if shader.0 {
            commands.run_system_cached(open_canvas);
        } else {
            commands.run_system_cached(close_canvas);
        }
    }
}

fn open_canvas(
    window: Query<&Window, With<PrimaryWindow>>,
    mut canvas: ResMut<Canvas>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(Entity, &mut Projection), With<RunCamera>>,
    mut commands: Commands,
) {
    let Ok(window) = window.single() else {
        return;
    };
    let size = canvas_size(window.size());
    let image = images.add(canvas_image(size));
    for (entity, mut projection) in &mut cameras {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = 1.0;
        }
        commands
            .entity(entity)
            .insert((RenderTarget::Image(image.clone().into()), Msaa::Off));
    }
    commands.spawn((
        Name::new("Canvas Camera"),
        CanvasCamera,
        Camera2d,
        Camera { order: 1, ..default() },
        Msaa::Off,
        IsDefaultUiCamera,
        RenderLayers::layer(CANVAS_LAYER),
        DespawnOnExit(GameState::Run),
    ));
    commands.spawn((
        Name::new("Canvas"),
        CanvasSprite,
        Sprite {
            image: image.clone(),
            custom_size: Some(size.as_vec2() * PIXEL_SCALE),
            ..default()
        },
        RenderLayers::layer(CANVAS_LAYER),
        DespawnOnExit(GameState::Run),
    ));
    canvas.image = Some(image);
    canvas.size = size;
}

#[allow(clippy::type_complexity)]
fn close_canvas(
    mut canvas: ResMut<Canvas>,
    mut cameras: Query<(Entity, &mut Projection), With<RunCamera>>,
    others: Query<Entity, Or<(With<CanvasCamera>, With<CanvasSprite>)>>,
    mut commands: Commands,
) {
    if !canvas.is_open() {
        return;
    }
    for (entity, mut projection) in &mut cameras {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = 1.0 / PIXEL_SCALE;
        }
        commands.entity(entity).insert((RenderTarget::default(), Msaa::default()));
    }
    for entity in &others {
        commands.entity(entity).despawn();
    }
    canvas.image = None;
}

/// A new canvas image when the window size changes.
fn fit_canvas(
    window: Query<&Window, With<PrimaryWindow>>,
    mut canvas: ResMut<Canvas>,
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<&mut Sprite, With<CanvasSprite>>,
    cameras: Query<Entity, With<RunCamera>>,
    mut commands: Commands,
) {
    let (Ok(window), true) = (window.single(), canvas.is_open()) else {
        return;
    };
    let size = canvas_size(window.size());
    if size == canvas.size {
        return;
    }
    let image = images.add(canvas_image(size));
    for entity in &cameras {
        commands.entity(entity).insert(RenderTarget::Image(image.clone().into()));
    }
    for mut sprite in &mut sprites {
        sprite.image = image.clone();
        sprite.custom_size = Some(size.as_vec2() * PIXEL_SCALE);
    }
    canvas.image = Some(image);
    canvas.size = size;
}

fn canvas_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image
}

/// Adds a shader quad to the player's cell, next to its sprite pivot.
#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn attach_shader_body(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    bodies: Query<&ShaderBody>,
    cells: Query<&CellSize, With<Cell>>,
    pivots: Query<&ChildOf, With<BodyDeform>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CellMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut commands: Commands,
) {
    for controlled in &players {
        let cell = controlled.0;
        // Wait for the body (the sprite pivot), and add one quad only.
        if bodies.iter().any(|b| b.cell == cell) || !pivots.iter().any(|p| p.parent() == cell) {
            continue;
        }
        let Ok(size) = cells.get(cell) else {
            continue;
        };
        let mesh = quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
        let material = materials.add(CellMaterial {
            params: cell_params(world_radius(size), BodyDeform::default(), 0.0),
        });
        commands.entity(cell).with_child((
            Name::new("Shader Body"),
            ShaderBody { cell },
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// Sets the shader inputs and the quad size from the cell each frame.
#[allow(clippy::type_complexity)]
fn update_shader_body(
    time: Res<Time>,
    cells: Query<(&Transform, &CellSize, &Children), (With<Cell>, Without<ShaderBody>)>,
    pivots: Query<&BodyDeform>,
    mut bodies: Query<(&ShaderBody, &MeshMaterial2d<CellMaterial>, &mut Transform)>,
    mut materials: ResMut<Assets<CellMaterial>>,
) {
    let phase = (time.elapsed_secs() / IDLE_LOOP_SECONDS).fract();
    for (body, material, mut transform) in &mut bodies {
        let Ok((cell_transform, size, children)) = cells.get(body.cell) else {
            continue;
        };
        let deform = children.iter().find_map(|c| pivots.get(c).ok()).copied().unwrap_or_default();
        let params = cell_params(world_radius(size), deform, phase);
        // On a whole pixel, like the sprite pivot, so the pieces and the
        // overlays stay where they are on the body.
        let side = 2.0 * params.half_extent;
        *transform = Transform::from_translation(snap_offset(cell_transform.translation.truncate()).extend(0.0))
            .with_scale(Vec3::new(side, side, 1.0));
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.params = params;
        }
    }
}

/// In shader mode, the quad takes the visibility that the sprite had this
/// frame (the engulf clip hides it), and the sprite is hidden.
fn shader_body_visibility(
    shader: Res<ShaderCells>,
    canvas: Res<Canvas>,
    mut sprites: Query<(&BlobSprite, &mut Visibility), Without<ShaderBody>>,
    mut bodies: Query<(&ShaderBody, &mut Visibility), Without<BlobSprite>>,
) {
    let on = shader.0 && canvas.is_open();
    for (body, mut visibility) in &mut bodies {
        let mut wanted = Visibility::Hidden;
        if on {
            for (blob, mut sprite_visibility) in &mut sprites {
                if blob.cell == body.cell {
                    wanted = *sprite_visibility;
                    sprite_visibility.set_if_neq(Visibility::Hidden);
                }
            }
        }
        visibility.set_if_neq(wanted);
    }
}

// --- Capture ---

#[derive(Resource)]
struct Capture {
    dir: PathBuf,
    frame: u32,
    /// (mode, frame seconds).
    samples: Vec<(&'static str, f32)>,
}

/// What the capture does at each frame of the Run screen.
enum Step {
    Shot(&'static str),
    Shader(bool),
    Radius(f32),
    Keys(&'static [KeyCode]),
    Sample(&'static str),
    Exit,
}

const CAPTURE_STEPS: &[(u32, Step)] = &[
    (150, Step::Shot("sprite_r20")),
    (160, Step::Shader(true)),
    (220, Step::Shot("shader_r20")),
    (230, Step::Radius(1.25)),
    (260, Step::Shot("shader_r25")),
    (270, Step::Radius(1.5)),
    (300, Step::Shot("shader_r30")),
    (310, Step::Radius(1.0)),
    (320, Step::Keys(&[KeyCode::KeyD])),
    (370, Step::Shot("shader_move_e")),
    (375, Step::Keys(&[KeyCode::KeyS, KeyCode::KeyA])),
    (395, Step::Shot("shader_turn_1")),
    (402, Step::Shot("shader_turn_2")),
    (430, Step::Shot("shader_move_sw")),
    (431, Step::Keys(&[])),
    (500, Step::Sample("shader")),
    (501, Step::Shader(false)),
    (560, Step::Keys(&[KeyCode::KeyD])),
    (610, Step::Shot("sprite_move_e")),
    (615, Step::Keys(&[KeyCode::KeyS, KeyCode::KeyA])),
    (635, Step::Shot("sprite_turn_1")),
    (642, Step::Shot("sprite_turn_2")),
    (670, Step::Shot("sprite_move_sw")),
    (671, Step::Keys(&[])),
    (740, Step::Sample("sprite")),
    (760, Step::Exit),
];

#[allow(clippy::too_many_arguments)]
fn capture(
    time: Res<Time>,
    mut capture: ResMut<Capture>,
    mut shader: ResMut<ShaderCells>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut held: Local<Vec<KeyCode>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut sizes: Query<&mut CellSize, With<Cell>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
) {
    capture.frame += 1;
    let frame = capture.frame;
    let mode = if shader.0 { "shader" } else { "sprite" };
    if frame > 100 {
        capture.samples.push((mode, time.delta_secs()));
    }
    for key in held.iter() {
        keys.press(*key);
    }
    let Some((_, step)) = CAPTURE_STEPS.iter().find(|(f, _)| *f == frame) else {
        return;
    };
    match step {
        Step::Shot(name) => {
            let path = capture.dir.join(format!("{name}.png"));
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
        Step::Shader(on) => shader.0 = *on,
        Step::Radius(radius) => {
            for controlled in &players {
                if let Ok(mut size) = sizes.get_mut(controlled.0) {
                    size.radius = *radius;
                }
            }
        }
        Step::Keys(new) => {
            for key in held.drain(..) {
                keys.release(key);
            }
            held.extend_from_slice(new);
        }
        Step::Sample(name) => {
            let frames: Vec<f32> = capture.samples.iter().filter(|(m, _)| m == name).map(|(_, s)| *s).collect();
            let mean = frames.iter().sum::<f32>() / frames.len().max(1) as f32;
            info!("spike capture: {name}: {} frames, mean {:.2} ms", frames.len(), mean * 1000.0);
            capture.samples.clear();
        }
        Step::Exit => {
            exit.write(AppExit::Success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_has_one_even_texel_count_per_world_pixel() {
        assert_eq!(canvas_size(Vec2::new(1280.0, 720.0)), UVec2::new(428, 240));
        assert_eq!(canvas_size(Vec2::new(3.0, 3.0)), UVec2::new(2, 2));
    }

    #[test]
    fn the_quad_holds_the_largest_body() {
        for radius in [20.0, 25.0, 30.0] {
            assert!(quad_half_extent(radius) >= radius * 1.12 * 1.15 + 1.0);
        }
    }

    #[test]
    fn the_canvas_cursor_maps_back_to_the_same_world_point() {
        let mut window = Window::default();
        window.resolution.set(1280.0, 720.0);
        let camera = GlobalTransform::from_xyz(300.0, -40.0, 0.0);
        let world = Vec2::new(355.0, 12.0);
        let cursor = canvas_world_to_cursor(window.size(), &camera, world);
        window.set_cursor_position(Some(cursor));
        let back = canvas_cursor_world(&window, &camera).unwrap();
        assert!(back.distance(world) < 1e-3, "{back} != {world}");
    }

    #[test]
    fn f8_cycles_the_test_radii() {
        assert_eq!(next_radius(1.0), 1.25);
        assert_eq!(next_radius(1.25), 1.5);
        assert_eq!(next_radius(1.5), 1.0);
        assert_eq!(next_radius(0.7), 1.0);
    }
}
