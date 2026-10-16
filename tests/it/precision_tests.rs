//! # Floating-Point Precision Tests for Metabolic Calculations
//!
//! These tests verify that floating-point arithmetic in metabolic calculations
//! maintains acceptable precision and doesn't accumulate errors over time.

use approx::{assert_abs_diff_eq, assert_relative_eq};
use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::genome::Genome;
use metabolistic3d::cell::CellSize;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::*;
use metabolistic3d::player::spawn_local_player_cell;
use proptest::prelude::*;

use crate::common::*;

// --- Floating-Point Precision Constants ---

/// Acceptable relative error for currency calculations (0.1%)
const CURRENCY_RELATIVE_EPSILON: f32 = 1e-3;

/// Acceptable absolute error for small currency amounts
const CURRENCY_ABSOLUTE_EPSILON: f32 = 1e-6;

/// Maximum acceptable drift in long-running simulations, relative to the pool sizes
const LONG_SIMULATION_RELATIVE_EPSILON: f32 = 1e-5;

// --- Precision Tests for Currency Operations ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that repeated small currency operations don't accumulate precision errors
    #[test]
    fn currency_precision_accumulation(
        initial_amount in 100.0f32..1000.0f32,
        operation_size in 0.01f32..1.0f32,
        iterations in 10..50usize  // Reduced for performance
    ) {
        let mut app = MetabolisticApp::new_headless();

        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();

        app.world_mut().get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().set(Currency::ATP, initial_amount);

        // Perform many small operations
        let mut expected_total = initial_amount;
        for _ in 0..iterations {
            if expected_total >= operation_size {
                let atp_before = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP);
                if atp_before >= operation_size {
                    app.world_mut().get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().modify(Currency::ATP, -operation_size);
                    expected_total -= operation_size;
                }
            }
        }

        let actual_total = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP);

        // Verify precision is maintained within acceptable bounds
        assert_relative_eq!(actual_total, expected_total, epsilon = CURRENCY_RELATIVE_EPSILON);
        assert_abs_diff_eq!(actual_total, expected_total, epsilon = CURRENCY_ABSOLUTE_EPSILON);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Fat storage conserves free + stored lipids over many ticks.
    #[test]
    fn transfer_precision(
        initial_lipids in 200.0f32..800.0f32,
        poly_rate in 0.5f32..25.0f32,
        lipo_rate in 0.1f32..10.0f32,
        cycles in 5..20usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));
        let cell = spawn_test_cell(&mut app, &[BlockKind::FatStorage], &[
            (Currency::Lipids, initial_lipids),
            (Currency::ATP, 1000.0),
        ]);
        set_stored_lipids(&mut app, cell, 50.0);
        set_fat_storage_rates(&mut app, cell, poly_rate, lipo_rate);
        let initial_total = initial_lipids + 50.0;

        for _ in 0..cycles {
            sim_step(&mut app);
            assert_relative_eq!(
                total_lipids(&app, cell),
                initial_total,
                epsilon = CURRENCY_RELATIVE_EPSILON
            );
        }
    }
}

// --- Long-Running Simulation Precision Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4, // Fewer cases for long simulations
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Fermentation inputs used equal its outputs over a long run, and the
    /// error does not grow.
    #[test]
    fn long_fermentation_precision(
        initial_pyruvate in 500.0f32..2000.0f32,
        initial_rp in 500.0f32..2000.0f32,
        long_simulation_steps in 20..50usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        let cell = spawn_test_cell(&mut app, &[BlockKind::Fermentation], &[
            (Currency::Pyruvate, initial_pyruvate),
            (Currency::ReducingPower, initial_rp),
        ]);
        // No maintenance cost: only fermentation changes the pools.
        *app.world_mut().get_mut::<CellSize>(cell).unwrap() = CellSize { radius: 0.0 };
        let initial_inputs = initial_pyruvate + initial_rp;
        let mut precision_errors = Vec::new();

        for step in 0..long_simulation_steps {
            sim_step(&mut app);
            if step % 10 == 0 {
                let remaining = pool(&app, cell, Currency::Pyruvate) + pool(&app, cell, Currency::ReducingPower);
                let outputs = pool(&app, cell, Currency::ATP) + pool(&app, cell, Currency::Waste);
                let precision_error = ((initial_inputs - remaining) - outputs).abs();
                precision_errors.push(precision_error);
                let threshold = LONG_SIMULATION_RELATIVE_EPSILON * initial_inputs;
                prop_assert!(
                    precision_error <= threshold,
                    "Precision error {} exceeded threshold {} at step {}",
                    precision_error,
                    threshold,
                    step
                );
            }
        }

        if precision_errors.len() > 2 {
            let half = precision_errors.len() / 2;
            let first_half_avg: f32 = precision_errors[..half].iter().sum::<f32>() / half as f32;
            let second_half_avg: f32 = precision_errors[half..].iter().sum::<f32>()
                / (precision_errors.len() - half) as f32;
            // f32 rounding grows with each solver sub-step, not each tick.
            let dt = app
                .world()
                .resource::<metabolistic3d::metabolism::FlowmapTimingConfig>()
                .target_delta();
            let substeps = (dt / metabolistic3d::metabolism::MAX_SUBSTEP).ceil().max(1.0);
            let rounding_bound =
                f32::EPSILON * initial_inputs * long_simulation_steps as f32 * substeps;
            prop_assert!(
                second_half_avg <= first_half_avg * 2.0 || second_half_avg <= rounding_bound,
                "Precision degradation detected: early avg {}, late avg {}",
                first_half_avg,
                second_half_avg
            );
        }
    }
}

// --- Edge Case Precision Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test precision with very small currency amounts (near floating-point limits)
    #[test]
    fn small_amount_precision(
        tiny_amount in 1e-6f32..1e-3f32,
        operations in 1..20usize  // Reduced for performance
    ) {
        let mut app = MetabolisticApp::new_headless();
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();

        app.world_mut().get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().set(Currency::ATP, tiny_amount);

        let initial_amount = tiny_amount;

        // Attempt operations on very small amounts
        for _ in 0..operations {
            let current_amount = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP);

            // Try to consume half of remaining amount
            let consumption_amount = current_amount * 0.5;
            if consumption_amount > 1e-10 { // Avoid underflow
                if app.world().get::<CurrencyPools>(cell_entity).unwrap().can_consume(Currency::ATP, consumption_amount) {
                    app.world_mut().get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().modify(Currency::ATP, -consumption_amount);
                }

                let new_amount = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP);

                // Verify precision maintained even for tiny operations
                let expected = current_amount - consumption_amount;
                assert_abs_diff_eq!(
                    new_amount,
                    expected,
                    epsilon = 1e-10
                );
            }
        }

        // Final amount should still be non-negative and reasonable
        let final_amount = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP);
        prop_assert!(final_amount >= 0.0);
        prop_assert!(final_amount <= initial_amount + 1e-10);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test precision with large currency amounts (near overflow limits)
    #[test]
    fn large_amount_precision(
        large_amount in 1e6f32..1e8f32,
        small_operations in prop::collection::vec(1.0f32..100.0f32, 5..15)  // Reduced for performance
    ) {
        let mut app = MetabolisticApp::new_headless();
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();

        app.world_mut().get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().set(Currency::ATP, large_amount);

        let mut expected_amount = large_amount;

        // Perform many small operations on large amount
        for &op_amount in &small_operations {
            if expected_amount >= op_amount
                && app.world().get::<CurrencyPools>(cell_entity).unwrap().can_consume(Currency::ATP, op_amount) {
                    app.world_mut().get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().modify(Currency::ATP, -op_amount);
                    expected_amount -= op_amount;
                }
        }

        let actual_amount = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP);

        // Verify precision maintained even with large base amounts
        assert_relative_eq!(
            actual_amount,
            expected_amount,
            epsilon = CURRENCY_RELATIVE_EPSILON
        );
    }
}

// --- System Integration Precision Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4, // Fewer cases for complex scenarios
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Large pools in several blocks stay finite and at 0 or more.
    #[test]
    fn multi_system_precision(
        initial_values in (
            200.0f32..1000.0f32, // pyruvate
            200.0f32..1000.0f32, // reducing power
            300.0f32..1500.0f32, // lipids
            100.0f32..500.0f32   // waste
        ),
        simulation_steps in 20..100usize
    ) {
        let (pyruvate, rp, lipids, waste) = initial_values;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));
        let cell = spawn_test_cell(
            &mut app,
            &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage],
            &[
                (Currency::Pyruvate, pyruvate),
                (Currency::ReducingPower, rp),
                (Currency::Lipids, lipids),
                (Currency::Waste, waste),
                (Currency::ATP, 100.0),
            ],
        );
        set_stored_lipids(&mut app, cell, 50.0);

        for _ in 0..simulation_steps {
            sim_step(&mut app);
            for (i, &current) in cell_snapshot(&app, cell).iter().enumerate() {
                prop_assert!(current.is_finite(), "Currency {} became non-finite: {}", i, current);
                prop_assert!(
                    current >= -CURRENCY_ABSOLUTE_EPSILON,
                    "Currency {} went significantly negative: {}",
                    i,
                    current
                );
            }
        }
    }
}
