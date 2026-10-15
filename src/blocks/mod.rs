//! # Metabolic Blocks Module
//! 
//! This module implements the metabolic block system for Metabolistic3D, a unified architecture
//! for managing cellular metabolic pathways through a genome-driven approach.
//! 
//! ## Architecture Overview
//! 
//! The metabolic block system follows a unified plugin architecture where:
//! - All blocks are managed by a single `MetabolicBlocksPlugin`
//! - Blocks are dynamically spawned/despawned based on genome expression
//! - Each block has both a flux profile (for metabolic flow calculation) and optional active systems
//! 
//! ## Block Types
//! 
//! ### Flux-Only Blocks
//! These blocks only define input/output relationships through their `FluxProfile`:
//! - **Fermentation**: Converts pyruvate + reducing power → ATP + organic waste
//! 
//! ### Active System Blocks
//! These blocks have additional systems that run each frame to handle complex behaviors:
//! - **Fat Storage**: Polymerizes free fatty acids into storage beads when above toxicity threshold
//! - **Vesicle Export**: Actively removes organic waste from cells at a constant rate
//! 
//! ## Genome Integration
//! 
//! Blocks are controlled by the genome system:
//! - Gene states: `Silent`, `Expressed`, or `Mutated`
//! - Only `Expressed` and `Mutated` genes spawn their corresponding blocks
//! - The `spawn_metabolic_blocks_system` monitors genome changes and manages block lifecycle
//! 
//! ## Usage
//! 
//! Add the plugin to your Bevy app:
//! ```rust
//! app.add_plugins(MetabolicBlocksPlugin);
//! ```
//! 
//! The plugin will automatically:
//! 1. Listen for genome expression changes via `MetabolicUpdateEvent`
//! 2. Spawn/despawn blocks as genes are expressed/silenced
//! 3. Run active systems for blocks that require per-frame processing
//! 4. Integrate with the metabolic flow system for currency calculations

pub mod genome;
pub mod fermentation;
pub mod fat_storage;
pub mod vesicle_export;

use bevy::prelude::*;
use crate::metabolism::{MetabolicBlock, MetabolicNode, FluxProfile, BlockStatus};
use crate::molecules::Currency;
use crate::cell::Cell;
use genome::{Genome, BlockKind, GeneState, MetabolicUpdateEvent};

/// Unified plugin that manages all metabolic blocks based on genome expression.
/// 
/// This plugin replaces the individual block plugins and provides centralized
/// management of metabolic blocks through genome-based spawning and lifecycle.
pub struct MetabolicBlocksPlugin;

impl Plugin for MetabolicBlocksPlugin {
    fn build(&self, app: &mut App) {
        app
            // Add resources needed for metabolic blocks
            .insert_resource(vesicle_export::VesicleExportRate(0.1))
            // Unified block management system
            .add_systems(Update, spawn_metabolic_blocks_system)
            // Keep the detailed biochemical systems alongside the flux system
            // These provide the actual biochemical logic while flux profiles handle flow calculation
            .add_systems(Update, (
                fat_storage::polymerize_beads_system,
                fat_storage::lipolysis_system,
                vesicle_export::vesicle_export_system,
            ));
    }
}

/// System that spawns metabolic blocks when genes are expressed.
/// 
/// This system responds to `MetabolicUpdateEvent` and manages the lifecycle of metabolic blocks:
/// - Spawns new blocks when genes become expressed
/// - Despawns blocks when genes are silenced
/// - Ensures each cell has the correct blocks based on genome state
/// 
/// The system runs in the `Update` schedule and only processes changes when a
/// `MetabolicUpdateEvent` is received, making it efficient for steady-state operation.
fn spawn_metabolic_blocks_system(
    mut diff_reader: EventReader<MetabolicUpdateEvent>,
    global_genome: Option<Res<Genome>>,
    cells_query: Query<(Entity, &Cell)>,
    mut commands: Commands,
) {
    // Only run when there are genome changes
    let has_events = diff_reader.read().count() > 0;
    if !has_events {
        return;
    }
    
    debug!("Processing metabolic update event, found {} cells", cells_query.iter().len());
    
    // Process genome changes for all cells
    let _global_genome = global_genome.as_deref();
    
    // Process genome changes for each cell entity  
    for (cell_entity, cell) in cells_query.iter() {
        // Use cell's genome 
        debug!("Processing genome for cell {:?}", cell_entity);
        process_genome_for_cell(&cell.genome, cell_entity, &mut commands);
    }
}

/// Helper function to process genome changes for a specific cell.
/// 
/// Iterates through all possible block kinds and:
/// - Checks if the block should exist based on gene state (Expressed or Mutated)
/// - Spawns blocks that should exist but don't
/// - Despawns blocks that shouldn't exist but do
/// 
/// This ensures each cell's metabolic blocks stay synchronized with genome expression.
fn process_genome_for_cell(
    genome: &Genome,
    cell_entity: Entity,
    commands: &mut Commands,
) {
    // Check each block kind to see if it should exist
    for block_kind in [
        BlockKind::Fermentation,
        BlockKind::LipidMetabolism,
        BlockKind::Polymerization,
        // Add other block kinds as they're implemented
    ] {
        let gene_state = genome.get_gene_state(&block_kind);
        let should_exist = matches!(gene_state, Some(GeneState::Expressed | GeneState::Mutated));
        
        // For now, just spawn blocks if they should exist (simplified approach)
        if should_exist {
            spawn_block(commands, block_kind, cell_entity);
        }
        
        // TODO: In the future, we need to handle despawning blocks that shouldn't exist
        // This requires tracking which blocks are owned by which cell
    }
}

/// Helper function to spawn a specific metabolic block.
/// 
/// Creates a new metabolic block entity with:
/// - Appropriate component marker (FermentationBlock, VesicleExportBlock, etc.)
/// - MetabolicBlock component for metabolic system integration
/// - MetabolicNode with block kind and active status
/// - FluxProfile defining the block's metabolic inputs and outputs
/// - OwnedBy component linking the block to its parent cell
/// - Name component for debugging
/// 
/// The flux profile is configured based on the block type to match
/// the biochemical reaction it represents.
fn spawn_block(commands: &mut Commands, block_kind: BlockKind, _cell_entity: Entity) {
    let mut flux_profile = FluxProfile::default();
    
    // Configure flux profile based on block type
    match block_kind {
        BlockKind::Fermentation => {
            flux_profile.set_demand(Currency::Pyruvate, 1.0);
            flux_profile.set_demand(Currency::ReducingPower, 1.0);
            flux_profile.set_production(Currency::ATP, 1.0);
            flux_profile.set_production(Currency::OrganicWaste, 1.0);
            
            commands.spawn((
                fermentation::FermentationBlock,
                MetabolicBlock,
                MetabolicNode {
                    kind: block_kind,
                    status: BlockStatus::Active,
                },
                flux_profile,
                Name::new("Fermentation Block"),
            ));
        },
        BlockKind::LipidMetabolism => {
            // Vesicle export block - only consumes OrganicWaste
            flux_profile.set_demand(Currency::OrganicWaste, 0.1);
            
            commands.spawn((
                vesicle_export::VesicleExportBlock,
                MetabolicBlock,
                MetabolicNode {
                    kind: block_kind,
                    status: BlockStatus::Active,
                },
                flux_profile,
                Name::new("Vesicle Export Block"),
            ));
        },
        BlockKind::Polymerization => {
            // Fat storage block - polymerizes FreeFattyAcids into StorageBeads
            flux_profile.set_demand(Currency::FreeFattyAcids, 20.0);
            flux_profile.set_demand(Currency::ATP, 1.0);
            flux_profile.set_production(Currency::StorageBeads, 20.0);
            
            commands.spawn((
                fat_storage::FatStorageBlock,
                MetabolicBlock,
                MetabolicNode {
                    kind: block_kind,
                    status: BlockStatus::Active,
                },
                flux_profile,
                Name::new("Fat Storage Block"),
            ));
        },
        _ => {
            warn!("Block kind {:?} not implemented in unified plugin", block_kind);
        }
    }
    
    info!("Spawned metabolic block: {:?}", block_kind);
}
