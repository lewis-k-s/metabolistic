//! # Milestone M1: simulation core
//!
//! Done when (see `docs/gameplay-roadmap.md`, M1): a headless test cell lives
//! on a constant mineral supply, builds enzymes from a queue, and two pathways
//! share an input in proportion to their enzymes.

use bevy::prelude::*;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::genome::{BlockKind, create_starter_genome};
use metabolistic3d::cell::{CellBlocks, is_alive};
use metabolistic3d::environment::LocalEnvironment;
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::{BlockTurnover, CurrencyPools};
use metabolistic3d::molecules::{Currency, EnvironmentalCurrencyPools};
use metabolistic3d::player::spawn_local_player_cell;
use metabolistic3d::proteins::{
    MembraneProtein, ProteinTarget, Proteome, QueueProtein, RecycleProtein, SynthesisQueue,
    recycle_yield,
};
use metabolistic3d::shared::{STARTER_VENT_LEVEL, starter_proteome};

mod common;
use common::*;

/// A vent cell with the starter genome and proteome and small pools. The app
/// has no outside pools, so the vent supply is the only food.
fn vent_cell(env: LocalEnvironment) -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    app.world_mut()
        .remove_resource::<EnvironmentalCurrencyPools>();
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, create_starter_genome()).1
    };
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(cell)
        .insert((starter_proteome(), env));
    set_pools(
        &mut app,
        cell,
        &[
            (Currency::ATP, 20.0),
            (Currency::ReducingPower, 5.0),
            (Currency::CarbonSkeletons, 5.0),
            (Currency::AminoAcids, 5.0),
            (Currency::Lipids, 10.0),
            (Currency::Metals, 5.0),
        ],
    );
    app.update(); // spawn the blocks
    (app, cell)
}

fn alive(app: &App, cell: Entity) -> bool {
    is_alive(
        app.world().get::<CurrencyPools>(cell).unwrap(),
        app.world().get::<Membrane>(cell).unwrap(),
    )
}

/// Ticks per simulated second (default timing).
const TICKS_PER_SEC: usize = 5;

#[test]
fn cell_lives_on_constant_mineral_supply_and_builds_enzymes() {
    let (mut app, cell) = vent_cell(LocalEnvironment::vent(STARTER_VENT_LEVEL));
    app.world_mut()
        .get_mut::<Membrane>(cell)
        .unwrap()
        .damage(20.0);
    let orders = [
        ProteinTarget::Enzyme(BlockKind::Chemosynthesis),
        ProteinTarget::Enzyme(BlockKind::Chemosynthesis),
        ProteinTarget::Enzyme(BlockKind::ProteinSynthesis),
        ProteinTarget::Membrane(MembraneProtein::Flagellum),
    ];
    for target in orders {
        app.world_mut().write_message(QueueProtein { cell, target });
    }
    let before = app.world().get::<Proteome>(cell).unwrap().clone();

    // Three simulated minutes.
    for tick in 0..180 * TICKS_PER_SEC {
        sim_step(&mut app);
        assert!(alive(&app, cell), "cell died at tick {tick}");
        assert!(cell_currencies_non_negative(&app, cell));
    }

    let proteome = app.world().get::<Proteome>(cell).unwrap();
    let queue = app.world().get::<SynthesisQueue>(cell).unwrap();
    assert!(queue.is_empty(), "queue not finished: {queue:?}");
    assert_eq!(
        proteome.enzymes(BlockKind::Chemosynthesis),
        before.enzymes(BlockKind::Chemosynthesis) + 2
    );
    assert_eq!(
        proteome.enzymes(BlockKind::ProteinSynthesis),
        before.enzymes(BlockKind::ProteinSynthesis) + 1
    );
    assert_eq!(
        proteome.count(ProteinTarget::Membrane(MembraneProtein::Flagellum)),
        1
    );
    let membrane = app.world().get::<Membrane>(cell).unwrap();
    assert_eq!(
        membrane.integrity, membrane.max,
        "lipids repaired the membrane"
    );
}

#[test]
fn cell_without_mineral_supply_dies() {
    let (mut app, cell) = vent_cell(LocalEnvironment::default());
    let mut died_at = None;
    for tick in 0..600 * TICKS_PER_SEC {
        sim_step(&mut app);
        if !alive(&app, cell) {
            died_at = Some(tick);
            break;
        }
    }
    assert!(died_at.is_some(), "a cell with no food must run out of ATP");
}

#[test]
fn two_pathways_share_an_input_in_proportion_to_their_enzymes() {
    // Amino acid and lipid synthesis both use carbon skeletons. ATP and
    // reducing power are plentiful. With many enzymes, the carbon demand in
    // one sub-step is more than the pool holds, so the blocks must share it.
    let mut app = MetabolisticApp::new_headless();
    app.world_mut()
        .remove_resource::<EnvironmentalCurrencyPools>();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::AminoAcidBiosynthesis, BlockKind::LipidMetabolism],
        &[
            (Currency::CarbonSkeletons, 0.5),
            (Currency::ATP, 1000.0),
            (Currency::ReducingPower, 1000.0),
        ],
    );
    set_enzymes(&mut app, cell, BlockKind::AminoAcidBiosynthesis, 90);
    set_enzymes(&mut app, cell, BlockKind::LipidMetabolism, 30);

    sim_step(&mut app);

    let turnover = |kind| {
        let block = app
            .world()
            .get::<CellBlocks>(cell)
            .unwrap()
            .get(kind)
            .unwrap();
        app.world().get::<BlockTurnover>(block).unwrap().0
    };
    let amino = turnover(BlockKind::AminoAcidBiosynthesis);
    let lipid = turnover(BlockKind::LipidMetabolism);
    assert!(amino > 0.0 && lipid > 0.0);
    assert!(
        (amino / lipid - 3.0).abs() < 0.01,
        "turnover ratio {} for 90:30 enzymes",
        amino / lipid
    );
    // The shared input ran out, so the share rule (not saturation) set the split.
    assert!(pool(&app, cell, Currency::CarbonSkeletons) < 1e-3);
}

#[test]
fn recycling_a_protein_returns_half_its_amino_acids() {
    let (mut app, cell) = vent_cell(LocalEnvironment::default());
    let amino_before = pool(&app, cell, Currency::AminoAcids);
    let target = ProteinTarget::Enzyme(BlockKind::LipidMetabolism);
    app.world_mut()
        .write_message(RecycleProtein { cell, target });
    app.update();

    let proteome = app.world().get::<Proteome>(cell).unwrap();
    assert_eq!(proteome.enzymes(BlockKind::LipidMetabolism), 0);
    assert_eq!(
        pool(&app, cell, Currency::AminoAcids),
        amino_before + recycle_yield()
    );
}
