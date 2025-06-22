use bevy::prelude::*;
use crate::{GameState, genome};

/// Shared resources and systems that persist across all game states
pub fn setup_shared_resources(mut commands: Commands) {
    // Initialize genome with starter genes
    let starter_genome = genome::create_starter_genome();
    commands.insert_resource(starter_genome);
    
    // Note: Metabolic block entities will be spawned by individual scenes as needed
}

/// Input system for state transitions
pub fn state_transition_input(
    input: Res<ButtonInput<KeyCode>>,
    current_state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    // Press '1' for 3D scene
    if input.just_pressed(KeyCode::Digit1) {
        if current_state.get() != &GameState::Scene3D {
            next_state.set(GameState::Scene3D);
            info!("Switching to 3D scene");
        }
    }
    
    // Press '2' for 2D scene
    if input.just_pressed(KeyCode::Digit2) {
        if current_state.get() != &GameState::Scene2D {
            next_state.set(GameState::Scene2D);
            info!("Switching to 2D scene");
        }
    }
    
    // Press 'Escape' for main menu
    if input.just_pressed(KeyCode::Escape) {
        if current_state.get() != &GameState::MainMenu {
            next_state.set(GameState::MainMenu);
            info!("Returning to main menu");
        }
    }
}

/// Demo system to showcase genome functionality (works in all states)
pub fn genome_demo_system(
    mut genome: ResMut<genome::Genome>,
    input: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    // Press 'G' to express sugar catabolism gene
    if input.just_pressed(KeyCode::KeyG) {
        if genome.express_gene(genome::BlockKind::SugarCatabolism) {
            info!("Expressed SugarCatabolism gene!");
        } else {
            warn!("Failed to express SugarCatabolism gene - already expressed or not present");
        }
    }
    
    // Press 'H' to silence fermentation gene
    if input.just_pressed(KeyCode::KeyH) {
        if genome.silence_gene(genome::BlockKind::Fermentation) {
            info!("Silenced Fermentation gene!");
        } else {
            warn!("Failed to silence Fermentation gene - not expressed or not present");
        }
    }
    
    // Press 'J' to add a new gene
    if input.just_pressed(KeyCode::KeyJ) {
        genome.add_gene(genome::BlockKind::LightCapture);
        info!("Added LightCapture gene to genome!");
    }
    
    // Press 'K' to spawn metabolic block entities
    if input.just_pressed(KeyCode::KeyK) {
        genome::spawn_metabolic_block(&mut commands, genome::BlockKind::Respiration);
        info!("Spawned Respiration metabolic block entity!");
    }
} 