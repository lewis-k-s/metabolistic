//! # Cell Factory and Management
//!
//! This module provides a convenient interface for creating and managing cells
//! with their associated genomes and currency pools. It acts as a wrapper around
//! the existing component system to provide a cleaner API for both gameplay and testing.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::blocks::genome::{BlockKind, GeneState, Genome, MetabolicUpdateEvent};
use crate::metabolism::{
    BlockStatus, CurrencyPools, FluxProfile, MetabolicBlock, MetabolicGraph, MetabolicNode,
};
use crate::molecules::Currency;

// --- Cell Ownership Components ---

/// Comprehensive cell component that manages all cell-specific data
#[derive(Component)]
#[require(Name)]
#[derive(Default)]
pub struct Cell {
    /// The cell's genome containing gene expression states
    pub genome: Genome,
    /// Currency pools for metabolic resources
    pub currency_pools: CurrencyPools,
    /// Metabolic graph representation of the cell's metabolic network
    pub metabolic_graph: MetabolicGraph,
    /// Direct ownership of metabolic blocks by type
    pub metabolic_blocks: HashMap<BlockKind, Entity>,
}

/// A convenience wrapper around a cell entity that provides factory functions
/// and wrapper methods for common cell operations.
#[derive(Debug, Clone, Copy)]
pub struct CellWrapper {
    entity: Entity,
}

impl CellWrapper {
    /// Create a new Cell wrapper around an existing entity
    ///
    /// # Safety
    /// The caller must ensure the entity exists and has the required Cell component
    pub fn from_entity(entity: Entity) -> Self {
        Self { entity }
    }

    /// Get the underlying entity ID
    pub fn entity(&self) -> Entity {
        self.entity
    }

    /// Express a gene in this cell's genome
    ///
    /// This is a wrapper around the genome's express_gene method
    pub fn express_gene(&self, mut cell_query: Query<&mut Cell>, block_kind: BlockKind) -> bool {
        if let Ok(mut cell) = cell_query.get_mut(self.entity) {
            cell.genome.express_gene(block_kind);
            true
        } else {
            false
        }
    }

    /// Add a gene to this cell's genome
    ///
    /// This is a wrapper around the genome's add_gene method
    pub fn add_gene(&self, mut cell_query: Query<&mut Cell>, block_kind: BlockKind) -> bool {
        if let Ok(mut cell) = cell_query.get_mut(self.entity) {
            cell.genome.add_gene(block_kind);
            true
        } else {
            false
        }
    }

    /// Silence a gene in this cell's genome
    ///
    /// This is a wrapper around the genome's silence_gene method
    pub fn silence_gene(&self, mut cell_query: Query<&mut Cell>, block_kind: BlockKind) -> bool {
        if let Ok(mut cell) = cell_query.get_mut(self.entity) {
            cell.genome.silence_gene(block_kind);
            true
        } else {
            false
        }
    }

    /// Get the state of a gene in this cell's genome
    pub fn get_gene_state(
        &self,
        cell_query: &Query<&Cell>,
        block_kind: BlockKind,
    ) -> Option<GeneState> {
        if let Ok(cell) = cell_query.get(self.entity) {
            cell.genome.get_gene_state(&block_kind).cloned()
        } else {
            None
        }
    }

    /// Set the amount of a specific currency in this cell
    pub fn set_currency(
        &self,
        mut cell_query: Query<&mut Cell>,
        currency: Currency,
        amount: f32,
    ) -> bool {
        if let Ok(mut cell) = cell_query.get_mut(self.entity) {
            cell.currency_pools.set(currency, amount);
            true
        } else {
            false
        }
    }

    /// Get the amount of a specific currency in this cell
    pub fn get_currency(&self, cell_query: &Query<&Cell>, currency: Currency) -> f32 {
        if let Ok(cell) = cell_query.get(self.entity) {
            cell.currency_pools.get(currency)
        } else {
            0.0
        }
    }

    /// Modify the amount of a specific currency in this cell
    pub fn modify_currency(
        &self,
        mut cell_query: Query<&mut Cell>,
        currency: Currency,
        delta: f32,
    ) -> bool {
        if let Ok(mut cell) = cell_query.get_mut(self.entity) {
            cell.currency_pools.modify(currency, delta);
            true
        } else {
            false
        }
    }

    /// Check if this cell can consume a specific amount of currency
    pub fn can_consume_currency(
        &self,
        cell_query: &Query<&Cell>,
        currency: Currency,
        amount: f32,
    ) -> bool {
        if let Ok(cell) = cell_query.get(self.entity) {
            cell.currency_pools.can_consume(currency, amount)
        } else {
            false
        }
    }

    /// Get a snapshot of all currencies in this cell
    pub fn get_currency_snapshot(&self, cell_query: &Query<&Cell>) -> HashMap<Currency, f32> {
        let mut snapshot = HashMap::new();

        if let Ok(cell) = cell_query.get(self.entity) {
            snapshot.insert(Currency::ATP, cell.currency_pools.get(Currency::ATP));
            snapshot.insert(
                Currency::ReducingPower,
                cell.currency_pools.get(Currency::ReducingPower),
            );
            snapshot.insert(
                Currency::AcetylCoA,
                cell.currency_pools.get(Currency::AcetylCoA),
            );
            snapshot.insert(
                Currency::CarbonSkeletons,
                cell.currency_pools.get(Currency::CarbonSkeletons),
            );
            snapshot.insert(
                Currency::FreeFattyAcids,
                cell.currency_pools.get(Currency::FreeFattyAcids),
            );
            snapshot.insert(
                Currency::Pyruvate,
                cell.currency_pools.get(Currency::Pyruvate),
            );
            snapshot.insert(
                Currency::OrganicWaste,
                cell.currency_pools.get(Currency::OrganicWaste),
            );
            snapshot.insert(
                Currency::StorageBeads,
                cell.currency_pools.get(Currency::StorageBeads),
            );
        }

        snapshot
    }

    /// Trigger a metabolic update event for this cell
    ///
    /// This will cause the metabolic block spawning system to check this cell's
    /// genome and spawn/despawn blocks as needed
    pub fn trigger_metabolic_update(&self, world: &mut World) {
        if let Some(mut event_writer) = world.get_resource_mut::<Events<MetabolicUpdateEvent>>() {
            event_writer.send(MetabolicUpdateEvent);
        }
    }

    /// Convenience method to express a gene and trigger metabolic update
    pub fn express_gene_and_update(
        &self,
        cell_query: Query<&mut Cell>,
        world: &mut World,
        block_kind: BlockKind,
    ) -> bool {
        if self.express_gene(cell_query, block_kind) {
            self.trigger_metabolic_update(world);
            true
        } else {
            false
        }
    }

    /// Check if this cell entity exists and has the required components
    pub fn is_valid(&self, cell_query: &Query<&Cell>) -> bool {
        cell_query.get(self.entity).is_ok()
    }

    /// Get all expressed genes from this cell's genome
    pub fn get_expressed_genes(&self, cell_query: &Query<&Cell>) -> Vec<BlockKind> {
        if let Ok(cell) = cell_query.get(self.entity) {
            cell.genome.get_expressed_genes()
        } else {
            Vec::new()
        }
    }

    // === METABOLIC BLOCK MANAGEMENT ===

    /// Process genome changes for this cell and spawn/despawn metabolic blocks accordingly
    /// This replaces the global process_genome_for_cell function
    pub fn process_genome_changes(&self, world: &mut World) {
        // Get the genome from this cell
        let genome = if let Ok(entity_ref) = world.get_entity(self.entity) {
            if let Some(genome) = entity_ref.get::<Genome>() {
                genome.clone()
            } else {
                return;
            }
        } else {
            return;
        };

        // Check each implemented block kind
        for block_kind in [
            BlockKind::Fermentation,
            BlockKind::LipidMetabolism,
            // Add other block kinds as they're implemented
        ] {
            let gene_state = genome.get_gene_state(&block_kind);
            let should_exist =
                matches!(gene_state, Some(GeneState::Expressed | GeneState::Mutated));

            // Check if block already exists for this cell
            let block_exists = self.has_metabolic_block(world, block_kind);

            if should_exist && !block_exists {
                self.spawn_metabolic_block(world, block_kind);
            } else if !should_exist && block_exists {
                self.despawn_metabolic_block(world, block_kind);
            }
        }
    }

    /// Spawn a specific metabolic block for this cell
    pub fn spawn_metabolic_block(&self, world: &mut World, block_kind: BlockKind) {
        let mut flux_profile = FluxProfile::default();

        // Configure flux profile based on block type
        let block_entity = match block_kind {
            BlockKind::Fermentation => {
                flux_profile.set_demand(Currency::Pyruvate, 1.0);
                flux_profile.set_demand(Currency::ReducingPower, 1.0);
                flux_profile.set_production(Currency::ATP, 1.0);
                flux_profile.set_production(Currency::OrganicWaste, 1.0);

                world
                    .spawn((
                        crate::blocks::fermentation::FermentationBlock,
                        MetabolicBlock,
                        MetabolicNode {
                            kind: block_kind,
                            status: BlockStatus::Active,
                        },
                        flux_profile,
                        Name::new("Fermentation Block"),
                    ))
                    .id()
            }
            BlockKind::LipidMetabolism => {
                // For fat storage, we don't use the flux profile system yet
                // since it has its own direct currency manipulation systems
                world
                    .spawn((
                        crate::blocks::vesicle_export::VesicleExportBlock,
                        Name::new("Vesicle Export Block"),
                    ))
                    .id()
            }
            _ => {
                warn!("Block kind {:?} not implemented in cell spawn", block_kind);
                return;
            }
        };

        // Store the block entity in the cell's metabolic_blocks
        if let Ok(mut cell) = world.get_entity_mut(self.entity) {
            if let Some(mut cell_component) = cell.get_mut::<Cell>() {
                cell_component
                    .metabolic_blocks
                    .insert(block_kind, block_entity);
            }
        }

        info!("Cell spawned metabolic block: {:?}", block_kind);
    }

    /// Check if this cell has a specific metabolic block
    pub fn has_metabolic_block(&self, world: &World, block_kind: BlockKind) -> bool {
        if let Ok(cell) = world.get_entity(self.entity) {
            if let Some(cell_component) = cell.get::<Cell>() {
                return cell_component.metabolic_blocks.contains_key(&block_kind);
            }
        }
        false
    }

    /// Despawn a specific metabolic block for this cell
    pub fn despawn_metabolic_block(&self, world: &mut World, block_kind: BlockKind) {
        if let Ok(mut cell) = world.get_entity_mut(self.entity) {
            if let Some(mut cell_component) = cell.get_mut::<Cell>() {
                if let Some(block_entity) = cell_component.metabolic_blocks.remove(&block_kind) {
                    if let Ok(entity_commands) = world.get_entity_mut(block_entity) {
                        entity_commands.despawn();
                        info!("Cell despawned metabolic block: {:?}", block_kind);
                    }
                }
            }
        }
    }

    /// Synchronize metabolic blocks with the current genome state
    /// This ensures all blocks match the genome expression
    pub fn sync_blocks_with_genome(&self, world: &mut World) {
        self.process_genome_changes(world);

        // Also trigger metabolic update to ensure other systems know about changes
        self.trigger_metabolic_update(world);
    }
}

/// Utility function to find the local player's cell in the world
pub fn find_local_player_cell(world: &mut World) -> Option<CellWrapper> {
    use crate::player::{ControlledCell, LocalPlayer};

    let mut query = world.query_filtered::<&ControlledCell, With<LocalPlayer>>();
    if let Ok(controlled_cell) = query.get_single(world) {
        Some(CellWrapper::from_entity(controlled_cell.0))
    } else {
        None
    }
}
