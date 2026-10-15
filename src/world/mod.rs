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
//! - `hud`: ATP, membrane and pool bars.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::level::{self, VENTS};
use crate::shared::spawn_starter_cell_at_start;
use crate::sprites::Sheets;
use crate::InRun;

pub mod grid_view;
pub mod hud;
pub mod player_body;

/// Deep-water background colour (the art doc's `WATER` ramp).
const WATER_COLOR: Color = Color::srgb(0.039, 0.071, 0.149);

/// Thickness of the invisible walls around the field.
const WALL_THICKNESS: f32 = 200.0;

pub fn plugin(app: &mut App) {
    app.add_plugins(PhysicsPlugins::default().with_length_unit(20.0))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(ClearColor(WATER_COLOR))
        .add_plugins((grid_view::plugin, player_body::plugin, hud::plugin))
        .add_systems(OnEnter(InRun), spawn_world)
        .add_systems(OnExit(InRun), remove_grid);
}

fn spawn_world(mut commands: Commands, asset_server: Res<AssetServer>, sheets: Res<Sheets>) {
    let grid = level::vent_field();
    let world_size = grid.world_size();
    let origin = grid.origin();

    grid_view::spawn_grid_view(&mut commands, &grid);
    for vent in VENTS {
        grid_view::spawn_vent(&mut commands, &asset_server, vent);
    }
    grid_view::spawn_mineral_particles(&mut commands, &asset_server, &grid);
    spawn_field_walls(&mut commands, origin, world_size);
    commands.insert_resource(grid);

    let (player, cell) = spawn_starter_cell_at_start(&mut commands);
    commands.entity(player).insert(DespawnOnExit(InRun));
    player_body::attach_body(&mut commands, &sheets, cell);
}

fn remove_grid(mut commands: Commands) {
    commands.remove_resource::<crate::envgrid::EnvGrid>();
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
