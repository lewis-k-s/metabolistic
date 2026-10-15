//! Fermentation block: Pyruvate + ReducingPower -> ATP + OrganicWaste (1:1:1:1).

use bevy::prelude::*;
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::blocks::fermentation;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomePlugin, MutationConfig};
use metabolistic3d::metabolism::{FlowmapTimingConfig, MetabolicFlowPlugin, run_metabolic_tick};
use metabolistic3d::molecules::{Currency, CurrencyPools};
use metabolistic3d::player::spawn_local_player_cell;

/// App with one cell that expresses `genes`. Blocks exist after the first frame.
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
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation]);
    set_pools(
        &mut app,
        cell,
        &[(Currency::Pyruvate, 10.0), (Currency::ReducingPower, 10.0)],
    );

    run_metabolic_tick(app.world_mut());

    let dt = app.world().resource::<FlowmapTimingConfig>().target_delta();
    let expected = fermentation::RATE * dt;
    assert!((pool(&app, cell, Currency::Pyruvate) - (10.0 - expected)).abs() < 1e-5);
    assert!((pool(&app, cell, Currency::ReducingPower) - (10.0 - expected)).abs() < 1e-5);
    assert!((pool(&app, cell, Currency::ATP) - expected).abs() < 1e-5);
    assert!((pool(&app, cell, Currency::OrganicWaste) - expected).abs() < 1e-5);
}

#[test]
fn test_fermentation_requires_inputs() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation]);
    set_pools(&mut app, cell, &[(Currency::ReducingPower, 10.0)]);

    run_metabolic_tick(app.world_mut());

    assert_eq!(pool(&app, cell, Currency::ATP), 0.0);
    assert_eq!(pool(&app, cell, Currency::OrganicWaste), 0.0);
    assert_eq!(pool(&app, cell, Currency::ReducingPower), 10.0);
}

#[test]
fn test_fermentation_stoichiometry() {
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation]);
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

    // Enough ticks to use up the limiting input.
    for _ in 0..100 {
        run_metabolic_tick(app.world_mut());
    }

    let pyruvate_consumed = initial_pyruvate - pool(&app, cell, Currency::Pyruvate);
    let reducing_consumed = initial_reducing - pool(&app, cell, Currency::ReducingPower);
    let atp = pool(&app, cell, Currency::ATP);
    let waste = pool(&app, cell, Currency::OrganicWaste);

    assert!(
        (pyruvate_consumed - initial_pyruvate).abs() < 1e-4,
        "{pyruvate_consumed}"
    );
    assert!((reducing_consumed - pyruvate_consumed).abs() < 1e-4);
    assert!((atp - pyruvate_consumed).abs() < 1e-4);
    assert!((waste - pyruvate_consumed).abs() < 1e-4);
}

#[test]
fn test_no_fermentation_without_gene() {
    let (mut app, cell) = app_with_cell(&[]);
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
    let (mut app, cell) = app_with_cell(&[BlockKind::Fermentation]);
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
