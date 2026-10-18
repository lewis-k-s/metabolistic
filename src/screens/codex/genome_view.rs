//! The genome of the codex, drawn in the world.
//!
//! The genome lives inside the cell. On the Manage screen it sits below the
//! metabolic map, on the map layer, and opening the Codex moves the map camera
//! there: a pan with a zoom, and a small pull back halfway (`aim_camera`).
//! Closing moves it back. On the menu and the Prepare screen there is no map,
//! so the Codex brings its own camera, which zooms in from far out.
//!
//! The scene (`Scene`) is one shader quad for the strand (`GenomeMaterial`,
//! `assets/shaders/genome.wgsl`), a product icon above each locus, the gene
//! symbols, the operon bars and their names, and the `oriC` and `ter` labels.
//! The geometry is `genome_map::GenomeLayout`. The icons and the symbols move
//! with the wobble of the strand: the CPU and the shader use the same formula.
//!
//! While the Codex is open, the UI of the screen under it is hidden
//! (`HiddenByCodex`), so the genome and the Codex panel are the only things on
//! the screen. The run inset of the Manage screen stays: the Codex column has a
//! frame in the same place.

use bevy::camera::visibility::VisibilitySystems;
use bevy::camera::{CameraUpdateSystems, ClearColorConfig, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite::Anchor;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy::window::PrimaryWindow;

use super::{CodexData, CodexHover, CodexRoot, CodexShown, CodexUi};
use crate::GameState;
use crate::codex_catalog::{Category, Icon, entry, entry_state};
use crate::genome_damage::GenomeIntegrity;
use crate::genome_map::{
    GenomeLayout, Look, MAX_LOCI, MAX_SEGMENTS, OPERON_DROP, OPERON_NAME_DROP, SYMBOL_DROP, Segment, best_rows,
    flags, look,
};
use crate::player::{ControlledCell, LocalPlayer};
use crate::screens::manage::{MapCamera, MapLayout, map_layer, right_column_width};
use crate::sprites::Sheets;

const SHADER_PATH: &str = "shaders/genome.wgsl";
/// Seconds of the pan from the map to the genome.
const PAN_SECONDS: f32 = 0.6;
/// How far the camera pulls back halfway through the pan (0.5 = 50 % more
/// of the world in view).
const PAN_PULL_BACK: f32 = 0.45;
/// The Codex camera of the menu starts this many times further out.
const MENU_ZOOM_FROM: f32 = 6.0;
/// Space between the bottom of the map and the top of the genome.
const BELOW_MAP: f32 = 160.0;
/// Space that the Codex header and the legend take, in logical pixels.
pub(super) const HEADER_HEIGHT: f32 = 48.0;
pub(super) const LEGEND_HEIGHT: f32 = 40.0;
const AREA_PAD: f32 = 12.0;
/// The background behind the genome on screens without a map: the cytoplasm
/// of the Manage screen.
const CYTOPLASM: Color = Color::srgb(0.05, 0.07, 0.12);
/// The crop of a membrane protein sheet: frame 0 of the row that points north
/// (row 6 of 8), the 24 x 32 pixels over the mount point.
const PROTEIN_CROP: Rect = Rect { min: Vec2::new(16.0, 336.0), max: Vec2::new(40.0, 368.0) };
/// The mount point (`anchor_px`) is 4 pixels above the bottom of the crop.
const PROTEIN_ANCHOR: Anchor = Anchor(Vec2::new(0.0, -0.5 + 4.0 / 32.0));
/// The strand quad reaches this far past the layout bounds, so the dithered
/// edge of the nucleoid is not cut.
const QUAD_PAD: f32 = 40.0;
/// Pixels that a hovered icon rises.
const HOVER_RISE: f32 = 2.0;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<GenomeMaterial>::default())
        .init_resource::<GenomeView>()
        .add_systems(
            Update,
            (open_scene, build_content, place_icons, update_material, pick_locus, close_scene)
                .chain()
                .after(super::update_data)
                .before(super::lifecycle),
        )
        .add_systems(
            PostUpdate,
            (
                aim_camera.before(CameraUpdateSystems).before(TransformSystems::Propagate),
                hide_other_ui.before(VisibilitySystems::VisibilityPropagate),
            ),
        );
}

// --- Colours ---

/// The ramp of each `Look`: outline, dark, base, light.
const RAMPS: [[&str; 4]; Look::COUNT] = [
    // Linker: grey violet.
    ["#1e1c2c", "#3d3a55", "#615c80", "#8f89ad"],
    // Expressed: the mint of the player's cell.
    ["#17352f", "#3c8a6e", "#78d49a", "#b4f0b8"],
    // Enhanced: gold.
    ["#3a2a0c", "#9a6a1c", "#e8b04a", "#ffe08a"],
    // Mutated: crimson.
    ["#3a1014", "#8c2a2c", "#d4504a", "#ff9a80"],
    // Silent: cold teal grey.
    ["#141c22", "#27353d", "#3f5560", "#5f7d88"],
    // In the codex only: pale blue.
    ["#20304c", "#3f5f94", "#7aa0e0", "#b8d0ff"],
    // Unknown: nearly black.
    ["#0e0f14", "#1e2028", "#2e313c", "#444856"],
    // Membrane protein: teal.
    ["#10363a", "#22767a", "#48bcb8", "#9ae8dc"],
];
const NUCLEOID: &str = "#121828";
const ACCENT: &str = "#f2cc59";

fn hex(code: &str) -> Color {
    Color::Srgba(Srgba::hex(code).unwrap_or(Srgba::WHITE))
}

fn linear(code: &str) -> Vec4 {
    Vec4::from_array(LinearRgba::from(hex(code)).to_f32_array())
}

/// A light tone of a look: for the gene symbol and the legend.
pub(super) fn look_color(look: Look) -> Color {
    hex(RAMPS[look as usize][3])
}

/// The base tone of a look: for the legend swatches.
pub(super) fn look_swatch(look: Look) -> Color {
    hex(RAMPS[look as usize][2])
}

/// The colour of an operon bar.
fn category_color(category: Category) -> Color {
    hex(match category {
        Category::Energy => "#f0c050",
        Category::Biosynthesis => "#78d49a",
        Category::Storage => "#c8a070",
        Category::Defence => "#d46a6a",
        Category::Motility => "#7aa0e0",
        Category::Interaction => "#48bcb8",
        Category::Planned => "#8f89ad",
    })
}

// --- Material ---

#[derive(ShaderType, Debug, Clone)]
pub struct GenomeParams {
    tones: [Vec4; 4 * Look::COUNT],
    seg_a: [Vec4; MAX_SEGMENTS],
    seg_b: [Vec4; MAX_SEGMENTS],
    loci: [Vec4; MAX_LOCI],
    rect: Vec4,
    loop_info: Vec4,
    counts: Vec4,
    marks: Vec4,
    nucleoid: Vec4,
    accent: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GenomeMaterial {
    #[uniform(0)]
    params: GenomeParams,
}

impl Material2d for GenomeMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Mask(0.5)
    }
}

/// The parts of the shader input that come from the layout.
fn static_params(layout: &GenomeLayout) -> GenomeParams {
    let mut tones = [Vec4::ZERO; 4 * Look::COUNT];
    for (i, ramp) in RAMPS.iter().enumerate() {
        for (k, code) in ramp.iter().enumerate() {
            tones[i * 4 + k] = linear(code);
        }
    }
    let mut seg_a = [Vec4::ZERO; MAX_SEGMENTS];
    let mut seg_b = [Vec4::ZERO; MAX_SEGMENTS];
    for (i, (s0, seg)) in layout.segments.iter().take(MAX_SEGMENTS).enumerate() {
        match *seg {
            Segment::Line { from, to } => {
                seg_a[i] = Vec4::new(0.0, *s0, seg.length(), 0.0);
                seg_b[i] = Vec4::new(from.x, from.y, to.x, to.y);
            }
            Segment::Arc { centre, radius, start, sweep } => {
                seg_a[i] = Vec4::new(1.0, *s0, seg.length(), sweep);
                seg_b[i] = Vec4::new(centre.x, centre.y, radius, start);
            }
        }
    }
    let b = quad_rect(layout);
    GenomeParams {
        tones,
        seg_a,
        seg_b,
        loci: [Vec4::ZERO; MAX_LOCI],
        rect: Vec4::new(b.min.x, b.min.y, b.width(), b.height()),
        loop_info: Vec4::new(layout.length, layout.waves, layout.turns, 0.0),
        counts: Vec4::new(layout.segments.len().min(MAX_SEGMENTS) as f32, layout.loci.len().min(MAX_LOCI) as f32, 1.0, 0.0),
        marks: Vec4::new(layout.ori.x, layout.ori.y, layout.ter.x, layout.ter.y),
        nucleoid: linear(NUCLEOID),
        accent: linear(ACCENT),
    }
}

/// The rectangle of the strand quad, in layout units.
fn quad_rect(layout: &GenomeLayout) -> Rect {
    layout.bounds.inflate(QUAD_PAD)
}

/// The flags of a locus in the shader.
pub fn locus_flags(active: bool, dimmed: bool, hovered: bool, selected: bool) -> u32 {
    let mut f = 0;
    if active {
        f |= flags::ACTIVE;
    }
    if dimmed {
        f |= flags::DIMMED;
    }
    if hovered {
        f |= flags::HOVERED;
    }
    if selected {
        f |= flags::SELECTED;
    }
    f
}

// --- Scene ---

/// The genome scene while the Codex is open or moving.
#[derive(Resource, Default)]
pub(crate) struct GenomeView {
    /// 0: the view of the screen below. 1: at the genome.
    pub t: f32,
    pub scene: Option<Scene>,
}

impl GenomeView {
    /// The camera that shows the genome (and draws the Codex UI).
    pub fn camera(&self) -> Option<Entity> {
        self.scene.as_ref().map(|s| s.camera)
    }

    /// The genome is on the screen, or the camera moves to it or back.
    pub fn in_view(&self) -> bool {
        self.scene.is_some()
    }

    /// Fully at the genome: the icons can be picked.
    fn arrived(&self) -> bool {
        self.t >= 0.999
    }
}

pub(crate) struct Scene {
    pub layout: GenomeLayout,
    /// The world position of the layout origin.
    pub origin: Vec2,
    pub camera: Entity,
    /// The camera belongs to the Codex (menu and Prepare).
    pub own_camera: bool,
    /// The parent of the drawn things. `None`: build them (again).
    content: Option<Entity>,
    material: Option<Handle<GenomeMaterial>>,
    /// The window size and the known entries that the content was built for.
    built_for: (Vec2, Vec<bool>),
}

/// A product icon (or its stand-in text) and the index of its locus.
#[derive(Component)]
pub(super) struct LocusIcon(usize);

/// A gene symbol under a locus.
#[derive(Component)]
struct LocusSymbol(usize);

/// The Codex's own camera on screens without a map.
#[derive(Component)]
struct CodexCamera;

/// A root UI node that the Codex hid, with its visibility before.
#[derive(Component)]
struct HiddenByCodex(Visibility);

/// Logical pixels for each world pixel, and the camera position (relative to
/// the centre of the bounds) that puts the genome in the middle of the free
/// area: left of the Codex column, between the header and the legend. The
/// scale is a whole number of physical pixels from 2 up, as on the map.
pub fn genome_frame(window: Vec2, scale_factor: f32, bounds_size: Vec2) -> (f32, Vec2) {
    let (min, size) = free_area(window);
    let fit = (size * scale_factor / bounds_size).min_element();
    // A scale a little over the fit is allowed: the padding round the free
    // area takes it.
    let physical = if fit >= 2.0 { (fit + 0.1).floor() } else { ((fit * 4.0).floor() / 4.0).max(0.5) };
    let logical = physical / scale_factor;
    // The free area's centre, from the window centre, in screen pixels (+y down).
    let delta = min + size / 2.0 - window / 2.0;
    (logical, Vec2::new(-delta.x, delta.y) / logical)
}

/// The top-left corner and the size of the free area, in logical pixels.
fn free_area(window: Vec2) -> (Vec2, Vec2) {
    let min = Vec2::new(AREA_PAD, HEADER_HEIGHT + AREA_PAD);
    let size = Vec2::new(
        window.x - right_column_width(window.x) - 2.0 * AREA_PAD,
        window.y - HEADER_HEIGHT - LEGEND_HEIGHT - 2.0 * AREA_PAD,
    )
    .max(Vec2::splat(100.0));
    (min, size)
}

/// Opens the scene when the Codex opens: the layout, the place and the camera.
#[allow(clippy::too_many_arguments)]
fn open_scene(
    ui: Res<CodexUi>,
    state: Res<State<GameState>>,
    mut view: ResMut<GenomeView>,
    window: Query<&Window, With<PrimaryWindow>>,
    map_cameras: Query<Entity, With<MapCamera>>,
    map: Option<Res<MapLayout>>,
    mut commands: Commands,
) {
    if !ui.open || view.scene.is_some() {
        return;
    }
    let Ok(window) = window.single() else {
        return;
    };
    let layout = GenomeLayout::of_catalog(best_rows(free_area(window.size()).1));
    let map_camera = map_cameras.iter().next().filter(|_| *state.get() == GameState::Manage);
    let (origin, camera, own_camera) = match (map_camera, map) {
        (Some(camera), Some(map)) => {
            // Below the map, inside the cell.
            let top = -map.size.y / 2.0 - BELOW_MAP;
            (Vec2::new(0.0, (top - layout.bounds.max.y).round()), camera, false)
        }
        _ => {
            let camera = commands
                .spawn((
                    Name::new("Codex Camera"),
                    CodexCamera,
                    Camera2d,
                    Camera { order: 3, clear_color: ClearColorConfig::Custom(CYTOPLASM), ..default() },
                    Projection::Orthographic(OrthographicProjection {
                        scaling_mode: ScalingMode::WindowSize,
                        ..OrthographicProjection::default_2d()
                    }),
                    map_layer(),
                ))
                .id();
            (Vec2::ZERO, camera, true)
        }
    };
    view.t = 0.0;
    view.scene = Some(Scene {
        layout,
        origin,
        camera,
        own_camera,
        content: None,
        material: None,
        built_for: (Vec2::ZERO, Vec::new()),
    });
}

/// Builds the drawn things, and builds them again when the window size or
/// the known entries change.
#[allow(clippy::too_many_arguments)]
fn build_content(
    mut view: ResMut<GenomeView>,
    data: Res<CodexData>,
    window: Query<&Window, With<PrimaryWindow>>,
    server: Res<AssetServer>,
    sheets: Option<Res<Sheets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GenomeMaterial>>,
    mut commands: Commands,
) {
    let Ok(window) = window.single() else {
        return;
    };
    let Some(scene) = view.scene.as_mut() else {
        return;
    };
    let known: Vec<bool> = scene.layout.loci.iter().map(|l| entry_state(l.id, &data.0).known).collect();
    let wanted = (window.size(), known.clone());
    if scene.content.is_some() && scene.built_for == wanted {
        return;
    }
    if let Some(old) = scene.content.take() {
        commands.entity(old).despawn();
    }
    if scene.built_for.0 != window.size() && scene.built_for.0 != Vec2::ZERO {
        scene.layout = GenomeLayout::of_catalog(best_rows(free_area(window.size()).1));
    }
    scene.built_for = wanted;
    let layout = &scene.layout;
    let (logical, _) = genome_frame(window.size(), window.scale_factor(), layout.bounds.size());
    // Text is drawn at this many times its size and scaled down, so the glyphs
    // are as sharp as the screen.
    let sharp = logical.ceil().max(2.0);
    let material = materials.add(GenomeMaterial { params: static_params(layout) });
    let b = quad_rect(layout);
    let content = commands
        .spawn((
            Name::new("Genome"),
            Transform::from_translation(scene.origin.extend(-5.0)),
            Visibility::default(),
        ))
        .with_children(|root| {
            root.spawn((
                Name::new("Genome Strand"),
                Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
                MeshMaterial2d(material.clone()),
                Transform::from_translation(b.center().extend(0.0)).with_scale(b.size().extend(1.0)),
                map_layer(),
            ));
            for (i, locus) in layout.loci.iter().enumerate() {
                let e = entry(locus.id);
                let x = (locus.x0 + locus.x1) / 2.0;
                let icon = if known[i] { e.icon } else { Icon::Missing };
                let sprite = match icon {
                    Icon::Image(path) => Some((Sprite::from_image(server.load(path.to_string())), Anchor::BOTTOM_CENTER)),
                    Icon::Overlay(name) => sheets.as_deref().and_then(|s| s.sheet(name, "idle")).map(|sheet| {
                        (Sprite { image: sheet.image.clone(), rect: Some(PROTEIN_CROP), ..default() }, PROTEIN_ANCHOR)
                    }),
                    Icon::Missing => None,
                };
                match sprite {
                    Some((sprite, anchor)) => {
                        root.spawn((LocusIcon(i), sprite, anchor, Transform::from_xyz(x, locus.y, 2.0), map_layer()));
                    }
                    None => {
                        let (text, size) = if known[i] {
                            (e.name.chars().take(3).collect::<String>().to_uppercase(), 8.0)
                        } else {
                            ("?".to_string(), 7.0)
                        };
                        root.spawn((
                            LocusIcon(i),
                            Text2d::new(text),
                            TextFont { font_size: FontSize::Px(size * sharp), ..default() },
                            TextColor(look_color(Look::Unknown)),
                            Anchor::BOTTOM_CENTER,
                            Transform::from_xyz(x, locus.y, 2.0).with_scale(Vec3::splat(1.0 / sharp)),
                            map_layer(),
                        ));
                    }
                }
                // An unknown gene has no name yet.
                if !known[i] {
                    continue;
                }
                root.spawn((
                    LocusSymbol(i),
                    Text2d::new(e.symbol),
                    TextFont { font_size: FontSize::Px(6.0 * sharp), ..default() },
                    TextColor(look_color(Look::Linker)),
                    Transform::from_xyz(x, locus.y - SYMBOL_DROP, 2.0).with_scale(Vec3::splat(1.0 / sharp)),
                    map_layer(),
                ));
            }
            for operon in &layout.operons {
                let color = category_color(operon.category);
                let width = operon.x1 - operon.x0 - 4.0;
                root.spawn((
                    Sprite::from_color(color, Vec2::new(width, 1.0)),
                    Transform::from_xyz((operon.x0 + operon.x1) / 2.0, operon.y - OPERON_DROP, 1.0),
                    map_layer(),
                ));
                if operon.continued {
                    continue;
                }
                root.spawn((
                    Text2d::new(operon.category.name()),
                    TextFont { font_size: FontSize::Px(5.0 * sharp), ..default() },
                    TextColor(color),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(operon.x0 + 2.0, operon.y - OPERON_NAME_DROP, 1.0).with_scale(Vec3::splat(1.0 / sharp)),
                    map_layer(),
                ));
            }
            for (text, at, anchor) in [
                ("oriC", layout.ori + Vec2::new(-6.0, 0.0), Anchor::CENTER_RIGHT),
                ("ter", layout.ter + Vec2::new(6.0, 0.0), Anchor::CENTER_LEFT),
            ] {
                root.spawn((
                    Text2d::new(text),
                    TextFont { font_size: FontSize::Px(6.0 * sharp), ..default() },
                    TextColor(hex(ACCENT)),
                    anchor,
                    Transform::from_translation(at.extend(1.0)).with_scale(Vec3::splat(1.0 / sharp)),
                    map_layer(),
                ));
            }
        })
        .id();
    scene.content = Some(content);
    scene.material = Some(material);
}

/// The time of the animation. Real time: the genome moves while the game is
/// paused in a shelter too.
fn anim_time(time: &Time<Real>) -> f32 {
    // Wrap, so the f32 keeps its precision in a long session.
    time.elapsed_secs_wrapped()
}

/// Moves the icons and the symbols with the strand. Dims the ones that the
/// filter leaves out.
#[allow(clippy::type_complexity)]
fn place_icons(
    time: Res<Time<Real>>,
    view: Res<GenomeView>,
    ui: Res<CodexUi>,
    data: Res<CodexData>,
    hover: Res<CodexHover>,
    mut icons: Query<(&LocusIcon, &mut Transform, Option<&mut Sprite>), Without<LocusSymbol>>,
    mut symbols: Query<(&LocusSymbol, &mut Transform, &mut TextColor), Without<LocusIcon>>,
) {
    let Some(scene) = &view.scene else {
        return;
    };
    let t = anim_time(&time);
    let layout = &scene.layout;
    for (icon, mut transform, sprite) in &mut icons {
        let locus = &layout.loci[icon.0];
        let rise = if hover.0 == Some(locus.id) { HOVER_RISE } else { 0.0 };
        let foot = layout.icon_foot(icon.0, t) + Vec2::new(0.0, rise);
        transform.translation.x = foot.x.round();
        transform.translation.y = foot.y.round();
        if let Some(mut sprite) = sprite {
            let state = entry_state(locus.id, &data.0);
            let shown = ui.filter.accepts(locus.id, &state);
            let tint = match (shown, look(locus.id, &state)) {
                (false, _) => Color::srgb(0.3, 0.32, 0.38),
                (true, Look::InCodex) => Color::srgb(0.6, 0.7, 0.95),
                (true, Look::Silent) => Color::srgb(0.55, 0.62, 0.66),
                _ => Color::WHITE,
            };
            if sprite.color != tint {
                sprite.color = tint;
            }
        }
    }
    for (symbol, mut transform, mut color) in &mut symbols {
        let locus = &layout.loci[symbol.0];
        let y = layout.strand_point(locus.mid_s(), t).y - SYMBOL_DROP;
        transform.translation.y = y.round();
        let state = entry_state(locus.id, &data.0);
        let want = if ui.filter.accepts(locus.id, &state) {
            look_color(look(locus.id, &state))
        } else {
            look_color(Look::Unknown)
        };
        if color.0 != want {
            color.0 = want;
        }
    }
}

/// The per-frame shader input: the time, the looks and flags of the loci, and
/// the genome integrity.
#[allow(clippy::too_many_arguments)]
fn update_material(
    time: Res<Time<Real>>,
    view: Res<GenomeView>,
    ui: Res<CodexUi>,
    data: Res<CodexData>,
    shown: Res<CodexShown>,
    hover: Res<CodexHover>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    integrity: Query<&GenomeIntegrity>,
    mut materials: ResMut<Assets<GenomeMaterial>>,
) {
    let Some(scene) = &view.scene else {
        return;
    };
    let Some(mut material) = scene.material.as_ref().and_then(|h| materials.get_mut(h)) else {
        return;
    };
    let p = &mut material.params;
    p.loop_info.w = anim_time(&time);
    p.counts.z = players
        .iter()
        .next()
        .and_then(|c| integrity.get(c.0).ok())
        .map_or(1.0, |g| g.0.clamp(0.0, 1.0));
    for (i, locus) in scene.layout.loci.iter().take(MAX_LOCI).enumerate() {
        let state = entry_state(locus.id, &data.0);
        let f = locus_flags(
            state.count > 0,
            !ui.filter.accepts(locus.id, &state),
            hover.0 == Some(locus.id),
            shown.0 == Some(locus.id),
        );
        p.loci[i] = Vec4::new(locus.s0, locus.s1, look(locus.id, &state) as u32 as f32, f as f32);
    }
}

/// The locus under the pointer becomes the hovered entry. Moving onto a locus
/// shows it in the panel, and the panel stays on it.
fn pick_locus(
    view: Res<GenomeView>,
    time: Res<Time<Real>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut hover: ResMut<CodexHover>,
    mut shown: ResMut<CodexShown>,
) {
    let found = (|| {
        let scene = view.scene.as_ref().filter(|_| view.arrived())?;
        let window = window.single().ok()?;
        let cursor = window.cursor_position()?;
        let (min, size) = free_area(window.size());
        // Only over the free area, not over the panel or the header.
        if cursor.x > min.x + size.x + AREA_PAD || cursor.y < HEADER_HEIGHT {
            return None;
        }
        let (camera, transform) = cameras.get(scene.camera).ok()?;
        let world = camera.viewport_to_world_2d(transform, cursor).ok()?;
        let i = scene.layout.pick(world - scene.origin, anim_time(&time))?;
        Some(scene.layout.loci[i].id)
    })();
    // Only a move onto a new locus changes the panel, so the panel keeps an
    // entry that was chosen in another way.
    if hover.0 != found {
        hover.0 = found;
        if let Some(id) = found {
            shown.0 = Some(id);
        }
    }
}

/// Removes the scene (and the Codex camera) once the view is back.
fn close_scene(ui: Res<CodexUi>, mut view: ResMut<GenomeView>, mut hover: ResMut<CodexHover>, mut commands: Commands) {
    if ui.open || view.t > 0.0 {
        return;
    }
    let Some(scene) = view.scene.take() else {
        return;
    };
    if let Some(content) = scene.content {
        commands.entity(content).despawn();
    }
    if scene.own_camera {
        commands.entity(scene.camera).despawn();
    }
    hover.0 = None;
}

/// A smooth step from 0 to 1 with zero slope at both ends.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// The camera scale and place on the way from `from` to `to` at `e` (eased,
/// 0..1). The scale moves evenly in log space; `pull` adds a pull back that is
/// largest halfway.
pub fn pan_step(from: (f32, Vec2), to: (f32, Vec2), e: f32, pull: f32) -> (f32, Vec2) {
    let scale = (from.0.ln() + (to.0.ln() - from.0.ln()) * e).exp() * (1.0 + pull * (std::f32::consts::PI * e).sin());
    (scale, from.1.lerp(to.1, e))
}

/// Moves the view to the genome and back. On the Manage screen this moves the
/// map camera, after `fit_map` has set it for the map this frame.
fn aim_camera(
    time: Res<Time<Real>>,
    ui: Res<CodexUi>,
    mut view: ResMut<GenomeView>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Projection, &mut Transform)>,
) {
    let target = if ui.open { 1.0 } else { 0.0 };
    let step = time.delta_secs() / PAN_SECONDS;
    view.t = if view.t < target { (view.t + step).min(target) } else { (view.t - step).max(target) };
    let t = view.t;
    let (Some(scene), Ok(window)) = (&view.scene, window.single()) else {
        return;
    };
    let Ok((mut projection, mut transform)) = cameras.get_mut(scene.camera) else {
        return;
    };
    let Projection::Orthographic(ortho) = &mut *projection else {
        return;
    };
    let (logical, offset) = genome_frame(window.size(), window.scale_factor(), scene.layout.bounds.size());
    let at_genome = (1.0 / logical, scene.origin + scene.layout.bounds.center() + offset);
    let (from, pull) = if scene.own_camera {
        ((at_genome.0 * MENU_ZOOM_FROM, at_genome.1), 0.0)
    } else {
        ((ortho.scale, transform.translation.truncate()), PAN_PULL_BACK)
    };
    let (scale, mut pos) = pan_step(from, at_genome, ease(t), pull);
    if t >= 1.0 {
        // On a whole world pixel, so the sprites stay on the pixel grid.
        pos = pos.round();
    }
    ortho.scale = scale;
    transform.translation.x = pos.x;
    transform.translation.y = pos.y;
}

/// While the Codex is open, the UI of the screen below is hidden. A screen
/// that shows a node again (a tooltip) is hidden again in the same frame, and
/// its wish is kept for when the Codex closes.
#[allow(clippy::type_complexity)]
fn hide_other_ui(
    ui: Res<CodexUi>,
    mut roots: Query<(Entity, &mut Visibility, Option<&mut HiddenByCodex>), (With<Node>, Without<ChildOf>, Without<CodexRoot>)>,
    mut commands: Commands,
) {
    for (entity, mut visibility, hidden) in &mut roots {
        match (ui.open, hidden) {
            (true, None) => {
                commands.entity(entity).insert(HiddenByCodex(*visibility));
                *visibility = Visibility::Hidden;
            }
            (true, Some(mut saved)) => {
                if *visibility != Visibility::Hidden {
                    saved.0 = *visibility;
                    *visibility = Visibility::Hidden;
                }
            }
            (false, Some(saved)) => {
                *visibility = saved.0;
                commands.entity(entity).remove::<HiddenByCodex>();
            }
            (false, None) => {}
        }
    }
}

/// The number of loci on the screen, for the smoke test.
pub(super) fn locus_count(world: &mut World) -> usize {
    world.query::<&LocusIcon>().iter(world).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_genome_fills_the_free_area_at_a_whole_pixel_scale() {
        let bounds = GenomeLayout::of_catalog(4).bounds.size();
        for (window, factor) in [(Vec2::new(1280.0, 800.0), 2.0), (Vec2::new(1920.0, 1080.0), 1.0)] {
            let (logical, _) = genome_frame(window, factor, bounds);
            let physical = logical * factor;
            assert!(physical >= 2.0 && physical.fract() == 0.0, "{physical}");
            let (_, size) = free_area(window);
            let room = size + Vec2::splat(2.0 * AREA_PAD);
            assert!(bounds.x * logical <= room.x && bounds.y * logical <= room.y, "{window}: {}", bounds * logical);
        }
    }

    #[test]
    fn the_camera_offset_moves_the_genome_left_of_the_column() {
        let (logical, offset) = genome_frame(Vec2::new(1280.0, 800.0), 1.0, Vec2::new(300.0, 280.0));
        // The free area is left of the column, so the camera looks right of
        // the genome: the genome shows left of the window centre.
        assert!(offset.x > 0.0);
        let column = right_column_width(1280.0);
        assert!((offset.x * logical - column / 2.0).abs() < 1.0);
    }

    #[test]
    fn the_pan_starts_and_ends_where_it_should_and_pulls_back_halfway() {
        let from = (0.5, Vec2::ZERO);
        let to = (0.25, Vec2::new(0.0, -800.0));
        assert_eq!(pan_step(from, to, 0.0, 0.45), from);
        let (scale, pos) = pan_step(from, to, 1.0, 0.45);
        assert!((scale - 0.25).abs() < 1e-5 && pos.distance(to.1) < 1e-3);
        let (mid, _) = pan_step(from, to, 0.5, 0.45);
        assert!(mid > 0.5, "the view pulls back halfway: {mid}");
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn flags_are_bits() {
        assert_eq!(locus_flags(true, false, false, true), flags::ACTIVE | flags::SELECTED);
        assert_eq!(locus_flags(false, true, true, false), flags::DIMMED | flags::HOVERED);
    }
}
