//! # Milestone M3: genome damage (headless)
//!
//! UV from light shafts, the pigment shield, mutations, translation
//! efficiency and repair. Light is set on `LocalEnvironment`
//! (no grid), except in the tests that use the vent field.

use approx::assert_abs_diff_eq;
use bevy::prelude::*;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::genome::{BlockKind, GeneState, Genome};
use metabolistic3d::cell::{DamageSource, DamageTaken, DeathCause, death_cause, is_alive};
use metabolistic3d::environment::{Channel, LocalEnvironment};
use metabolistic3d::genome_damage::*;
use metabolistic3d::level;
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::{CurrencyPools, FlowmapTimingConfig};
use metabolistic3d::molecules::Currency;
use metabolistic3d::proteins::{ProteinTarget, Proteome, SynthesisQueue};
use metabolistic3d::shared::spawn_starter_cell;

mod common;
use common::*;

/// Genes of the mutation tests. None of them needs an outside supply.
const GENES: [BlockKind; 5] = [
    BlockKind::VesicleExport,
    BlockKind::Fermentation,
    BlockKind::LipidMetabolism,
    BlockKind::AminoAcidBiosynthesis,
    BlockKind::Chemosynthesis,
];

/// A headless app with one test cell that has `GENES`, lots of ATP and no
/// nucleotides (so nothing is repaired), in `light`.
fn app_with_cell(light: f32, seed: u64) -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(GeneRng::seeded(seed));
    let cell = spawn_test_cell(
        &mut app,
        &GENES,
        &[
            (Currency::ATP, 5000.0),
            (Currency::Lipids, 500.0),
            (Currency::Nucleotides, 0.0),
        ],
    );
    set_light(&mut app, cell, light);
    (app, cell)
}

fn set_light(app: &mut App, cell: Entity, light: f32) {
    app.world_mut()
        .get_mut::<LocalEnvironment>(cell)
        .unwrap()
        .set(Channel::Light, light);
}

fn genome_integrity(app: &App, cell: Entity) -> f32 {
    app.world().get::<GenomeIntegrity>(cell).unwrap().0
}

fn set_genome_integrity(app: &mut App, cell: Entity, value: f32) {
    app.world_mut().get_mut::<GenomeIntegrity>(cell).unwrap().0 = value;
}

fn membrane(app: &App, cell: Entity) -> f32 {
    app.world().get::<Membrane>(cell).unwrap().integrity
}

fn uv_damage(app: &App, cell: Entity) -> f32 {
    app.world().get::<DamageTaken>(cell).unwrap().uv
}

fn mutated_genes(app: &App, cell: Entity) -> Vec<BlockKind> {
    let genome = app.world().get::<Genome>(cell).unwrap();
    let mut kinds: Vec<_> = genome
        .table
        .iter()
        .filter(|(_, state)| **state == GeneState::Mutated)
        .map(|(kind, _)| *kind)
        .collect();
    kinds.sort();
    kinds
}

fn alive(app: &App, cell: Entity) -> bool {
    is_alive(
        app.world().get::<CurrencyPools>(cell).unwrap(),
        app.world().get::<Membrane>(cell).unwrap(),
    )
}

fn run_ticks(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        sim_step(app);
    }
}

// --- UV damage ---

#[test]
fn without_light_nothing_changes() {
    let (mut app, cell) = app_with_cell(0.0, 1);
    run_ticks(&mut app, 100);
    assert_eq!(genome_integrity(&app, cell), 1.0);
    assert_eq!(app.world().get::<MutationLoad>(cell).unwrap().0, 0.0);
    assert!(mutated_genes(&app, cell).is_empty());
    assert_eq!(membrane(&app, cell), 100.0);
    assert_eq!(uv_damage(&app, cell), 0.0);
}

#[test]
fn light_without_pigment_lowers_integrity_and_mutates_genes() {
    let (mut app, cell) = app_with_cell(3.0, 1);
    run_ticks(&mut app, 100); // 20 s
    assert!(genome_integrity(&app, cell) < 0.9);
    assert!(!mutated_genes(&app, cell).is_empty(), "UV mutated genes");
    assert!(uv_damage(&app, cell) > 0.0);
}

#[test]
fn a_cell_without_pigment_dies_in_a_strong_shaft() {
    let (mut app, cell) = app_with_cell(level::SHAFT_LEVEL, 1);
    let mut ticks = 0;
    while ticks < 500 && alive(&app, cell) {
        sim_step(&mut app);
        ticks += 1;
    }
    assert!(ticks < 500, "the cell should die in a strong shaft");
    let pools = app.world().get::<CurrencyPools>(cell).unwrap();
    let membrane = app.world().get::<Membrane>(cell).unwrap();
    assert_eq!(death_cause(pools, membrane), Some(DeathCause::MembraneBroken));
    assert_eq!(
        app.world().get::<DamageTaken>(cell).unwrap().main_source(),
        Some(DamageSource::Uv)
    );
}

#[test]
fn pigments_cut_the_damage_by_the_shield() {
    let damage_after_one_tick = |pigments: f32| {
        let (mut app, cell) = app_with_cell(5.0, 1);
        set_pools(&mut app, cell, &[(Currency::Pigments, pigments)]);
        sim_step(&mut app);
        (uv_damage(&app, cell), 1.0 - genome_integrity(&app, cell))
    };
    let (bare_membrane, bare_genome) = damage_after_one_tick(0.0);
    assert!(bare_membrane > 0.0 && bare_genome > 0.0);
    // Pigments decay once before the damage in the same tick.
    let pigments = 30.0;
    let s = shield(decayed_pigments(pigments, 0.2));
    let (m, g) = damage_after_one_tick(pigments);
    assert_abs_diff_eq!(m, bare_membrane * (1.0 - s), epsilon = 1e-5);
    assert_abs_diff_eq!(g, bare_genome * (1.0 - s), epsilon = 1e-6);
    assert!(s > 0.5);
}

#[test]
fn the_shield_is_never_one() {
    let (mut app, cell) = app_with_cell(5.0, 1);
    set_pools(&mut app, cell, &[(Currency::Pigments, 1.0e9)]);
    sim_step(&mut app);
    assert!(shield(1.0e9) < 1.0);
    assert!(uv_damage(&app, cell) > 0.0, "some UV always gets through");
}

// --- Effects of genome damage ---

#[test]
fn damage_slows_protein_synthesis_and_a_fragmented_genome_stops_it() {
    let progress_after = |integrity: f32| {
        let (mut app, cell) = app_with_cell(0.0, 1);
        express_blocks(&mut app, cell, &[BlockKind::ProteinSynthesis]);
        set_pools(&mut app, cell, &[(Currency::AminoAcids, 100.0)]);
        set_genome_integrity(&mut app, cell, integrity);
        app.world_mut()
            .get_mut::<SynthesisQueue>(cell)
            .unwrap()
            .orders
            .push_back(ProteinTarget::Enzyme(BlockKind::Fermentation));
        run_ticks(&mut app, 25); // 5 s: less than one protein
        let progress = app.world().get::<SynthesisQueue>(cell).unwrap().progress;
        let enzymes = app
            .world()
            .get::<Proteome>(cell)
            .unwrap()
            .enzymes(BlockKind::Fermentation);
        (progress, enzymes)
    };
    let (whole, _) = progress_after(1.0);
    let (damaged, _) = progress_after(0.6);
    let (fragmented, enzymes) = progress_after(FRAGMENT_THRESHOLD - 0.05);
    assert!(whole > damaged && damaged > 0.0, "{whole} {damaged}");
    assert_eq!(fragmented, 0.0);
    assert_eq!(enzymes, 1, "no new protein from a fragmented genome");
}

#[test]
fn the_same_exposure_gives_the_same_mutations_for_any_tick_length() {
    let mutations_at = |ticks_per_sec: f32, seed: u64| {
        let (mut app, cell) = app_with_cell(3.0, seed);
        app.insert_resource(FlowmapTimingConfig {
            updates_per_second: ticks_per_sec,
            ..FlowmapTimingConfig::manual()
        });
        // 30 s of exposure: load 3 x 0.04 x 30 = 3.6.
        run_ticks(&mut app, (30.0 * ticks_per_sec) as usize);
        mutated_genes(&app, cell)
    };
    let reference = mutations_at(5.0, 42);
    assert_eq!(reference.len(), 3);
    for rate in [1.0, 2.0, 10.0, 20.0] {
        assert_eq!(mutations_at(rate, 42), reference, "at {rate} ticks per second");
    }
    // The seeded RNG repeats itself.
    assert_eq!(mutations_at(5.0, 42), reference);
}

// --- Repair ---

/// A cell for the repair tests: its genes use no ATP, and it has 1000 ATP.
fn repair_cell(integrity: f32, nucleotides: f32) -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::VesicleExport, BlockKind::Chemosynthesis],
        &[(Currency::ATP, 1000.0), (Currency::Nucleotides, nucleotides)],
    );
    set_genome_integrity(&mut app, cell, integrity);
    (app, cell)
}

#[test]
fn repair_costs_atp_and_nucleotides() {
    let (mut app, cell) = repair_cell(0.5, 100.0);
    sim_step(&mut app);
    let work = REPAIR_WORK_PER_SEC * 0.2;
    assert_abs_diff_eq!(genome_integrity(&app, cell), 0.5 + work, epsilon = 1e-6);
    assert_abs_diff_eq!(
        pool(&app, cell, Currency::Nucleotides),
        100.0 - work * REPAIR_NUCLEOTIDES_PER_WORK,
        epsilon = 1e-4
    );
    // ATP: the repair and a little cell maintenance.
    let atp_used = 1000.0 - pool(&app, cell, Currency::ATP);
    assert!(atp_used >= work * REPAIR_ATP_PER_WORK - 1e-3);
    assert!(atp_used < work * REPAIR_ATP_PER_WORK + 0.5);
}

#[test]
fn repair_stops_at_full_integrity() {
    let (mut app, cell) = repair_cell(0.99, 100.0);
    run_ticks(&mut app, 100);
    assert_eq!(genome_integrity(&app, cell), 1.0);
    // Only the gap of 0.01 was paid for.
    assert_abs_diff_eq!(
        pool(&app, cell, Currency::Nucleotides),
        100.0 - 0.01 * REPAIR_NUCLEOTIDES_PER_WORK,
        epsilon = 1e-3
    );
}

#[test]
fn repair_stops_when_nucleotides_run_out() {
    let (mut app, cell) = repair_cell(0.5, 1.0);
    run_ticks(&mut app, 200);
    assert_abs_diff_eq!(
        genome_integrity(&app, cell),
        0.5 + 1.0 / REPAIR_NUCLEOTIDES_PER_WORK,
        epsilon = 1e-4
    );
    assert!(pool(&app, cell, Currency::Nucleotides) < 1e-4);
}

#[test]
fn repair_turns_mutated_genes_back_to_expressed() {
    let (mut app, cell) = repair_cell(1.0, 100.0);
    app.world_mut()
        .get_mut::<Genome>(cell)
        .unwrap()
        .mutate_gene(BlockKind::Chemosynthesis);
    assert_eq!(mutated_genes(&app, cell), vec![BlockKind::Chemosynthesis]);
    // 0.1 work at 0.02 per second is 5 s: 25 ticks.
    run_ticks(&mut app, 24);
    assert_eq!(mutated_genes(&app, cell).len(), 1, "not yet");
    run_ticks(&mut app, 3);
    assert!(mutated_genes(&app, cell).is_empty());
    let genome = app.world().get::<Genome>(cell).unwrap();
    assert_eq!(
        genome.get_gene_state(&BlockKind::Chemosynthesis),
        Some(&GeneState::Expressed)
    );
    assert_abs_diff_eq!(
        pool(&app, cell, Currency::Nucleotides),
        100.0 - GENE_REPAIR_WORK * REPAIR_NUCLEOTIDES_PER_WORK,
        epsilon = 1e-2
    );
}

// --- New blocks ---

#[test]
fn the_starter_genome_makes_nucleotides_and_has_no_pigment_gene() {
    let genome = metabolistic3d::blocks::genome::create_starter_genome();
    assert_eq!(
        genome.get_gene_state(&BlockKind::NucleotideCofactorSynthesis),
        Some(&GeneState::Expressed)
    );
    assert_eq!(genome.get_gene_state(&BlockKind::SecondaryMetabolites), None);
}

#[test]
fn nucleotide_synthesis_fills_the_pool_up_to_its_target() {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::NucleotideCofactorSynthesis],
        &[
            (Currency::Nucleotides, 0.0),
            (Currency::CarbonSkeletons, 500.0),
            (Currency::ATP, 5000.0),
            (Currency::ReducingPower, 500.0),
        ],
    );
    let mut highest = 0.0f32;
    for _ in 0..300 {
        sim_step(&mut app);
        highest = highest.max(pool(&app, cell, Currency::Nucleotides));
    }
    let target = metabolistic3d::blocks::nucleotides::TARGET_NUCLEOTIDES;
    assert!(highest <= target + 1e-3, "no overshoot: {highest}");
    assert_abs_diff_eq!(pool(&app, cell, Currency::Nucleotides), target, epsilon = 0.1);
}

#[test]
fn the_pigment_block_makes_pigments_that_decay_slowly() {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::SecondaryMetabolites],
        &[(Currency::CarbonSkeletons, 5000.0), (Currency::ATP, 5000.0)],
    );
    run_ticks(&mut app, 400);
    let pigments = pool(&app, cell, Currency::Pigments);
    let target = metabolistic3d::blocks::secondary_metabolites::TARGET_PIGMENTS;
    assert!(pigments > target * 0.9 && pigments <= target + 1e-3, "{pigments}");
    assert!(shield(pigments) > 0.7 && shield(pigments) < 1.0);

    // Without the block, the pool decays: 10 s lose about 10 percent.
    let (mut app, cell) = app_with_cell(0.0, 1);
    set_pools(&mut app, cell, &[(Currency::Pigments, 40.0)]);
    run_ticks(&mut app, 50);
    assert_abs_diff_eq!(
        pool(&app, cell, Currency::Pigments),
        40.0 * (-PIGMENT_DECAY_PER_SEC * 10.0).exp(),
        epsilon = 0.01
    );
}

// --- The vent field ---

/// A starter cell with plenty of everything, at `position` in the vent field.
fn cell_in_field(position: Vec2, pigment_gene: bool) -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(level::vent_field());
    app.insert_resource(UvFraction(level::UV_FRACTION));
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, position).1
    };
    app.world_mut().flush();
    if pigment_gene {
        let mut genome = app.world_mut().get_mut::<Genome>(cell).unwrap();
        genome.add_gene(BlockKind::SecondaryMetabolites);
        genome.express_gene(BlockKind::SecondaryMetabolites);
        app.world_mut()
            .get_mut::<Proteome>(cell)
            .unwrap()
            .add(ProteinTarget::Enzyme(BlockKind::SecondaryMetabolites));
    }
    set_pools(
        &mut app,
        cell,
        &[
            (Currency::ATP, 5000.0),
            (Currency::CarbonSkeletons, 2000.0),
            (Currency::ReducingPower, 500.0),
            (Currency::Lipids, 500.0),
            (Currency::AminoAcids, 100.0),
        ],
    );
    app.update();
    (app, cell)
}

/// The middle of the first shaft, below the top edge.
fn shaft_core() -> Vec2 {
    let shaft = level::SHAFTS[0];
    Vec2::new((shaft.min.x + shaft.max.x) * 0.5, 450.0)
}

#[test]
fn the_shafts_are_bright_inside_and_dark_at_the_start() {
    let grid = level::vent_field();
    let core = grid.sample(Channel::Light, shaft_core());
    assert!(core > 7.0 && core <= level::SHAFT_LEVEL, "{core}");
    assert!(grid.sample(Channel::Light, level::START_POSITION) < 0.2);
    assert!(grid.sample(Channel::Light, Vec2::ZERO) < 0.2);
    // The pigment prey wait in the dim edge: some light, far less than the core.
    let orb_light = grid.sample(Channel::Light, level::PIGMENT_PREY_POSITION);
    assert!(orb_light > 0.3 && orb_light < 3.0, "{orb_light}");
}

#[test]
fn light_shafts_kill_a_cell_without_pigment_and_a_cell_with_pigment_lives() {
    // 90 s in the core of the shaft.
    let (mut bare, bare_cell) = cell_in_field(shaft_core(), false);
    let (mut shielded, shielded_cell) = cell_in_field(shaft_core(), true);
    set_pools(&mut shielded, shielded_cell, &[(Currency::Pigments, 40.0)]);
    let mut bare_died = false;
    for tick in 0..450 {
        sim_step(&mut bare);
        sim_step(&mut shielded);
        bare_died |= !alive(&bare, bare_cell);
        assert!(
            alive(&shielded, shielded_cell),
            "the shielded cell died at tick {tick}"
        );
    }
    assert!(bare_died, "the cell without pigment should die");
    assert!(membrane(&shielded, shielded_cell) > 20.0);
    assert!(pool(&shielded, shielded_cell, Currency::Pigments) > 20.0);
}

#[test]
fn the_start_is_safe_from_uv() {
    let (mut app, cell) = cell_in_field(level::START_POSITION, false);
    run_ticks(&mut app, 300); // 60 s
    assert!(uv_damage(&app, cell) < 5.0, "{}", uv_damage(&app, cell));
    assert!(genome_integrity(&app, cell) > 0.95);
    assert!(mutated_genes(&app, cell).is_empty());
}

#[test]
fn the_pigment_prey_are_reachable_with_little_damage() {
    let (mut app, cell) = cell_in_field(level::PIGMENT_PREY_POSITION, false);
    run_ticks(&mut app, 10); // 2 s at the prey
    assert!(uv_damage(&app, cell) < 10.0, "{}", uv_damage(&app, cell));
    assert!(membrane(&app, cell) > 85.0);
}
