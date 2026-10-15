//! # Property Testing Utilities
//!
//! This module provides proptest strategies for generating test data for the metabolic simulation.
//! It includes generators for currency values, metabolic states, and system configurations.
#![allow(dead_code)] // Each test binary uses a subset of these helpers.

use bevy::prelude::*;
pub use metabolistic3d::blocks::genome::{BlockKind, GeneState};
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::*;
use proptest::prelude::*;

// --- Currency Value Strategies ---

/// Generates reasonable currency values (0.0 to 1000.0)
/// Avoids extreme values that could cause overflow or precision issues
pub fn currency_amount() -> impl Strategy<Value = f32> {
    0.0f32..1000.0f32
}

/// Generates currency consumption amounts (positive values only)
pub fn consumption_amount() -> impl Strategy<Value = f32> {
    0.1f32..50.0f32
}

/// Generates different BlockKind variants for testing
pub fn block_kind() -> impl Strategy<Value = BlockKind> {
    prop_oneof![
        Just(BlockKind::LightCapture),
        Just(BlockKind::SugarCatabolism),
        Just(BlockKind::Respiration),
        Just(BlockKind::Fermentation),
        Just(BlockKind::LipidMetabolism),
        Just(BlockKind::Polymerization),
    ]
}

/// Generates different GeneState variants for testing
pub fn gene_state() -> impl Strategy<Value = GeneState> {
    prop_oneof![
        Just(GeneState::Expressed),
        Just(GeneState::Mutated),
        Just(GeneState::Silent),
    ]
}

/// Generates polymer rates for testing
pub fn polymer_rate() -> impl Strategy<Value = f32> {
    1.0f32..50.0f32
}

/// Get the first cell entity in the world (for single-cell tests)
pub fn get_first_cell_entity(world: &World) -> Option<Entity> {
    for entity in world.iter_entities() {
        if entity.contains::<metabolistic3d::cell::Cell>() {
            return Some(entity.id());
        }
    }
    None
}

/// Checks if all currencies in an app are non-negative
pub fn all_currencies_non_negative(app: &App) -> bool {
    if let Some(cell_entity) = get_first_cell_entity(app.world()) {
        if let Ok(entity) = app.world().get_entity(cell_entity) {
            if let Some(currency_pools) = entity.get::<CurrencyPools>() {
                return currency_pools.pools.values().all(|&v| v >= 0.0);
            }
        }
    }
    false
}

/// Gets all currency amounts as a vector for easy comparison
pub fn get_currency_snapshot(app: &App) -> Vec<f32> {
    if let Some(cell_entity) = get_first_cell_entity(app.world()) {
        if let Ok(entity) = app.world().get_entity(cell_entity) {
            if let Some(currency_pools) = entity.get::<CurrencyPools>() {
                return vec![
                    currency_pools.get(Currency::ATP),
                    currency_pools.get(Currency::ReducingPower),
                    currency_pools.get(Currency::AcetylCoA),
                    currency_pools.get(Currency::CarbonSkeletons),
                    currency_pools.get(Currency::FreeFattyAcids),
                    currency_pools.get(Currency::Pyruvate),
                    currency_pools.get(Currency::OrganicWaste),
                    currency_pools.get(Currency::StorageBeads),
                ];
            }
        }
    }
    vec![0.0; 8]
}

/// Modify currencies in a specific cell
pub fn modify_cell_currencies<F>(app: &mut App, cell_entity: Entity, f: F) -> bool
where
    F: FnOnce(&mut CurrencyPools),
{
    if let Ok(mut entity_mut) = app.world_mut().get_entity_mut(cell_entity) {
        if let Some(mut currency_pools) = entity_mut.get_mut::<CurrencyPools>() {
            f(&mut currency_pools);
            return true;
        }
    }
    false
}

/// Run metabolic processing for a cell
pub fn run_cell_metabolic_cycle(app: &mut App, cell_entity: Entity) {
    run_metabolic_cycle(app, cell_entity);
}

/// Get currency pools from a specific cell
pub fn get_cell_currencies(app: &App, cell_entity: Entity) -> Option<&CurrencyPools> {
    if let Ok(entity_ref) = app.world().get_entity(cell_entity) {
        entity_ref.get::<CurrencyPools>()
    } else {
        None
    }
}

/// Check if all currencies in a specific cell are non-negative
pub fn cell_currencies_non_negative(app: &App, cell_entity: Entity) -> bool {
    if let Some(currency_pools) = get_cell_currencies(app, cell_entity) {
        currency_pools.pools.values().all(|&v| v >= 0.0)
    } else {
        false
    }
}

/// Run a few metabolic ticks for a cell (block sync, then flux).
pub fn run_metabolic_cycle(app: &mut App, _cell_entity: Entity) {
    for _ in 0..3 {
        sim_step(app);
    }
}

/// One simulation step: a frame (block sync from genome changes), then one
/// metabolic tick. Test apps use manual ticks, so this is deterministic.
pub fn sim_step(app: &mut App) {
    app.update();
    metabolistic3d::metabolism::run_metabolic_tick(app.world_mut());
}

/// Add and express genes on a cell. The blocks spawn on the next frame.
pub fn express_blocks(app: &mut App, cell: Entity, kinds: &[BlockKind]) {
    let mut genome = app
        .world_mut()
        .get_mut::<metabolistic3d::blocks::genome::Genome>(cell)
        .expect("cell has a genome");
    for &kind in kinds {
        if genome.get_gene_state(&kind).is_none() {
            genome.add_gene(kind);
        }
        genome.express_gene(kind);
    }
}

/// Set the rates of the cell's fat storage block. Runs a frame first so that the
/// block exists. Express `BlockKind::FatStorage` before calling this.
pub fn set_fat_storage_rates(app: &mut App, cell: Entity, poly_rate: f32, lipo_rate: f32) {
    app.update();
    let block = app
        .world()
        .get::<metabolistic3d::cell::CellBlocks>(cell)
        .and_then(|blocks| blocks.get(BlockKind::FatStorage))
        .expect("cell has a fat storage block");
    *app.world_mut()
        .get_mut::<metabolistic3d::blocks::fat_storage::FatStorageBlock>(block)
        .unwrap() = metabolistic3d::blocks::fat_storage::FatStorageBlock {
        poly_rate,
        lipo_rate,
    };
}
