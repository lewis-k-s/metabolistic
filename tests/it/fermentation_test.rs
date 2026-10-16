//! Fermentation block: Pyruvate + ReducingPower -> ATP + Waste (1:1:1:1).

use bevy::prelude::*;
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::blocks::fermentation;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomePlugin};
use metabolistic3d::cell::CellSize;
use metabolistic3d::metabolism::{
    DEFAULT_K_HALF, FlowmapTimingConfig, MetabolicFlowPlugin, run_metabolic_tick,
};
use metabolistic3d::molecules::{Currency, CurrencyPools};
use metabolistic3d::player::spawn_local_player_cell;
use metabolistic3d::proteins::Proteome;

/// App with one cell that expresses `genes` with `enzymes` each. The cell has
/// no maintenance cost. Blocks exist after the first frame.
fn app_with_cell(genes: &[BlockKind], enzymes: u32) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((GenomePlugin, MetabolicBlocksPlugin, MetabolicFlowPlugin))
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
    app.world_mut().flush();
    let enzymes: Vec<_> = genes.iter().map(|&k| (k, enzymes)).collect();
    app.world_mut()
        .entity_mut(cell)
        .insert((Proteome::with_enzymes(&enzymes), CellSize { radius: 0.0 }));
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

#[test]
fn test_fermentation_basic_flow() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation], 1);
    set_pools(
        &mut app,
        cell,
        &[
            (Currency::Pyruvate, 1000.0),
            (Currency::ReducingPower, 1000.0),
        ],
    );

    run_metabolic_tick(app.world_mut());

    // With large pools the block is close to saturation.
    let dt = app.world().resource::<FlowmapTimingConfig>().target_delta();
    let saturation = 1000.0 / (DEFAULT_K_HALF + 1000.0);
    let expected = fermentation::RATE * dt * saturation;
    let atp = pool(&app, cell, Currency::ATP);
    assert!((atp - expected).abs() < 1e-3, "{atp} vs {expected}");
    assert!((pool(&app, cell, Currency::Waste) - atp).abs() < 1e-5);
    assert!((pool(&app, cell, Currency::Pyruvate) - (1000.0 - atp)).abs() < 1e-3);
    assert!((pool(&app, cell, Currency::ReducingPower) - (1000.0 - atp)).abs() < 1e-3);
}

#[test]
fn test_more_enzymes_ferment_faster() {
    let start = [
        (Currency::Pyruvate, 100.0),
        (Currency::ReducingPower, 100.0),
    ];
    let (mut one, cell_one) = app_with_cell(&[BlockKind::Fermentation], 1);
    let (mut three, cell_three) = app_with_cell(&[BlockKind::Fermentation], 3);
    set_pools(&mut one, cell_one, &start);
    set_pools(&mut three, cell_three, &start);

    run_metabolic_tick(one.world_mut());
    run_metabolic_tick(three.world_mut());

    let ratio = pool(&three, cell_three, Currency::ATP) / pool(&one, cell_one, Currency::ATP);
    assert!((ratio - 3.0).abs() < 0.05, "{ratio}");
}

#[test]
fn test_fermentation_requires_inputs() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation], 1);
    set_pools(&mut app, cell, &[(Currency::ReducingPower, 10.0)]);

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::ATP), 0.0);
    assert_eq!(pool(&app, cell, Currency::Waste), 0.0);
    assert_eq!(pool(&app, cell, Currency::ReducingPower), 10.0);
}

#[test]
fn test_fermentation_requires_enzymes() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation], 0);
    set_pools(
        &mut app,
        cell,
        &[(Currency::Pyruvate, 10.0), (Currency::ReducingPower, 10.0)],
    );

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::ATP), 0.0);
}

#[test]
fn test_fermentation_stoichiometry() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation], 1);
    let initial_pyruvate = 5.0;
    let initial_reducing = 8.0;
    set_pools(
        &mut app,
        cell,
        &[
            (Currency::Pyruvate, initial_pyruvate),
            (Currency::ReducingPower, initial_reducing),
        ],
    );

    // Saturation slows the block as pyruvate runs low; most of it is used.
    for _ in 0..500 {
        run_metabolic_tick(app.world_mut());
    }

    let pyruvate_used = initial_pyruvate - pool(&app, cell, Currency::Pyruvate);
    let reducing_used = initial_reducing - pool(&app, cell, Currency::ReducingPower);
    let atp = pool(&app, cell, Currency::ATP);
    let waste = pool(&app, cell, Currency::Waste);

    assert!(pyruvate_used > 0.95 * initial_pyruvate, "{pyruvate_used}");
    assert!((reducing_used - pyruvate_used).abs() < 1e-4);
    assert!((atp - pyruvate_used).abs() < 1e-4);
    assert!((waste - pyruvate_used).abs() < 1e-4);
}

#[test]
fn test_no_fermentation_without_gene() {
    let (mut app, cell) = app_with_cell(&[], 1);
    set_pools(
        &mut app,
        cell,
        &[(Currency::Pyruvate, 10.0), (Currency::ReducingPower, 10.0)],
    );

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::Pyruvate), 10.0);
    assert_eq!(pool(&app, cell, Currency::ATP), 0.0);
}

#[test]
fn test_silenced_gene_stops_flux() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation], 1);
    set_pools(
        &mut app,
        cell,
        &[(Currency::Pyruvate, 10.0), (Currency::ReducingPower, 10.0)],
    );
    app.world_mut()
        .get_mut::<Genome>(cell)
        .unwrap()
        .silence_gene(BlockKind::Fermentation);
    app.update(); // block sync

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::Pyruvate), 10.0);
}
