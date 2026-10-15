use bevy::prelude::*;

use crate::blocks::genome::Genome;
use crate::metabolism::CurrencyPools;

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
        let cell = self.players.single().ok()?.0;
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
