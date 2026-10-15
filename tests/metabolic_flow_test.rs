//! Metabolic flow plugin: schedule, genome -> block sync, block ownership, timing.

use bevy::prelude::*;
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomePlugin, MutationConfig};
use metabolistic3d::cell::{CellBlocks, OwnedBy};
use metabolistic3d::metabolism::*;
use metabolistic3d::molecules::Currency;
use metabolistic3d::player::spawn_local_player_cell;

fn app(timing: FlowmapTimingConfig) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins((GenomePlugin, MetabolicBlocksPlugin, MetabolicFlowPlugin))
        .insert_resource(MutationConfig::deterministic())
        .insert_resource(timing);
    app
}

fn spawn_cell(app: &mut App, genome: Genome) -> Entity {
    let mut commands = app.world_mut().commands();
    spawn_local_player_cell(&mut commands, genome).1
}

fn block_status(app: &App, cell: Entity, kind: BlockKind) -> Option<BlockStatus> {
    let block = app.world().get::<CellBlocks>(cell)?.get(kind)?;
    app.world().get::<MetabolicNode>(block).map(|n| n.status)
}

#[test]
fn metabolic_flow_plugin_adds_schedule_and_timing() {
    let app = app(FlowmapTimingConfig::default());
    assert!(
        app.world()
            .resource::<Schedules>()
            .contains(MetabolicSchedule)
    );
    assert!(app.world().contains_resource::<FlowmapTimingConfig>());
}

#[test]
fn block_status_follows_cell_genome() {
    let mut app = app(FlowmapTimingConfig::manual());
    let mut genome = Genome::default();
    genome.add_gene(BlockKind::Fermentation);
    let cell = spawn_cell(&mut app, genome);

    app.update();
    assert_eq!(
        block_status(&app, cell, BlockKind::Fermentation),
        Some(BlockStatus::Silent)
    );
    let block = app
        .world()
        .get::<CellBlocks>(cell)
        .unwrap()
        .get(BlockKind::Fermentation)
        .unwrap();
    assert_eq!(app.world().get::<OwnedBy>(block), Some(&OwnedBy(cell)));

    app.world_mut()
        .get_mut::<Genome>(cell)
        .unwrap()
        .express_gene(BlockKind::Fermentation);
    app.update();
    assert_eq!(
        block_status(&app, cell, BlockKind::Fermentation),
        Some(BlockStatus::Active)
    );

    app.world_mut()
        .get_mut::<Genome>(cell)
        .unwrap()
        .mutate_gene(BlockKind::Fermentation);
    app.update();
    assert_eq!(
        block_status(&app, cell, BlockKind::Fermentation),
        Some(BlockStatus::Mutated)
    );
}

#[test]
fn removing_gene_or_cell_despawns_blocks() {
    let mut app = app(FlowmapTimingConfig::manual());
    let mut genome = Genome::default();
    genome.add_gene(BlockKind::Fermentation);
    genome.add_gene(BlockKind::VesicleExport);
    let cell = spawn_cell(&mut app, genome);
    app.update();

    let blocks = app.world().get::<CellBlocks>(cell).unwrap().clone();
    assert_eq!(blocks.len(), 2);
    let fermentation = blocks.get(BlockKind::Fermentation).unwrap();
    let export = blocks.get(BlockKind::VesicleExport).unwrap();

    app.world_mut()
        .get_mut::<Genome>(cell)
        .unwrap()
        .table
        .remove(&BlockKind::Fermentation);
    app.update();
    assert!(app.world().get_entity(fermentation).is_err());
    assert!(app.world().get_entity(export).is_ok());

    app.world_mut().despawn(cell);
    app.update();
    assert!(app.world().get_entity(export).is_err());
}

#[test]
fn cells_are_isolated() {
    let mut app = app(FlowmapTimingConfig::manual());
    let mut genome = Genome::default();
    genome.add_gene(BlockKind::Fermentation);
    genome.express_gene(BlockKind::Fermentation);
    let fermenter = spawn_cell(&mut app, genome);
    let bystander = spawn_cell(&mut app, Genome::default());
    app.update();

    for cell in [fermenter, bystander] {
        let mut pools = app.world_mut().get_mut::<CurrencyPools>(cell).unwrap();
        pools.set(Currency::Pyruvate, 10.0);
        pools.set(Currency::ReducingPower, 10.0);
    }
    run_metabolic_tick(app.world_mut());

    let atp = |cell| {
        app.world()
            .get::<CurrencyPools>(cell)
            .unwrap()
            .get(Currency::ATP)
    };
    assert!(atp(fermenter) > 0.0);
    assert_eq!(atp(bystander), 0.0);
}

#[test]
fn real_time_driver_runs_ticks_unless_manual() {
    // Manual: frames do not tick.
    let mut manual = app(FlowmapTimingConfig::manual());
    let mut genome = Genome::default();
    genome.add_gene(BlockKind::VesicleExport);
    genome.express_gene(BlockKind::VesicleExport);
    let cell = spawn_cell(&mut manual, genome.clone());
    manual.update();
    manual
        .world_mut()
        .get_mut::<CurrencyPools>(cell)
        .unwrap()
        .set(Currency::OrganicWaste, 10.0);
    manual
        .world_mut()
        .resource_mut::<FlowmapTimingConfig>()
        .accumulator = 10.0;
    manual.update();
    let waste = manual
        .world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(Currency::OrganicWaste);
    assert_eq!(waste, 10.0);

    // Real time: one full tick of accumulated time runs one tick.
    let mut timed = app(FlowmapTimingConfig::default());
    let cell = spawn_cell(&mut timed, genome);
    timed.update();
    timed
        .world_mut()
        .get_mut::<CurrencyPools>(cell)
        .unwrap()
        .set(Currency::OrganicWaste, 10.0);
    let step = timed
        .world()
        .resource::<FlowmapTimingConfig>()
        .target_delta();
    timed
        .world_mut()
        .resource_mut::<FlowmapTimingConfig>()
        .accumulator = step;
    timed.update();
    let waste = timed
        .world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(Currency::OrganicWaste);
    assert!(waste < 10.0);
}
