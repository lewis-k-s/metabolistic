#[cfg(feature = "full")]
use avian3d::prelude::*;
use bevy::prelude::*;
#[cfg(feature = "full")]
use leafwing_input_manager::prelude::*;
#[cfg(feature = "full")]
use std::f32::consts::PI;

use crate::blocks::genome::Genome;
use crate::metabolism::CurrencyPools;

#[cfg(feature = "full")]
pub mod controller;

/// Core player component with network identifier
/// client_id: 0 for single-player, unique ID for multiplayer
#[derive(Component)]
pub struct Player {
    pub client_id: u32,
}

/// Marker component for the local player (the one controlled by this client)
#[derive(Component)]
pub struct LocalPlayer;

/// Component linking a player to their controlled cell
#[derive(Component)]
pub struct ControlledCell(pub Entity);

#[cfg(feature = "full")]
pub struct PlayerPlugin;

#[cfg(feature = "full")]
impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(controller::CharacterControllerPlugin)
            .add_systems(Startup, spawn_player);
    }
}

#[cfg(feature = "full")]
pub fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let player_transform = Transform::from_xyz(0.0, 1.0, 0.0);
    let radius = 0.5;

    // The physics avatar. The local player's cell is a separate entity
    // (see `spawn_local_player_cell`).
    commands.spawn((
        Player { client_id: 0 },
        Mesh3d(meshes.add(Sphere::new(radius).mesh())),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
        player_transform,
        InputManagerBundle::with_map(controller::Action::input_map()),
        controller::CharacterControllerBundle::new(Collider::sphere(radius)).with_movement(
            0.5,
            5.0,
            7.0,
            PI * 0.45,
        ),
    ));
}

// === Player-Cell Spawn Utilities ===

/// Spawn a cell entity with empty pools. See `crate::cell::spawn_cell`.
pub fn spawn_cell(commands: &mut Commands, genome: Genome) -> Entity {
    crate::cell::spawn_cell(commands, genome, CurrencyPools::default())
}

/// Spawn a player-cell pair with ownership link
/// Returns (player_entity, cell_entity)
pub fn spawn_player_cell(
    commands: &mut Commands,
    client_id: u32,
    genome: Genome,
) -> (Entity, Entity) {
    let cell_entity = spawn_cell(commands, genome);

    let player_entity = commands
        .spawn((
            Player { client_id },
            ControlledCell(cell_entity),
            Name::new("Player"),
        ))
        .id();

    (player_entity, cell_entity)
}

/// Spawn a local player-cell pair for single-player mode
/// Returns (player_entity, cell_entity)
pub fn spawn_local_player_cell(commands: &mut Commands, genome: Genome) -> (Entity, Entity) {
    let cell_entity = spawn_cell(commands, genome);

    let player_entity = commands
        .spawn((
            Player { client_id: 0 },
            LocalPlayer,
            ControlledCell(cell_entity),
            Name::new("Local Player"),
        ))
        .id();

    (player_entity, cell_entity)
}

/// System param: the genome of the local player's cell, if there is one.
#[derive(bevy::ecs::system::SystemParam)]
pub struct LocalCellGenome<'w, 's> {
    players: Query<'w, 's, &'static ControlledCell, With<LocalPlayer>>,
    genomes: Query<'w, 's, &'static Genome>,
}

impl LocalCellGenome<'_, '_> {
    pub fn get(&self) -> Option<&Genome> {
        let cell = self.players.get_single().ok()?.0;
        self.genomes.get(cell).ok()
    }
}

/// Query helper to find the cell controlled by the local player
/// Returns None if no local player exists or they don't control a cell
pub fn get_local_player_cell(
    local_players: &Query<&ControlledCell, With<LocalPlayer>>,
) -> Option<Entity> {
    local_players.iter().next().map(|controlled| controlled.0)
}
