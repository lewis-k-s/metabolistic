//! Fat storage block: stores FreeFattyAcids as StorageBeads above the lipid
//! toxicity threshold, and releases them at or below it.

use bevy::prelude::*;
use metabolistic3d::blocks::fat_storage::FatStorageBlock;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomePlugin, MutationConfig};
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::metabolism::{run_metabolic_tick, FlowmapTimingConfig, MetabolicFlowPlugin};
use metabolistic3d::molecules::{Currency, CurrencyPools, LipidToxicityThreshold};
use metabolistic3d::player::spawn_local_player_cell;

fn app_with_cell(genes: &[BlockKind]) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((GenomePlugin, MetabolicBlocksPlugin, MetabolicFlowPlugin))
        .insert_resource(MutationConfig::deterministic())
        .insert_resource(FlowmapTimingConfig::manual())
        .insert_resource(LipidToxicityThreshold(50.0));

    let mut genome = Genome::default();
    for &kind in genes {
        genome.add_gene(kind);
        genome.express_gene(kind);
    }
    let (_player, cell) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, genome)
    };
    app.update();
    (app, cell)
}

fn set_pools(app: &mut App, cell: Entity, values: &[(Currency, f32)]) {
    let mut pools = app.world_mut().get_mut::<CurrencyPools>(cell).unwrap();
    for &(currency, amount) in values {
        pools.set(currency, amount);
    }
}

fn pool(app: &App, cell: Entity, currency: Currency) -> f32 {
    app.world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(currency)
}

fn dt(app: &App) -> f32 {
    app.world().resource::<FlowmapTimingConfig>().target_delta()
}

#[test]
fn test_fat_storage_polymerization() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_pools(&mut app, cell, &[(Currency::FreeFattyAcids, 80.0)]);

    run_metabolic_tick(app.world_mut());

    let stored = FatStorageBlock::default().poly_rate * dt(&app);
    let ffa = pool(&app, cell, Currency::FreeFattyAcids);
    let beads = pool(&app, cell, Currency::StorageBeads);
    assert!((ffa - (80.0 - stored)).abs() < 1e-4, "{ffa}");
    assert!((beads - stored).abs() < 1e-4, "{beads}");
    assert!((ffa + beads - 80.0).abs() < 1e-4, "lipids are conserved");
}

#[test]
fn test_fat_storage_lipolysis() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_pools(
        &mut app,
        cell,
        &[
            (Currency::FreeFattyAcids, 10.0),
            (Currency::StorageBeads, 40.0),
        ],
    );

    run_metabolic_tick(app.world_mut());

    let released = FatStorageBlock::default().lipo_rate * dt(&app);
    let ffa = pool(&app, cell, Currency::FreeFattyAcids);
    let beads = pool(&app, cell, Currency::StorageBeads);
    assert!((ffa - (10.0 + released)).abs() < 1e-4, "{ffa}");
    assert!((beads - (40.0 - released)).abs() < 1e-4, "{beads}");
}

#[test]
fn test_no_polymerization_below_threshold() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_pools(&mut app, cell, &[(Currency::FreeFattyAcids, 30.0)]);

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::FreeFattyAcids), 30.0);
    assert_eq!(pool(&app, cell, Currency::StorageBeads), 0.0);
}

#[test]
fn test_no_fat_storage_without_gene() {
    let (mut app, cell) = app_with_cell(&[]);
    set_pools(&mut app, cell, &[(Currency::FreeFattyAcids, 80.0)]);

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::FreeFattyAcids), 80.0);
    assert_eq!(pool(&app, cell, Currency::StorageBeads), 0.0);
}
