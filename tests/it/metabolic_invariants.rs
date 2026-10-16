//! # Game-level metabolic invariants
//!
//! Property tests for economic balance, genome-to-block consistency, and the
//! limits of cell viability.

use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::block_spec;
use metabolistic3d::blocks::genome::{GeneState, Genome};
use metabolistic3d::cell::CellBlocks;
use metabolistic3d::metabolism::{BlockStatus, MetabolicNode};
use metabolistic3d::molecules::*;
use proptest::prelude::*;

use crate::common::*;

/// Blocks of a cell that runs on fermentation.
const FERMENTER: [BlockKind; 3] = [
    BlockKind::Fermentation,
    BlockKind::VesicleExport,
    BlockKind::FatStorage,
];

// --- Economic balance ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// A fermenting cell with enough inputs makes more ATP than it uses.
    #[test]
    fn metabolic_economic_viability(
        initial_resources in (
            100.0f32..400.0f32, // pyruvate
            100.0f32..400.0f32, // reducing power
            50.0f32..200.0f32,  // atp
            200.0f32..600.0f32  // lipids
        ),
        economic_stress_duration in 5..20usize
    ) {
        let (pyruvate, rp, atp, lipids) = initial_resources;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));
        let cell = spawn_test_cell(&mut app, &[BlockKind::Fermentation], &[
            (Currency::Pyruvate, pyruvate),
            (Currency::ReducingPower, rp),
            (Currency::ATP, atp),
            (Currency::Lipids, lipids),
        ]);

        for step in 0..economic_stress_duration {
            let before = pool(&app, cell, Currency::ATP);
            sim_step(&mut app);
            let after = pool(&app, cell, Currency::ATP);
            prop_assert!(after > before, "ATP did not grow at step {}: {} -> {}", step, before, after);
            prop_assert!(cell_currencies_non_negative(&app, cell));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Scarcity slows the cell down. It never makes a pool negative, infinite or huge.
    #[test]
    fn scarcity_graceful_degradation(
        scarcity_scenario in prop_oneof![
            Just("energy_scarce"),
            Just("carbon_scarce"),
            Just("lipid_overwhelmed")
        ],
        degradation_period in 5..15usize
    ) {
        let pools: &[(Currency, f32)] = match scarcity_scenario {
            "energy_scarce" => &[
                (Currency::ATP, 1.0),
                (Currency::Pyruvate, 2.0),
                (Currency::ReducingPower, 1.0),
                (Currency::Lipids, 50.0),
            ],
            "carbon_scarce" => &[
                (Currency::ATP, 50.0),
                (Currency::Pyruvate, 50.0),
                (Currency::ReducingPower, 50.0),
                (Currency::CarbonSkeletons, 0.5),
            ],
            "lipid_overwhelmed" => &[
                (Currency::ATP, 20.0),
                (Currency::Pyruvate, 30.0),
                (Currency::ReducingPower, 30.0),
                (Currency::Lipids, 5000.0),
            ],
            _ => unreachable!(),
        };
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));
        let cell = spawn_test_cell(&mut app, &FERMENTER, pools);

        for step in 0..degradation_period {
            sim_step(&mut app);
            for (i, &amount) in cell_snapshot(&app, cell).iter().enumerate() {
                prop_assert!(amount >= 0.0, "{}: pool {} negative at step {}", scarcity_scenario, i, step);
                prop_assert!(amount.is_finite() && amount <= 1e9,
                    "{}: pool {} out of range at step {}: {}", scarcity_scenario, i, step, amount);
            }
        }
    }
}

// --- Genome and blocks ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Each implemented gene has one block, and the block status follows the
    /// gene state.
    #[test]
    fn genome_metabolic_consistency(
        gene_operations in prop::collection::vec((block_kind(), gene_state()), 1..6),
        consistency_check_steps in 3..8usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        let cell = spawn_test_cell(&mut app, &[], &[
            (Currency::ATP, 100.0),
            (Currency::Pyruvate, 100.0),
            (Currency::ReducingPower, 100.0),
        ]);
        {
            let mut genome = app.world_mut().get_mut::<Genome>(cell).unwrap();
            for (kind, _) in &gene_operations {
                genome.add_gene(*kind);
            }
        }
        app.update();

        for &(kind, ref target_state) in &gene_operations {
            {
                let mut genome = app.world_mut().get_mut::<Genome>(cell).unwrap();
                match target_state {
                    GeneState::Expressed => {
                        genome.express_gene(kind);
                    }
                    GeneState::Mutated => {
                        genome.express_gene(kind);
                        genome.mutate_gene(kind);
                    }
                    GeneState::Silent => {
                        genome.silence_gene(kind);
                    }
                    GeneState::Enhanced => {
                        genome.express_gene(kind);
                        genome.enhance_gene(kind);
                    }
                }
            }
            for _ in 0..consistency_check_steps {
                sim_step(&mut app);
            }

            let state = app.world().get::<Genome>(cell).unwrap().get_gene_state(&kind).cloned();
            let implemented = block_spec(kind).is_some();
            let block = app.world().get::<CellBlocks>(cell).unwrap().get(kind);
            prop_assert_eq!(block.is_some(), implemented && state.is_some(),
                "block existence for {:?}", kind);
            if let (Some(block), Some(state)) = (block, state) {
                let node = app.world().get::<MetabolicNode>(block).unwrap();
                prop_assert_eq!(node.status, BlockStatus::from(state));
            }
            prop_assert!(cell_currencies_non_negative(&app, cell));
        }
    }
}

// --- Cell viability ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// High waste goes down when the cell has vesicle export.
    #[test]
    fn toxicity_response_mechanisms(
        toxicity_scenario in (
            200.0f32..800.0f32, // initial waste
            50.0f32..150.0f32   // toxicity threshold
        ),
        toxicity_resolution_time in 5..15usize
    ) {
        let (initial_waste, threshold) = toxicity_scenario;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(threshold));
        let cell = spawn_test_cell(&mut app, &[BlockKind::Fermentation, BlockKind::VesicleExport], &[
            (Currency::Waste, initial_waste),
            (Currency::ATP, 50.0),
            (Currency::Pyruvate, 50.0),
            (Currency::ReducingPower, 50.0),
        ]);

        let mut previous = initial_waste;
        for step in 0..toxicity_resolution_time {
            sim_step(&mut app);
            let waste = pool(&app, cell, Currency::Waste);
            prop_assert!(waste < previous, "waste did not go down at step {}: {} -> {}", step, previous, waste);
            prop_assert!(cell_currencies_non_negative(&app, cell));
            previous = waste;
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Free lipids above the toxicity threshold go into storage.
    #[test]
    fn lipid_toxicity_protection(
        lipid_stress in (
            300.0f32..1000.0f32, // initial lipids
            50.0f32..200.0f32,   // toxicity threshold
            15.0f32..50.0f32     // storage rate
        ),
        protection_duration in 5..15usize
    ) {
        let (initial_lipids, threshold, poly_rate) = lipid_stress;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(threshold));
        let cell = spawn_test_cell(&mut app, &[BlockKind::FatStorage], &[
            (Currency::Lipids, initial_lipids),
            (Currency::ATP, 50.0),
        ]);
        set_fat_storage_rates(&mut app, cell, poly_rate, 5.0);

        for step in 0..protection_duration {
            let before = pool(&app, cell, Currency::Lipids);
            sim_step(&mut app);
            let after = pool(&app, cell, Currency::Lipids);
            if before > threshold {
                prop_assert!(after < before, "no storage at step {}", step);
            }
            prop_assert!(after >= threshold - 1e-3, "storage went below the threshold");
            prop_assert!(cell_currencies_non_negative(&app, cell));
        }
        prop_assert!((total_lipids(&app, cell) - initial_lipids).abs() < 1e-2);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// A full fermenting cell keeps ATP, and keeps waste and free lipids under
    /// control.
    #[test]
    fn cellular_health_maintenance(
        health_scenario in (
            100.0f32..300.0f32, // pyruvate
            100.0f32..300.0f32, // reducing power
            200.0f32..600.0f32, // lipids
            50.0f32..150.0f32,  // atp
            100.0f32..300.0f32  // toxicity threshold
        ),
        health_monitoring_period in 10..25usize
    ) {
        let (pyruvate, rp, lipids, atp, threshold) = health_scenario;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(threshold));
        let cell = spawn_test_cell(&mut app, &FERMENTER, &[
            (Currency::Pyruvate, pyruvate),
            (Currency::ReducingPower, rp),
            (Currency::ATP, atp),
            (Currency::Waste, 20.0),
            (Currency::Lipids, lipids),
        ]);
        set_stored_lipids(&mut app, cell, 25.0);

        for step in 0..health_monitoring_period {
            sim_step(&mut app);
            prop_assert!(cell_currencies_non_negative(&app, cell), "negative pool at step {}", step);
            prop_assert!(pool(&app, cell, Currency::ATP) >= atp, "ATP fell at step {}", step);
        }
        prop_assert!(pool(&app, cell, Currency::Waste) < 20.0 + health_monitoring_period as f32);
        prop_assert!(pool(&app, cell, Currency::Lipids) <= lipids.max(threshold));
    }
}
