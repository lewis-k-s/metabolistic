//! Fat storage block: stores free Lipids as StoredLipids above the lipid
//! toxicity threshold, and releases them below it.

use bevy::prelude::*;
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::blocks::fat_storage::FatStorageBlock;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomePlugin};
use metabolistic3d::metabolism::{FlowmapTimingConfig, MetabolicFlowPlugin, run_metabolic_tick};
use metabolistic3d::molecules::{Currency, CurrencyPools, LipidToxicityThreshold, StoredLipids};
use metabolistic3d::player::spawn_local_player_cell;
use metabolistic3d::proteins::Proteome;

fn app_with_cell(genes: &[BlockKind]) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((GenomePlugin, MetabolicBlocksPlugin, MetabolicFlowPlugin))
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
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(cell)
        .insert(Proteome::with_enzymes(&[(BlockKind::FatStorage, 1)]));
    app.update();
    (app, cell)
}

fn set_lipids(app: &mut App, cell: Entity, free: f32, stored: f32) {
    app.world_mut()
        .get_mut::<CurrencyPools>(cell)
        .unwrap()
        .set(Currency::Lipids, free);
    app.world_mut().get_mut::<StoredLipids>(cell).unwrap().0 = stored;
}

/// (free, stored) lipids.
fn lipids(app: &App, cell: Entity) -> (f32, f32) {
    let free = app
        .world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(Currency::Lipids);
    (free, app.world().get::<StoredLipids>(cell).unwrap().0)
}

fn dt(app: &App) -> f32 {
    app.world().resource::<FlowmapTimingConfig>().target_delta()
}

#[test]
fn test_fat_storage_stores_excess() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_lipids(&mut app, cell, 80.0, 0.0);

    run_metabolic_tick(app.world_mut());

    let stored = FatStorageBlock::default().poly_rate * dt(&app);
    let (free, beads) = lipids(&app, cell);
    assert!((free - (80.0 - stored)).abs() < 1e-4, "{free}");
    assert!((beads - stored).abs() < 1e-4, "{beads}");
    assert!((free + beads - 80.0).abs() < 1e-4, "lipids are conserved");
}

#[test]
fn test_fat_storage_lipolysis() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_lipids(&mut app, cell, 10.0, 40.0);

    run_metabolic_tick(app.world_mut());

    let released = FatStorageBlock::default().lipo_rate * dt(&app);
    let (free, beads) = lipids(&app, cell);
    assert!((free - (10.0 + released)).abs() < 1e-4, "{free}");
    assert!((beads - (40.0 - released)).abs() < 1e-4, "{beads}");
}

#[test]
fn test_no_storage_below_threshold() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_lipids(&mut app, cell, 30.0, 0.0);

    run_metabolic_tick(app.world_mut());

    assert_eq!(lipids(&app, cell), (30.0, 0.0));
}

#[test]
fn test_no_fat_storage_without_gene() {
    let (mut app, cell) = app_with_cell(&[]);
    set_lipids(&mut app, cell, 80.0, 0.0);

    run_metabolic_tick(app.world_mut());

    assert_eq!(lipids(&app, cell), (80.0, 0.0));
}

#[test]
fn test_stored_lipids_survive_gene_loss() {
    let (mut app, cell) = app_with_cell(&[BlockKind::FatStorage]);
    set_lipids(&mut app, cell, 80.0, 0.0);
    run_metabolic_tick(app.world_mut());
    let before = lipids(&app, cell);

    app.world_mut()
        .get_mut::<Genome>(cell)
        .unwrap()
        .table
        .remove(&BlockKind::FatStorage);
    app.update();
    run_metabolic_tick(app.world_mut());

    assert_eq!(lipids(&app, cell), before, "beads stay on the cell");
}
