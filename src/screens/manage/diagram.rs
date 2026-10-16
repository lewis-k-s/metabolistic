//! The metabolic map: pools, pathway plates with enzymes, pipes and the
//! molecules that flow in them, and the ribosome.
//!
//! The pools and plates are spawned once when the screen opens. Pipes are
//! spawned the first time the view has an edge for them (the map is fixed, so
//! that is the first frame, and later for flows of the ledger). Each frame the
//! systems here only change what the sprites show, from `LocalMetabolismView`:
//! nothing is rebuilt, so hover and animation stay smooth. The screen computes
//! no flow: it draws the `flux` of each edge.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::prelude::*;
use bevy::sprite::{Anchor, BorderRect, SliceScaleMode, SpriteImageMode, TextureSlicer};

use super::layout::{self, Place};
use super::{BAD, Pick, make_pickable, map_layer};
use crate::GameState;
use crate::actions::{HIGH_LEVEL, LOW_LEVEL};
use crate::blocks::genome::BlockKind;
use crate::environment::Channel;
use crate::metabolism::BlockStatus;
use crate::metabolism::ledger::FlowSource;
use crate::metabolism::solver::Limit;
use crate::metabolism::view::{EdgeKind, EdgeView, LocalMetabolismView, MetabolismView, Node, PathwayView};
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{SynthesisQueue, UNITS_PER_PROTEIN, synthesis_eta};
use crate::sprites::Sheets;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PipeIndex>()
        .add_systems(OnEnter(GameState::Manage), spawn_map)
        .add_systems(OnExit(GameState::Manage), reset_index)
        .add_systems(
            Update,
            (
                sync_pipes,
                draw_pipes,
                emit_specks,
                move_specks,
                update_pools,
                update_plates,
                update_ribosome_text,
            )
                .chain()
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

/// A short name that fits on a plate.
pub fn short_name(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Chemosynthesis => "Chemo",
        BlockKind::LightCapture => "Light",
        BlockKind::Respiration => "Respire",
        BlockKind::Fermentation => "Ferment",
        BlockKind::Catalase => "Catalase",
        BlockKind::AminoAcidBiosynthesis => "Amino ac.",
        BlockKind::LipidMetabolism => "Lipids",
        BlockKind::NucleotideCofactorSynthesis => "Nucleotid",
        BlockKind::SecondaryMetabolites => "Pigments",
        BlockKind::ProteinSynthesis => "Proteins",
        BlockKind::FatStorage => "Fat store",
        BlockKind::Cuticle => "Cuticle",
        BlockKind::VesicleExport => "Export",
        _ => "?",
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
pub const MOL_ICONS: [(Currency, &str); 10] = [
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
];

/// The pathway icons that exist in `assets/sprites`.
pub const PATHWAY_ICONS: [(BlockKind, &str); 7] = [
    (BlockKind::Chemosynthesis, "sprites/pathway_icon_chemosynthesis.png"),
    (BlockKind::Fermentation, "sprites/pathway_icon_fermentation.png"),
    (BlockKind::AminoAcidBiosynthesis, "sprites/pathway_icon_amino_acid_biosynthesis.png"),
    (BlockKind::LipidMetabolism, "sprites/pathway_icon_lipid_metabolism.png"),
    (BlockKind::ProteinSynthesis, "sprites/pathway_icon_protein_synthesis.png"),
    (BlockKind::FatStorage, "sprites/pathway_icon_fat_storage.png"),
    (BlockKind::VesicleExport, "sprites/pathway_icon_vesicle_export.png"),
];

fn mol_icon(currency: Currency) -> Option<&'static str> {
    MOL_ICONS.iter().find(|(c, _)| *c == currency).map(|(_, p)| *p)
}

fn pathway_icon(kind: BlockKind) -> Option<&'static str> {
    PATHWAY_ICONS.iter().find(|(k, _)| *k == kind).map(|(_, p)| *p)
}

/// The tint of a pathway: its stand-in icon and its enzymes.
fn accent(kind: BlockKind) -> Color {
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
struct PlateFrame(BlockKind);

#[derive(Component)]
struct PlateIcon(BlockKind);

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
struct Badge(BlockKind);

#[derive(Component)]
struct Valve(BlockKind);

#[derive(Component)]
struct RibosomeText;

/// A pipe. Its sprites are `segments`.
#[derive(Component)]
struct Pipe {
    route: Vec<Vec2>,
    length: f32,
    segments: Vec<Entity>,
    kind: EdgeKind,
    color: Color,
    /// The icon of the molecules on it.
    speck_image: Option<Handle<Image>>,
    /// What the pipe shows now.
    flux: f32,
    limiting: bool,
    accum: f32,
}

#[derive(Component)]
struct PipeSegment;

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
}

fn reset_index(mut index: ResMut<PipeIndex>) {
    *index = PipeIndex::default();
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

fn spawn_map(mut commands: Commands, server: Res<AssetServer>, sheets: Res<Sheets>) {
    // Pools.
    for currency in Currency::ALL {
        let place = layout::pool_place(currency);
        commands
            .spawn((
                Name::new(format!("Pool {currency:?}")),
                Transform::from_translation(place.pos.extend(Z_NODE)),
                Visibility::default(),
                map_layer(),
                DespawnOnExit(GameState::Manage),
            ))
            .with_children(|pool| {
                let mut icon = pool.spawn((
                    PoolIcon(currency),
                    icon_sprite(&server, mol_icon(currency), currency_color(currency), Vec2::splat(12.0)),
                    Transform::from_xyz(0.0, 3.0, 0.5),
                    map_layer(),
                ));
                make_pickable(&mut icon, Pick::Pool(currency));
                // The gauge: a dark track and a fill.
                pool.spawn((
                    Sprite::from_color(Color::srgb(0.12, 0.13, 0.2), Vec2::new(20.0, 3.0)),
                    Transform::from_xyz(0.0, -5.0, 0.4),
                    map_layer(),
                ));
                pool.spawn((
                    PoolGauge(currency),
                    Sprite::from_color(currency_color(currency), Vec2::new(18.0, 2.0)),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(-9.0, -5.0, 0.5),
                    map_layer(),
                ));
                pool.spawn((
                    world_text(layout::pool_label(currency), 6.0, Color::srgb(0.75, 0.78, 0.85)),
                    text_at(0.0, -10.5, 0.6),
                ));
            });
    }

    // Plates.
    for kind in layout::mapped_kinds() {
        let Some(place) = layout::plate_place(kind) else {
            continue;
        };
        commands
            .spawn((
                Name::new(format!("Plate {kind:?}")),
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
                    icon_sprite(&server, pathway_icon(kind), accent(kind), Vec2::splat(14.0)),
                    Transform::from_xyz(-place.size.x / 2.0 + 12.0, 5.0, Z_ICON - Z_NODE),
                    map_layer(),
                ));
                plate.spawn((
                    PlateLabel(kind),
                    world_text("", 6.0, Color::srgb(0.85, 0.88, 0.95)),
                    Anchor::CENTER_LEFT,
                    text_at(-place.size.x / 2.0 + 21.0, 5.0, Z_TEXT - Z_NODE),
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
                        Transform::from_xyz(0.0, -8.0, Z_ICON - Z_NODE),
                        Visibility::Hidden,
                        map_layer(),
                    ));
                }
                plate.spawn((
                    EnzymeMore(kind),
                    world_text("", 7.0, Color::srgb(0.85, 0.88, 0.95)),
                    text_at(14.0, -8.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                ));
                plate.spawn((
                    Badge(kind),
                    Sprite {
                        image: server.load("sprites/badge_limit.png"),
                        custom_size: Some(Vec2::splat(11.0)),
                        ..default()
                    },
                    Transform::from_xyz(-place.size.x / 2.0, place.size.y / 2.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                    map_layer(),
                ));
                plate.spawn((
                    Valve(kind),
                    Sprite {
                        image: server.load("sprites/valve_high.png"),
                        custom_size: Some(Vec2::splat(11.0)),
                        ..default()
                    },
                    Transform::from_xyz(place.size.x / 2.0 - 2.0, place.size.y / 2.0 - 1.0, Z_TEXT - Z_NODE),
                    Visibility::Hidden,
                    map_layer(),
                ));
            });
    }

    // The ribosome and the text of the queue.
    let ribosome = layout::ribosome_place();
    if let Some(bundle) = sheets.bundle("ribosome", "loop") {
        commands.spawn((
            Name::new("Ribosome"),
            bundle,
            Transform::from_translation(ribosome.pos.extend(Z_NODE)),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
    }
    commands.spawn((
        RibosomeText,
        world_text("", 7.0, Color::srgb(0.85, 0.88, 0.95)),
        Anchor::CENTER_LEFT,
        text_at(ribosome.pos.x + 18.0, ribosome.pos.y, Z_TEXT),
        DespawnOnExit(GameState::Manage),
    ));
}

// --- Pools ---

fn update_pools(
    time: Res<Time>,
    view: Res<LocalMetabolismView>,
    mut gauges: Query<(&PoolGauge, &mut Sprite), Without<PoolIcon>>,
    mut icons: Query<(&PoolIcon, &mut Sprite), Without<PoolGauge>>,
) {
    let Some(view) = &view.0 else { return };
    let flash = 0.5 + 0.5 * (time.elapsed_secs() * 8.0).sin();
    for (gauge, mut sprite) in &mut gauges {
        let level = view.pool(gauge.0).map_or(0.0, |p| p.level);
        sprite.custom_size = Some(Vec2::new(18.0 * gauge_fraction(level), 2.0));
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
}

// --- Plates ---

#[allow(clippy::type_complexity)]
fn update_plates(
    time: Res<Time>,
    server: Res<AssetServer>,
    view: Res<LocalMetabolismView>,
    mut frames: Query<(&PlateFrame, &mut Sprite)>,
    mut icons: Query<(&PlateIcon, &mut Sprite), Without<PlateFrame>>,
    mut labels: Query<(&PlateLabel, &mut Text2d), Without<EnzymeMore>>,
    mut enzymes: Query<
        (&EnzymeSprite, &mut Transform, &mut Visibility),
        (Without<EnzymeMore>, Without<Badge>, Without<Valve>),
    >,
    mut more: Query<(&EnzymeMore, &mut Text2d, &mut Visibility), (Without<PlateLabel>, Without<EnzymeSprite>, Without<Badge>, Without<Valve>)>,
    mut badges: Query<(&Badge, &mut Visibility, &mut Sprite), (Without<EnzymeSprite>, Without<EnzymeMore>, Without<Valve>, Without<PlateFrame>, Without<PlateIcon>)>,
    mut valves: Query<(&Valve, &mut Visibility, &mut Sprite), (Without<EnzymeSprite>, Without<EnzymeMore>, Without<Badge>, Without<PlateFrame>, Without<PlateIcon>)>,
) {
    let Some(view) = &view.0 else { return };
    let t = time.elapsed_secs();
    let pulse = 0.5 + 0.5 * (t * 6.0).sin();
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
    for (icon, mut sprite) in &mut icons {
        let Some(p) = view.pathway(icon.0) else { continue };
        let alpha = if p.owned { 1.0 } else { 0.4 };
        sprite.color = if pathway_icon(icon.0).is_some() {
            Color::srgba(1.0, 1.0, 1.0, alpha)
        } else {
            accent(icon.0).with_alpha(alpha)
        };
    }
    for (label, mut text) in &mut labels {
        let Some(p) = view.pathway(label.0) else { continue };
        let want = if p.known { short_name(label.0) } else { "?" };
        if text.0 != want {
            text.0 = want.to_string();
        }
    }
    for (enzyme, mut transform, mut visibility) in &mut enzymes {
        let Some(p) = view.pathway(enzyme.kind) else { continue };
        let shown = enzymes_shown(p.enzymes);
        let on = p.owned && enzyme.index < shown;
        *visibility = if on { Visibility::Inherited } else { Visibility::Hidden };
        if on {
            let x = (enzyme.index as f32 - (shown as f32 - 1.0) / 2.0) * ENZYME_PITCH
                - if p.enzymes as usize > MAX_ENZYME_SPRITES { 8.0 } else { 0.0 };
            // The bob: faster and higher with the activity; still at 0.
            let omega = 2.0 + 10.0 * p.activity;
            let phase = enzyme.index as f32 * 0.9;
            let bob = (t * omega + phase).sin() * 1.6 * p.activity.sqrt();
            transform.translation.x = x;
            transform.translation.y = (-8.0 + bob).round();
        }
    }
    for (m, mut text, mut visibility) in &mut more {
        let Some(p) = view.pathway(m.0) else { continue };
        let on = p.owned && p.enzymes as usize > MAX_ENZYME_SPRITES;
        *visibility = if on { Visibility::Inherited } else { Visibility::Hidden };
        let want = format!("x{}", p.enzymes);
        if on && text.0 != want {
            text.0 = want;
        }
    }
    for (badge, mut visibility, mut sprite) in &mut badges {
        let Some(p) = view.pathway(badge.0) else { continue };
        let on = shows_badge(p);
        *visibility = if on { Visibility::Inherited } else { Visibility::Hidden };
        sprite.color = Color::WHITE.mix(&BAD, pulse * 0.6);
    }
    for (valve, mut visibility, mut sprite) in &mut valves {
        let Some(p) = view.pathway(valve.0) else { continue };
        *visibility = if p.owned { Visibility::Inherited } else { Visibility::Hidden };
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

// --- Pipes ---

fn key_hash(key: &EdgeKey) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize
}

/// The route and look of the pipe of an edge, or `None` if the map has no pipe
/// for it.
fn pipe_route(edge: &EdgeView, stubs: &mut HashMap<Currency, usize>) -> Option<Vec<Vec2>> {
    let key: EdgeKey = (edge.from, edge.to, edge.currency);
    let slot = key_hash(&key) % 4;
    match (edge.from, edge.to) {
        (Node::Pool(c), Node::Pathway(k)) => {
            Some(layout::route(layout::pool_place(c), layout::plate_place(k)?, slot))
        }
        (Node::Pathway(k), Node::Pool(c)) => {
            Some(layout::route(layout::plate_place(k)?, layout::pool_place(c), slot))
        }
        (Node::Outside(channel), Node::Pathway(k)) => match channel {
            // Minerals and oxygen come in through the pools.
            Channel::Heat | Channel::Light => {
                Some(layout::route_from_band(layout::plate_place(k)?, 22.0))
            }
            _ => None,
        },
        (Node::Source(FlowSource::Intake), Node::Pool(c)) => {
            Some(layout::route_from_band(layout::pool_place(c), -5.0))
        }
        (Node::Pool(c), Node::Source(FlowSource::Export)) => {
            Some(layout::route_to_band(layout::pool_place(c), 5.0))
        }
        (Node::Source(_), Node::Pool(c)) => {
            let index = stubs.entry(c).or_default();
            let route = layout::stub_route(layout::pool_place(c), *index, true);
            *index += 1;
            Some(route)
        }
        (Node::Pool(c), Node::Source(_)) => {
            let index = stubs.entry(c).or_default();
            let route = layout::stub_route(layout::pool_place(c), *index, false);
            *index += 1;
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
    edge: &EdgeView,
    route: Vec<Vec2>,
) -> Entity {
    let color = edge_color(edge);
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
                    Sprite::from_color(color, Vec2::ONE),
                    Transform::default(),
                    map_layer(),
                ))
                .id(),
        );
    }
    let length = layout::route_length(&route);
    let pipe = commands
        .spawn((
            Name::new("Pipe"),
            Pipe {
                route: route.clone(),
                length,
                segments: segments.clone(),
                kind: edge.kind,
                color,
                speck_image,
                flux: 0.0,
                limiting: false,
                accum: 0.0,
            },
            Transform::default(),
            Visibility::default(),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ))
        .add_children(&segments)
        .id();
    // The label icon at the far end of a stub.
    if let (EdgeKind::Other, Some(currency)) = (edge.kind, edge.currency) {
        if let (Node::Source(s), _) | (_, Node::Source(s)) = (edge.from, edge.to) {
            let on_band = matches!(s, FlowSource::Intake | FlowSource::Export);
            if !on_band {
                let pool = layout::pool_place(currency);
                let index = route_stub_index(&route, pool);
                let pos = layout::stub_icon_pos(pool, index);
                commands.entity(pipe).with_children(|p| {
                    p.spawn((
                        Sprite::from_color(source_color(s), Vec2::splat(8.0)),
                        Transform::from_xyz(pos.x, pos.y, Z_ICON),
                        map_layer(),
                    ));
                    p.spawn((
                        world_text(source_letter(s), 6.0, Color::srgb(0.05, 0.06, 0.1)),
                        text_at(pos.x, pos.y, Z_TEXT),
                    ));
                });
            }
        }
    }
    pipe
}

/// The index of a stub from its route (the y offset from the pool).
fn route_stub_index(route: &[Vec2], pool: Place) -> usize {
    let y = route[0].y;
    ((pool.pos.y + 7.0 - y) / 7.0).round().max(0.0) as usize
}

fn sync_pipes(
    mut commands: Commands,
    server: Res<AssetServer>,
    view: Res<LocalMetabolismView>,
    mut index: ResMut<PipeIndex>,
    mut pipes: Query<&mut Pipe>,
) {
    let Some(view) = &view.0 else { return };
    for mut pipe in &mut pipes {
        pipe.flux = 0.0;
        pipe.limiting = false;
    }
    for edge in &view.edges {
        let key: EdgeKey = (edge.from, edge.to, edge.currency);
        let entity = match index.pipes.get(&key) {
            Some(entity) => *entity,
            None => {
                let route = pipe_route(edge, &mut index.stubs);
                let entity = route.map(|r| spawn_pipe(&mut commands, &server, edge, r));
                index.pipes.insert(key, entity);
                entity
            }
        };
        let Some(entity) = entity else { continue };
        if let Ok(mut pipe) = pipes.get_mut(entity) {
            pipe.flux = shown_flux(edge, view);
            pipe.limiting = is_limiting(edge, view);
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
    time: Res<Time>,
    pipes: Query<&Pipe>,
    mut segments: Query<(&mut Sprite, &mut Transform), With<PipeSegment>>,
) {
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 6.0).sin();
    for pipe in &pipes {
        let mut width = match pipe.kind {
            EdgeKind::Cofactor => 1.0,
            _ => width_tier(pipe.flux),
        };
        let mut color = match pipe.kind {
            // A cofactor is a thin dim pin: needed, not used up.
            EdgeKind::Cofactor => pipe.color.with_alpha(0.4),
            _ if pipe.flux > 0.0 => pipe.color.with_alpha(0.75),
            _ => pipe.color.with_alpha(0.28),
        };
        if pipe.limiting {
            width = width.max(2.0);
            color = color.with_alpha(1.0).mix(&BAD, 0.4 + 0.6 * pulse);
        }
        for (i, &entity) in pipe.segments.iter().enumerate() {
            let Ok((mut sprite, mut transform)) = segments.get_mut(entity) else {
                continue;
            };
            let (a, b) = (pipe.route[i], pipe.route[i + 1]);
            let size = if (a.y - b.y).abs() < 1e-3 {
                Vec2::new((a.x - b.x).abs() + width, width)
            } else {
                Vec2::new(width, (a.y - b.y).abs() + width)
            };
            sprite.custom_size = Some(size);
            sprite.color = color;
            transform.translation = ((a + b) / 2.0).round().extend(Z_PIPE);
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
        let rate = speck_rate(pipe.flux);
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
        let rate = view
            .0
            .as_ref()
            .map_or(0.0, |v| protein_rate(v));
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
            assert_ne!(short_name(kind), "?", "{kind:?}");
            assert!(short_name(kind).len() <= 10, "{kind:?}");
        }
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
        for (_, path) in PATHWAY_ICONS {
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
        let mut out = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        }
        // The pool names that have a different file name.
        if out == "a_t_p" { "atp".into() } else { out }
    }
}
