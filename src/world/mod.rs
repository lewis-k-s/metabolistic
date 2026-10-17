//! # The world of a run
//!
//! The world is the level of the current stage (`LevelDef`) and the player's cell. It exists while `InRun`
//! is active: it is spawned when a run starts, it stays when the player
//! switches between Run and Manage, and it is removed at death or when the
//! player goes back to the menu. Every world entity has
//! `DespawnOnExit(InRun)`, and the `EnvGrid` resource is removed on exit.
//!
//! - `grid_view`: draws the grid as one texture, plus the vents and the
//!   mineral particles.
//! - `player_body`: the physics body, thrust, camera and blob look of the
//!   player's cell. The cell plays baked sprite sheets.
//! - `actions_view`: slot hotkeys, toxin droplets, membrane protein overlays,
//!   shelter discs.
//! - `pilus_view`: the pilus joint and its line sprite.
//! - `walls_view`: tissue tile bodies and sprites.
//! - `interior_view`: interior pieces of a cell (genome and pathways), display mode key.
//! - `npc_view`: NPC bodies, particle sprites, death, engulf and victory clips.
//! - `hud`: ATP, membrane, genome, UV and pool bars, gate progress.
//! - `shader_cells`: spike, off by default: the player's body as an SDF shader
//!   on a low-res canvas (F7).

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::genome_damage::UvFraction;
use crate::level::{CurrentLevel, LevelDef};
use crate::run::RunState;
use crate::npc::{KeyGeneRule, NpcRng, NpcSlots, RespawnTimers, spawn_npc, spawn_slot};
use crate::particles::{
    ParticleTimers, ParticleZones, point_in_circle, spawn_food, spawn_metal, spawn_virus,
    VIRUS_SPEED,
};
use crate::proteins::Proteome;
use crate::shared::spawn_cell_from_carry;
use crate::shelter::Shelters;
use crate::sprites::Sheets;
use crate::walls::{WallMap, solid_for, WallKind};
use crate::InRun;
use rand::prelude::*;

pub mod actions_view;
pub mod grid_view;
pub mod hud;
pub mod interior_view;
pub mod npc_view;
pub mod pilus_view;
pub mod player_body;
pub mod shader_cells;
pub mod walls_view;

/// Collision layers of the world. Cells collide with the field walls and vents
/// (`Default`), other cells, and the tissue (see `cell_layers`).
#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum Layer {
    #[default]
    Default,
    Cell,
    Wall,
    Pore,
}

/// The layers of a cell body: what it collides with follows its proteome and
/// whether it crawls through walls (see `walls::solid_for`).
pub fn cell_layers(proteome: &Proteome, ghosting: bool) -> CollisionLayers {
    let mut filters = Layer::Default.to_bits() | Layer::Cell.to_bits();
    if solid_for(WallKind::Wall, proteome, ghosting) {
        filters |= Layer::Wall.to_bits();
    }
    if solid_for(WallKind::Pore, proteome, ghosting) {
        filters |= Layer::Pore.to_bits();
    }
    CollisionLayers::new(Layer::Cell, LayerMask(filters))
}

/// Deep-water background colour (the art doc's `WATER` ramp). A level sets its own
/// at the start (`FieldKind::background`: dry earth on land).
const WATER_COLOR: Color = Color::srgb(0.039, 0.071, 0.149);

/// Thickness of the invisible walls around the field.
const WALL_THICKNESS: f32 = 200.0;

pub fn plugin(app: &mut App) {
    app.add_plugins(PhysicsPlugins::default().with_length_unit(20.0))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(ClearColor(WATER_COLOR))
        .add_plugins((
            grid_view::plugin,
            player_body::plugin,
            pilus_view::plugin,
            hud::plugin,
            actions_view::plugin,
            interior_view::plugin,
            walls_view::plugin,
            npc_view::plugin,
            shader_cells::plugin,
        ))
        .add_systems(OnEnter(InRun), spawn_world)
        .add_systems(OnExit(InRun), (remove_grid, reset_background));
}

#[allow(clippy::too_many_arguments)]
fn spawn_world(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sheets: Res<Sheets>,
    mut images: ResMut<Assets<Image>>,
    mut rng: ResMut<NpcRng>,
    run: Res<RunState>,
) {
    // The stage decides the level. A stage without a level is never entered
    // (the run completes instead); the base vents are the safe fallback.
    let def = run.level_def().unwrap_or_else(|| {
        warn!("the stage {:?} has no level; using the base vents", run.stage());
        LevelDef::vents_base()
    });
    let grid = def.grid();
    let [r, g, b] = def.field.background();
    commands.insert_resource(ClearColor(Color::srgb(r, g, b)));
    let world_size = grid.world_size();
    let origin = grid.origin();

    grid_view::spawn_grid_view(&mut commands, &grid, grid_view::GridStyle::for_field(def.field));
    for vent in def.vents.iter().copied() {
        grid_view::spawn_vent(&mut commands, &asset_server, vent);
    }
    grid_view::spawn_mineral_particles(&mut commands, &asset_server, &grid);
    spawn_field_walls(&mut commands, origin, world_size);
    let walls = def.wall_map();
    walls_view::spawn_walls(&mut commands, &asset_server, &walls, &grid);
    commands.insert_resource(walls);
    commands.insert_resource(grid);
    commands.insert_resource(UvFraction(def.uv_fraction));
    for shelter in def.shelters.iter().copied() {
        actions_view::spawn_shelter(&mut commands, &mut images, shelter);
    }
    commands.insert_resource(Shelters(def.shelters.clone()));

    // The creatures, viruses and particles of the fixed layout.
    let slots = NpcSlots(
        def.npcs
            .iter()
            .map(|n| (n.archetype, n.position, n.signature))
            .collect(),
    );
    for slot in 0..slots.0.len() {
        spawn_slot(&mut commands, &slots, slot);
    }
    commands.insert_resource(slots);
    commands.insert_resource(RespawnTimers::default());
    commands.insert_resource(KeyGeneRule(def.key_gene.map(|k| (k.gene, k.count))));
    let gate = spawn_npc(
        &mut commands,
        crate::npc::Archetype::Gate,
        def.gate_position,
        Some(def.gate_gene),
        def.npcs.len() as u32,
    );
    commands.entity(gate).insert((
        crate::gate::GateProgress::default(),
        crate::gate::GateSprite(def.gate_sprite),
    ));
    for (index, position) in def.viruses.iter().enumerate() {
        let angle = index as f32 * 2.1;
        spawn_virus(&mut commands, *position, Vec2::new(angle.cos(), angle.sin()) * VIRUS_SPEED);
    }
    for _ in 0..def.initial_food {
        let (centre, radius) = def.food_zones[rng.0.random_range(0..def.food_zones.len())];
        spawn_food(&mut commands, point_in_circle(centre, radius, &mut rng.0), crate::particles::FOOD_CARBON);
    }
    for _ in 0..def.initial_metals {
        let (centre, radius) = def.metal_zones[rng.0.random_range(0..def.metal_zones.len())];
        spawn_metal(&mut commands, point_in_circle(centre, radius, &mut rng.0));
    }
    commands.insert_resource(ParticleZones {
        food: def.food_zones.clone(),
        metals: def.metal_zones.clone(),
    });
    commands.insert_resource(ParticleTimers::default());

    let (player, cell) = spawn_cell_from_carry(&mut commands, run.spawn.unwrap_or(def.start), run.cell_carry());
    commands.entity(player).insert(DespawnOnExit(InRun));
    player_body::attach_body(&mut commands, &sheets, cell);
    commands.insert_resource(CurrentLevel(def));
}

fn reset_background(mut commands: Commands) {
    commands.insert_resource(ClearColor(WATER_COLOR));
}

fn remove_grid(mut commands: Commands) {
    commands.remove_resource::<crate::envgrid::EnvGrid>();
    commands.remove_resource::<CurrentLevel>();
    commands.insert_resource(Shelters::default());
    commands.insert_resource(WallMap::default());
    commands.insert_resource(ParticleZones::default());
    commands.insert_resource(NpcSlots::default());
    commands.insert_resource(KeyGeneRule::default());
}

/// Four invisible walls keep the player inside the field.
fn spawn_field_walls(commands: &mut Commands, origin: Vec2, size: Vec2) {
    let center = origin + size * 0.5;
    let t = WALL_THICKNESS;
    let walls = [
        // (offset from the field centre, size)
        (Vec2::new(0.0, size.y * 0.5 + t * 0.5), Vec2::new(size.x + 2.0 * t, t)),
        (Vec2::new(0.0, -size.y * 0.5 - t * 0.5), Vec2::new(size.x + 2.0 * t, t)),
        (Vec2::new(size.x * 0.5 + t * 0.5, 0.0), Vec2::new(t, size.y)),
        (Vec2::new(-size.x * 0.5 - t * 0.5, 0.0), Vec2::new(t, size.y)),
    ];
    for (offset, wall_size) in walls {
        commands.spawn((
            Name::new("Field Wall"),
            RigidBody::Static,
            Collider::rectangle(wall_size.x, wall_size.y),
            Transform::from_translation((center + offset).extend(0.0)),
            DespawnOnExit(InRun),
        ));
    }
}
