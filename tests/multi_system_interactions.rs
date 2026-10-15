//! # Interactions between blocks
//!
//! Property tests for blocks that share pools in one cell.

use approx::assert_relative_eq;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::molecules::*;
use proptest::prelude::*;

mod common;
use common::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Fermentation makes waste, and vesicle export removes it. With more
    /// export enzymes, less waste stays in the cell.
    #[test]
    fn fermentation_vesicle_export_coordination(
        initial_pyruvate in 100.0f32..500.0f32,
        initial_rp in 100.0f32..500.0f32,
        simulation_steps in 3..15usize
    ) {
        let pools = [
            (Currency::Pyruvate, initial_pyruvate),
            (Currency::ReducingPower, initial_rp),
            (Currency::ATP, 50.0),
        ];
        let mut waste = Vec::new();
        for export_enzymes in [0, 1, 4] {
            let mut app = MetabolisticApp::new_headless();
            let cell = spawn_test_cell(&mut app, &[BlockKind::Fermentation, BlockKind::VesicleExport], &pools);
            set_enzymes(&mut app, cell, BlockKind::VesicleExport, export_enzymes);
            for _ in 0..simulation_steps {
                sim_step(&mut app);
                prop_assert!(cell_currencies_non_negative(&app, cell));
            }
            waste.push(pool(&app, cell, Currency::Waste));
        }
        prop_assert!(waste[0] > 0.0, "fermentation makes waste");
        prop_assert!(waste[1] < waste[0] && waste[2] < waste[1], "more export, less waste: {:?}", waste);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Fermentation and fat storage together keep all pools at 0 or more, and
    /// fermentation only uses pyruvate.
    #[test]
    fn fermentation_fat_storage_atp_competition(
        initial_pyruvate in 100.0f32..300.0f32,
        initial_rp in 100.0f32..300.0f32,
        initial_lipids in 200.0f32..600.0f32,
        initial_atp in 20.0f32..100.0f32,
        toxicity_threshold in 50.0f32..150.0f32,
        simulation_steps in 3..10usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        let cell = spawn_test_cell(&mut app, &[BlockKind::Fermentation, BlockKind::FatStorage], &[
            (Currency::Pyruvate, initial_pyruvate),
            (Currency::ReducingPower, initial_rp),
            (Currency::Lipids, initial_lipids),
            (Currency::ATP, initial_atp),
        ]);

        for step in 0..simulation_steps {
            sim_step(&mut app);
            prop_assert!(cell_currencies_non_negative(&app, cell), "negative pool at step {}", step);
            prop_assert!(pool(&app, cell, Currency::Pyruvate) <= initial_pyruvate + 1e-3);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Fermentation, lipid metabolism and fat storage together. With no carbon,
    /// lipid metabolism makes no lipids, so free + stored lipids stay constant.
    #[test]
    fn three_system_integration(
        metabolic_inputs in (
            100.0f32..400.0f32, // pyruvate
            100.0f32..400.0f32, // reducing power
            200.0f32..800.0f32, // lipids
            50.0f32..200.0f32   // atp
        ),
        system_parameters in (
            20.0f32..100.0f32, // toxicity threshold
            10.0f32..40.0f32,  // storage rate
            2.0f32..15.0f32    // release rate
        ),
        simulation_steps in 5..15usize
    ) {
        let (pyruvate, rp, lipids, atp) = metabolic_inputs;
        let (toxicity_threshold, poly_rate, lipo_rate) = system_parameters;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        // No outside carbon, so lipid metabolism has no input.
        app.world_mut().remove_resource::<EnvironmentalCurrencyPools>();
        let cell = spawn_test_cell(
            &mut app,
            &[BlockKind::Fermentation, BlockKind::LipidMetabolism, BlockKind::FatStorage],
            &[
                (Currency::Pyruvate, pyruvate),
                (Currency::ReducingPower, rp),
                (Currency::Lipids, lipids),
                (Currency::ATP, atp),
            ],
        );
        set_stored_lipids(&mut app, cell, 20.0);
        set_fat_storage_rates(&mut app, cell, poly_rate, lipo_rate);
        let initial_total: f32 = cell_snapshot(&app, cell).iter().sum();

        for step in 0..simulation_steps {
            sim_step(&mut app);
            prop_assert!(cell_currencies_non_negative(&app, cell), "negative pool at step {}", step);
            // Fermentation conserves the sum of its pools; maintenance only uses ATP.
            let total: f32 = cell_snapshot(&app, cell).iter().sum();
            prop_assert!(total <= initial_total + 1e-2, "pools grew: {} -> {}", initial_total, total);
            assert_relative_eq!(total_lipids(&app, cell), lipids + 20.0, epsilon = 1e-2);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// A cell with one pool at zero keeps all pools finite and at 0 or more.
    #[test]
    fn depleted_system_interactions(
        depletion_scenario in prop_oneof![
            Just("no_fermentation_inputs"),
            Just("no_atp"),
            Just("no_lipids"),
            Just("no_waste")
        ],
        simulation_steps in 3..10usize
    ) {
        let mut pools = vec![
            (Currency::Pyruvate, 50.0),
            (Currency::ReducingPower, 50.0),
            (Currency::ATP, 50.0),
            (Currency::Lipids, 100.0),
            (Currency::Waste, 10.0),
            (Currency::CarbonSkeletons, 20.0),
        ];
        let empty: &[Currency] = match depletion_scenario {
            "no_fermentation_inputs" => &[Currency::Pyruvate, Currency::ReducingPower],
            "no_atp" => &[Currency::ATP],
            "no_lipids" => &[Currency::Lipids],
            "no_waste" => &[Currency::Waste],
            _ => unreachable!(),
        };
        for entry in &mut pools {
            if empty.contains(&entry.0) {
                entry.1 = 0.0;
            }
        }
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));
        let cell = spawn_test_cell(
            &mut app,
            &[BlockKind::Fermentation, BlockKind::LipidMetabolism, BlockKind::FatStorage],
            &pools,
        );

        for step in 0..simulation_steps {
            sim_step(&mut app);
            for &amount in &cell_snapshot(&app, cell) {
                prop_assert!(amount.is_finite() && amount >= 0.0,
                    "{}: bad pool at step {}: {}", depletion_scenario, step, amount);
            }
        }
    }
}
