//! # The world of a run
//!
//! The world is the vent field and the player's cell. It exists while `InRun`
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
//! - `walls_view`: tissue tile bodies and sprites.
//! - `npc_view`: NPC bodies, particle sprites, death, engulf and victory clips.
//! - `hud`: ATP, membrane, genome, UV and pool bars, gate progress.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::genome_damage::UvFraction;
use crate::level::{self, VENTS};
use crate::npc::{NpcRng, NpcSlots, RespawnTimers, spawn_npc, spawn_slot};
use crate::particles::{
    ParticleTimers, ParticleZones, point_in_circle, spawn_food, spawn_metal, spawn_virus,
    VIRUS_SPEED,
};
use crate::proteins::Proteome;
use crate::shared::spawn_starter_cell_at_start;
use crate::shelter::Shelters;
use crate::sprites::Sheets;
use crate::walls::{WallMap, solid_for, WallKind};
use crate::InRun;
use rand::prelude::*;

pub mod actions_view;
pub mod grid_view;
pub mod hud;
pub mod npc_view;
pub mod player_body;
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

/// Deep-water background colour (the art doc's `WATER` ramp).
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
            hud::plugin,
            actions_view::plugin,
            walls_view::plugin,
            npc_view::plugin,
        ))
        .add_systems(OnEnter(InRun), spawn_world)
        .add_systems(OnExit(InRun), remove_grid);
}

#[allow(clippy::too_many_arguments)]
fn spawn_world(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sheets: Res<Sheets>,
    mut images: ResMut<Assets<Image>>,
    mut rng: ResMut<NpcRng>,
) {
    let grid = level::vent_field();
    let world_size = grid.world_size();
    let origin = grid.origin();

    grid_view::spawn_grid_view(&mut commands, &grid);
    for vent in VENTS {
        grid_view::spawn_vent(&mut commands, &asset_server, vent);
    }
    grid_view::spawn_mineral_particles(&mut commands, &asset_server, &grid);
    spawn_field_walls(&mut commands, origin, world_size);
    let walls = level::wall_map();
    walls_view::spawn_walls(&mut commands, &asset_server, &walls, &grid);
    commands.insert_resource(walls);
    commands.insert_resource(grid);
    commands.insert_resource(UvFraction(level::UV_FRACTION));
    for shelter in level::SHELTERS {
        actions_view::spawn_shelter(&mut commands, &mut images, shelter);
    }
    commands.insert_resource(Shelters(level::SHELTERS.to_vec()));

    // The creatures, viruses and particles of the fixed layout.
    let slots = NpcSlots(
        level::NPC_SPAWNS
            .iter()
            .map(|n| (n.archetype, n.position, n.signature))
            .collect(),
    );
    for slot in 0..slots.0.len() {
        spawn_slot(&mut commands, &slots, slot);
    }
    commands.insert_resource(slots);
    commands.insert_resource(RespawnTimers::default());
    let gate = spawn_npc(
        &mut commands,
        crate::npc::Archetype::Gate,
        level::GATE_POSITION,
        None,
        level::NPC_SPAWNS.len() as u32,
    );
    commands.entity(gate).insert(crate::gate::GateProgress::default());
    for (index, position) in level::VIRUS_SPAWNS.iter().enumerate() {
        let angle = index as f32 * 2.1;
        spawn_virus(&mut commands, *position, Vec2::new(angle.cos(), angle.sin()) * VIRUS_SPEED);
    }
    for _ in 0..level::INITIAL_FOOD {
        let (centre, radius) = level::FOOD_ZONES[rng.0.random_range(0..level::FOOD_ZONES.len())];
        spawn_food(&mut commands, point_in_circle(centre, radius, &mut rng.0), crate::particles::FOOD_CARBON);
    }
    for _ in 0..level::INITIAL_METALS {
        let (centre, radius) = level::METAL_ZONES[rng.0.random_range(0..level::METAL_ZONES.len())];
        spawn_metal(&mut commands, point_in_circle(centre, radius, &mut rng.0));
    }
    commands.insert_resource(ParticleZones {
        food: level::FOOD_ZONES.to_vec(),
        metals: level::METAL_ZONES.to_vec(),
    });
    commands.insert_resource(ParticleTimers::default());

    let (player, cell) = spawn_starter_cell_at_start(&mut commands);
    commands.entity(player).insert(DespawnOnExit(InRun));
    player_body::attach_body(&mut commands, &sheets, cell);
}

fn remove_grid(mut commands: Commands) {
    commands.remove_resource::<crate::envgrid::EnvGrid>();
    commands.insert_resource(Shelters::default());
    commands.insert_resource(WallMap::default());
    commands.insert_resource(ParticleZones::default());
    commands.insert_resource(NpcSlots::default());
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
