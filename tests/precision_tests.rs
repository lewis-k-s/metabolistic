//! # Floating-Point Precision Tests for Metabolic Calculations
//!
//! These tests verify that floating-point arithmetic in metabolic calculations
//! maintains acceptable precision and doesn't accumulate errors over time.

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

// --- Floating-Point Precision Constants ---

/// Acceptable relative error for currency calculations (0.1%)
const CURRENCY_RELATIVE_EPSILON: f32 = 1e-3;

/// Acceptable absolute error for small currency amounts
const CURRENCY_ABSOLUTE_EPSILON: f32 = 1e-6;

/// Maximum acceptable drift in long-running simulations
const LONG_SIMULATION_EPSILON: f32 = 1e-2;

// --- Precision Tests for Currency Operations ---

proptest! {
    /// Test that repeated small currency operations don't accumulate precision errors
    #[test]
    fn currency_precision_accumulation(
        initial_amount in 100.0f32..1000.0f32,
        operation_size in 0.01f32..1.0f32,
        iterations in 10..1000usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, initial_amount);
        
        // Perform many small operations
        let mut expected_total = initial_amount;
        for _ in 0..iterations {
            if expected_total >= operation_size {
                let atp_before = app.world().resource::<CurrencyPools>().get(Currency::ATP);
                if atp_before >= operation_size {
                    app.world_mut().resource_mut::<CurrencyPools>().modify(Currency::ATP, -operation_size);
                    expected_total -= operation_size;
                }
            }
        }
        
        let actual_total = app.world().resource::<CurrencyPools>().get(Currency::ATP);
        
        // Verify precision is maintained within acceptable bounds
        assert_relative_eq!(actual_total, expected_total, epsilon = CURRENCY_RELATIVE_EPSILON);
        assert_abs_diff_eq!(actual_total, expected_total, epsilon = CURRENCY_ABSOLUTE_EPSILON);
    }
}

proptest! {
    /// Test precision in currency transfer operations (polymerization/lipolysis)
    #[test]
    fn transfer_precision(
        initial_ffa in 200.0f32..800.0f32,
        poly_rate in 0.5f32..25.0f32,
        lipo_rate in 0.1f32..10.0f32,
        cycles in 5..100usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::FreeFattyAcids, initial_ffa);
        currency_pools.set(Currency::StorageBeads, 50.0);
        currency_pools.set(Currency::ATP, 1000.0); // Plenty of ATP
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));
        
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate,
                lipo_rate,
            },
        ));
        
        let initial_total_lipids = initial_ffa + 50.0;
        
        // Run multiple cycles to test precision over time
        for _ in 0..cycles {
            app.update();
            
            let currency_pools = app.world().resource::<CurrencyPools>();
            let current_ffa = currency_pools.get(Currency::FreeFattyAcids);
            let current_storage = currency_pools.get(Currency::StorageBeads);
            let current_total = current_ffa + current_storage;
            
            // Verify lipid conservation with tight precision bounds
            assert_relative_eq!(
                current_total, 
                initial_total_lipids, 
                epsilon = CURRENCY_RELATIVE_EPSILON
            );
        }
    }
}

// --- Long-Running Simulation Precision Tests ---

proptest! {
    /// Test that fermentation maintains precision over extended simulations  
    #[test]
    fn long_fermentation_precision(
        initial_pyruvate in 500.0f32..2000.0f32,
        initial_rp in 500.0f32..2000.0f32,
        long_simulation_steps in 100..500usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, initial_pyruvate);
        currency_pools.set(Currency::ReducingPower, initial_rp);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        
        app.world_mut().spawn(FermentationBlock);
        
        let initial_total_inputs = initial_pyruvate + initial_rp;
        
        // Track precision over long simulation
        let mut precision_errors = Vec::new();
        
        for step in 0..long_simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Check mass balance precision every 10 steps
            if step % 10 == 0 {
                let currency_pools = app.world().resource::<CurrencyPools>();
                let current_pyruvate = currency_pools.get(Currency::Pyruvate);
                let current_rp = currency_pools.get(Currency::ReducingPower);
                let current_atp = currency_pools.get(Currency::ATP);
                let current_waste = currency_pools.get(Currency::OrganicWaste);
                
                let remaining_inputs = current_pyruvate + current_rp;
                let total_outputs = current_atp + current_waste;
                let consumed_inputs = initial_total_inputs - remaining_inputs;
                
                let precision_error = (consumed_inputs - total_outputs).abs();
                precision_errors.push(precision_error);
                
                // Verify precision doesn't degrade beyond acceptable bounds
                prop_assert!(
                    precision_error <= LONG_SIMULATION_EPSILON,
                    "Precision error {} exceeded threshold {} at step {}",
                    precision_error,
                    LONG_SIMULATION_EPSILON,
                    step
                );
            }
        }
        
        // Verify precision errors don't trend upward (no systematic drift)
        if precision_errors.len() > 2 {
            let first_half_avg: f32 = precision_errors[..precision_errors.len()/2].iter().sum::<f32>() 
                / (precision_errors.len()/2) as f32;
            let second_half_avg: f32 = precision_errors[precision_errors.len()/2..].iter().sum::<f32>() 
                / (precision_errors.len() - precision_errors.len()/2) as f32;
            
            // Second half should not have significantly higher errors than first half
            prop_assert!(
                second_half_avg <= first_half_avg * 2.0,
                "Precision degradation detected: early avg {}, late avg {}",
                first_half_avg,
                second_half_avg  
            );
        }
    }
}

// --- Edge Case Precision Tests ---

proptest! {
    /// Test precision with very small currency amounts (near floating-point limits)
    #[test]
    fn small_amount_precision(
        tiny_amount in 1e-6f32..1e-3f32,
        operations in 1..100usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, tiny_amount);
        
        let initial_amount = tiny_amount;
        
        // Attempt operations on very small amounts
        for _ in 0..operations {
            let current_amount = app.world().resource::<CurrencyPools>().get(Currency::ATP);
            
            // Try to consume half of remaining amount
            let consumption_amount = current_amount * 0.5;
            if consumption_amount > 1e-10 { // Avoid underflow
                if app.world().resource::<CurrencyPools>().can_consume(Currency::ATP, consumption_amount) {
                    app.world_mut().resource_mut::<CurrencyPools>().modify(Currency::ATP, -consumption_amount);
                }
                
                let new_amount = app.world().resource::<CurrencyPools>().get(Currency::ATP);
                
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
        let final_amount = app.world().resource::<CurrencyPools>().get(Currency::ATP);
        prop_assert!(final_amount >= 0.0);
        prop_assert!(final_amount <= initial_amount + 1e-10);
    }
}

proptest! {
    /// Test precision with large currency amounts (near overflow limits)
    #[test]
    fn large_amount_precision(
        large_amount in 1e6f32..1e8f32,
        small_operations in prop::collection::vec(1.0f32..100.0f32, 10..50)
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, large_amount);
        
        let mut expected_amount = large_amount;
        
        // Perform many small operations on large amount
        for &op_amount in &small_operations {
            if expected_amount >= op_amount {
                if app.world().resource::<CurrencyPools>().can_consume(Currency::ATP, op_amount) {
                    app.world_mut().resource_mut::<CurrencyPools>().modify(Currency::ATP, -op_amount);
                    expected_amount -= op_amount;
                }
            }
        }
        
        let actual_amount = app.world().resource::<CurrencyPools>().get(Currency::ATP);
        
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
    /// Test precision in complex multi-system scenarios
    #[test]
    fn multi_system_precision(
        initial_values in (
            200.0f32..1000.0f32, // pyruvate
            200.0f32..1000.0f32, // reducing power  
            300.0f32..1500.0f32, // free fatty acids
            100.0f32..500.0f32   // organic waste
        ),
        simulation_steps in 20..100usize
    ) {
        let (pyruvate, rp, ffa, waste) = initial_values;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize all systems
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate);
        currency_pools.set(Currency::ReducingPower, rp);
        currency_pools.set(Currency::FreeFattyAcids, ffa);
        currency_pools.set(Currency::OrganicWaste, waste);
        currency_pools.set(Currency::ATP, 100.0);
        currency_pools.set(Currency::StorageBeads, 50.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));
        
        // Spawn all metabolic blocks
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 3000.0,
                target_fill: 1500.0,
                poly_rate: 15.0,
                lipo_rate: 7.0,
            },
        ));
        
        let initial_snapshot = get_currency_snapshot(&app);
        
        // Run multi-system simulation
        for _ in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let current_snapshot = get_currency_snapshot(&app);
            
            // Verify all amounts remain within reasonable precision bounds
            for (i, (&_initial, &current)) in initial_snapshot.iter().zip(current_snapshot.iter()).enumerate() {
                prop_assert!(
                    current.is_finite(),
                    "Currency {} became non-finite: {}",
                    i,
                    current
                );
                
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