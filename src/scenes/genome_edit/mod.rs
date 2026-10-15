//! src/scenes/genome_edit/mod.rs
//!
//! This module orchestrates the genome editing scene by combining the
//! logic and view sub-modules.

pub mod logic;
pub mod view;

use bevy::prelude::*;

use crate::GameState;
use logic::LogicPlugin;
use view::ViewPlugin;

/// The main plugin for the genome editing scene.
/// It sets up the logic and view plugins and handles the scene's
/// overall lifecycle.
pub struct GenomeEditPlugin;

impl Plugin for GenomeEditPlugin {
    fn build(&self, app: &mut App) {
        app
            // Register the plugins from the sub-modules.
            .add_plugins((LogicPlugin, ViewPlugin))
            // Add the top-level cleanup system.
            .add_systems(OnExit(GameState::GenomeEditing), cleanup_genome_scene);
    }
}

/// A marker component for any entity that is part of the genome editing scene,
/// whether its a logical or visual entity. Used for easy cleanup.
#[derive(Component)]
pub struct GenomeSceneEntity;

/// Cleans up all entities created for the genome scene upon exiting the state.
/// This is placed here because it needs to despawn entities from both the
/// logic and view modules.
fn cleanup_genome_scene(
    mut commands: Commands,
    entities: Query<Entity, (With<GenomeSceneEntity>, Without<ChildOf>)>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
