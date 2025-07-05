//! # Temporal Consistency Tests
//!
//! These tests verify that metabolic systems maintain consistent behavior
//! over extended periods of simulation, without drift, oscillation, or
//! accumulation of errors that could destabilize the system.

use proptest::prelude::*;
use approx::{assert_relative_eq, assert_abs_diff_eq};
use metabolistic3d::molecules::*;
use metabolistic3d::blocks::fermentation::FermentationBlock;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::{PolyMer, CellMass};
use metabolistic3d::blocks::vesicle_export::VesicleExportBlock;
use metabolistic3d::MetabolisticApp;
use bevy::prelude::*;
use bevy::time::{Time, Fixed};

mod property_utils;
use property_utils::*;

// --- Temporal Stability Tests ---

proptest! {
    /// Test that currency levels stabilize over time rather than oscillating wildly
    #[test]
    fn currency_stabilization_over_time(
        initial_values in (
            100.0f32..300.0f32, // pyruvate
            100.0f32..300.0f32, // reducing power
            200.0f32..600.0f32, // free fatty acids
            50.0f32..150.0f32   // atp
        ),
        long_simulation in 100..500usize
    ) {
        let (pyruvate, rp, ffa, atp) = initial_values;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up stable system configuration
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate);
        currency_pools.set(Currency::ReducingPower, rp);
        currency_pools.set(Currency::ATP, atp);
        currency_pools.set(Currency::OrganicWaste, 10.0);
        currency_pools.set(Currency::FreeFattyAcids, ffa);
        currency_pools.set(Currency::StorageBeads, 30.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));
        
        // Spawn all systems
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 1500.0,
                target_fill: 750.0,
                poly_rate: 15.0,
                lipo_rate: 7.0,
            },
        ));
        
        let mut currency_history = Vec::new();
        
        // Run long simulation and track currency levels
        for step in 0..long_simulation {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
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
    /// Test that system behavior is consistent across identical initial conditions
    #[test]
    fn reproducible_behavior(
        test_conditions in (
            50.0f32..200.0f32, // pyruvate
            50.0f32..200.0f32, // rp
            100.0f32..400.0f32, // ffa
            25.0f32..75.0f32    // atp
        ),
        simulation_length in 20..100usize
    ) {
        let (pyruvate, rp, ffa, atp) = test_conditions;
        
        // Run identical simulation twice
        let mut results1 = Vec::new();
        let mut results2 = Vec::new();
        
        for run in 0..2 {
            let mut app = MetabolisticApp::new_headless();
            
            // Identical initial conditions
            let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
            currency_pools.set(Currency::Pyruvate, pyruvate);
            currency_pools.set(Currency::ReducingPower, rp);
            currency_pools.set(Currency::ATP, atp);
            currency_pools.set(Currency::OrganicWaste, 5.0);
            currency_pools.set(Currency::FreeFattyAcids, ffa);
            currency_pools.set(Currency::StorageBeads, 15.0);
            app.world_mut().insert_resource(LipidToxicityThreshold(80.0));
            
            // Identical system configuration
            app.world_mut().spawn(FermentationBlock);
            app.world_mut().spawn(VesicleExportBlock);
            app.world_mut().spawn((
                CellMass { base: 1.0, extra: 0.0 },
                PolyMer {
                    capacity: 1000.0,
                    target_fill: 500.0,
                    poly_rate: 20.0,
                    lipo_rate: 10.0,
                },
            ));
            
            // Run identical simulation
            for _ in 0..simulation_length {
                let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
                app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
                app.update();
                app.world_mut().run_schedule(FixedUpdate);
            }
            
            let final_snapshot = get_currency_snapshot(&app);
            if run == 0 {
                results1 = final_snapshot;
            } else {
                results2 = final_snapshot;
            }
        }
        
        // Verify identical results from identical conditions
        for (_i, (&result1, &result2)) in results1.iter().zip(results2.iter()).enumerate() {
            assert_abs_diff_eq!(result1, result2, epsilon = 1e-6);
        }
    }
}

// --- Long-Term System Health Tests ---

proptest! {
    /// Test that systems don't accumulate errors over very long simulations
    #[test]
    fn long_term_error_accumulation(
        stable_config in (
            200.0f32..500.0f32, // initial pyruvate
            200.0f32..500.0f32, // initial rp
            400.0f32..800.0f32, // initial ffa
            100.0f32..200.0f32  // initial atp
        ),
        very_long_simulation in 500..2000usize
    ) {
        let (pyruvate, rp, ffa, atp) = stable_config;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up stable, well-resourced system
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate);
        currency_pools.set(Currency::ReducingPower, rp);
        currency_pools.set(Currency::ATP, atp);
        currency_pools.set(Currency::OrganicWaste, 20.0);
        currency_pools.set(Currency::FreeFattyAcids, ffa);
        currency_pools.set(Currency::StorageBeads, 50.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(200.0));
        
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate: 12.0,
                lipo_rate: 6.0,
            },
        ));
        
        let initial_snapshot = get_currency_snapshot(&app);
        let mut error_samples = Vec::new();
        
        // Run very long simulation with periodic error checking
        for step in 0..very_long_simulation {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
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
    /// Test for healthy cyclic behavior vs problematic oscillations
    #[test]
    fn cyclic_behavior_analysis(
        oscillation_config in (
            80.0f32..150.0f32,  // pyruvate
            80.0f32..150.0f32,  // rp
            300.0f32..600.0f32, // ffa (high to trigger polymerization)
            30.0f32..80.0f32    // atp
        ),
        cycle_observation_length in 50..200usize
    ) {
        let (pyruvate, rp, ffa, atp) = oscillation_config;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up conditions that might produce cyclic behavior
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate);
        currency_pools.set(Currency::ReducingPower, rp);
        currency_pools.set(Currency::ATP, atp);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        currency_pools.set(Currency::FreeFattyAcids, ffa);
        currency_pools.set(Currency::StorageBeads, 10.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(250.0)); // Low to trigger cycles
        
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 1500.0,
                target_fill: 750.0,
                poly_rate: 30.0,  // High rates to encourage cycling
                lipo_rate: 15.0,
            },
        ));
        
        let mut ffa_history = Vec::new();
        let mut waste_history = Vec::new();
        
        // Observe system for cyclic patterns
        for _ in 0..cycle_observation_length {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let currency_pools = app.world().resource::<CurrencyPools>();
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
        for app in [&mut normal_app, &mut scaled_app] {
            let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
            currency_pools.set(Currency::Pyruvate, pyruvate);
            currency_pools.set(Currency::ReducingPower, rp);
            currency_pools.set(Currency::ATP, atp);
            currency_pools.set(Currency::OrganicWaste, 5.0);
            currency_pools.set(Currency::FreeFattyAcids, ffa);
            currency_pools.set(Currency::StorageBeads, 20.0);
            app.world_mut().insert_resource(LipidToxicityThreshold(120.0));
            
            app.world_mut().spawn(FermentationBlock);
            app.world_mut().spawn(VesicleExportBlock);
            app.world_mut().spawn((
                CellMass { base: 1.0, extra: 0.0 },
                PolyMer {
                    capacity: 1200.0,
                    target_fill: 600.0,
                    poly_rate: 18.0,
                    lipo_rate: 9.0,
                },
            ));
        }
        
        // Run for equivalent total simulation time
        let base_steps = 50;
        let scaled_steps = (base_steps as f32 / time_scale_factor) as usize;
        
        // Normal simulation
        for _ in 0..base_steps {
            let fixed_time_step = normal_app.world().resource::<Time<Fixed>>().delta();
            normal_app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            normal_app.update();
            normal_app.world_mut().run_schedule(FixedUpdate);
        }
        
        // Scaled simulation (adjust time step)
        for _ in 0..scaled_steps {
            let base_time_step = scaled_app.world().resource::<Time<Fixed>>().delta();
            let scaled_time_step = base_time_step.mul_f32(time_scale_factor);
            scaled_app.world_mut().resource_mut::<Time>().advance_by(scaled_time_step);
            scaled_app.update();
            scaled_app.world_mut().run_schedule(FixedUpdate);
        }
        
        let normal_results = get_currency_snapshot(&normal_app);
        let scaled_results = get_currency_snapshot(&scaled_app);
        
        // Results should be approximately equivalent (allowing for numerical differences)
        for (_i, (&normal, &scaled)) in normal_results.iter().zip(scaled_results.iter()).enumerate() {
            if normal > 1.0 {
                assert_relative_eq!(normal, scaled, epsilon = 0.2);
            } else {
                assert_abs_diff_eq!(normal, scaled, epsilon = 1.0);
            }
        }
    }
}