//! # Milestone M4: build and regulate (headless)
//!
//! Action slots (pathway levels, ramp, ATP cost), shelters, the editor's
//! synthesis queue, induced mutation and the `Enhanced` gene state.

use approx::assert_abs_diff_eq;
use bevy::prelude::*;
use metabolistic3d::actions::*;
use metabolistic3d::blocks::genome::{BlockKind, GeneState, Genome};
use metabolistic3d::cell::{CellBlocks, CellSize, DamageTaken, membrane_slots};
use metabolistic3d::environment::{Channel, LocalEnvironment};
use metabolistic3d::genome_damage::*;
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::{
    BlockInput, BlockStatus, ENHANCED_THROUGHPUT, FlowmapTimingConfig, FluxProfile,
    MetabolicNode, solve_tick,
};
use metabolistic3d::molecules::Currency;
use metabolistic3d::proteins::{
    MembraneProtein, ProteinTarget, Proteome, QueueProtein, RecycleProtein, SynthesisQueue,
    UNITS_PER_PROTEIN, recycle_yield,
};
use metabolistic3d::shelter::{Shelter, Shelters};
use metabolistic3d::{GameState, MetabolisticApp};

use crate::common::*;

fn throttle_of(app: &App, cell: Entity, kind: BlockKind) -> f32 {
    let block = app
        .world()
        .get::<CellBlocks>(cell)
        .unwrap()
        .get(kind)
        .expect("the block exists");
    app.world().get::<Throttle>(block).unwrap().0
}

fn use_slot(app: &mut App, cell: Entity, slot: usize) {
    app.world_mut().write_message(UseSlot { cell, slot });
    app.update();
}

fn set_bar(app: &mut App, cell: Entity, slots: [Option<ActionKind>; SLOT_COUNT]) {
    app.world_mut().entity_mut(cell).insert(ActionBar { slots });
}

/// An app with the given tick length (ticks per second) and a fermentation cell.
fn fermenting_cell(ticks_per_second: f32) -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(FlowmapTimingConfig {
        manual: true,
        ..FlowmapTimingConfig::new(ticks_per_second)
    });
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::Fermentation],
        &[
            (Currency::ATP, 50.0),
            (Currency::Pyruvate, 1000.0),
            (Currency::ReducingPower, 1000.0),
        ],
    );
    set_bar(
        &mut app,
        cell,
        [Some(ActionKind::Pathway(BlockKind::Fermentation)), None, None, None],
    );
    app.update();
    (app, cell)
}

fn ticks_for(seconds: f32, ticks_per_second: f32) -> usize {
    (seconds * ticks_per_second).round() as usize
}

// --- Slots: pathway levels ---

#[test]
fn a_slot_use_costs_one_atp_and_sets_the_level() {
    let (mut app, cell) = fermenting_cell(5.0);
    let atp = pool(&app, cell, Currency::ATP);
    use_slot(&mut app, cell, 0);
    assert_abs_diff_eq!(pool(&app, cell, Currency::ATP), atp - LEVEL_CHANGE_ATP, epsilon = 1e-4);
    let levels = app.world().get::<PathwayLevels>(cell).unwrap();
    assert_eq!(levels.level(BlockKind::Fermentation), Level::Off);
    // Off -> Low -> High -> Off
    use_slot(&mut app, cell, 0);
    use_slot(&mut app, cell, 0);
    let levels = app.world().get::<PathwayLevels>(cell).unwrap();
    assert_eq!(levels.level(BlockKind::Fermentation), Level::High);
}

#[test]
fn with_too_little_atp_the_level_does_not_change() {
    let (mut app, cell) = fermenting_cell(5.0);
    set_pools(&mut app, cell, &[(Currency::ATP, 0.5)]);
    use_slot(&mut app, cell, 0);
    assert_eq!(pool(&app, cell, Currency::ATP), 0.5);
    let levels = app.world().get::<PathwayLevels>(cell).unwrap();
    assert_eq!(levels.level(BlockKind::Fermentation), Level::High);
}

#[test]
fn off_stops_the_flux_of_a_block_after_the_ramp() {
    let (mut app, cell) = fermenting_cell(5.0);
    for _ in 0..5 {
        sim_step(&mut app);
    }
    let waste = pool(&app, cell, Currency::Waste);
    assert!(waste > 0.0, "fermentation runs at the start");

    use_slot(&mut app, cell, 0); // Off
    // The ramp takes a second at 5 ticks per second; after 2 s it is done.
    for _ in 0..ticks_for(2.0, 5.0) {
        sim_step(&mut app);
    }
    assert_eq!(throttle_of(&app, cell, BlockKind::Fermentation), 0.0);
    let waste = pool(&app, cell, Currency::Waste);
    let pyruvate = pool(&app, cell, Currency::Pyruvate);
    for _ in 0..20 {
        sim_step(&mut app);
    }
    assert_eq!(pool(&app, cell, Currency::Waste), waste);
    assert_eq!(pool(&app, cell, Currency::Pyruvate), pyruvate);
}

#[test]
fn the_throttle_ramps_without_overshoot_and_does_not_depend_on_the_tick_length() {
    for rate in [5.0, 10.0, 20.0, 50.0] {
        let (mut app, cell) = fermenting_cell(rate);
        use_slot(&mut app, cell, 0); // Off: the target is 0
        use_slot(&mut app, cell, 0); // Low: the target is 0.5
        let mut previous = 1.0;
        // After 0.2 s the throttle is 0.8, whatever the tick length.
        for _ in 0..ticks_for(0.2, rate) {
            sim_step(&mut app);
            let now = throttle_of(&app, cell, BlockKind::Fermentation);
            assert!(now <= previous && now >= 0.5, "rate {rate}: {now}");
            previous = now;
        }
        assert_abs_diff_eq!(previous, 0.8, epsilon = 1e-3);
        // Then it stops exactly at the target.
        for _ in 0..ticks_for(1.0, rate) {
            sim_step(&mut app);
        }
        assert_eq!(throttle_of(&app, cell, BlockKind::Fermentation), 0.5);
    }
}

#[test]
fn low_runs_the_block_at_half_rate() {
    let (mut app, cell) = fermenting_cell(5.0);
    sim_step(&mut app);
    let before = pool(&app, cell, Currency::Waste);
    sim_step(&mut app);
    let full = pool(&app, cell, Currency::Waste) - before;

    use_slot(&mut app, cell, 0);
    use_slot(&mut app, cell, 0); // Low
    for _ in 0..ticks_for(2.0, 5.0) {
        sim_step(&mut app);
    }
    let before = pool(&app, cell, Currency::Waste);
    sim_step(&mut app);
    let half = pool(&app, cell, Currency::Waste) - before;
    assert_abs_diff_eq!(half, full * 0.5, epsilon = full * 0.02);
}

// --- Shelters ---

fn hazard_cell() -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::VesicleExport],
        &[(Currency::ATP, 5000.0), (Currency::Lipids, 0.0)],
    );
    {
        let mut env = app.world_mut().get_mut::<LocalEnvironment>(cell).unwrap();
        env.set(Channel::Heat, 100.0);
        env.set(Channel::Light, 10.0);
    }
    app.world_mut()
        .entity_mut(cell)
        .insert(Transform::from_xyz(10.0, 10.0, 0.0));
    (app, cell)
}

#[test]
fn heat_and_uv_do_not_hurt_inside_a_shelter() {
    let (mut app, cell) = hazard_cell();
    app.insert_resource(Shelters(vec![Shelter {
        position: Vec2::new(0.0, 0.0),
        radius: 50.0,
    }]));
    for _ in 0..50 {
        sim_step(&mut app);
    }
    assert_eq!(app.world().get::<Membrane>(cell).unwrap().integrity, 100.0);
    assert_eq!(*app.world().get::<DamageTaken>(cell).unwrap(), DamageTaken::default());
    assert_eq!(app.world().get::<GenomeIntegrity>(cell).unwrap().0, 1.0);
    assert_eq!(app.world().get::<MutationLoad>(cell).unwrap().0, 0.0);
}

#[test]
fn heat_and_uv_hurt_outside_a_shelter() {
    let (mut app, cell) = hazard_cell();
    app.insert_resource(Shelters(vec![Shelter {
        position: Vec2::new(500.0, 0.0),
        radius: 50.0,
    }]));
    for _ in 0..20 {
        sim_step(&mut app);
    }
    let taken = *app.world().get::<DamageTaken>(cell).unwrap();
    assert!(taken.heat > 0.0 && taken.uv > 0.0, "{taken:?}");
}

#[test]
fn time_stops_in_a_shelter_with_the_editor_open_and_only_then() {
    let (mut app, cell) = hazard_cell();
    app.insert_resource(Shelters(vec![Shelter {
        position: Vec2::ZERO,
        radius: 50.0,
    }]));
    let paused = |app: &App| app.world().resource::<Time<Virtual>>().is_paused();

    // The shelter flag is set in the metabolic tick (the Sense step).
    sim_step(&mut app);
    app.world_mut()
        .insert_resource(NextState::Pending(GameState::Run));
    app.update();
    app.update();
    assert!(!paused(&app), "the editor is closed");

    app.world_mut()
        .insert_resource(NextState::Pending(GameState::Manage));
    app.update();
    app.update();
    assert!(paused(&app), "in a shelter with the editor open");

    app.world_mut()
        .insert_resource(NextState::Pending(GameState::Run));
    app.update();
    app.update();
    assert!(!paused(&app));

    // Outside a shelter, the editor does not stop time.
    app.world_mut()
        .entity_mut(cell)
        .insert(Transform::from_xyz(900.0, 0.0, 0.0));
    sim_step(&mut app);
    app.world_mut()
        .insert_resource(NextState::Pending(GameState::Manage));
    app.update();
    app.update();
    assert!(!paused(&app));
}

// --- The synthesis queue from the editor ---

fn builder_cell() -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::ProteinSynthesis],
        &[(Currency::ATP, 1000.0), (Currency::AminoAcids, 500.0)],
    );
    (app, cell)
}

#[test]
fn a_queued_enzyme_appears_after_the_synthesis_time_and_not_before() {
    let (mut app, cell) = builder_cell();
    let target = ProteinTarget::Enzyme(BlockKind::Fermentation);
    app.world_mut().write_message(QueueProtein { cell, target });
    app.update();
    assert_eq!(app.world().get::<SynthesisQueue>(cell).unwrap().orders.len(), 1);

    // One ribosome makes about 1 unit a second at full saturation.
    let proteins = |app: &App| app.world().get::<Proteome>(cell).unwrap().count(target);
    let mut seconds = 0.0;
    while proteins(&app) == 0 && seconds < 60.0 {
        sim_step(&mut app);
        seconds += 0.2;
        if proteins(&app) == 0 {
            assert!(seconds < UNITS_PER_PROTEIN, "too slow");
        }
    }
    assert_eq!(proteins(&app), 1);
    assert!(seconds >= 0.5 * UNITS_PER_PROTEIN, "not before the time: {seconds}");
    assert!(app.world().get::<SynthesisQueue>(cell).unwrap().is_empty());
}

#[test]
fn recycling_from_the_editor_gives_half_the_amino_acids_back() {
    let (mut app, cell) = builder_cell();
    let target = ProteinTarget::Enzyme(BlockKind::Chemosynthesis);
    app.world_mut()
        .get_mut::<Proteome>(cell)
        .unwrap()
        .add(target);
    let before = pool(&app, cell, Currency::AminoAcids);
    app.world_mut().write_message(RecycleProtein { cell, target });
    app.update();
    assert_abs_diff_eq!(
        pool(&app, cell, Currency::AminoAcids) - before,
        recycle_yield(),
        epsilon = 1e-4
    );
    assert_eq!(recycle_yield(), 0.5 * UNITS_PER_PROTEIN);
    // A second recycle finds no protein and gives nothing.
    let after = pool(&app, cell, Currency::AminoAcids);
    app.world_mut().write_message(RecycleProtein { cell, target });
    app.update();
    assert_eq!(pool(&app, cell, Currency::AminoAcids), after);
}

#[test]
fn the_editor_cannot_queue_past_the_free_membrane_slots() {
    let (mut app, cell) = builder_cell();
    let slots = membrane_slots(&CellSize::default()) as usize;
    let target = ProteinTarget::Membrane(MembraneProtein::Toxin);
    for _ in 0..slots + 3 {
        app.world_mut().write_message(QueueProtein { cell, target });
    }
    app.update();
    assert_eq!(
        app.world().get::<SynthesisQueue>(cell).unwrap().orders.len(),
        slots
    );
}

// --- Induced mutation and the Enhanced state ---

fn genome_of(kinds: &[BlockKind]) -> Genome {
    let mut genome = Genome::default();
    for &kind in kinds {
        genome.add_gene(kind);
        genome.express_gene(kind);
    }
    genome
}

const GENES: [BlockKind; 3] = [
    BlockKind::Fermentation,
    BlockKind::VesicleExport,
    BlockKind::LipidMetabolism,
];

#[test]
fn induced_mutation_outcomes_follow_the_seed_and_the_chance() {
    let outcome = |seed: u64, chance: f32| {
        let mut genome = genome_of(&GENES);
        let mut rng = GeneRng::seeded(seed);
        apply_induced_mutation_load(&mut genome, 1.0, 1, chance, &mut rng.0)
    };
    // The same seed gives the same result.
    assert_eq!(outcome(7, ENHANCE_CHANCE), outcome(7, ENHANCE_CHANCE));
    // The chance 1 always enhances, the chance 0 never does.
    for seed in 0..20 {
        assert_eq!(outcome(seed, 1.0).changes[0].1, GeneState::Enhanced);
        assert_eq!(outcome(seed, 0.0).changes[0].1, GeneState::Mutated);
        let done = outcome(seed, 1.0);
        assert_eq!((done.load, done.induced), (0.0, 0));
    }
    // At 0.15, about 15 in 100.
    let enhanced = (0..1000)
        .filter(|&seed| outcome(seed, ENHANCE_CHANCE).changes[0].1 == GeneState::Enhanced)
        .count();
    assert!((100..200).contains(&enhanced), "{enhanced} of 1000");
}

#[test]
fn uv_mutations_never_enhance() {
    for seed in 0..50 {
        let mut genome = genome_of(&GENES);
        let mut rng = GeneRng::seeded(seed);
        let outcome = apply_induced_mutation_load(&mut genome, 2.0, 0, 1.0, &mut rng.0);
        assert!(outcome.changes.iter().all(|(_, s)| *s == GeneState::Mutated));
    }
}

#[test]
fn an_enhanced_block_runs_at_one_and_a_quarter() {
    assert_eq!(BlockStatus::Enhanced.throughput(), ENHANCED_THROUGHPUT);
    assert_eq!(ENHANCED_THROUGHPUT, 1.25);
    // Needs inputs for the faster rate: plenty of everything.
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::Pyruvate, 1.0);
    profile.set_production(Currency::ATP, 1.0);
    let mut pools = metabolistic3d::metabolism::CurrencyPools::default();
    pools.set(Currency::Pyruvate, 1000.0);
    let rate = |status: BlockStatus, dt: f32| {
        let input = BlockInput::new(&profile, 1.0).with_status(status);
        solve_tick(&pools, &[input], dt).turnover[0]
    };
    for dt in [0.05, 0.2, 1.0] {
        assert_abs_diff_eq!(
            rate(BlockStatus::Enhanced, dt) / rate(BlockStatus::Active, dt),
            1.25,
            epsilon = 1e-4
        );
    }
}

/// A seed for which the first induced mutation of `GENES` is `want`.
fn seed_for(want: GeneState) -> u64 {
    (0..1000)
        .find(|&seed| {
            let mut genome = genome_of(&GENES);
            let mut rng = GeneRng::seeded(seed);
            apply_induced_mutation_load(&mut genome, 1.0, 1, ENHANCE_CHANCE, &mut rng.0)
                .changes[0]
                .1
                == want
        })
        .expect("a seed exists")
}

fn induce_in_app(seed: u64) -> (App, Entity, BlockKind, GeneState) {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(GeneRng::seeded(seed));
    let cell = spawn_test_cell(
        &mut app,
        &GENES,
        &[
            (Currency::ATP, 5000.0),
            (Currency::Nucleotides, 500.0),
            (Currency::Lipids, 500.0),
        ],
    );
    set_bar(&mut app, cell, [Some(ActionKind::InduceMutation), None, None, None]);
    app.update();
    use_slot(&mut app, cell, 0);
    assert_eq!(pool(&app, cell, Currency::ATP), 5000.0 - INDUCE_ATP);
    // The mutation shows after one tick (the Genome step).
    sim_step(&mut app);
    let genome = app.world().get::<Genome>(cell).unwrap();
    let changed: Vec<_> = genome
        .table
        .iter()
        .filter(|(_, s)| **s != GeneState::Expressed)
        .map(|(k, s)| (*k, s.clone()))
        .collect();
    assert_eq!(changed.len(), 1, "one gene changed: {changed:?}");
    let (kind, state) = changed[0].clone();
    (app, cell, kind, state)
}

#[test]
fn an_induced_mutation_mutates_one_gene_and_repair_fixes_it() {
    let (mut app, cell, kind, state) = induce_in_app(seed_for(GeneState::Mutated));
    assert_eq!(state, GeneState::Mutated);
    assert_eq!(app.world().get::<MutationLoad>(cell).unwrap().0, 0.0);
    for _ in 0..200 {
        sim_step(&mut app);
    }
    let genome = app.world().get::<Genome>(cell).unwrap();
    assert_eq!(genome.get_gene_state(&kind), Some(&GeneState::Expressed));
}

#[test]
fn an_enhanced_gene_stays_enhanced_through_repair() {
    let (mut app, cell, kind, state) = induce_in_app(seed_for(GeneState::Enhanced));
    assert_eq!(state, GeneState::Enhanced);
    let nucleotides = pool(&app, cell, Currency::Nucleotides);
    // Damage the genome so that repair has work to do.
    app.world_mut().get_mut::<GenomeIntegrity>(cell).unwrap().0 = 0.5;
    for _ in 0..200 {
        sim_step(&mut app);
    }
    assert!(app.world().get::<GenomeIntegrity>(cell).unwrap().0 > 0.5, "repair ran");
    // Repair of integrity alone leaves the enhanced gene as it is.
    assert!(pool(&app, cell, Currency::Nucleotides) < nucleotides);
    let genome = app.world().get::<Genome>(cell).unwrap();
    assert_eq!(genome.get_gene_state(&kind), Some(&GeneState::Enhanced));
    // The block follows the gene state.
    let block = app.world().get::<CellBlocks>(cell).unwrap().get(kind).unwrap();
    assert_eq!(
        app.world().get::<MetabolicNode>(block).unwrap().status,
        BlockStatus::Enhanced
    );
}

#[test]
fn induced_mutation_is_refused_without_atp() {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(&mut app, &GENES, &[(Currency::ATP, 5.0)]);
    set_bar(&mut app, cell, [Some(ActionKind::InduceMutation), None, None, None]);
    app.update();
    use_slot(&mut app, cell, 0);
    assert_eq!(pool(&app, cell, Currency::ATP), 5.0);
    assert_eq!(app.world().get::<MutationLoad>(cell).unwrap().0, 0.0);
    assert_eq!(app.world().get::<InducedMutations>(cell).unwrap().0, 0);
}

// --- Adhesin upkeep ---

#[test]
fn a_held_adhesin_pays_atp_each_second_and_lets_go_when_empty() {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::ATP, 100.0)]);
    app.world_mut()
        .get_mut::<Proteome>(cell)
        .unwrap()
        .add(ProteinTarget::Membrane(MembraneProtein::Adhesin));
    set_bar(&mut app, cell, [Some(ActionKind::Adhesin), None, None, None]);
    use_slot(&mut app, cell, 0);
    assert!(app.world().get::<ActionState>(cell).unwrap().adhesin_held);
    // Maintenance also costs ATP: compare with a free-running twin.
    let before = pool(&app, cell, Currency::ATP);
    for _ in 0..50 {
        sim_step(&mut app); // 10 s
    }
    let spent = before - pool(&app, cell, Currency::ATP);
    assert!(spent >= 10.0 * ADHESIN_ATP_PER_SEC - 1e-3, "spent {spent}");

    set_pools(&mut app, cell, &[(Currency::ATP, 0.0)]);
    sim_step(&mut app);
    assert!(!app.world().get::<ActionState>(cell).unwrap().adhesin_held);
}

#[test]
fn a_pathway_cycled_out_of_the_bar_returns_to_high() {
    let (mut app, cell) = fermenting_cell(5.0);
    use_slot(&mut app, cell, 0); // Off
    for _ in 0..ticks_for(2.0, 5.0) {
        sim_step(&mut app);
    }
    assert_eq!(throttle_of(&app, cell, BlockKind::Fermentation), 0.0);
    set_bar(&mut app, cell, [None, None, None, None]);
    for _ in 0..ticks_for(2.0, 5.0) {
        sim_step(&mut app);
    }
    assert_eq!(throttle_of(&app, cell, BlockKind::Fermentation), 1.0);
}

#[test]
fn an_enhanced_gene_can_mutate_and_repair_gives_expressed() {
    let mut genome = genome_of(&[BlockKind::Fermentation]);
    assert!(genome.enhance_gene(BlockKind::Fermentation));
    let mut rng = GeneRng::seeded(1);
    let outcome = apply_mutation_load(&mut genome, 1.0, &mut rng.0);
    assert_eq!(outcome.1, vec![BlockKind::Fermentation]);
    assert_eq!(
        genome.get_gene_state(&BlockKind::Fermentation),
        Some(&GeneState::Mutated)
    );
    assert!(genome.restore_gene(BlockKind::Fermentation));
    assert_eq!(
        genome.get_gene_state(&BlockKind::Fermentation),
        Some(&GeneState::Expressed),
        "the bonus is lost"
    );
}
