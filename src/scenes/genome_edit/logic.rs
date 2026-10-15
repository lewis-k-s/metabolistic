//! src/scenes/genome_edit/logic.rs
//!
//! This module contains the core logic for the genome editing scene.
//! It is responsible for managing the state of the editor, such as which
//! genome section is currently selected, and for handling user input
//! that modifies this state. It does not deal with any rendering.

use bevy::prelude::*;

use crate::{
    blocks::genome::{self, BlockKind},
    GameState,
};

use super::GenomeSceneEntity;

pub struct LogicPlugin;

impl Plugin for LogicPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(GameState::GenomeEditing),
            setup_genome_logic,
        )
        .add_systems(
            Update,
            navigate_genome.run_if(in_state(GameState::GenomeEditing)),
        )
        .add_systems(
            OnExit(GameState::GenomeEditing),
            cleanup_genome_logic,
        );
    }
}

// --- Component Definitions ---

/// A component representing a logical section of the genome.
/// It holds the data necessary for identification and logic, but no
/// visual information.
#[derive(Component, Debug)]
pub struct GenomeSection {
    pub block_kind: BlockKind,
    pub section_index: usize,
}

/// A marker component to indicate which `GenomeSection` is currently selected.
/// Navigation works by moving this component between entities.
#[derive(Component)]
pub struct SelectedSection;

// --- Resource Definition ---

/// A resource to hold the state of the genome editing scene.
#[derive(Resource, Default)]
pub struct GenomeEditorState {
    /// An ordered list of the block kinds being displayed.
    pub blocks: Vec<BlockKind>,
    /// A list of the logical entities created for the genome sections.
    pub section_entities: Vec<Entity>,
}

// --- Systems ---

/// Creates the logical entities for each genome block.
/// These entities only contain data components, not anything for rendering.
/// It also sets the initial selected section.
fn setup_genome_logic(
    mut commands: Commands,
    genome: Res<genome::Genome>,
) {
    let blocks: Vec<BlockKind> = genome.table.keys().copied().collect();
    let mut section_entities = Vec::new();

    for (i, block_kind) in blocks.iter().enumerate() {
        let entity = commands
            .spawn((
                GenomeSection {
                    block_kind: *block_kind,
                    section_index: i,
                },
                GenomeSceneEntity, // Mark for cleanup
                Name::new(format!("GenomeSection {:?}", block_kind)),
            ))
            .id();
        section_entities.push(entity);
    }

    // Mark the first section as selected.
    if let Some(first_entity) = section_entities.first() {
        commands.entity(*first_entity).insert(SelectedSection);
    }

    commands.insert_resource(GenomeEditorState {
        blocks,
        section_entities,
    });
}

/// Handles keyboard input to navigate between genome sections.
/// It works by moving the `SelectedSection` component from one entity to another.
fn navigate_genome(
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    // Query to find the currently selected entity.
    selected_query: Query<(Entity, &GenomeSection), With<SelectedSection>>,
    state: Res<GenomeEditorState>,
) {
    if !input.just_pressed(KeyCode::ArrowRight) && !input.just_pressed(KeyCode::ArrowLeft) {
        return;
    }

    if let Ok((current_entity, section)) = selected_query.get_single() {
        let current_index = section.section_index;
        let num_blocks = state.blocks.len();
        let next_index = if input.just_pressed(KeyCode::ArrowRight) {
            (current_index + 1) % num_blocks
        } else {
            (current_index + num_blocks - 1) % num_blocks
        };

        // Remove the marker from the current entity.
        commands.entity(current_entity).remove::<SelectedSection>();

        // Add the marker to the new target entity.
        if let Some(next_entity) = state.section_entities.get(next_index) {
            commands.entity(*next_entity).insert(SelectedSection);
        }
    }
}

/// Cleans up the logic-related resources.
fn cleanup_genome_logic(mut commands: Commands) {
    commands.remove_resource::<GenomeEditorState>();
}
