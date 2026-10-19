//! The metabolic map: pools, pathway plates with enzymes, pipes and the
//! molecules that flow in them, and the ribosome.
//!
//! The pools and plates are spawned once for a map size (`MapLayout`); a new
//! size (the window changed) spawns them again. Pipes are spawned the first
//! time the view has an edge for them (the map is fixed, so that is the first
//! frame, and later for flows of the ledger). Each frame the systems here only
//! change what the sprites show, from `LocalMetabolismView`: nothing is
//! rebuilt, so hover and animation stay smooth. The screen computes no flow:
//! it draws the `flux` of each edge.
//!
//! A pathway that the cell does not have (a ghost) is a dim plate with no
//! pipes. If the player does not know it, the plate shows only a "?".

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite::{Anchor, BorderRect, SliceScaleMode, SpriteImageMode, TextureSlicer};
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use super::layout::{self, Layout, Router};
use super::{BAD, GOOD, MapEntity, MapLayout, Pick, make_pickable, make_valve_clickable, map_layer};
use crate::GameState;
use crate::actions::{HIGH_LEVEL, LOW_LEVEL};
use crate::blocks::genome::BlockKind;
use crate::environment::Channel;
use crate::metabolism::BlockStatus;
use crate::metabolism::ledger::FlowSource;
use crate::metabolism::solver::Limit;
use crate::metabolism::view::{
    EdgeKind, EdgeView, LocalMetabolismView, MetabolismView, Node, PathwayView, net_rate,
};
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{SynthesisQueue, UNITS_PER_PROTEIN, synthesis_eta};
use crate::sprites::Sheets;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<PipeMaterial>::default())
        .init_resource::<PipeIndex>()
        .add_systems(
        Update,
        (
            spawn_map.run_if(resource_exists_and_changed::<MapLayout>),
            sync_pipes,
            draw_pipes,
            update_stub_labels,
            emit_specks,
            move_specks,
            update_pools,
            update_plates,
            update_ribosome_text,
        )
            .chain()
            .after(super::fit_map)
            .run_if(in_state(GameState::Manage)),
    );
}

// --- Z order (world units, only for the order of drawing) ---
const Z_PIPE: f32 = 1.0;
const Z_SPECK: f32 = 2.0;
const Z_NODE: f32 = 3.0;
const Z_ICON: f32 = 4.0;
const Z_TEXT: f32 = 5.0;

/// Molecules per second on a pipe at a flux, on a log scale: 0 with no flux.
pub fn speck_rate(flux: f32) -> f32 {
    if flux < 0.01 {
        0.0
    } else {
        (0.5 + 1.6 * (1.0 + 10.0 * flux).log10()).min(6.0)
    }
}

/// The width of a pipe in pixels (tier 1 to 3) from its flux.
pub fn width_tier(flux: f32) -> f32 {
    if flux < 0.2 {
        1.0
    } else if flux < 1.0 {
        2.0
    } else {
        3.0
    }
}

/// How full the gauge of a pool is, in [0, 1], on a log scale.
pub fn gauge_fraction(level: f32) -> f32 {
    const FULL: f32 = 200.0;
    ((1.0 + level.max(0.0)).ln() / (1.0 + FULL).ln()).clamp(0.0, 1.0)
}

/// A pool below this level flashes red.
pub const LOW_MARK: f32 = 3.0;

/// The valve icon for a throttle (drive x action level).
pub fn valve_sprite(throttle: f32) -> &'static str {
    if throttle < 0.05 {
        "sprites/valve_off.png"
    } else if throttle < (LOW_LEVEL + HIGH_LEVEL) / 2.0 + 0.01 {
        "sprites/valve_low.png"
    } else {
        "sprites/valve_high.png"
    }
}

/// The name of a pathway with spaces: "AminoAcidBiosynthesis" becomes
/// "Amino Acid Biosynthesis".
pub fn spaced_name(kind: BlockKind) -> String {
    let raw = format!("{kind:?}");
    let mut out = String::new();
    for (i, c) in raw.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// The name on a plate, in one or two lines (a line is at most 14 letters).
pub fn plate_label(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Chemosynthesis => "Chemosynthesis",
        BlockKind::LightCapture => "Light\ncapture",
        BlockKind::Respiration => "Respiration",
        BlockKind::Fermentation => "Fermentation",
        BlockKind::Catalase => "Catalase",
        BlockKind::AminoAcidBiosynthesis => "Amino acid\nbiosynthesis",
        BlockKind::LipidMetabolism => "Lipid\nmetabolism",
        BlockKind::NucleotideCofactorSynthesis => "Nucleotide\ncofactors",
        BlockKind::SecondaryMetabolites => "Secondary\nmetabolites",
        BlockKind::ProteinSynthesis => "Protein\nsynthesis",
        BlockKind::FatStorage => "Fat storage",
        BlockKind::Cuticle => "Cuticle",
        BlockKind::VesicleExport => "Vesicle\nexport",
        BlockKind::Microcin => "Microcin",
        BlockKind::Slime => "Slime",
        BlockKind::Siderophore => "Siderophore",
        BlockKind::Autoinducer => "AI-2 call",
        _ => "?",
    }
}

/// The name of a pool, as the player reads it.
pub fn pool_name(currency: Currency) -> &'static str {
    match currency {
        Currency::ATP => "ATP",
        Currency::ReducingPower => "Reducing power",
        Currency::Pyruvate => "Pyruvate",
        Currency::CarbonSkeletons => "Carbon",
        Currency::AminoAcids => "Amino acids",
        Currency::Nucleotides => "Nucleotides",
        Currency::Lipids => "Lipids",
        Currency::Metals => "Metals",
        Currency::Waste => "Waste",
        Currency::Minerals => "Minerals",
        Currency::Pigments => "Pigments",
        Currency::O2 => "Oxygen",
        Currency::ROS => "ROS",
    }
}

/// The colour of a currency: the pipes, the specks and the stand-in icons.
pub fn currency_color(currency: Currency) -> Color {
    match currency {
        Currency::ATP => Color::srgb(1.0, 0.82, 0.25),
        Currency::ReducingPower => Color::srgb(0.95, 0.5, 0.75),
        Currency::Pyruvate => Color::srgb(0.95, 0.65, 0.3),
        Currency::CarbonSkeletons => Color::srgb(0.7, 0.62, 0.5),
        Currency::AminoAcids => Color::srgb(0.45, 0.8, 0.5),
        Currency::Nucleotides => Color::srgb(0.6, 0.55, 0.95),
        Currency::Lipids => Color::srgb(0.95, 0.9, 0.5),
        Currency::Metals => Color::srgb(0.75, 0.55, 0.45),
        Currency::Waste => Color::srgb(0.5, 0.42, 0.3),
        Currency::Minerals => Color::srgb(0.45, 0.8, 0.85),
        Currency::Pigments => Color::srgb(0.8, 0.4, 0.85),
        Currency::O2 => Color::srgb(0.6, 0.8, 1.0),
        Currency::ROS => Color::srgb(0.7, 1.0, 0.3),
    }
}

fn channel_color(channel: Channel) -> Color {
    match channel {
        Channel::Heat => Color::srgb(1.0, 0.45, 0.2),
        Channel::Light => Color::srgb(1.0, 0.95, 0.6),
        Channel::Minerals => currency_color(Currency::Minerals),
        Channel::O2 => currency_color(Currency::O2),
        Channel::Water => Color::srgb(0.4, 0.6, 1.0),
        Channel::Microcin => Color::srgb(0.47, 0.75, 0.16),
        Channel::Slime => Color::srgb(0.86, 0.86, 0.8),
        Channel::Siderophore | Channel::FeSiderophore => currency_color(Currency::Metals),
        Channel::Autoinducer => Color::srgb(0.47, 0.83, 0.6),
    }
}

fn source_color(source: FlowSource) -> Color {
    match source {
        FlowSource::Upkeep => Color::srgb(0.6, 0.6, 0.65),
        FlowSource::Movement => Color::srgb(0.45, 0.65, 0.95),
        FlowSource::Action => Color::srgb(0.95, 0.6, 0.35),
        FlowSource::MembraneRepair => Color::srgb(0.95, 0.9, 0.5),
        FlowSource::GenomeRepair => Color::srgb(0.6, 0.55, 0.95),
        FlowSource::Intake => Color::srgb(0.45, 0.8, 0.85),
        FlowSource::Food => Color::srgb(0.7, 0.62, 0.5),
        FlowSource::Engulf => Color::srgb(0.9, 0.4, 0.4),
        FlowSource::Recycle => Color::srgb(0.45, 0.8, 0.5),
        FlowSource::Export => Color::srgb(0.6, 0.8, 1.0),
    }
}

/// The name of a flow source for the panel and the tooltips.
pub fn source_name(source: FlowSource) -> &'static str {
    match source {
        FlowSource::Upkeep => "upkeep",
        FlowSource::Movement => "movement",
        FlowSource::Action => "actions",
        FlowSource::MembraneRepair => "membrane repair",
        FlowSource::GenomeRepair => "genome repair",
        FlowSource::Intake => "intake",
        FlowSource::Food => "food",
        FlowSource::Engulf => "engulfed cell",
        FlowSource::Recycle => "recycled protein",
        FlowSource::Export => "export",
    }
}

fn source_letter(source: FlowSource) -> &'static str {
    match source {
        FlowSource::Upkeep => "U",
        FlowSource::Movement => "M",
        FlowSource::Action => "A",
        FlowSource::MembraneRepair => "R",
        FlowSource::GenomeRepair => "G",
        FlowSource::Intake => "I",
        FlowSource::Food => "F",
        FlowSource::Engulf => "E",
        FlowSource::Recycle => "C",
        FlowSource::Export => "X",
    }
}

/// The molecule icons that exist in `assets/sprites`. The others get a tinted
/// stand-in (a test checks this list against the folder).
pub const MOL_ICONS: [(Currency, &str); 13] = [
    (Currency::ATP, "sprites/mol_atp.png"),
    (Currency::ReducingPower, "sprites/mol_reducing_power.png"),
    (Currency::Pyruvate, "sprites/mol_pyruvate.png"),
    (Currency::CarbonSkeletons, "sprites/mol_carbon_skeletons.png"),
    (Currency::AminoAcids, "sprites/mol_amino_acids.png"),
    (Currency::Nucleotides, "sprites/mol_nucleotides.png"),
    (Currency::Lipids, "sprites/mol_lipids.png"),
    (Currency::Metals, "sprites/mol_metals.png"),
    (Currency::Waste, "sprites/mol_waste.png"),
    (Currency::Minerals, "sprites/mol_minerals.png"),
    (Currency::Pigments, "sprites/mol_pigments.png"),
    (Currency::O2, "sprites/mol_o2.png"),
    (Currency::ROS, "sprites/mol_ros.png"),
];


fn mol_icon(currency: Currency) -> Option<&'static str> {
    MOL_ICONS.iter().find(|(c, _)| *c == currency).map(|(_, p)| *p)
}

fn pathway_icon(kind: BlockKind) -> Option<&'static str> {
    crate::codex_catalog::gene_icon(kind)
}

/// The tint of a pathway: its stand-in icon and its enzymes.
pub(super) fn accent(kind: BlockKind) -> Color {
    match kind {
        BlockKind::Chemosynthesis => Color::srgb(0.95, 0.5, 0.3),
        BlockKind::LightCapture => Color::srgb(1.0, 0.95, 0.5),
        BlockKind::Respiration => Color::srgb(0.9, 0.35, 0.35),
        BlockKind::Fermentation => Color::srgb(0.9, 0.7, 0.3),
        BlockKind::Catalase => Color::srgb(0.7, 1.0, 0.4),
        BlockKind::AminoAcidBiosynthesis => Color::srgb(0.45, 0.8, 0.5),
        BlockKind::LipidMetabolism => Color::srgb(0.95, 0.9, 0.5),
        BlockKind::NucleotideCofactorSynthesis => Color::srgb(0.6, 0.55, 0.95),
        BlockKind::SecondaryMetabolites => Color::srgb(0.8, 0.4, 0.85),
        BlockKind::ProteinSynthesis => Color::srgb(0.45, 0.6, 1.0),
        BlockKind::FatStorage => Color::srgb(0.95, 0.85, 0.5),
        BlockKind::Cuticle => Color::srgb(0.75, 0.7, 0.55),
        BlockKind::VesicleExport => Color::srgb(0.7, 0.6, 0.45),
        BlockKind::Microcin => Color::srgb(0.47, 0.75, 0.16),
        BlockKind::Slime => Color::srgb(0.86, 0.86, 0.8),
        BlockKind::Siderophore => Color::srgb(0.83, 0.69, 0.19),
        BlockKind::Autoinducer => Color::srgb(0.47, 0.83, 0.6),
        _ => Color::WHITE,
    }
}

/// The tint of a plate from the gene state.
fn plate_tint(state: Option<BlockStatus>, owned: bool) -> Color {
    if !owned {
        return Color::srgba(0.7, 0.75, 0.9, 0.5);
    }
    match state {
        Some(BlockStatus::Silent) => Color::srgb(0.45, 0.45, 0.5),
        Some(BlockStatus::Mutated) => Color::srgb(0.95, 0.55, 0.5),
        Some(BlockStatus::Enhanced) => Color::srgb(1.0, 0.88, 0.5),
        _ => Color::WHITE,
    }
}

// --- Components ---

#[derive(Component)]
struct PoolGauge(Currency);

#[derive(Component)]
struct PoolIcon(Currency);

#[derive(Component)]
struct PoolRate(Currency);

#[derive(Component)]
struct PlateFrame(BlockKind);

#[derive(Component)]
struct PlateIcon(BlockKind);

#[derive(Component)]
struct PlateQuestion(BlockKind);

#[derive(Component)]
struct PlateLabel(BlockKind);

/// One of the enzyme sprites of a plate. `index` is its place in the row.
#[derive(Component)]
struct EnzymeSprite {
    kind: BlockKind,
    index: usize,
}

#[derive(Component)]
struct EnzymeMore(BlockKind);

#[derive(Component)]
pub(super) struct Badge(pub BlockKind);

#[derive(Component)]
struct Valve(BlockKind);

#[derive(Component)]
struct RibosomeText;

/// The most route points that the pipe shader takes.
const MAX_PIPE_POINTS: usize = 16;

#[derive(ShaderType, Debug, Clone)]
pub struct PipeParams {
    points: [Vec4; MAX_PIPE_POINTS],
    color: Vec4,
    bad: Vec4,
    info: Vec4,
    state: Vec4,
}

/// The look of a pipe (`assets/shaders/pipe.wgsl`): one material for the
/// route, drawn on the quad of each segment.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PipeMaterial {
    #[uniform(0)]
    params: PipeParams,
}

impl Material2d for PipeMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/pipe.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Mask(0.5)
    }
}

/// The route points with the arc length at each one, for the shader.
pub fn pipe_points(route: &[Vec2]) -> [Vec4; MAX_PIPE_POINTS] {
    let mut out = [Vec4::ZERO; MAX_PIPE_POINTS];
    let mut s = 0.0;
    for (i, p) in route.iter().take(MAX_PIPE_POINTS).enumerate() {
        if i > 0 {
            s += p.distance(route[i - 1]);
        }
        out[i] = Vec4::new(p.x, p.y, s, 0.0);
    }
    out
}

fn linear4(color: Color) -> Vec4 {
    let l = LinearRgba::from(color);
    Vec4::new(l.red, l.green, l.blue, 1.0)
}

/// A pipe. Its quads are `segments`.
#[derive(Component)]
pub(super) struct Pipe {
    route: Vec<Vec2>,
    length: f32,
    segments: Vec<Entity>,
    material: Handle<PipeMaterial>,
    kind: EdgeKind,
    color: Color,
    /// The icon of the molecules on it.
    speck_image: Option<Handle<Image>>,
    /// What the pipe shows now.
    flux: f32,
    pub(super) limiting: bool,
    /// A ghost pathway has no pipes; a stub with no flow is not drawn.
    hidden: bool,
    accum: f32,
}

#[derive(Component)]
struct PipeSegment;

/// The rate text of a stub, at the far end of its pipe.
#[derive(Component)]
struct StubRate {
    incoming: bool,
}

#[derive(Component)]
struct Speck {
    pipe: Entity,
    distance: f32,
}

type EdgeKey = (Node, Node, Option<Currency>);

#[derive(Resource, Default)]
struct PipeIndex {
    /// `None`: the edge has no pipe on the map (for example the minerals that
    /// a pathway needs: they come through the Minerals pool).
    pipes: HashMap<EdgeKey, Option<Entity>>,
    /// Stubs on the left of each pool.
    stubs: HashMap<Currency, usize>,
    router: Router,
}

/// Text in world space. The font is twice the size: the caller sets a
/// transform with `text_at`, which halves the scale, so the glyphs are drawn at
/// the sharpness of the screen.
fn world_text(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text2d::new(text),
        TextFont {
            font_size: FontSize::Px(size * 2.0),
            ..default()
        },
        TextColor(color),
        map_layer(),
    )
}

/// The transform of a `world_text`.
pub(super) fn text_at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z).with_scale(Vec3::splat(0.5))
}

fn icon_sprite(server: &AssetServer, path: Option<&'static str>, tint: Color, size: Vec2) -> Sprite {
    match path {
        Some(path) => Sprite {
            image: server.load(path),
            custom_size: Some(size),
            ..default()
        },
        // The icon does not exist yet: a tinted block.
        None => Sprite::from_color(tint, size * 0.75),
    }
}

fn plate_slicer() -> SpriteImageMode {
    SpriteImageMode::Sliced(TextureSlicer {
        border: BorderRect::all(7.0),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 1.0,
    })
}

const MAX_ENZYME_SPRITES: usize = 6;
const ENZYME_SIZE: f32 = 9.0;
const ENZYME_PITCH: f32 = 10.0;
const LABEL_COLOR: Color = Color::srgb(0.85, 0.88, 0.95);

/// Spawns the pools, plates and the ribosome for the current layout. Called
/// again when the layout changes (`fit_map` removed the old ones).
fn spawn_map(
    mut commands: Commands,
    server: Res<AssetServer>,
    sheets: Res<Sheets>,
    layout: Res<MapLayout>,
    mut index: ResMut<PipeIndex>,
) {
    *index = PipeIndex::default();
    // Pools.
    for currency in Currency::ALL {
        let place = layout.pool_place(currency);
        commands
            .spawn((
                Name::new(format!("Pool {currency:?}")),
                MapEntity,
                Transform::from_translation(place.pos.extend(Z_NODE)),
                Visibility::default(),
                map_layer(),
                DespawnOnExit(GameState::Manage),
            ))
            .with_children(|pool| {
                let mut frame = pool.spawn((
                    Sprite {
                        image: server.load("sprites/pathway_plate.png"),
                        custom_size: Some(place.size),
                        image_mode: plate_slicer(),
                        color: Color::srgb(0.6, 0.65, 0.8),
                        ..default()
                    },
                    map_layer(),
                ));
                make_pickable(&mut frame, Pick::Pool(currency));
                pool.spawn((
                    PoolIcon(currency),
                    icon_sprite(&server, mol_icon(currency), currency_color(currency), Vec2::splat(12.0)),
                    Transform::from_xyz(-place.size.x / 2.0 + 11.0, 2.0, Z_ICON - Z_NODE),
                    map_layer(),
                ));
                pool.spawn((
                    world_text(pool_name(currency), 6.0, LABEL_COLOR),
                    Anchor::CENTER_LEFT,
                    text_at(-place.size.x / 2.0 + 20.0, 5.0, Z_TEXT - Z_NODE),
                ));
                pool.spawn((
                    PoolRate(currency),
                    world_text("", 6.0, GOOD),
                    Anchor::CENTER_LEFT,
                    text_at(-place.size.x / 2.0 + 20.0, -3.0, Z_TEXT - Z_NODE),
                ));
                // The gauge: a dark track along the bottom, and a fill.
                let track = place.size.x - 14.0;
                pool.spawn((
                    Sprite::from_color(Color::srgb(0.12, 0.13, 0.2), Vec2::new(track, 3.0)),
                    Transform::from_xyz(0.0, -place.size.y / 2.0 + 4.0, Z_ICON - Z_NODE),
                    map_layer(),
                ));
                pool.spawn((
                    PoolGauge(currency),
                    Sprite::from_color(currency_color(currency), Vec2::new(track, 2.0)),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(-track / 2.0, -place.size.y / 2.0 + 4.0, Z_ICON - Z_NODE + 0.1),
                    map_layer(),
                ));
            });
    }

    // Plates.
    for kind in layout::mapped_kinds() {
        let Some(place) = layout.plate_place(kind) else {
            continue;
        };
        let left = -place.size.x / 2.0;
        commands
            .spawn((
                Name::new(format!("Plate {kind:?}")),
                MapEntity,
                Transform::from_translation(place.pos.extend(Z_NODE)),
                Visibility::default(),
                map_layer(),
                DespawnOnExit(GameState::Manage),
            ))
            .with_children(|plate| {
                let mut frame = plate.spawn((
                    PlateFrame(kind),
                    Sprite {
                        image: server.load("sprites/pathway_plate_ghost.png"),
                        custom_size: Some(place.size),
                        image_mode: plate_slicer(),
                        ..default()
                    },
                    map_layer(),
                ));
                make_pickable(&mut frame, Pick::Pathway(kind));
                plate.spawn((
                    PlateIcon(kind),
                    icon_sprite(&server, pathway_icon(kind), accent(kind), Vec2::splat(16.0)),
                    Transform::from_xyz(left + 15.0, 6.0, Z_ICON - Z_NODE),
                    map_layer(),
                ));
                plate.spawn((
                    PlateQuestion(kind),
                    world_text("?", 14.0, LABEL_COLOR),
                    text_at(left + 15.0, 6.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                ));
                plate.spawn((
                    PlateLabel(kind),
                    world_text("", 6.0, LABEL_COLOR),
                    Anchor::CENTER_LEFT,
                    text_at(left + 27.0, 6.0, Z_TEXT - Z_NODE),
                ));
                for index in 0..MAX_ENZYME_SPRITES {
                    plate.spawn((
                        EnzymeSprite { kind, index },
                        Sprite {
                            image: server.load("sprites/enzyme.png"),
                            custom_size: Some(Vec2::splat(ENZYME_SIZE)),
                            color: accent(kind),
                            ..default()
                        },
                        Transform::from_xyz(0.0, -place.size.y / 2.0 + 7.0, Z_ICON - Z_NODE),
                        Visibility::Hidden,
                        map_layer(),
                    ));
                }
                plate.spawn((
                    EnzymeMore(kind),
                    world_text("", 6.0, LABEL_COLOR),
                    text_at(26.0, -place.size.y / 2.0 + 7.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                ));
                plate.spawn((
                    Badge(kind),
                    Sprite {
                        image: server.load("sprites/badge_limit.png"),
                        custom_size: Some(Vec2::splat(11.0)),
                        ..default()
                    },
                    Transform::from_xyz(left, place.size.y / 2.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                    map_layer(),
                ));
                let mut valve = plate.spawn((
                    Valve(kind),
                    Sprite {
                        image: server.load("sprites/valve_high.png"),
                        custom_size: Some(Vec2::splat(11.0)),
                        ..default()
                    },
                    Transform::from_xyz(-left - 2.0, place.size.y / 2.0 - 1.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                    map_layer(),
                ));
                make_valve_clickable(&mut valve, kind);
            });
    }

    // The ribosome and the text of the queue.
    let ribosome = layout.ribosome_place();
    if let Some(bundle) = sheets.bundle("ribosome", "loop") {
        commands.spawn((
            Name::new("Ribosome"),
            MapEntity,
            bundle,
            Transform::from_translation(ribosome.pos.extend(Z_NODE)),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
    }
    commands.spawn((
        RibosomeText,
        MapEntity,
        world_text("", 7.0, LABEL_COLOR),
        Anchor::CENTER_LEFT,
        text_at(ribosome.pos.x + 18.0, ribosome.pos.y, Z_TEXT),
        DespawnOnExit(GameState::Manage),
    ));
}

// --- Pools ---

/// The text of a net rate: "+1.2/s", "-0.4/s", "0.0/s".
pub fn rate_text(net: f32) -> String {
    if net.abs() < 0.005 {
        "0.0/s".to_string()
    } else {
        format!("{net:+.1}/s")
    }
}

#[allow(clippy::type_complexity)]
fn update_pools(
    time: Res<Time>,
    view: Res<LocalMetabolismView>,
    mut gauges: Query<(&PoolGauge, &mut Sprite), Without<PoolIcon>>,
    mut icons: Query<(&PoolIcon, &mut Sprite), Without<PoolGauge>>,
    mut rates: Query<(&PoolRate, &mut Text2d, &mut TextColor)>,
) {
    let Some(view) = &view.0 else { return };
    let flash = 0.5 + 0.5 * (time.elapsed_secs() * 8.0).sin();
    for (gauge, mut sprite) in &mut gauges {
        let level = view.pool(gauge.0).map_or(0.0, |p| p.level);
        let track = layout::POOL_SIZE.x - 14.0;
        sprite.custom_size = Some(Vec2::new(track * gauge_fraction(level), 2.0));
        let base = currency_color(gauge.0);
        sprite.color = if level < LOW_MARK && gauge.0 != Currency::Waste && gauge.0 != Currency::ROS {
            base.mix(&BAD, flash)
        } else {
            base
        };
    }
    for (icon, mut sprite) in &mut icons {
        let level = view.pool(icon.0).map_or(0.0, |p| p.level);
        // A pool with nothing in it is dim; one with something is bright.
        let alpha = if level < 0.05 { 0.45 } else { 1.0 };
        sprite.color = if mol_icon(icon.0).is_some() {
            Color::srgba(1.0, 1.0, 1.0, alpha)
        } else {
            currency_color(icon.0).with_alpha(alpha)
        };
    }
    for (rate, mut text, mut color) in &mut rates {
        let net = net_rate(view, rate.0);
        let want = rate_text(net);
        if text.0 != want {
            text.0 = want;
        }
        // Waste and ROS are bad when they rise; the others when they fall.
        let bad_when_up = matches!(rate.0, Currency::Waste | Currency::ROS);
        let shown = if net.abs() < 0.005 {
            Color::srgb(0.5, 0.5, 0.55)
        } else if (net > 0.0) != bad_when_up {
            GOOD
        } else {
            BAD
        };
        if color.0 != shown {
            color.0 = shown;
        }
    }
}

// --- Plates ---

#[allow(clippy::type_complexity)]
fn update_plates(
    time: Res<Time>,
    server: Res<AssetServer>,
    view: Res<LocalMetabolismView>,
    mut frames: Query<(&PlateFrame, &mut Sprite)>,
    mut icons: Query<(&PlateIcon, &mut Sprite, &mut Visibility), Without<PlateFrame>>,
    mut questions: Query<(&PlateQuestion, &mut Visibility), (Without<PlateIcon>, Without<EnzymeSprite>, Without<EnzymeMore>, Without<Badge>, Without<Valve>)>,
    mut labels: Query<(&PlateLabel, &mut Text2d), Without<EnzymeMore>>,
    mut enzymes: Query<
        (&EnzymeSprite, &mut Transform, &mut Visibility),
        (Without<EnzymeMore>, Without<Badge>, Without<Valve>, Without<PlateIcon>, Without<PlateQuestion>),
    >,
    mut more: Query<(&EnzymeMore, &mut Text2d, &mut Visibility), (Without<PlateLabel>, Without<EnzymeSprite>, Without<Badge>, Without<Valve>, Without<PlateIcon>, Without<PlateQuestion>)>,
    mut badges: Query<(&Badge, &mut Visibility, &mut Sprite), (Without<EnzymeSprite>, Without<EnzymeMore>, Without<Valve>, Without<PlateFrame>, Without<PlateIcon>, Without<PlateQuestion>)>,
    mut valves: Query<(&Valve, &mut Visibility, &mut Sprite), (Without<EnzymeSprite>, Without<EnzymeMore>, Without<Badge>, Without<PlateFrame>, Without<PlateIcon>, Without<PlateQuestion>)>,
) {
    let Some(view) = &view.0 else { return };
    let t = time.elapsed_secs();
    let pulse = 0.5 + 0.5 * (t * 6.0).sin();
    let show = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    for (frame, mut sprite) in &mut frames {
        let Some(p) = view.pathway(frame.0) else { continue };
        let image = if p.owned {
            "sprites/pathway_plate.png"
        } else {
            "sprites/pathway_plate_ghost.png"
        };
        let handle = server.load(image);
        if sprite.image != handle {
            sprite.image = handle;
        }
        sprite.color = plate_tint(p.state, p.owned);
    }
    for (icon, mut sprite, mut visibility) in &mut icons {
        let Some(p) = view.pathway(icon.0) else { continue };
        // A pathway that the player does not know shows no icon, so the
        // picture does not give the gene away.
        *visibility = show(p.known);
        let alpha = if p.owned { 1.0 } else { 0.4 };
        sprite.color = if pathway_icon(icon.0).is_some() {
            Color::srgba(1.0, 1.0, 1.0, alpha)
        } else {
            accent(icon.0).with_alpha(alpha)
        };
    }
    for (question, mut visibility) in &mut questions {
        let known = view.pathway(question.0).is_none_or(|p| p.known);
        *visibility = show(!known);
    }
    for (label, mut text) in &mut labels {
        let Some(p) = view.pathway(label.0) else { continue };
        let want = if p.known { plate_label(label.0) } else { "" };
        if text.0 != want {
            text.0 = want.to_string();
        }
    }
    for (enzyme, mut transform, mut visibility) in &mut enzymes {
        let Some(p) = view.pathway(enzyme.kind) else { continue };
        let shown = enzymes_shown(p.enzymes);
        let on = p.owned && enzyme.index < shown;
        *visibility = show(on);
        if on {
            let x = (enzyme.index as f32 - (shown as f32 - 1.0) / 2.0) * ENZYME_PITCH
                - if p.enzymes as usize > MAX_ENZYME_SPRITES { 8.0 } else { 0.0 };
            // The bob: faster and higher with the activity; still at 0.
            let omega = 2.0 + 10.0 * p.activity;
            let phase = enzyme.index as f32 * 0.9;
            let bob = (t * omega + phase).sin() * 1.6 * p.activity.sqrt();
            transform.translation.x = x;
            transform.translation.y = (-layout::PLATE_SIZE.y / 2.0 + 7.0 + bob).round();
        }
    }
    for (m, mut text, mut visibility) in &mut more {
        let Some(p) = view.pathway(m.0) else { continue };
        let on = p.owned && p.enzymes as usize > MAX_ENZYME_SPRITES;
        *visibility = show(on);
        let want = format!("x{}", p.enzymes);
        if on && text.0 != want {
            text.0 = want;
        }
    }
    for (badge, mut visibility, mut sprite) in &mut badges {
        let Some(p) = view.pathway(badge.0) else { continue };
        *visibility = show(shows_badge(p));
        sprite.color = Color::WHITE.mix(&BAD, pulse * 0.6);
    }
    for (valve, mut visibility, mut sprite) in &mut valves {
        let Some(p) = view.pathway(valve.0) else { continue };
        *visibility = show(p.owned);
        let handle = server.load(valve_sprite(p.throttle));
        if sprite.image != handle {
            sprite.image = handle;
        }
    }
}

/// How many enzyme sprites a plate shows: one for each enzyme up to 6, and
/// one sprite with "xN" beyond that.
pub fn enzymes_shown(enzymes: u32) -> usize {
    if enzymes as usize > MAX_ENZYME_SPRITES {
        1
    } else {
        enzymes as usize
    }
}

/// The "!" badge: the pathway is owned and something holds it back.
pub fn shows_badge(p: &PathwayView) -> bool {
    p.owned && !matches!(p.limit, Limit::None | Limit::Off)
}

// --- Probe for the smoke test ---

/// The pathways with a visible "!" badge, and the number of visible pipes
/// that pulse red.
pub(super) fn probe(world: &mut World) -> (Vec<BlockKind>, usize) {
    let badges = world
        .query::<(&Badge, &Visibility)>()
        .iter(world)
        .filter(|(_, v)| **v != Visibility::Hidden)
        .map(|(b, _)| b.0)
        .collect();
    let limiting = world
        .query::<&Pipe>()
        .iter(world)
        .filter(|p| p.limiting && !p.hidden)
        .count();
    (badges, limiting)
}

// --- Pipes ---

/// The route of the pipe of an edge, or `None` if the map has no pipe for it.
fn pipe_route(edge: &EdgeView, layout: &Layout, index: &mut PipeIndex) -> Option<Vec<Vec2>> {
    match (edge.from, edge.to) {
        (Node::Pool(c), Node::Pathway(k)) => {
            Some(index.router.route(layout, Some(c), layout.pool_place(c), layout.plate_place(k)?))
        }
        (Node::Pathway(k), Node::Pool(c)) => {
            Some(index.router.route(layout, Some(c), layout.plate_place(k)?, layout.pool_place(c)))
        }
        (Node::Outside(channel), Node::Pathway(k)) => match channel {
            // Minerals and oxygen come in through the pools.
            Channel::Heat | Channel::Light => {
                let plate = layout.plate_place(k)?;
                let shift = plate.size.x / 2.0 - 6.0;
                Some(layout.route_from_band(plate, if channel == Channel::Heat { shift } else { -shift }))
            }
            _ => None,
        },
        (Node::Source(FlowSource::Intake), Node::Pool(c)) => {
            Some(layout.route_from_band(layout.pool_place(c), -20.0))
        }
        (Node::Pool(c), Node::Source(FlowSource::Export)) => {
            Some(layout.route_to_band(layout.pool_place(c), 20.0))
        }
        (Node::Source(_), Node::Pool(c)) | (Node::Pool(c), Node::Source(_)) => {
            let incoming = matches!(edge.from, Node::Source(_));
            let n = index.stubs.entry(c).or_default();
            let route = layout.stub_route(layout.pool_place(c), *n, incoming);
            *n += 1;
            Some(route)
        }
        _ => None,
    }
}

fn edge_color(edge: &EdgeView) -> Color {
    match edge.kind {
        EdgeKind::Outside => match edge.from {
            Node::Outside(channel) => channel_color(channel),
            _ => Color::WHITE,
        },
        EdgeKind::Other => match (edge.from, edge.to) {
            (Node::Source(s), _) | (_, Node::Source(s)) => source_color(s),
            _ => Color::WHITE,
        },
        _ => edge.currency.map_or(Color::WHITE, currency_color),
    }
}

fn spawn_pipe(
    commands: &mut Commands,
    server: &AssetServer,
    quad: &Handle<Mesh>,
    materials: &mut Assets<PipeMaterial>,
    edge: &EdgeView,
    mut route: Vec<Vec2>,
) -> Entity {
    let color = edge_color(edge);
    if route.len() > MAX_PIPE_POINTS {
        warn!("a pipe route has {} points; the shader takes {MAX_PIPE_POINTS}", route.len());
        route.truncate(MAX_PIPE_POINTS);
    }
    let material = materials.add(PipeMaterial {
        params: PipeParams {
            points: pipe_points(&route),
            color: linear4(color),
            bad: linear4(BAD),
            info: Vec4::new(route.len() as f32, 1.0, 0.0, 0.0),
            state: Vec4::new(0.0, (edge.kind == EdgeKind::Cofactor) as u32 as f32, SPECK_SPEED, 0.0),
        },
    });
    let speck_image = match (edge.kind, edge.currency) {
        (EdgeKind::Outside, _) => None,
        (_, Some(c)) => mol_icon(c).map(|p| server.load(p)),
        _ => None,
    };
    let mut segments = Vec::new();
    for _ in route.windows(2) {
        segments.push(
            commands
                .spawn((
                    PipeSegment,
                    Mesh2d(quad.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::default(),
                    map_layer(),
                    Pickable::IGNORE,
                ))
                .id(),
        );
    }
    let length = layout::route_length(&route);
    let pipe = commands
        .spawn((
            Name::new("Pipe"),
            MapEntity,
            Pipe {
                route: route.clone(),
                length,
                segments: segments.clone(),
                material,
                kind: edge.kind,
                color,
                speck_image,
                flux: 0.0,
                limiting: false,
                hidden: false,
                accum: 0.0,
            },
            Transform::default(),
            Visibility::default(),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ))
        .add_children(&segments)
        .id();
    // The label of a stub: an icon and the rate, at the far end.
    if edge.kind == EdgeKind::Other
        && let (Node::Source(s), _) | (_, Node::Source(s)) = (edge.from, edge.to)
        && !matches!(s, FlowSource::Intake | FlowSource::Export)
    {
        let far = if route[0].x < route[1].x { route[0] } else { route[1] };
        let icon = far + Vec2::new(-4.0, 0.0);
        let incoming = matches!(edge.from, Node::Source(_));
        commands.entity(pipe).with_children(|p| {
            p.spawn((
                Sprite::from_color(source_color(s), Vec2::splat(7.0)),
                Transform::from_xyz(icon.x, icon.y, Z_ICON),
                map_layer(),
            ));
            p.spawn((
                world_text(source_letter(s), 6.0, Color::srgb(0.05, 0.06, 0.1)),
                text_at(icon.x, icon.y, Z_TEXT),
            ));
            p.spawn((
                StubRate { incoming },
                world_text("", 6.0, source_color(s)),
                Anchor::CENTER_RIGHT,
                text_at(icon.x - 6.0, icon.y, Z_TEXT),
            ));
        });
    }
    pipe
}

#[allow(clippy::too_many_arguments)]
fn sync_pipes(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PipeMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    view: Res<LocalMetabolismView>,
    layout: Res<MapLayout>,
    mut index: ResMut<PipeIndex>,
    mut pipes: Query<&mut Pipe>,
) {
    let Some(view) = &view.0 else { return };
    for mut pipe in &mut pipes {
        pipe.flux = 0.0;
        pipe.limiting = false;
        pipe.hidden = true;
    }
    for edge in &view.edges {
        // A ghost pathway has no pipes.
        let pathway = match (edge.from, edge.to) {
            (Node::Pathway(k), _) | (_, Node::Pathway(k)) => Some(k),
            _ => None,
        };
        if pathway.is_some_and(|k| !view.pathway(k).is_some_and(|p| p.owned)) {
            continue;
        }
        let key: EdgeKey = (edge.from, edge.to, edge.currency);
        let entity = match index.pipes.get(&key) {
            Some(entity) => *entity,
            None => {
                let route = pipe_route(edge, &layout, &mut index);
                let quad = quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0)));
                let entity = route.map(|r| spawn_pipe(&mut commands, &server, quad, &mut materials, edge, r));
                index.pipes.insert(key, entity);
                entity
            }
        };
        let Some(entity) = entity else { continue };
        if let Ok(mut pipe) = pipes.get_mut(entity) {
            pipe.flux = shown_flux(edge, view);
            pipe.limiting = is_limiting(edge, view);
            // A stub with no flow is not drawn.
            pipe.hidden = edge.kind == EdgeKind::Other && edge.flux < 0.005;
        }
    }
}

/// The flux that a pipe shows. A pipe of a block moves what the edge says. An
/// outside pipe (heat, light) has no flux in the data: it shows waves while
/// its pathway runs.
fn shown_flux(edge: &EdgeView, view: &MetabolismView) -> f32 {
    match edge.kind {
        EdgeKind::Cofactor => 0.0,
        EdgeKind::Outside => {
            let Node::Pathway(kind) = edge.to else { return 0.0 };
            let Some(p) = view.pathway(kind) else { return 0.0 };
            let blocked = matches!((p.limit, edge.from), (Limit::External(a), Node::Outside(b)) if a == b);
            if p.owned && !blocked && p.enzymes > 0 && p.throttle > 0.0 {
                0.15 + p.activity * 2.0
            } else {
                0.0
            }
        }
        _ => edge.flux,
    }
}

/// True if this edge is the one that limits its pathway.
pub fn is_limiting(edge: &EdgeView, view: &MetabolismView) -> bool {
    let kind = match (edge.from, edge.to) {
        (Node::Pool(_) | Node::Outside(_), Node::Pathway(k)) => k,
        _ => return false,
    };
    let Some(p) = view.pathway(kind) else { return false };
    if !p.owned {
        return false;
    }
    match (p.limit, edge.kind, edge.from) {
        (Limit::Input(c) | Limit::Shared(c), EdgeKind::Input, _) => edge.currency == Some(c),
        (Limit::Cofactor(c), EdgeKind::Cofactor, _) => edge.currency == Some(c),
        (Limit::External(a), EdgeKind::Outside, Node::Outside(b)) => a == b,
        _ => false,
    }
}

fn draw_pipes(
    time: Res<Time<Real>>,
    mut pipes: Query<(&Pipe, &mut Visibility)>,
    mut segments: Query<&mut Transform, With<PipeSegment>>,
    mut materials: ResMut<Assets<PipeMaterial>>,
) {
    let t = time.elapsed_secs_wrapped();
    for (pipe, mut visibility) in &mut pipes {
        let want = if pipe.hidden { Visibility::Hidden } else { Visibility::Inherited };
        if *visibility != want {
            *visibility = want;
        }
        if pipe.hidden {
            continue;
        }
        let mut width = match pipe.kind {
            EdgeKind::Cofactor => 1.0,
            _ => width_tier(pipe.flux),
        };
        if pipe.limiting {
            width = width.max(2.0);
        }
        if let Some(mut material) = materials.get_mut(&pipe.material) {
            let p = &mut material.params;
            p.info = Vec4::new(p.info.x, width, pipe.flux, t);
            p.state.x = pipe.limiting as u32 as f32;
            p.state.w = pipe.flux.min(1.0);
        }
        // Each quad covers its segment, the wall and a pixel to spare.
        let pad = width + 4.0;
        for (i, &entity) in pipe.segments.iter().enumerate() {
            let (Ok(mut transform), Some(&a), Some(&b)) =
                (segments.get_mut(entity), pipe.route.get(i), pipe.route.get(i + 1))
            else {
                continue;
            };
            let size = (b - a).abs() + Vec2::splat(pad);
            *transform = Transform::from_translation(((a + b) / 2.0).extend(Z_PIPE)).with_scale(size.extend(1.0));
        }
    }
}

/// The rate text of the stubs: "+0.4/s" into the pool, "-0.8/s" out of it.
fn update_stub_labels(
    pipes: Query<&Pipe>,
    mut labels: Query<(&StubRate, &ChildOf, &mut Text2d)>,
) {
    for (stub, parent, mut text) in &mut labels {
        let Ok(pipe) = pipes.get(parent.parent()) else { continue };
        let signed = if stub.incoming { pipe.flux } else { -pipe.flux };
        // Short, so that it fits between two pools: the pool tags say "/s".
        let want = rate_text(signed).trim_end_matches("/s").to_string();
        if text.0 != want {
            text.0 = want;
        }
    }
}

const SPECK_SPEED: f32 = 36.0;
const MAX_SPECKS: usize = 600;

fn emit_specks(
    mut commands: Commands,
    time: Res<Time>,
    mut pipes: Query<(Entity, &mut Pipe)>,
    specks: Query<(), With<Speck>>,
) {
    let mut count = specks.iter().count();
    for (entity, mut pipe) in &mut pipes {
        let rate = if pipe.hidden { 0.0 } else { speck_rate(pipe.flux) };
        if rate <= 0.0 || pipe.length <= 0.0 {
            pipe.accum = 0.0;
            continue;
        }
        pipe.accum = (pipe.accum + rate * time.delta_secs()).min(3.0);
        while pipe.accum >= 1.0 && count < MAX_SPECKS {
            pipe.accum -= 1.0;
            count += 1;
            let sprite = match &pipe.speck_image {
                Some(image) => Sprite {
                    image: image.clone(),
                    custom_size: Some(Vec2::splat(7.0)),
                    ..default()
                },
                None => Sprite::from_color(pipe.color, Vec2::splat(4.0)),
            };
            commands.spawn((
                Speck {
                    pipe: entity,
                    distance: 0.0,
                },
                MapEntity,
                sprite,
                Transform::from_translation(pipe.route[0].extend(Z_SPECK)),
                map_layer(),
                DespawnOnExit(GameState::Manage),
            ));
        }
    }
}

fn move_specks(
    mut commands: Commands,
    time: Res<Time>,
    pipes: Query<&Pipe>,
    mut specks: Query<(Entity, &mut Speck, &mut Transform)>,
) {
    for (entity, mut speck, mut transform) in &mut specks {
        let Ok(pipe) = pipes.get(speck.pipe) else {
            commands.entity(entity).despawn();
            continue;
        };
        speck.distance += SPECK_SPEED * time.delta_secs();
        if speck.distance >= pipe.length {
            commands.entity(entity).despawn();
            continue;
        }
        let at = layout::point_along(&pipe.route, speck.distance).round();
        transform.translation = at.extend(Z_SPECK);
    }
}

// --- The ribosome ---

fn update_ribosome_text(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    queues: Query<&SynthesisQueue>,
    view: Res<LocalMetabolismView>,
    mut text: Query<&mut Text2d, With<RibosomeText>>,
) {
    let (Some(cell), Ok(mut text)) = (super::local_cell(&players), text.single_mut()) else {
        return;
    };
    let Ok(queue) = queues.get(cell) else { return };
    let line = if queue.is_empty() {
        "Ribosome: nothing queued".to_string()
    } else {
        // The synthesis rate comes from the flux of the pathway's edges.
        let rate = view.0.as_ref().map_or(0.0, protein_rate);
        let first = queue.orders.front().map_or(String::new(), |o| super::panel::protein_name(*o));
        let eta = synthesis_eta(queue, rate).map_or("no synthesis".to_string(), |s| format!("{s:.0}s"));
        format!(
            "Ribosome: {first} {:.0}/{}   {} queued, done in {eta}",
            queue.progress,
            UNITS_PER_PROTEIN,
            queue.orders.len()
        )
    };
    if text.0 != line {
        text.0 = line;
    }
}

/// The protein units per second that the ribosomes make: the amino acids that
/// the ProteinSynthesis pathway takes in per second (one per unit).
pub fn protein_rate(view: &MetabolismView) -> f32 {
    view.flux(
        Node::Pool(Currency::AminoAcids),
        Node::Pathway(BlockKind::ProteinSynthesis),
        Currency::AminoAcids,
    )
    .unwrap_or(0.0)
        / crate::proteins::AMINO_ACIDS_PER_UNIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn specks_follow_the_flux_on_a_log_scale() {
        assert_eq!(speck_rate(0.0), 0.0);
        assert_eq!(speck_rate(0.005), 0.0);
        let low = speck_rate(0.1);
        let mid = speck_rate(1.0);
        let high = speck_rate(10.0);
        assert!(0.0 < low && low < mid && mid < high && high <= 6.0);
        // A hundred times the flux is not a hundred times the specks.
        assert!(speck_rate(10.0) < 5.0 * speck_rate(0.1));
    }

    #[test]
    fn the_pipe_tier_grows_with_the_flux() {
        assert_eq!(width_tier(0.0), 1.0);
        assert_eq!(width_tier(0.5), 2.0);
        assert_eq!(width_tier(4.0), 3.0);
    }

    #[test]
    fn rates_show_a_sign_and_zero_is_plain() {
        assert_eq!(rate_text(1.24), "+1.2/s");
        assert_eq!(rate_text(-0.4), "-0.4/s");
        assert_eq!(rate_text(0.001), "0.0/s");
    }

    #[test]
    fn the_gauge_is_log_and_bounded() {
        assert_eq!(gauge_fraction(0.0), 0.0);
        assert!(gauge_fraction(10.0) > 0.4, "{}", gauge_fraction(10.0));
        assert_eq!(gauge_fraction(1.0e6), 1.0);
    }

    #[test]
    fn valves_follow_the_throttle() {
        assert!(valve_sprite(0.0).ends_with("off.png"));
        assert!(valve_sprite(LOW_LEVEL).ends_with("low.png"));
        assert!(valve_sprite(HIGH_LEVEL).ends_with("high.png"));
    }

    #[test]
    fn names_fit_the_plate_and_are_spaced() {
        assert_eq!(spaced_name(BlockKind::AminoAcidBiosynthesis), "Amino Acid Biosynthesis");
        for kind in layout::mapped_kinds() {
            assert_ne!(plate_label(kind), "?", "{kind:?}");
            for line in plate_label(kind).lines() {
                assert!(line.len() <= 14, "{kind:?}: {line}");
            }
        }
        for currency in Currency::ALL {
            assert!(pool_name(currency).len() <= 14, "{currency:?}");
        }
        assert_eq!(pool_name(Currency::CarbonSkeletons), "Carbon");
    }

    #[test]
    fn enzyme_sprites_cap_at_six_then_one_with_a_count() {
        assert_eq!(enzymes_shown(0), 0);
        assert_eq!(enzymes_shown(4), 4);
        assert_eq!(enzymes_shown(6), 6);
        assert_eq!(enzymes_shown(7), 1);
    }

    /// The icon lists are the files that exist: a new icon from the art
    /// session must go into the list (and the stand-in goes away).
    #[test]
    fn the_icon_lists_match_the_sprite_folder() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for currency in Currency::ALL {
            let name = format!("sprites/mol_{}.png", snake(&format!("{currency:?}")));
            assert_eq!(
                dir.join(&name).exists(),
                mol_icon(currency).is_some(),
                "{currency:?}: {name}"
            );
        }
        for kind in layout::mapped_kinds() {
            let name = format!("sprites/pathway_icon_{}.png", snake(&format!("{kind:?}")));
            assert_eq!(
                dir.join(&name).exists(),
                pathway_icon(kind).is_some(),
                "{kind:?}: {name}"
            );
        }
        for (_, path) in MOL_ICONS {
            assert!(dir.join(path).exists(), "{path}");
        }
        for (_, path) in crate::codex_catalog::PATHWAY_ICONS {
            assert!(dir.join(path).exists(), "{path}");
        }
        for name in [
            "pathway_plate", "pathway_plate_ghost", "enzyme", "badge_limit", "valve_off", "valve_low",
            "valve_high", "slot_socket", "wall_membrane", "ribosome",
        ] {
            assert!(dir.join(format!("sprites/{name}.png")).exists(), "{name}");
        }
    }

    fn snake(name: &str) -> String {
        // A capital after a small letter starts a new word, so acronyms
        // stay whole: ReducingPower -> reducing_power, ROS -> ros.
        let mut out = String::new();
        let mut prev_lower = false;
        for c in name.chars() {
            if c.is_uppercase() && prev_lower {
                out.push('_');
            }
            prev_lower = c.is_lowercase();
            out.push(c.to_ascii_lowercase());
        }
        out
    }
}
