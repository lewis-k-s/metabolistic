//! The metabolism view (Manage screen data): `block_limit`, the `FlowLedger`
//! and `MetabolismView`. See `docs/metabolism-view.md`.

use std::collections::HashMap;

use bevy::prelude::*;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::genome::{BlockKind, create_starter_genome};
use metabolistic3d::cell::CellBlocks;
use metabolistic3d::environment::{Channel, LocalEnvironment};
use metabolistic3d::metabolism::view::{LocalMetabolismView, MetabolismView, Node};
use metabolistic3d::metabolism::{
    BlockInput, BlockStatus, BlockTurnover, CurrencyPools, FlowLedger, FlowSource, FluxProfile,
    Limit, MAX_SUBSTEP, block_limit, block_limits,
};
use metabolistic3d::molecules::Currency;
use metabolistic3d::movement::ThrustEffort;
use metabolistic3d::player::spawn_local_player_cell;
use metabolistic3d::shared::{STARTER_VENT_LEVEL, starter_proteome};

use crate::common::*;

// --- block_limit: hand-made cases ---

fn profile(entries: &[(Currency, f32)]) -> FluxProfile {
    FluxProfile(entries.iter().copied().collect())
}

fn pools(entries: &[(Currency, f32)]) -> HashMap<Currency, f32> {
    entries.iter().copied().collect()
}

#[test]
fn limit_names_the_starved_input() {
    let p = profile(&[
        (Currency::Pyruvate, -1.0),
        (Currency::ReducingPower, -1.0),
        (Currency::ATP, 1.0),
    ]);
    let block = BlockInput::new(&p, 1.0).with_k_half(5.0);
    let rich = pools(&[(Currency::Pyruvate, 100.0), (Currency::ReducingPower, 100.0)]);
    assert_eq!(block_limit(&rich, &block), Limit::None);

    let starved = pools(&[(Currency::Pyruvate, 100.0), (Currency::ReducingPower, 0.1)]);
    assert_eq!(block_limit(&starved, &block), Limit::Input(Currency::ReducingPower));
    let empty = pools(&[(Currency::Pyruvate, 0.0), (Currency::ReducingPower, 100.0)]);
    assert_eq!(block_limit(&empty, &block), Limit::Input(Currency::Pyruvate));
}

#[test]
fn limit_names_the_cofactor() {
    let p = profile(&[(Currency::CarbonSkeletons, -1.0), (Currency::ATP, 1.0)]);
    let cofactors = [Currency::Metals];
    let block = BlockInput::new(&p, 1.0).with_k_half(5.0).with_cofactors(&cofactors);
    let no_metals = pools(&[(Currency::CarbonSkeletons, 100.0)]);
    assert_eq!(block_limit(&no_metals, &block), Limit::Cofactor(Currency::Metals));
}

#[test]
fn limit_off_and_no_enzymes() {
    let p = profile(&[(Currency::ATP, 1.0)]);
    let silent = BlockInput::new(&p, 1.0).with_status(BlockStatus::Silent);
    assert_eq!(block_limit(&HashMap::new(), &silent), Limit::Off);
    let drive_off = BlockInput { drive: 0.0, ..BlockInput::new(&p, 1.0) };
    assert_eq!(block_limit(&HashMap::new(), &drive_off), Limit::Off);
    let none = BlockInput::new(&p, 0.0);
    assert_eq!(block_limit(&HashMap::new(), &none), Limit::NoEnzymes);
}

#[test]
fn limit_names_the_outside_channel() {
    let p = profile(&[(Currency::ATP, 1.0)]);
    let block = BlockInput {
        external: 0.05,
        external_channel: Some(Channel::Heat),
        ..BlockInput::new(&p, 1.0)
    };
    assert_eq!(block_limit(&HashMap::new(), &block), Limit::External(Channel::Heat));
}

#[test]
fn limit_says_shared_only_when_other_blocks_want_the_pool() {
    let p = profile(&[(Currency::CarbonSkeletons, -1.0), (Currency::ATP, 1.0)]);
    let scarce = pools(&[(Currency::CarbonSkeletons, 0.02)]);
    let two = [BlockInput::new(&p, 1.0), BlockInput::new(&p, 1.0)];
    assert_eq!(
        block_limits(&scarce, &two, MAX_SUBSTEP),
        vec![Limit::Shared(Currency::CarbonSkeletons); 2]
    );
    let one = [BlockInput::new(&p, 1.0)];
    assert_eq!(
        block_limits(&scarce, &one, MAX_SUBSTEP),
        vec![Limit::Input(Currency::CarbonSkeletons)]
    );
    let plenty = pools(&[(Currency::CarbonSkeletons, 50.0)]);
    assert_eq!(block_limits(&plenty, &two, MAX_SUBSTEP), vec![Limit::None; 2]);
}

// --- A running cell ---

fn vent_cell() -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, create_starter_genome()).1
    };
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(cell)
        .insert((starter_proteome(), LocalEnvironment::vent(STARTER_VENT_LEVEL)));
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

/// The view of the local cell now, with no smoothing.
fn fresh_view(app: &mut App) -> MetabolismView {
    app.world_mut().resource_mut::<LocalMetabolismView>().0 = None;
    app.update();
    app.world()
        .resource::<LocalMetabolismView>()
        .0
        .clone()
        .expect("the local player has a view")
}

fn all_pools(app: &App, cell: Entity) -> HashMap<Currency, f32> {
    app.world().get::<CurrencyPools>(cell).unwrap().pools.clone()
}

fn ledger_totals(app: &App, cell: Entity) -> HashMap<Currency, f32> {
    let ledger = app.world().get::<FlowLedger>(cell).unwrap();
    Currency::ALL.iter().map(|&c| (c, ledger.total_of(c))).collect()
}

#[test]
fn view_flux_matches_the_pool_change_of_a_tick() {
    let (mut app, cell) = vent_cell();
    let before = all_pools(&app, cell);
    let ledger_before = ledger_totals(&app, cell);
    sim_step(&mut app);
    let after = all_pools(&app, cell);
    let ledger_after = ledger_totals(&app, cell);

    let view = fresh_view(&mut app);
    let dt = 1.0 / 5.0;
    let mut moved = 0.0;
    for currency in Currency::ALL {
        let delta = after.get(&currency).copied().unwrap_or(0.0)
            - before.get(&currency).copied().unwrap_or(0.0);
        // What did not come from a pathway came through the ledger.
        let outside = ledger_after[&currency] - ledger_before[&currency];
        let from_pathways = view.pathway_net(currency) * dt;
        assert!(
            (delta - outside - from_pathways).abs() < 1e-3,
            "{currency:?}: delta {delta}, ledger {outside}, view {from_pathways}"
        );
        moved += from_pathways.abs();
    }
    assert!(moved > 0.0, "the cell ran at least one pathway");
    assert!(view.pathways.iter().any(|p| p.owned && p.activity > 0.0));
}

#[test]
fn view_marks_ghosts_and_names_only_known_ones() {
    let (mut app, _cell) = vent_cell();
    let view = fresh_view(&mut app);
    // Respiration has a spec; the starter genome does not have it.
    let ghost = view.pathway(BlockKind::Respiration).expect("on the map");
    assert!(!ghost.owned && !ghost.known && ghost.state.is_none());
    // Its pipes exist, with no flux.
    let pipe = view
        .flux(Node::Pool(Currency::O2), Node::Pathway(BlockKind::Respiration), Currency::O2)
        .expect("ghost pipe");
    assert_eq!(pipe, 0.0);
    // A silent gene the cell has is owned.
    let silent = view.pathway(BlockKind::Fermentation).expect("gene");
    assert!(silent.owned && silent.state == Some(BlockStatus::Silent));
    // A kind with no block spec is not on the map.
    assert!(view.pathway(BlockKind::Multicellularity).is_none());
    // The own pathway is known and owned.
    let own = view.pathway(BlockKind::Chemosynthesis).expect("owned");
    assert!(own.owned && own.known && own.state.is_some());
}

#[test]
fn view_names_a_ghost_that_the_codex_holds() {
    let (mut app, _cell) = vent_cell();
    app.insert_resource(metabolistic3d::codex::Codex {
        genes: [BlockKind::Respiration].into_iter().collect(),
    });
    let view = fresh_view(&mut app);
    let ghost = view.pathway(BlockKind::Respiration).unwrap();
    assert!(!ghost.owned && ghost.known);
}

#[test]
fn view_limit_names_missing_heat() {
    let (mut app, cell) = vent_cell();
    app.world_mut()
        .entity_mut(cell)
        .insert(LocalEnvironment::default()); // no heat
    sim_step(&mut app);
    let view = fresh_view(&mut app);
    let chemo = view.pathway(BlockKind::Chemosynthesis).unwrap();
    assert_eq!(chemo.limit, Limit::External(Channel::Heat));
}

// --- Ledger ---

#[test]
fn ledger_sums_match_the_pool_changes_outside_the_solver() {
    let (mut app, cell) = vent_cell();
    let start = all_pools(&app, cell);
    let mut block_flux: HashMap<Currency, f32> = HashMap::new();

    for _ in 0..12 {
        // Thrust costs ATP, and a damaged membrane takes lipids for repair.
        app.world_mut().get_mut::<ThrustEffort>(cell).unwrap().0 = 2.0;
        app.world_mut()
            .get_mut::<metabolistic3d::membrane::Membrane>(cell)
            .unwrap()
            .damage(3.0);
        sim_step(&mut app);
        let blocks = app.world().get::<CellBlocks>(cell).unwrap().0.clone();
        for (_, entity) in blocks {
            let turnover = app.world().get::<BlockTurnover>(entity).unwrap().0;
            let profile = app.world().get::<FluxProfile>(entity).unwrap().clone();
            for (currency, flow) in profile.0 {
                *block_flux.entry(currency).or_default() += flow * turnover;
            }
        }
    }

    let end = all_pools(&app, cell);
    let ledger = app.world().get::<FlowLedger>(cell).unwrap();
    for currency in Currency::ALL {
        let delta = end.get(&currency).copied().unwrap_or(0.0)
            - start.get(&currency).copied().unwrap_or(0.0);
        let expected = block_flux.get(&currency).copied().unwrap_or(0.0) + ledger.total_of(currency);
        assert!(
            (delta - expected).abs() < 1e-2,
            "{currency:?}: pool change {delta}, blocks + ledger {expected}"
        );
    }
    // The test is not empty: the writers were used.
    assert!(ledger.total(FlowSource::Movement, Currency::ATP) < 0.0);
    assert!(ledger.total(FlowSource::MembraneRepair, Currency::Lipids) < 0.0);
    assert!(ledger.total(FlowSource::Upkeep, Currency::ATP) < 0.0);
    assert!(ledger.total(FlowSource::Intake, Currency::Minerals) > 0.0);
    assert!(ledger.rate(FlowSource::Movement, Currency::ATP) < 0.0);
}
