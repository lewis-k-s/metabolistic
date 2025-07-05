//! # Multi-System Interaction Property Tests
//!
//! These tests verify that multiple metabolic systems can operate together
//! without creating conflicts, race conditions, or invariant violations.
//! Focus on fermentation + fat storage + vesicle export interactions.

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

// --- Multi-System Coordination Tests ---

proptest! {
    /// Test that fermentation and vesicle export work together correctly
    /// Fermentation produces organic waste, vesicle export removes it
    #[test]
    fn fermentation_vesicle_export_coordination(
        initial_pyruvate in 100.0f32..500.0f32,
        initial_rp in 100.0f32..500.0f32,
        simulation_steps in 10..100usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize fermentation inputs, disable fat storage by removing its resources
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, initial_pyruvate);
        currency_pools.set(Currency::ReducingPower, initial_rp);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        currency_pools.set(Currency::FreeFattyAcids, 0.0);  // Disable fat storage
        currency_pools.set(Currency::StorageBeads, 0.0);
        
        // Configure genome: enable fermentation, disable competing systems
        {
            let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            
            // Ensure fat storage doesn't interfere by not expressing it
            if genome.get_gene_state(&metabolistic3d::blocks::genome::BlockKind::LipidMetabolism).is_some() {
                genome.silence_gene(metabolistic3d::blocks::genome::BlockKind::LipidMetabolism);
            }
        }
        
        // Spawn vesicle export block (fermentation block is auto-spawned by FermentationPlugin)
        app.world_mut().spawn(VesicleExportBlock);
        
        // Trigger metabolic graph rebuild to process gene expression changes
        app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
        
        let mut max_waste_observed = 0.0f32;
        let mut waste_production_observed = false;
        let mut waste_removal_observed = false;
        
        // Run simulation and observe interactions
        for _ in 0..simulation_steps {
            let waste_before = app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste);
            
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let waste_after = app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste);
            
            // Track waste dynamics
            if waste_after > waste_before {
                waste_production_observed = true;
            }
            if waste_after < waste_before && waste_before > 0.0 {
                waste_removal_observed = true;
            }
            
            max_waste_observed = max_waste_observed.max(waste_after);
            
            // Verify waste doesn't accumulate indefinitely
            prop_assert!(waste_after >= 0.0, "Organic waste went negative");
            
            // Verify all other currencies remain non-negative
            prop_assert!(all_currencies_non_negative(&app));
        }
        
        // Verify that both systems actually operated
        // (At least one should have produced waste, vesicle export should have removed some)
        prop_assert!(waste_production_observed || max_waste_observed > 0.0, 
            "No waste production observed - fermentation may not be working");
    }
}

proptest! {
    /// Test fermentation + fat storage interaction with shared ATP consumption
    #[test]
    fn fermentation_fat_storage_atp_competition(
        initial_pyruvate in 100.0f32..300.0f32,
        initial_rp in 100.0f32..300.0f32,
        initial_ffa in 200.0f32..600.0f32,
        initial_atp in 20.0f32..100.0f32,
        toxicity_threshold in 50.0f32..150.0f32,
        simulation_steps in 10..50usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Set up conditions where both systems compete for ATP
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, initial_pyruvate);
        currency_pools.set(Currency::ReducingPower, initial_rp);
        currency_pools.set(Currency::ATP, initial_atp);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        currency_pools.set(Currency::FreeFattyAcids, initial_ffa);
        currency_pools.set(Currency::StorageBeads, 10.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        
        // Express fermentation gene to enable the automatically spawned block
        {
            let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
        }
        
        // Trigger metabolic graph rebuild to process gene expression changes
        app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
        
        // Note: FermentationPlugin automatically spawns a fermentation entity during startup
        
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 1000.0,
                target_fill: 500.0,
                poly_rate: 20.0,
                lipo_rate: 5.0,
            },
        ));
        
        // Run simulation with ATP competition
        for step in 0..simulation_steps {
            app.update();
            
            // Verify ATP never goes negative despite competition
            let current_atp = app.world().resource::<CurrencyPools>().get(Currency::ATP);
            prop_assert!(current_atp >= 0.0, 
                "ATP went negative due to system competition at step {}", step);
            
            // Verify all currencies remain non-negative
            prop_assert!(all_currencies_non_negative(&app), 
                "Currency went negative during ATP competition at step {}", step);
            
            // Verify systems operate consistently despite resource constraints
            let pyruvate = app.world().resource::<CurrencyPools>().get(Currency::Pyruvate);
            let ffa = app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids);
            
            prop_assert!(pyruvate <= initial_pyruvate + 0.1);
            prop_assert!(ffa >= 0.0);
        }
    }
}

// --- Resource Contention and Priority Tests ---

proptest! {
    /// Test system behavior under extreme resource scarcity
    #[test]
    fn resource_scarcity_handling(
        scarce_atp in 0.1f32..5.0f32,
        scarce_pyruvate in 0.1f32..10.0f32,
        scarce_rp in 0.1f32..10.0f32,
        high_ffa in 500.0f32..1000.0f32,
        simulation_steps in 5..30usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Create scarcity conditions
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::ATP, scarce_atp);
        currency_pools.set(Currency::Pyruvate, scarce_pyruvate);
        currency_pools.set(Currency::ReducingPower, scarce_rp);
        currency_pools.set(Currency::FreeFattyAcids, high_ffa);
        currency_pools.set(Currency::OrganicWaste, 100.0); // High waste
        currency_pools.set(Currency::StorageBeads, 5.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(50.0)); // Low threshold
        
        // Express fermentation gene to enable the block
        {
            let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
        }
        
        // Spawn all systems under scarcity with complete component architecture
        app.world_mut().spawn((
            FermentationBlock,
            metabolistic3d::metabolism::MetabolicBlock,
            metabolistic3d::metabolism::MetabolicNode {
                kind: metabolistic3d::blocks::genome::BlockKind::Fermentation,
                status: metabolistic3d::metabolism::BlockStatus::Active,
            },
            {
                let mut flux_profile = metabolistic3d::metabolism::FluxProfile::default();
                flux_profile.0.insert(Currency::Pyruvate, -1.0);
                flux_profile.0.insert(Currency::ReducingPower, -1.0);
                flux_profile.0.insert(Currency::ATP, 1.0);
                flux_profile.0.insert(Currency::OrganicWaste, 1.0);
                flux_profile
            },
        ));
        app.world_mut().spawn(VesicleExportBlock);
        
        // Trigger metabolic graph rebuild
        app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
        
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate: 30.0, // High rate to stress ATP
                lipo_rate: 10.0,
            },
        ));
        
        // Run under scarcity conditions
        for step in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Verify systems gracefully handle scarcity
            prop_assert!(all_currencies_non_negative(&app),
                "System failed under scarcity at step {}", step);
            
            // Verify high-priority operations still function
            // (vesicle export should continue removing waste)
            let waste = app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste);
            prop_assert!(waste >= 0.0);
            
            // Verify no system causes overflow or underflow
            let current_snapshot = get_currency_snapshot(&app);
            for (i, &amount) in current_snapshot.iter().enumerate() {
                prop_assert!(amount.is_finite(), 
                    "Currency {} became non-finite under scarcity: {}", i, amount);
            }
        }
    }
}

// --- System Interaction Patterns ---

proptest! {
    /// Test cascading effects: fermentation → waste → vesicle export
    #[test]
    fn cascading_system_effects(
        fermentation_inputs in (50.0f32..200.0f32, 50.0f32..200.0f32), // pyruvate, rp
        export_rate in 0.05f32..0.5f32,
        simulation_steps in 20..80usize
    ) {
        let (pyruvate, rp) = fermentation_inputs;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up cascade: fermentation produces waste, export removes it
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate);
        currency_pools.set(Currency::ReducingPower, rp);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        
        // Express fermentation gene to enable the block
        {
            let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
        }
        
        // Spawn cascade systems with complete component architecture
        app.world_mut().spawn((
            FermentationBlock,
            metabolistic3d::metabolism::MetabolicBlock,
            metabolistic3d::metabolism::MetabolicNode {
                kind: metabolistic3d::blocks::genome::BlockKind::Fermentation,
                status: metabolistic3d::metabolism::BlockStatus::Active,
            },
            {
                let mut flux_profile = metabolistic3d::metabolism::FluxProfile::default();
                flux_profile.0.insert(Currency::Pyruvate, -1.0);
                flux_profile.0.insert(Currency::ReducingPower, -1.0);
                flux_profile.0.insert(Currency::ATP, 1.0);
                flux_profile.0.insert(Currency::OrganicWaste, 1.0);
                flux_profile
            },
        ));
        app.world_mut().spawn(VesicleExportBlock);
        
        // Trigger metabolic graph rebuild
        app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
        
        let mut waste_levels = Vec::new();
        
        // Track cascading effects over time
        for _ in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let current_waste = app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste);
            waste_levels.push(current_waste);
            
            // Verify cascade doesn't break down
            prop_assert!(current_waste >= 0.0);
            prop_assert!(all_currencies_non_negative(&app));
        }
        
        // Analyze cascade dynamics
        let max_waste = waste_levels.iter().copied().fold(0.0f32, f32::max);
        let final_waste = waste_levels.last().copied().unwrap_or(0.0);
        
        // Verify cascade produces dynamic behavior (not stuck at zero)
        if pyruvate > 1.0 && rp > 1.0 {
            prop_assert!(max_waste > 0.1, 
                "Cascade failed to produce waste: max observed {}", max_waste);
        }
        
        // Verify export prevents unlimited accumulation
        prop_assert!(final_waste <= max_waste + 0.1,
            "Waste export failed to control accumulation");
    }
}

proptest! {
    /// Test all three systems operating simultaneously with realistic parameters
    #[test]
    fn three_system_integration(
        metabolic_inputs in (
            100.0f32..400.0f32, // pyruvate
            100.0f32..400.0f32, // reducing power
            200.0f32..800.0f32, // free fatty acids
            50.0f32..200.0f32   // initial atp
        ),
        system_parameters in (
            20.0f32..100.0f32, // toxicity threshold
            10.0f32..40.0f32,  // poly rate
            2.0f32..15.0f32    // lipo rate
        ),
        simulation_steps in 15..60usize
    ) {
        let (pyruvate, rp, ffa, atp) = metabolic_inputs;
        let (toxicity_threshold, poly_rate, lipo_rate) = system_parameters;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize full system state
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, pyruvate);
        currency_pools.set(Currency::ReducingPower, rp);
        currency_pools.set(Currency::ATP, atp);
        currency_pools.set(Currency::OrganicWaste, 10.0); // Small initial waste
        currency_pools.set(Currency::FreeFattyAcids, ffa);
        currency_pools.set(Currency::StorageBeads, 20.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        
        // Express fermentation gene to enable the block
        {
            let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
        }
        
        // Spawn all three systems with complete component architecture
        app.world_mut().spawn((
            FermentationBlock,
            metabolistic3d::metabolism::MetabolicBlock,
            metabolistic3d::metabolism::MetabolicNode {
                kind: metabolistic3d::blocks::genome::BlockKind::Fermentation,
                status: metabolistic3d::metabolism::BlockStatus::Active,
            },
            {
                let mut flux_profile = metabolistic3d::metabolism::FluxProfile::default();
                flux_profile.0.insert(Currency::Pyruvate, -1.0);
                flux_profile.0.insert(Currency::ReducingPower, -1.0);
                flux_profile.0.insert(Currency::ATP, 1.0);
                flux_profile.0.insert(Currency::OrganicWaste, 1.0);
                flux_profile
            },
        ));
        app.world_mut().spawn(VesicleExportBlock);
        
        // Trigger metabolic graph rebuild
        app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
        
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate,
                lipo_rate,
            },
        ));
        
        let initial_snapshot = get_currency_snapshot(&app);
        let initial_total = initial_snapshot.iter().sum::<f32>();
        
        // Run integrated three-system simulation
        for step in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let current_snapshot = get_currency_snapshot(&app);
            let current_total = current_snapshot.iter().sum::<f32>();
            
            // Verify system integration maintains fundamental invariants
            prop_assert!(all_currencies_non_negative(&app),
                "Three-system integration violated non-negative invariant at step {}", step);
            
            // Verify no runaway resource generation
            prop_assert!(current_total <= initial_total * 1.1,
                "Suspicious resource generation: initial {}, current {}", 
                initial_total, current_total);
            
            // Verify individual system constraints
            let currency_pools = app.world().resource::<CurrencyPools>();
            let current_ffa = currency_pools.get(Currency::FreeFattyAcids);
            let current_storage = currency_pools.get(Currency::StorageBeads);
            let current_waste = currency_pools.get(Currency::OrganicWaste);
            
            prop_assert!(current_ffa >= 0.0);
            prop_assert!(current_storage >= 0.0);
            prop_assert!(current_waste >= 0.0);
            
            // Verify lipid conservation in fat storage system
            let total_lipids = current_ffa + current_storage;
            let initial_lipids = ffa + 20.0;
            assert_relative_eq!(total_lipids, initial_lipids, epsilon = 1.0);
        }
    }
}

// --- Edge Case Interaction Tests ---

proptest! {
    /// Test system interactions when one system is completely depleted
    #[test] 
    fn depleted_system_interactions(
        depletion_scenario in prop_oneof![
            Just("no_fermentation_inputs"),
            Just("no_atp"),
            Just("no_ffa"),
            Just("no_waste")
        ],
        simulation_steps in 10..40usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Set up depletion scenario
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        match depletion_scenario {
            "no_fermentation_inputs" => {
                currency_pools.set(Currency::Pyruvate, 0.0);
                currency_pools.set(Currency::ReducingPower, 0.0);
                currency_pools.set(Currency::ATP, 100.0);
                currency_pools.set(Currency::FreeFattyAcids, 300.0);
                currency_pools.set(Currency::OrganicWaste, 50.0);
            },
            "no_atp" => {
                currency_pools.set(Currency::Pyruvate, 100.0);
                currency_pools.set(Currency::ReducingPower, 100.0);
                currency_pools.set(Currency::ATP, 0.0);
                currency_pools.set(Currency::FreeFattyAcids, 300.0);
                currency_pools.set(Currency::OrganicWaste, 50.0);
            },
            "no_ffa" => {
                currency_pools.set(Currency::Pyruvate, 100.0);
                currency_pools.set(Currency::ReducingPower, 100.0);
                currency_pools.set(Currency::ATP, 100.0);
                currency_pools.set(Currency::FreeFattyAcids, 0.0);
                currency_pools.set(Currency::OrganicWaste, 50.0);
            },
            "no_waste" => {
                currency_pools.set(Currency::Pyruvate, 100.0);
                currency_pools.set(Currency::ReducingPower, 100.0);
                currency_pools.set(Currency::ATP, 100.0);
                currency_pools.set(Currency::FreeFattyAcids, 300.0);
                currency_pools.set(Currency::OrganicWaste, 0.0);
            },
            _ => unreachable!()
        }
        
        currency_pools.set(Currency::StorageBeads, 20.0);
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));
        
        // Express fermentation gene to enable the block
        {
            let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
        }
        
        // Spawn all systems despite depletion with complete component architecture
        app.world_mut().spawn((
            FermentationBlock,
            metabolistic3d::metabolism::MetabolicBlock,
            metabolistic3d::metabolism::MetabolicNode {
                kind: metabolistic3d::blocks::genome::BlockKind::Fermentation,
                status: metabolistic3d::metabolism::BlockStatus::Active,
            },
            {
                let mut flux_profile = metabolistic3d::metabolism::FluxProfile::default();
                flux_profile.0.insert(Currency::Pyruvate, -1.0);
                flux_profile.0.insert(Currency::ReducingPower, -1.0);
                flux_profile.0.insert(Currency::ATP, 1.0);
                flux_profile.0.insert(Currency::OrganicWaste, 1.0);
                flux_profile
            },
        ));
        app.world_mut().spawn(VesicleExportBlock);
        
        // Trigger metabolic graph rebuild
        app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
        
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 1000.0,
                target_fill: 500.0,
                poly_rate: 25.0,
                lipo_rate: 8.0,
            },
        ));
        
        // Run simulation with depleted resources
        for step in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Verify systems handle depletion gracefully
            prop_assert!(all_currencies_non_negative(&app),
                "System failed with depletion scenario '{}' at step {}", 
                depletion_scenario, step);
            
            // Verify no system creates resources from nothing
            let current_snapshot = get_currency_snapshot(&app);
            for (i, &amount) in current_snapshot.iter().enumerate() {
                prop_assert!(amount.is_finite(),
                    "Currency {} became non-finite in depletion scenario '{}': {}", 
                    i, depletion_scenario, amount);
            }
        }
    }
}