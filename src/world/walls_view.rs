//! # Tissue walls in the world
//!
//! Each tile of the `WallMap` is a static square body (32 x 32) with a sprite.
//! `wall_membrane` is 16 x 26 and tiles sideways, so a tile of wall is two
//! pieces; `wall_membrane_pore` is 32 x 26, one piece. The art has no vertical
//! piece yet (see the art doc, "Not done yet"), so a tile in a vertical run is
//! turned by 90 degrees. The turn keeps the pixel grid, but it also turns the
//! light of the sprite: **art owner** should draw a vertical piece.
//!
//! The collision layers say who passes (see `world::cell_layers`): a wall
//! blocks every cell, a pore blocks a cell without a transporter.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::envgrid::{CELL_SIZE, EnvGrid};
use crate::walls::{TileLysed, WallKind, WallMap, tile_center};
use crate::world::Layer;
use crate::InRun;

/// Z of the wall sprites: above the grid, below the creatures.
const WALL_Z: f32 = -4.0;
/// Width of a `wall_membrane` piece along the wall.
const PIECE_WIDTH: f32 = 16.0;

/// A wall tile body.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallTile(pub (usize, usize));

pub fn plugin(app: &mut App) {
    app.add_systems(Update, remove_lysed_tiles.run_if(in_state(InRun)));
}

/// True if a tile is part of a vertical run: it has a neighbour above or below
/// and none to the left or right.
pub fn is_vertical(map: &WallMap, tile: (usize, usize)) -> bool {
    let (x, y) = tile;
    let has = |dx: isize, dy: isize| {
        let (nx, ny) = (x as isize + dx, y as isize + dy);
        nx >= 0 && ny >= 0 && map.get((nx as usize, ny as usize)).is_some()
    };
    !(has(-1, 0) || has(1, 0)) && (has(0, -1) || has(0, 1))
}

/// Spawns the bodies and sprites of all tiles.
pub fn spawn_walls(
    commands: &mut Commands,
    asset_server: &AssetServer,
    map: &WallMap,
    grid: &EnvGrid,
) {
    let wall = asset_server.load("sprites/wall_membrane.png");
    let pore = asset_server.load("sprites/wall_membrane_pore.png");
    for (tile, kind) in map.tiles() {
        let centre = tile_center(grid.origin(), tile).round();
        let layer = match kind {
            WallKind::Wall => Layer::Wall,
            WallKind::Pore => Layer::Pore,
        };
        let vertical = is_vertical(map, tile);
        let rotation = if vertical {
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
        } else {
            Quat::IDENTITY
        };
        let mut entity = commands.spawn((
            Name::new(format!("{kind:?} tile")),
            WallTile(tile),
            RigidBody::Static,
            Collider::rectangle(CELL_SIZE, CELL_SIZE),
            CollisionLayers::new(layer, [Layer::Cell]),
            Transform::from_translation(centre.extend(0.0)),
            Visibility::default(),
            DespawnOnExit(InRun),
        ));
        match kind {
            WallKind::Pore => {
                entity.with_child((
                    Sprite::from_image(pore.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, WALL_Z)).with_rotation(rotation),
                ));
            }
            WallKind::Wall => {
                for side in [-0.5, 0.5] {
                    // Along the wall: x for a horizontal tile, y for a vertical one.
                    let offset = side * PIECE_WIDTH;
                    let position = if vertical { Vec3::new(0.0, offset, WALL_Z) } else { Vec3::new(offset, 0.0, WALL_Z) };
                    entity.with_child((
                        Sprite::from_image(wall.clone()),
                        Transform::from_translation(position).with_rotation(rotation),
                    ));
                }
            }
        }
    }
}

/// A tile that was cut loses its body and sprite.
fn remove_lysed_tiles(
    mut cut: MessageReader<TileLysed>,
    tiles: Query<(Entity, &WallTile)>,
    mut commands: Commands,
) {
    for TileLysed(tile) in cut.read() {
        for (entity, wall) in &tiles {
            if wall.0 == *tile {
                commands.entity(entity).despawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_with_a_vertical_neighbour_only_is_vertical() {
        let mut map = WallMap::default();
        for y in 0..4 {
            map.insert((3, y), WallKind::Wall);
        }
        for x in 5..9 {
            map.insert((x, 0), WallKind::Wall);
        }
        assert!(is_vertical(&map, (3, 2)));
        assert!(!is_vertical(&map, (6, 0)));
        assert!(!is_vertical(&map, (20, 20)), "a lone tile is horizontal");
    }

    // --- Physics: who passes a wall or a pore ---

    use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};
    use crate::world::cell_layers;
    use std::time::Duration;

    /// A body of radius 20 starts left of a tile at x = 100 and is pushed east.
    /// Returns its x after two seconds.
    fn push_through(kind: WallKind, proteome: &Proteome, ghosting: bool) -> f32 {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::diagnostic::DiagnosticsPlugin,
            PhysicsPlugins::default().with_length_unit(20.0),
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 64.0),
        ));
        let layer = match kind {
            WallKind::Wall => Layer::Wall,
            WallKind::Pore => Layer::Pore,
        };
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::rectangle(CELL_SIZE, CELL_SIZE),
            CollisionLayers::new(layer, [Layer::Cell]),
            Transform::from_xyz(100.0, 0.0, 0.0),
        ));
        let body = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::circle(20.0),
                Mass(1.0),
                LinearDamping(3.0),
                LockedAxes::ROTATION_LOCKED,
                ConstantForce(Vec2::new(300.0, 0.0)),
                cell_layers(proteome, ghosting),
                Transform::from_xyz(40.0, 0.0, 0.0),
            ))
            .id();
        app.finish();
        app.cleanup();
        for _ in 0..140 {
            app.update();
        }
        app.world().get::<Transform>(body).unwrap().translation.x
    }

    #[test]
    fn walls_stop_a_cell_pores_open_for_a_transporter_and_an_adhesin_crawl_passes() {
        let bare = Proteome::default();
        let mut transporter = Proteome::default();
        transporter.add(ProteinTarget::Membrane(MembraneProtein::Transporter));
        // The tile spans x 84..116: a body of radius 20 stops at x = 64.
        let wall = push_through(WallKind::Wall, &bare, false);
        assert!(wall < 66.0, "a wall blocks: {wall}");
        assert!(push_through(WallKind::Wall, &transporter, false) < 66.0, "a transporter does not open a wall");
        let pore = push_through(WallKind::Pore, &bare, false);
        assert!(pore < 66.0, "a pore is solid without a transporter: {pore}");
        assert!(push_through(WallKind::Pore, &transporter, false) > 130.0, "a transporter passes a pore");
        assert!(push_through(WallKind::Wall, &bare, true) > 130.0, "an adhesin crawl passes a wall");
    }
}
