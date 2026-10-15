//! # Test helpers
//!
//! Proptest strategies and helpers to set up and read test cells.
#![allow(dead_code)] // Each test binary uses a subset of these helpers.

use bevy::prelude::*;
use metabolistic3d::blocks::genome::Genome;
pub use metabolistic3d::blocks::genome::{BlockKind, GeneState};
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::*;
use metabolistic3d::proteins::Proteome;
use proptest::prelude::*;

// --- Strategies ---

/// Reasonable pool amounts (0 to 1000).
pub fn currency_amount() -> impl Strategy<Value = f32> {
    0.0f32..1000.0f32
}

/// Consumption amounts (positive only).
pub fn consumption_amount() -> impl Strategy<Value = f32> {
    0.1f32..50.0f32
}

/// Block kinds, some with an implementation and some without.
pub fn block_kind() -> impl Strategy<Value = BlockKind> {
    prop_oneof![
        Just(BlockKind::LightCapture),
        Just(BlockKind::SugarCatabolism),
        Just(BlockKind::Respiration),
        Just(BlockKind::Fermentation),
        Just(BlockKind::LipidMetabolism),
        Just(BlockKind::Polymerization),
        Just(BlockKind::Chemosynthesis),
        Just(BlockKind::ProteinSynthesis),
    ]
}

/// Gene states.
pub fn gene_state() -> impl Strategy<Value = GeneState> {
    prop_oneof![
        Just(GeneState::Expressed),
        Just(GeneState::Mutated),
        Just(GeneState::Silent),
    ]
}

/// Fat storage rates.
pub fn polymer_rate() -> impl Strategy<Value = f32> {
    1.0f32..50.0f32
}

// --- Cell setup ---

/// Spawns a local player cell in a headless app. It expresses `genes`, with
/// one enzyme each, and starts with `pools`. Runs one frame so that the blocks
/// exist. No metabolic tick runs.
pub fn spawn_test_cell(app: &mut App, genes: &[BlockKind], pools: &[(Currency, f32)]) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        metabolistic3d::player::spawn_local_player_cell(&mut commands, Genome::default()).1
    };
    app.world_mut().flush();
    set_pools(app, cell, pools);
    express_blocks(app, cell, genes);
    app.update();
    cell
}

/// Sets pools of a cell.
pub fn set_pools(app: &mut App, cell: Entity, values: &[(Currency, f32)]) {
    let mut pools = app.world_mut().get_mut::<CurrencyPools>(cell).unwrap();
    for &(currency, amount) in values {
        pools.set(currency, amount);
    }
}

/// The amount in one pool of a cell.
pub fn pool(app: &App, cell: Entity, currency: Currency) -> f32 {
    app.world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(currency)
}

/// Lipids that a cell keeps as storage beads.
pub fn stored_lipids(app: &App, cell: Entity) -> f32 {
    app.world().get::<StoredLipids>(cell).unwrap().0
}

pub fn set_stored_lipids(app: &mut App, cell: Entity, amount: f32) {
    app.world_mut().get_mut::<StoredLipids>(cell).unwrap().0 = amount;
}

/// Free plus stored lipids of a cell.
pub fn total_lipids(app: &App, cell: Entity) -> f32 {
    pool(app, cell, Currency::Lipids) + stored_lipids(app, cell)
}

/// Sets the enzymes of a block kind on a cell.
pub fn set_enzymes(app: &mut App, cell: Entity, kind: BlockKind, count: u32) {
    app.world_mut()
        .get_mut::<Proteome>(cell)
        .unwrap()
        .enzymes
        .insert(kind, count);
}

/// Adds and expresses genes on a cell, and gives each block one enzyme if it
/// has none. The blocks spawn on the next frame.
pub fn express_blocks(app: &mut App, cell: Entity, kinds: &[BlockKind]) {
    {
        let mut genome = app
            .world_mut()
            .get_mut::<Genome>(cell)
            .expect("cell has a genome");
        for &kind in kinds {
            if genome.get_gene_state(&kind).is_none() {
                genome.add_gene(kind);
            }
            genome.express_gene(kind);
        }
    }
    let mut proteome = app.world_mut().get_mut::<Proteome>(cell).unwrap();
    for &kind in kinds {
        proteome.enzymes.entry(kind).or_insert(1);
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

// --- Reading state ---

/// Get the first cell entity in the world (for single-cell tests)
pub fn get_first_cell_entity(world: &World) -> Option<Entity> {
    world
        .iter_entities()
        .find(|e| e.contains::<metabolistic3d::cell::Cell>())
        .map(|e| e.id())
}

/// All pools of a cell in `Currency::ALL` order, then stored lipids.
pub fn cell_snapshot(app: &App, cell: Entity) -> Vec<f32> {
    let pools = app.world().get::<CurrencyPools>(cell).unwrap();
    let mut snapshot: Vec<f32> = Currency::ALL.iter().map(|&c| pools.get(c)).collect();
    snapshot.push(stored_lipids(app, cell));
    snapshot
}

/// `cell_snapshot` of the first cell.
pub fn get_currency_snapshot(app: &App) -> Vec<f32> {
    get_first_cell_entity(app.world())
        .map(|cell| cell_snapshot(app, cell))
        .unwrap_or_else(|| vec![0.0; Currency::ALL.len() + 1])
}

/// True if all pools of a cell are 0 or more.
pub fn cell_currencies_non_negative(app: &App, cell: Entity) -> bool {
    cell_snapshot(app, cell).iter().all(|&v| v >= 0.0)
}

/// True if all pools of the first cell are 0 or more.
pub fn all_currencies_non_negative(app: &App) -> bool {
    get_first_cell_entity(app.world()).is_some_and(|cell| cell_currencies_non_negative(app, cell))
}

// --- Stepping ---

/// One simulation step: a frame (block sync from genome changes), then one
/// metabolic tick. Test apps use manual ticks, so this is deterministic.
pub fn sim_step(app: &mut App) {
    app.update();
    metabolistic3d::metabolism::run_metabolic_tick(app.world_mut());
}
