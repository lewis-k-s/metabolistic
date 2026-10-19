//! # Shader cells on a low-res canvas
//!
//! Cell bodies are drawn by a signed-distance-field shader instead of baked
//! sprite sheets, so a cell can grow, stretch and turn without new art. Plan:
//! `docs/npc-shader-cells-plan.md`.
//!
//! - **Low-res canvas.** When shader cells are on (Run screen only), the run
//!   camera draws into an image at one texel per world pixel. A second camera
//!   draws that image `PIXEL_SCALE` x larger with nearest filtering, and the UI
//!   on top. Every world object is then on the same pixel grid.
//! - **Shader body.** Every cell with a body (the player and the NPCs) gets a
//!   quad with `CellMaterial` (`assets/shaders/cell.wgsl`, a port of the sprite
//!   generator). The body plan is the cell's `BodyLook` (`cell_look`). Its
//!   sprite stays, hidden, for the clips and the tint. The radius comes from
//!   `CellSize`, the stretch and the angle from `BodyDeform`, the facing from
//!   the velocity (smoothed, so a body turns without the 8-row snap).
//!
//! Keys (Run screen): **F7** turns shader cells on and off (back to the
//! sprites). **F8** cycles the player's radius through 1.0, 1.25 and 1.5 (a
//! visual test: the collider does not change).
//!
//! Shader cells are on by default. `METABOLISTIC_SHADER_CELLS=0` starts with
//! them off.
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
use bevy::render::view::prepare_view_uniforms;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;
use std::path::PathBuf;

use avian2d::prelude::LinearVelocity;

use crate::cell::{Cell, CellSize};
use crate::cell_look::{BodyLook, CellLook, Ramp, quad_half_extent};
use crate::membrane::Membrane;
use crate::npc::{Archetype, Npc, NpcDied, SYMBIONT_CONTACT_GAP};
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::screens::RunCamera;
use crate::sprites::PIXEL_SCALE;
use crate::world::player_body::{BlobSprite, BodyDeform, MOVE_SPEED_THRESHOLD, snap_offset};
use crate::{GameState, InRun};

const SHADER_PATH: &str = "shaders/cell.wgsl";
/// The layer of the scaled-up canvas and its camera.
const CANVAS_LAYER: usize = 7;
/// Seconds of one idle loop of the sprite (8 frames at 6 fps).
const IDLE_LOOP_SECONDS: f32 = 8.0 / 6.0;
/// The radii that F8 cycles through, in size units.
const TEST_RADII: [f32; 3] = [1.0, 1.25, 1.5];

pub fn plugin(app: &mut App) {
    // The canvas adds two cameras, so the view uniform buffer grows when it
    // opens. `bevy_enoki` binds that buffer in `PrepareResources`, in no order
    // with `prepare_view_uniforms`, and can bind the old, smaller buffer (a
    // wgpu validation error that quits the game). An early run of
    // `prepare_view_uniforms` sizes the buffer first; the second run reuses it.
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(Render, prepare_view_uniforms.in_set(RenderSystems::PrepareViews));
    }
    app.add_plugins(Material2dPlugin::<CellMaterial>::default())
        .insert_resource(ShaderCells(std::env::var("METABOLISTIC_SHADER_CELLS").map_or(true, |v| v != "0")))
        .init_resource::<Canvas>()
        .add_systems(
            Update,
            (
                (toggle_keys, sync_canvas, fit_canvas).chain().run_if(in_state(GameState::Run)),
                (attach_shader_body, update_shader_body).chain(),
                (spawn_lysis, advance_lysis).chain(),
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

/// The shader quad of a cell body. `facing` is the smoothed direction the
/// cell faces (world radians), `offset` shifts its idle loop.
#[derive(Component)]
struct ShaderBody {
    cell: Entity,
    facing: f32,
    offset: f32,
    /// Seconds since the quad appeared (the spawn growth).
    age: f32,
}

/// The lysis of a dead NPC: its body bursts where it died, then goes.
#[derive(Component)]
struct LysisBody {
    look: CellLook,
    radius: f32,
    offset: f32,
    age: f32,
}

/// Seconds that a new body takes to grow.
const SPAWN_SECONDS: f32 = 0.35;
/// Seconds of the lysis of a dead NPC.
const LYSIS_SECONDS: f32 = 0.8;

/// Marks a cell that has its shader quad.
#[derive(Component)]
struct HasShaderBody;

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct CellParams {
    pub tones: [Vec4; 5],
    pub rim: [Vec4; 5],
    pub inner: [Vec4; 5],
    pub extra: [Vec4; 5],
    pub glow: [Vec4; 5],
    pub organelles: [Vec4; 4],
    pub tint: Vec4,
    pub shape: u32,
    pub features: u32,
    pub spikes: u32,
    pub moving: u32,
    pub radius: f32,
    pub stretch: f32,
    pub angle: f32,
    pub facing: f32,
    pub phase: f32,
    pub band: f32,
    pub depth_width: f32,
    pub half_extent: f32,
    pub wobble: f32,
    pub breath: f32,
    pub h2_amp: f32,
    pub h2_phase: f32,
    pub h3_amp: f32,
    pub h3_phase: f32,
    pub spike_amp: f32,
    pub rod_half: f32,
    pub rod_radius: f32,
    pub sparks: f32,
    pub glow_in: f32,
    pub glow_out: f32,
    pub glow_strength: f32,
    pub drift: f32,
    pub unit: f32,
    pub life: f32,
    pub lysis: f32,
    pub integrity: f32,
    pub glow_boost: f32,
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
fn ramp(codes: Ramp) -> [Vec4; 5] {
    codes.map(|code| {
        let srgb = Srgba::hex(code).unwrap_or(Srgba::WHITE);
        Vec4::from_array(LinearRgba::from(srgb).to_f32_array())
    })
}

/// What changes from frame to frame on a body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyState {
    pub deform: BodyDeform,
    pub facing: f32,
    pub phase: f32,
    pub moving: bool,
    /// The colour multiplier (the sprite tint).
    pub tint: Color,
    /// Spawn (0 to 1), lysis (0 to 1), membrane integrity (0 to 1) and extra
    /// glow, as in the shader.
    pub life: f32,
    pub lysis: f32,
    pub integrity: f32,
    pub glow_boost: f32,
}

impl Default for BodyState {
    fn default() -> Self {
        BodyState {
            deform: BodyDeform::default(),
            facing: 0.0,
            phase: 0.0,
            moving: false,
            tint: Color::WHITE,
            life: 1.0,
            lysis: 0.0,
            integrity: 1.0,
            glow_boost: 0.0,
        }
    }
}

/// The shader inputs of a body of world radius `radius` pixels.
pub fn cell_params(look: &CellLook, radius: f32, state: BodyState) -> CellParams {
    let r = radius;
    let mut organelles = [Vec4::ZERO; 4];
    for (slot, organelle) in organelles.iter_mut().zip(look.organelles) {
        if let Some(o) = organelle {
            *slot = Vec4::new(o.at[0] * r, o.at[1] * r, o.radius * r, o.kind as u32 as f32);
        }
    }
    CellParams {
        tones: ramp(look.tones),
        rim: ramp(look.rim),
        inner: ramp(look.inner),
        extra: ramp(look.extra),
        glow: ramp(look.glow),
        organelles,
        tint: Vec4::from_array(LinearRgba::from(state.tint).to_f32_array()),
        shape: look.shape as u32,
        features: look.features,
        spikes: look.spikes,
        moving: u32::from(state.moving),
        radius: look.body * r,
        stretch: state.deform.stretch,
        angle: state.deform.angle,
        facing: state.facing,
        phase: state.phase,
        band: look.band * r,
        depth_width: look.depth * r,
        half_extent: quad_half_extent(look, r),
        wobble: 1.0,
        breath: look.breath,
        h2_amp: look.harmonics[0].0,
        h2_phase: look.harmonics[0].1,
        h3_amp: look.harmonics[1].0,
        h3_phase: look.harmonics[1].1,
        spike_amp: look.spike_amp,
        rod_half: look.rod_half * r,
        rod_radius: look.rod_radius * r,
        sparks: look.sparks,
        glow_in: look.glow_in * r,
        glow_out: look.glow_out * r,
        glow_strength: look.glow_strength,
        drift: look.drift * r,
        unit: look.pixel * r,
        life: state.life,
        lysis: state.lysis,
        integrity: state.integrity,
        glow_boost: state.glow_boost,
    }
}

/// One step of `facing` towards `target` (radians), by at most `max_step`,
/// the short way round.
pub fn turn_towards(facing: f32, target: f32, max_step: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let gap = (target - facing + PI).rem_euclid(TAU) - PI;
    facing + gap.clamp(-max_step, max_step)
}

/// The offset of a cell's idle loop: spread by the golden ratio, so that
/// cells do not breathe in step.
fn loop_offset(cell: Entity) -> f32 {
    (cell.index_u32() as f32 * 0.618_034).fract()
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

/// Adds a shader quad to each cell with a body (the `BodyDeform` pivot): the
/// player and the NPCs. The look comes from `BodyLook`; a cell with none is
/// drawn as the player.
#[allow(clippy::type_complexity)]
fn attach_shader_body(
    cells: Query<(Entity, &CellSize, Option<&BodyLook>, &Children), (With<Cell>, Without<HasShaderBody>)>,
    pivots: Query<(), With<BodyDeform>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CellMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut commands: Commands,
) {
    for (cell, size, look, children) in &cells {
        // Wait for the body (the sprite pivot).
        if !children.iter().any(|c| pivots.contains(c)) {
            continue;
        }
        let look = look.map_or_else(CellLook::player, |l| l.0);
        let facing = look.rest_facing;
        let mesh = quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
        let state = BodyState { facing, life: 0.0, ..default() };
        let material = materials.add(CellMaterial { params: cell_params(&look, world_radius(size), state) });
        let body = (
            Name::new("Shader Body"),
            ShaderBody { cell, facing, offset: loop_offset(cell), age: 0.0 },
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::default(),
            Visibility::Hidden,
            ChildOf(cell),
        );
        // The cell can go in this frame (engulfed, dead) before the commands run.
        commands.queue(move |world: &mut World| {
            if let Ok(mut entity) = world.get_entity_mut(cell) {
                entity.insert(HasShaderBody);
                world.spawn(body);
            }
        });
    }
}

/// Rate at which a body turns to its heading, radians per second.
const TURN_RATE: f32 = 10.0;

/// Sets the shader inputs and the quad size from the cell each frame.
#[allow(clippy::type_complexity)]
fn update_shader_body(
    time: Res<Time>,
    cells: Query<
        (&Transform, &CellSize, Option<&BodyLook>, Option<&LinearVelocity>, &Children, Option<&Membrane>, Option<&Npc>),
        (With<Cell>, Without<ShaderBody>),
    >,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    pivots: Query<(&BodyDeform, &Children)>,
    sprites: Query<&Sprite, With<BlobSprite>>,
    mut bodies: Query<(&mut ShaderBody, &MeshMaterial2d<CellMaterial>, &mut Transform)>,
    mut materials: ResMut<Assets<CellMaterial>>,
) {
    let dt = time.delta_secs();
    let player = players
        .iter()
        .find_map(|c| cells.get(c.0).ok())
        .map(|(t, size, ..)| (t.translation.truncate(), world_radius(size)));
    for (mut body, material, mut transform) in &mut bodies {
        let Ok((cell_transform, size, look, velocity, children, membrane, npc)) = cells.get(body.cell) else {
            continue;
        };
        body.age += dt;
        let look = look.map_or_else(CellLook::player, |l| l.0);
        let velocity = velocity.map_or(Vec2::ZERO, |v| v.0);
        let moving = velocity.length() > MOVE_SPEED_THRESHOLD;
        if moving {
            body.facing = turn_towards(body.facing, velocity.to_angle(), TURN_RATE * dt);
        }
        let (deform, tint) = children
            .iter()
            .find_map(|c| pivots.get(c).ok())
            .map_or((BodyDeform::default(), Color::WHITE), |(deform, kids)| {
                let tint = kids.iter().find_map(|k| sprites.get(k).ok()).map_or(Color::WHITE, |s| s.color);
                (*deform, tint)
            });
        let phase = (time.elapsed_secs() / IDLE_LOOP_SECONDS + body.offset).fract();
        let position = cell_transform.translation.truncate();
        let in_contact = npc.is_some_and(|n| n.archetype == Archetype::Symbiont)
            && player.is_some_and(|(at, r)| at.distance(position) - r - world_radius(size) <= SYMBIONT_CONTACT_GAP);
        let state = BodyState {
            deform,
            facing: body.facing,
            phase,
            moving,
            tint,
            life: (body.age / SPAWN_SECONDS).min(1.0),
            lysis: 0.0,
            integrity: membrane.map_or(1.0, |m| integrity_fraction(m.integrity, m.max)),
            glow_boost: if in_contact { 1.0 } else { 0.0 },
        };
        let params = cell_params(&look, world_radius(size), state);
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

/// The membrane integrity as a fraction of its maximum, 0 to 1.
pub fn integrity_fraction(integrity: f32, max: f32) -> f32 {
    if max <= 0.0 { 1.0 } else { (integrity / max).clamp(0.0, 1.0) }
}

/// A dead NPC bursts where it died (in shader mode; else `npc_view` plays the
/// sprite's death clip).
fn spawn_lysis(
    mut died: MessageReader<NpcDied>,
    shader: Res<ShaderCells>,
    canvas: Res<Canvas>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CellMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut commands: Commands,
) {
    for death in died.read() {
        if !(shader.0 && canvas.is_open()) {
            continue;
        }
        let radius = world_radius(&death.size);
        let offset = (death.position.x * 0.137 + death.position.y * 0.071).fract().abs();
        let state = BodyState { facing: death.look.rest_facing, ..default() };
        let params = cell_params(&death.look, radius, state);
        let side = 2.0 * params.half_extent;
        let mesh = quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
        commands.spawn((
            Name::new("Lysis"),
            LysisBody { look: death.look, radius, offset, age: 0.0 },
            Mesh2d(mesh),
            MeshMaterial2d(materials.add(CellMaterial { params })),
            Transform::from_translation(death.position.round().extend(LYSIS_Z))
                .with_scale(Vec3::new(side, side, 1.0)),
            DespawnOnExit(InRun),
        ));
    }
}

/// Z of a lysis: above the creatures, like the death clip.
const LYSIS_Z: f32 = 1.0;

/// Runs each lysis, and removes it at the end.
fn advance_lysis(
    time: Res<Time>,
    mut lyses: Query<(Entity, &mut LysisBody, &MeshMaterial2d<CellMaterial>)>,
    mut materials: ResMut<Assets<CellMaterial>>,
    mut commands: Commands,
) {
    for (entity, mut lysis, material) in &mut lyses {
        lysis.age += time.delta_secs();
        let amount = lysis.age / LYSIS_SECONDS;
        if amount >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let phase = (time.elapsed_secs() / IDLE_LOOP_SECONDS + lysis.offset).fract();
        let state = BodyState { facing: lysis.look.rest_facing, phase, lysis: amount, ..default() };
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.params = cell_params(&lysis.look, lysis.radius, state);
        }
    }
}

/// A render layer that no camera draws: the sprites go there in shader mode.
const HIDDEN_LAYER: usize = 6;

/// In shader mode, each quad takes the visibility of its sprite (the engulf
/// clip hides the player's), and the sprite moves to `HIDDEN_LAYER`. Its own
/// `Visibility` is left alone, so the systems that set it keep working.
#[allow(clippy::type_complexity)]
fn shader_body_visibility(
    shader: Res<ShaderCells>,
    canvas: Res<Canvas>,
    sprites: Query<(Entity, &BlobSprite, &Visibility, Has<RenderLayers>), Without<ShaderBody>>,
    mut bodies: Query<(&ShaderBody, &mut Visibility), Without<BlobSprite>>,
    mut commands: Commands,
) {
    let on = shader.0 && canvas.is_open();
    let mut wanted = std::collections::HashMap::new();
    for (entity, blob, visibility, layered) in &sprites {
        if on {
            wanted.insert(blob.cell, *visibility);
            if !layered {
                commands.entity(entity).insert(RenderLayers::layer(HIDDEN_LAYER));
            }
        } else if layered {
            commands.entity(entity).remove::<RenderLayers>();
        }
    }
    for (body, mut visibility) in &mut bodies {
        visibility.set_if_neq(wanted.get(&body.cell).copied().unwrap_or(Visibility::Hidden));
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
    fn integrity_is_a_clamped_fraction() {
        assert_eq!(integrity_fraction(50.0, 100.0), 0.5);
        assert_eq!(integrity_fraction(-5.0, 100.0), 0.0);
        assert_eq!(integrity_fraction(150.0, 100.0), 1.0);
        assert_eq!(integrity_fraction(0.0, 0.0), 1.0);
    }

    #[test]
    fn a_body_turns_the_short_way_round() {
        use std::f32::consts::PI;
        assert!((turn_towards(0.0, 0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!((turn_towards(0.0, 2.0, 0.25) - 0.25).abs() < 1e-6);
        // From just below +pi to just above -pi: a small step up, not a full turn.
        let next = turn_towards(PI - 0.1, -PI + 0.1, 0.05);
        assert!((next - (PI - 0.05)).abs() < 1e-5, "{next}");
    }

    #[test]
    fn the_player_params_keep_the_spike_values() {
        let params = cell_params(&CellLook::player(), 20.0, BodyState::default());
        assert_eq!(params.radius, 20.0);
        assert!((params.band - 2.6).abs() < 1e-5);
        assert!((params.depth_width - 15.0).abs() < 1e-5);
        assert_eq!(params.half_extent, 30.0);
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
