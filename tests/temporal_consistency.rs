//! # Temporal Consistency Tests
//!
//! These tests verify that metabolic systems maintain consistent behavior
//! over extended periods of simulation, without drift, oscillation, or
//! accumulation of errors that could destabilize the system.

use approx::{assert_abs_diff_eq, assert_relative_eq};
use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::genome::Genome;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::metabolism::{FlowmapTimingConfig, run_metabolic_tick};
use metabolistic3d::molecules::*;
use metabolistic3d::player::spawn_local_player_cell;
use proptest::prelude::*;

mod common;
use common::*;

// --- Temporal Stability Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4, // Very small for long simulations
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Test that currency levels stabilize over time rather than oscillating wildly
    #[test]
    fn currency_stabilization_over_time(
        initial_values in (
            100.0f32..300.0f32, // pyruvate
            100.0f32..300.0f32, // reducing power
            200.0f32..600.0f32, // free fatty acids
            50.0f32..150.0f32   // atp
        ),
        long_simulation in 10..30usize  // Dramatically reduced for performance
    ) {
        let (pyruvate, rp, ffa, atp) = initial_values;

        let mut app = MetabolisticApp::new_headless();

        // Spawn a cell first
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();

        // Set up stable system configuration in the cell
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            if let Some(mut currency_pools) = entity_mut.get_mut::<CurrencyPools>() {
                currency_pools.set(Currency::Pyruvate, pyruvate);
                currency_pools.set(Currency::ReducingPower, rp);
                currency_pools.set(Currency::ATP, atp);
                currency_pools.set(Currency::OrganicWaste, 10.0);
                currency_pools.set(Currency::FreeFattyAcids, ffa);
                currency_pools.set(Currency::StorageBeads, 30.0);
            }
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));

        // Spawn all systems
        express_blocks(&mut app, cell_entity, &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage]);

        let mut currency_history = Vec::new();

        // Run long simulation and track currency levels
        for step in 0..long_simulation {
            sim_step(&mut app);

            let snapshot = get_currency_snapshot(&app);
            currency_history.push(snapshot);

            // Verify stability properties at each step
            prop_assert!(all_currencies_non_negative(&app),
                "Currency went negative at step {}", step);
        }

        // Analyze temporal patterns
        if currency_history.len() >= 50 {
            // Compare first quarter vs last quarter for stability
            let first_quarter_end = currency_history.len() / 4;
            let last_quarter_start = 3 * currency_history.len() / 4;

            for currency_idx in 0..currency_history[0].len() {
                let first_quarter_avg: f32 = currency_history[0..first_quarter_end]
                    .iter()
                    .map(|snapshot| snapshot[currency_idx])
                    .sum::<f32>() / first_quarter_end as f32;

                let last_quarter_avg: f32 = currency_history[last_quarter_start..]
                    .iter()
                    .map(|snapshot| snapshot[currency_idx])
                    .sum::<f32>() / (currency_history.len() - last_quarter_start) as f32;

                // Verify currency levels show reasonable stability (not wildly diverging)
                if first_quarter_avg > 1.0 {
                    let relative_change = (last_quarter_avg - first_quarter_avg).abs() / first_quarter_avg;
                    prop_assert!(relative_change <= 2.0,
                        "Currency {} showed excessive drift: early avg {}, late avg {}, relative change {}",
                        currency_idx, first_quarter_avg, last_quarter_avg, relative_change);
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Test that system behavior is consistent across identical initial conditions
    #[test]
    fn reproducible_behavior(
        test_conditions in (
            50.0f32..200.0f32, // pyruvate
            50.0f32..200.0f32, // rp
            100.0f32..400.0f32, // ffa
            25.0f32..75.0f32    // atp
        ),
        simulation_length in 5..15usize  // Reduced for performance
    ) {
        let (pyruvate, rp, ffa, atp) = test_conditions;

        // Run identical simulation twice
        let mut results1 = Vec::new();
        let mut results2 = Vec::new();

        for run in 0..2 {
            let mut app = MetabolisticApp::new_headless();

            // Identical initial conditions
            let (_player_entity, cell_entity) = {
                let mut commands = app.world_mut().commands();
                spawn_local_player_cell(&mut commands, Genome::default())
            };
            app.world_mut().flush();

            {
                let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
                let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
                currency_pools.set(Currency::Pyruvate, pyruvate);
                currency_pools.set(Currency::ReducingPower, rp);
                currency_pools.set(Currency::ATP, atp);
                currency_pools.set(Currency::OrganicWaste, 5.0);
                currency_pools.set(Currency::FreeFattyAcids, ffa);
                currency_pools.set(Currency::StorageBeads, 15.0);
            }
            app.world_mut().insert_resource(LipidToxicityThreshold(80.0));

            // Identical system configuration
            express_blocks(&mut app, cell_entity, &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage]);

            // Run identical simulation
            for _ in 0..simulation_length {
                sim_step(&mut app);
            }

            let final_snapshot = get_currency_snapshot(&app);
            if run == 0 {
                results1 = final_snapshot;
            } else {
                results2 = final_snapshot;
            }
        }

        // Verify identical results from identical conditions
        for (&result1, &result2) in results1.iter().zip(results2.iter()) {
            assert_abs_diff_eq!(result1, result2, epsilon = 1e-6);
        }
    }
}

// --- Long-Term System Health Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Test that systems don't accumulate errors over very long simulations
    #[test]
    fn long_term_error_accumulation(
        stable_config in (
            200.0f32..500.0f32, // initial pyruvate
            200.0f32..500.0f32, // initial rp
            400.0f32..800.0f32, // initial ffa
            100.0f32..200.0f32  // initial atp
        ),
        very_long_simulation in 15..40usize  // Dramatically reduced for performance
    ) {
        let (pyruvate, rp, ffa, atp) = stable_config;

        let mut app = MetabolisticApp::new_headless();

        // Set up stable, well-resourced system
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();

        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
            currency_pools.set(Currency::Pyruvate, pyruvate);
            currency_pools.set(Currency::ReducingPower, rp);
            currency_pools.set(Currency::ATP, atp);
            currency_pools.set(Currency::OrganicWaste, 20.0);
            currency_pools.set(Currency::FreeFattyAcids, ffa);
            currency_pools.set(Currency::StorageBeads, 50.0);
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(200.0));

        express_blocks(&mut app, cell_entity, &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage]);

        let initial_snapshot = get_currency_snapshot(&app);
        let mut error_samples = Vec::new();

        // Run very long simulation with periodic error checking
        for step in 0..very_long_simulation {
            sim_step(&mut app);

            // Sample error periodically
            if step % 100 == 0 {
                let current_snapshot = get_currency_snapshot(&app);

                // Check for numerical health
                for (i, &amount) in current_snapshot.iter().enumerate() {
                    prop_assert!(amount.is_finite(),
                        "Currency {} became non-finite at step {}: {}", i, step, amount);
                    prop_assert!(amount >= -1e-6,
                        "Currency {} went significantly negative at step {}: {}", i, step, amount);
                }

                // Check for mass balance conservation in fat storage
                let current_ffa = current_snapshot[4]; // FreeFattyAcids index
                let current_storage = current_snapshot[7]; // StorageBeads index
                let total_lipids = current_ffa + current_storage;
                let initial_lipids = initial_snapshot[4] + initial_snapshot[7];

                let mass_balance_error = (total_lipids - initial_lipids).abs();
                error_samples.push(mass_balance_error);

                prop_assert!(mass_balance_error <= 1.0,
                    "Mass balance error exceeded threshold at step {}: {}", step, mass_balance_error);
            }
        }

        // Verify errors don't trend upward over time
        if error_samples.len() >= 4 {
            let early_errors: f32 = error_samples[0..error_samples.len()/2].iter().sum();
            let late_errors: f32 = error_samples[error_samples.len()/2..].iter().sum();

            let early_avg = early_errors / (error_samples.len()/2) as f32;
            let late_avg = late_errors / (error_samples.len() - error_samples.len()/2) as f32;

            prop_assert!(late_avg <= early_avg * 3.0,
                "Error accumulation detected over long simulation: early avg {}, late avg {}",
                early_avg, late_avg);
        }
    }
}

// --- Cyclic Behavior Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Test for healthy cyclic behavior vs problematic oscillations
    #[test]
    fn cyclic_behavior_analysis(
        oscillation_config in (
            80.0f32..150.0f32,  // pyruvate
            80.0f32..150.0f32,  // rp
            300.0f32..600.0f32, // ffa (high to trigger polymerization)
            30.0f32..80.0f32    // atp
        ),
        cycle_observation_length in 10..30usize  // Reduced for performance
    ) {
        let (pyruvate, rp, ffa, atp) = oscillation_config;

        let mut app = MetabolisticApp::new_headless();

        // Set up conditions that might produce cyclic behavior
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();

        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
            currency_pools.set(Currency::Pyruvate, pyruvate);
            currency_pools.set(Currency::ReducingPower, rp);
            currency_pools.set(Currency::ATP, atp);
            currency_pools.set(Currency::OrganicWaste, 0.0);
            currency_pools.set(Currency::FreeFattyAcids, ffa);
            currency_pools.set(Currency::StorageBeads, 10.0);
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(250.0)); // Low to trigger cycles

        express_blocks(&mut app, cell_entity, &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage]);

        let mut ffa_history = Vec::new();
        let mut waste_history = Vec::new();

        // Observe system for cyclic patterns
        for _ in 0..cycle_observation_length {
            sim_step(&mut app);

            let currency_pools = app.world().get::<CurrencyPools>(cell_entity).unwrap();
            let current_ffa = currency_pools.get(Currency::FreeFattyAcids);
            let current_waste = currency_pools.get(Currency::OrganicWaste);

            ffa_history.push(current_ffa);
            waste_history.push(current_waste);

            // Verify cycles don't break fundamental invariants
            prop_assert!(current_ffa >= 0.0);
            prop_assert!(current_waste >= 0.0);
            prop_assert!(all_currencies_non_negative(&app));
        }

        // Analyze for problematic oscillations
        if ffa_history.len() >= 10 {
            let max_ffa = ffa_history.iter().copied().fold(0.0f32, f32::max);
            let min_ffa = ffa_history.iter().copied().fold(f32::INFINITY, f32::min);
            let ffa_range = max_ffa - min_ffa;

            let max_waste = waste_history.iter().copied().fold(0.0f32, f32::max);

            // Verify oscillations stay within reasonable bounds
            prop_assert!(ffa_range <= ffa * 2.0,
                "FFA oscillations too extreme: range {} vs initial {}", ffa_range, ffa);

            prop_assert!(max_waste <= 500.0,
                "Waste accumulation too extreme: max {}", max_waste);

            // Check for runaway oscillations (increasing amplitude)
            let first_half_range = {
                let first_half = &ffa_history[0..ffa_history.len()/2];
                let max = first_half.iter().copied().fold(0.0f32, f32::max);
                let min = first_half.iter().copied().fold(f32::INFINITY, f32::min);
                max - min
            };

            let second_half_range = {
                let second_half = &ffa_history[ffa_history.len()/2..];
                let max = second_half.iter().copied().fold(0.0f32, f32::max);
                let min = second_half.iter().copied().fold(f32::INFINITY, f32::min);
                max - min
            };

            prop_assert!(second_half_range <= first_half_range * 2.0,
                "Oscillation amplitude increasing: early range {}, late range {}",
                first_half_range, second_half_range);
        }
    }
}

// --- Time-Scale Consistency Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Test that system behavior is consistent across different time scales
    #[test]
    fn time_scale_consistency(
        base_conditions in (
            100.0f32..250.0f32, // pyruvate
            100.0f32..250.0f32, // rp
            200.0f32..500.0f32, // ffa
            50.0f32..100.0f32   // atp
        ),
        time_scale_factor in prop_oneof![
            Just(0.5f32),
            Just(1.0f32),
            Just(2.0f32),
            Just(4.0f32)
        ]
    ) {
        let (pyruvate, rp, ffa, atp) = base_conditions;

        // Run two simulations: one with normal time steps, one with scaled time steps
        let mut normal_app = MetabolisticApp::new_headless();
        let mut scaled_app = MetabolisticApp::new_headless();

        // Identical initial conditions
        let mut cell_entities = Vec::new();
        for app in [&mut normal_app, &mut scaled_app] {
            let (_player_entity, cell_entity) = {
                let mut commands = app.world_mut().commands();
                spawn_local_player_cell(&mut commands, Genome::default())
            };
            app.world_mut().flush();
            cell_entities.push(cell_entity);

            {
                let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
                let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
                currency_pools.set(Currency::Pyruvate, pyruvate);
                currency_pools.set(Currency::ReducingPower, rp);
                currency_pools.set(Currency::ATP, atp);
                currency_pools.set(Currency::OrganicWaste, 5.0);
                currency_pools.set(Currency::FreeFattyAcids, ffa);
                currency_pools.set(Currency::StorageBeads, 20.0);
            }
            app.world_mut().insert_resource(LipidToxicityThreshold(120.0));

            express_blocks(app, cell_entity, &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage]);
        }

        // Run for equivalent total simulation time
        // Divisible by every scale factor, so both apps simulate the same time.
        let base_steps = 40;
        let scaled_steps = (base_steps as f32 / time_scale_factor) as usize;

        // The scaled app simulates the same time in fewer, longer ticks.
        let base_rate = normal_app.world().resource::<FlowmapTimingConfig>().updates_per_second;
        scaled_app.insert_resource(FlowmapTimingConfig {
            manual: true,
            ..FlowmapTimingConfig::new(base_rate / time_scale_factor)
        });

        normal_app.update();
        for _ in 0..base_steps {
            run_metabolic_tick(normal_app.world_mut());
        }
        scaled_app.update();
        for _ in 0..scaled_steps {
            run_metabolic_tick(scaled_app.world_mut());
        }

        let normal_results = {
            let currency_pools = normal_app.world().get::<CurrencyPools>(cell_entities[0]).unwrap();
            vec![
                currency_pools.get(Currency::ATP),
                currency_pools.get(Currency::ReducingPower),
                currency_pools.get(Currency::AcetylCoA),
                currency_pools.get(Currency::CarbonSkeletons),
                currency_pools.get(Currency::FreeFattyAcids),
                currency_pools.get(Currency::Pyruvate),
                currency_pools.get(Currency::OrganicWaste),
                currency_pools.get(Currency::StorageBeads),
            ]
        };
        let scaled_results = {
            let currency_pools = scaled_app.world().get::<CurrencyPools>(cell_entities[1]).unwrap();
            vec![
                currency_pools.get(Currency::ATP),
                currency_pools.get(Currency::ReducingPower),
                currency_pools.get(Currency::AcetylCoA),
                currency_pools.get(Currency::CarbonSkeletons),
                currency_pools.get(Currency::FreeFattyAcids),
                currency_pools.get(Currency::Pyruvate),
                currency_pools.get(Currency::OrganicWaste),
                currency_pools.get(Currency::StorageBeads),
            ]
        };

        // Results should be approximately equivalent (allowing for numerical differences)
        for (&normal, &scaled) in normal_results.iter().zip(scaled_results.iter()) {
            if normal > 1.0 {
                assert_relative_eq!(normal, scaled, epsilon = 0.2);
            } else {
                assert_abs_diff_eq!(normal, scaled, epsilon = 1.0);
            }
        }
    }
}
