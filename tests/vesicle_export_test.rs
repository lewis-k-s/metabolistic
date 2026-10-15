//! Vesicle export block: removes OrganicWaste from the cell.

use bevy::prelude::*;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomePlugin, MutationConfig};
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::metabolism::{run_metabolic_tick, FlowmapTimingConfig, MetabolicFlowPlugin};
use metabolistic3d::molecules::{Currency, CurrencyPools};
use metabolistic3d::player::spawn_local_player_cell;

fn app_with_cell(genes: &[BlockKind]) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((GenomePlugin, MetabolicBlocksPlugin, MetabolicFlowPlugin))
        .insert_resource(MutationConfig::deterministic())
        .insert_resource(FlowmapTimingConfig::manual());

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

fn waste(app: &App, cell: Entity) -> f32 {
    app.world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(Currency::OrganicWaste)
}

#[test]
fn test_vesicle_export_decreases_organic_waste() {
    let (mut app, cell) = app_with_cell(&[BlockKind::VesicleExport]);
    app.world_mut()
        .get_mut::<CurrencyPools>(cell)
        .unwrap()
        .set(Currency::OrganicWaste, 100.0);

    run_metabolic_tick(app.world_mut());
    let after_one = waste(&app, cell);
    assert!(after_one < 100.0, "{after_one}");

    for _ in 0..1000 {
        run_metabolic_tick(app.world_mut());
    }
    assert_eq!(waste(&app, cell), 0.0, "export stops at zero");
}

#[test]
fn test_no_export_without_gene() {
    let (mut app, cell) = app_with_cell(&[]);
    app.world_mut()
        .get_mut::<CurrencyPools>(cell)
        .unwrap()
        .set(Currency::OrganicWaste, 100.0);

    run_metabolic_tick(app.world_mut());

    assert_eq!(waste(&app, cell), 100.0);
}
