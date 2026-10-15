use crate::player::spawn_local_player_cell;
#[cfg(feature = "full")]
use crate::player::{ControlledCell, LocalPlayer};
#[cfg(feature = "full")]
use crate::{GameState, cell::Cell};
use crate::{blocks::genome, metabolism::CurrencyPools};
use bevy::prelude::*;

/// Spawns the local player's cell with the starter genome and default pools.
pub fn spawn_starter_cell(mut commands: Commands) {
    let (_player, cell) = spawn_local_player_cell(&mut commands, genome::create_starter_genome());
    commands.entity(cell).insert(CurrencyPools::with_defaults());
}

/// Input system for state transitions
#[cfg(feature = "full")]
pub fn state_transition_input(
    input: Res<ButtonInput<KeyCode>>,
    current_state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    // Press '1' for 3D scene
    if input.just_pressed(KeyCode::Digit1) && current_state.get() != &GameState::Scene3D {
        next_state.set(GameState::Scene3D);
        info!("Switching to 3D scene");
    }

    // Press '2' for 2D scene
    if input.just_pressed(KeyCode::Digit2) && current_state.get() != &GameState::Scene2D {
        next_state.set(GameState::Scene2D);
        info!("Switching to 2D scene");
    }

    // Press '3' for genome editing scene
    if input.just_pressed(KeyCode::Digit3) && current_state.get() != &GameState::GenomeEditing {
        next_state.set(GameState::GenomeEditing);
        info!("Opening genome editor");
    }

    // Press 'Escape' for main menu
    if input.just_pressed(KeyCode::Escape) && current_state.get() != &GameState::MainMenu {
        next_state.set(GameState::MainMenu);
        info!("Returning to main menu");
    }
}

/// Demo system to showcase genome functionality (works in all states).
/// Edits the genome of the local player's cell.
#[cfg(feature = "full")]
pub fn genome_demo_system(
    input: Res<ButtonInput<KeyCode>>,
    local_players: Query<&ControlledCell, With<LocalPlayer>>,
    mut genomes: Query<&mut genome::Genome, With<Cell>>,
) {
    let Some(mut genome) = local_players
        .single()
        .ok()
        .and_then(|controlled| genomes.get_mut(controlled.0).ok())
    else {
        return;
    };

    // Press 'G' to express sugar catabolism gene
    if input.just_pressed(KeyCode::KeyG) {
        genome.express_gene(genome::BlockKind::SugarCatabolism);
        info!("Expressed SugarCatabolism gene!");
    }

    // Press 'H' to silence fermentation gene
    if input.just_pressed(KeyCode::KeyH) {
        genome.silence_gene(genome::BlockKind::Fermentation);
        info!("Silenced Fermentation gene!");
    }

    // Press 'J' to add a new gene
    if input.just_pressed(KeyCode::KeyJ) {
        genome.add_gene(genome::BlockKind::LightCapture);
        info!("Added LightCapture gene to genome!");
    }
}
