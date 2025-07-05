//! # Currency Conservation Invariant Tests
//!
//! Property-based tests that verify fundamental invariants of the currency system:
//! 1. Currency pools never go below zero
//! 2. Mass balance is maintained in metabolic operations
//! 3. Transfer operations are zero-sum (conservation of total currency)

use proptest::prelude::*;
use approx::assert_relative_eq;
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

// --- Invariant 1: Currency Pools Never Go Below Zero ---

proptest! {
    /// Test that all currency pools remain non-negative after any sequence of consumption attempts
    #[test]
    fn currency_pools_never_negative(
        initial_atp in currency_amount(),
        initial_rp in currency_amount(),
        consumption_attempts in prop::collection::vec(consumption_amount(), 1..20)
    ) {
        let mut app = MetabolisticApp::new_headless();
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::ATP, initial_atp);
        currency_pools.set(Currency::ReducingPower, initial_rp);
        
        // Attempt various consumption operations
        for &amount in &consumption_attempts {
            let atp_before = app.world().resource::<CurrencyPools>().get(Currency::ATP);
            let rp_before = app.world().resource::<CurrencyPools>().get(Currency::ReducingPower);
            
            // Try to consume ATP (should fail gracefully when insufficient)
            if app.world().resource::<CurrencyPools>().can_consume(Currency::ATP, amount) {
                app.world_mut().resource_mut::<CurrencyPools>().modify(Currency::ATP, -amount);
            }
            
            // Try to consume ReducingPower (should fail gracefully when insufficient)
            if app.world().resource::<CurrencyPools>().can_consume(Currency::ReducingPower, amount) {
                app.world_mut().resource_mut::<CurrencyPools>().modify(Currency::ReducingPower, -amount);
            }
            
            let atp_after = app.world().resource::<CurrencyPools>().get(Currency::ATP);
            let rp_after = app.world().resource::<CurrencyPools>().get(Currency::ReducingPower);
            
            // Verify currencies never go negative
            prop_assert!(atp_after >= 0.0, "ATP went negative: {}", atp_after);
            prop_assert!(rp_after >= 0.0, "ReducingPower went negative: {}", rp_after);
        }
        
        // Final verification that all currencies are non-negative
        prop_assert!(all_currencies_non_negative(&app));
    }
}

proptest! {
    /// Test that fermentation system maintains non-negative currency invariant
    #[test]
    fn fermentation_maintains_non_negative_currencies(
        pyruvate_amount in currency_amount(),
        reducing_power_amount in currency_amount(),
        simulation_steps in 1..100usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize resources
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate_amount);
        currency_pools.set(Currency::ReducingPower, reducing_power_amount);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        
        // Spawn fermentation block
        app.world_mut().spawn(FermentationBlock);
        
        // Run simulation for multiple steps
        for step in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Verify all currencies remain non-negative after each step
            prop_assert!(all_currencies_non_negative(&app), 
                "Currencies went negative during fermentation simulation at step {}", step);
        }
    }
}

// --- Invariant 2: Mass Balance in Metabolic Operations ---

proptest! {
    /// Test that fermentation maintains mass balance (inputs consumed = outputs produced)
    #[test]
    fn fermentation_mass_balance(
        initial_pyruvate in 50.0f32..500.0f32,
        initial_rp in 50.0f32..500.0f32,
        simulation_steps in 1..50usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize resources with sufficient amounts for testing
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, initial_pyruvate);
        currency_pools.set(Currency::ReducingPower, initial_rp);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        
        app.world_mut().spawn(FermentationBlock);
        
        let initial_inputs = initial_pyruvate + initial_rp;
        
        // Run simulation
        for _ in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
        }
        
        let currency_pools = app.world().resource::<CurrencyPools>();
        let final_pyruvate = currency_pools.get(Currency::Pyruvate);
        let final_rp = currency_pools.get(Currency::ReducingPower);
        let final_atp = currency_pools.get(Currency::ATP);
        let final_waste = currency_pools.get(Currency::OrganicWaste);
        
        let remaining_inputs = final_pyruvate + final_rp;
        let total_outputs = final_atp + final_waste;
        let consumed_inputs = initial_inputs - remaining_inputs;
        
        // Mass balance: consumed inputs should approximately equal produced outputs
        // Allow for small floating-point precision differences
        assert_relative_eq!(consumed_inputs, total_outputs, epsilon = 0.1);
    }
}

// --- Invariant 3: Transfer Operations are Zero-Sum ---

proptest! {
    /// Test that fat storage polymerization is zero-sum (FFA decrease = StorageBeads increase)
    #[test]
    fn fat_storage_zero_sum_transfer(
        initial_ffa in 100.0f32..500.0f32,
        initial_storage in currency_amount(),
        toxicity_threshold in 10.0f32..90.0f32,
        poly_rate in polymer_rate(),
        simulation_steps in 1..20usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Set up conditions for polymerization (FFA above threshold)
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::FreeFattyAcids, initial_ffa);
        currency_pools.set(Currency::StorageBeads, initial_storage);
        currency_pools.set(Currency::ATP, 100.0); // Sufficient ATP
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        
        // Spawn polymer entity
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 1000.0,
                target_fill: 500.0,
                poly_rate,
                lipo_rate: 5.0,
            },
        ));
        
        let initial_total_lipids = initial_ffa + initial_storage;
        
        // Run simulation
        for _ in 0..simulation_steps {
            app.update();
            
            let currency_pools = app.world().resource::<CurrencyPools>();
            let current_ffa = currency_pools.get(Currency::FreeFattyAcids);
            let current_storage = currency_pools.get(Currency::StorageBeads);
            let current_total = current_ffa + current_storage;
            
            // Verify zero-sum property: total lipids should remain constant
            // (allowing for small floating-point precision differences)
            assert_relative_eq!(current_total, initial_total_lipids, epsilon = 0.1);
            
            // All amounts should remain non-negative
            prop_assert!(current_ffa >= 0.0);
            prop_assert!(current_storage >= 0.0);
        }
    }
}

proptest! {
    /// Test that vesicle export only removes waste (doesn't create currency from nothing)
    #[test]
    fn vesicle_export_only_removes(
        initial_waste in 50.0f32..300.0f32,
        simulation_steps in 1..50usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        app.world_mut().resource_mut::<CurrencyPools>().set(Currency::OrganicWaste, initial_waste);
        app.world_mut().spawn(VesicleExportBlock);
        
        let initial_snapshot = get_currency_snapshot(&app);
        let initial_total = initial_snapshot.iter().sum::<f32>();
        
        // Run simulation
        for _ in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let current_snapshot = get_currency_snapshot(&app);
            let current_total = current_snapshot.iter().sum::<f32>();
            
            // Total currency should only decrease (vesicle export removes waste)
            prop_assert!(current_total <= initial_total + 0.1); // Allow small epsilon for floating-point
            
            // Waste specifically should only decrease
            let current_waste = app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste);
            prop_assert!(current_waste <= initial_waste + 0.1);
            
            // No currency should go negative
            prop_assert!(all_currencies_non_negative(&app));
        }
    }
}

// --- Combined System Invariant Tests ---

proptest! {
    /// Test that complex multi-system interactions maintain all invariants
    #[test]
    fn multi_system_invariants(
        initial_pyruvate in 100.0f32..300.0f32,
        initial_rp in 100.0f32..300.0f32,
        initial_ffa in 150.0f32..400.0f32,
        toxicity_threshold in 50.0f32..100.0f32,
        simulation_steps in 1..30usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize all currencies
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, initial_pyruvate);
        currency_pools.set(Currency::ReducingPower, initial_rp);
        currency_pools.set(Currency::ATP, 50.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        currency_pools.set(Currency::FreeFattyAcids, initial_ffa);
        currency_pools.set(Currency::StorageBeads, 10.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        
        // Spawn all system blocks
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 1000.0,
                target_fill: 500.0,   
                poly_rate: 20.0,
                lipo_rate: 5.0,
            },
        ));
        
        // Run multi-system simulation
        for step in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Verify all invariants hold at each step
            prop_assert!(all_currencies_non_negative(&app), 
                "Currency went negative at step {}", step);
        }
    }
}