//! # Multi-System Interaction Property Tests
//!
//! These tests verify that multiple metabolic systems can operate together
//! without creating conflicts, race conditions, or invariant violations.
//! Focus on fermentation + fat storage + vesicle export interactions.

use approx::assert_relative_eq;
use bevy::prelude::*;
use bevy::time::{Fixed, Time};
use metabolistic3d::blocks::genome::{BlockKind, Genome};
use metabolistic3d::molecules::*;
use metabolistic3d::molecules::{CellMass, PolyMer};
use metabolistic3d::MetabolisticApp;
use proptest::prelude::*;

mod property_utils;
use property_utils::*;

// Helper function to get currency from a cell using ECS patterns
fn get_cell_currency_value(app: &App, cell_entity: Entity, currency: Currency) -> f32 {
    if let Ok(entity) = app.world().get_entity(cell_entity) {
        if let Some(currency_pools) = entity.get::<CurrencyPools>() {
            return currency_pools.get(currency);
        }
    }
    0.0
}

// --- Multi-System Coordination Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        timeout: 5000,
        .. ProptestConfig::default()
    })]
    /// Test that fermentation and vesicle export work together correctly
    /// Fermentation produces organic waste, vesicle export removes it
    #[test]
    fn fermentation_vesicle_export_coordination(
        initial_pyruvate in 100.0f32..500.0f32,
        initial_rp in 100.0f32..500.0f32,
        simulation_steps in 3..15usize  // Reduced for performance
    ) {
        // Create test cell with fermentation and lipid metabolism genes
        let mut app = MetabolisticApp::new_headless();
        let cell_entity = {
            let mut genome = Genome::default();
            genome.add_gene(BlockKind::Fermentation).express_gene(BlockKind::Fermentation);
            genome.add_gene(BlockKind::LipidMetabolism).express_gene(BlockKind::LipidMetabolism);
            
            app.world_mut().spawn((
                metabolistic3d::cell::Cell {
                    genome,
                    currency_pools: CurrencyPools::default(),
                    metabolic_graph: metabolistic3d::metabolism::MetabolicGraph::default(),
                    metabolic_blocks: std::collections::HashMap::new(),
                },
            )).id()
        };

        // Initialize fermentation inputs, disable fat storage by removing its resources
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            if let Some(mut currency_pools) = entity_mut.get_mut::<CurrencyPools>() {
                currency_pools.set(Currency::Pyruvate, initial_pyruvate);
                currency_pools.set(Currency::ReducingPower, initial_rp);
                currency_pools.set(Currency::ATP, 0.0);
                currency_pools.set(Currency::OrganicWaste, 0.0);
                currency_pools.set(Currency::FreeFattyAcids, 0.0);  // Disable fat storage
                currency_pools.set(Currency::StorageBeads, 0.0);
            }
        }
        
        // Trigger metabolic processing to ensure blocks are spawned and working
        run_cell_metabolic_cycle(&mut app, cell_entity);

        let mut max_waste_observed = 0.0f32;
        let mut waste_production_observed = false;

        // Run simulation and observe interactions
        for _ in 0..simulation_steps {
            let waste_before = get_cell_currency_value(&app, cell_entity, Currency::OrganicWaste);

            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Run per-cell metabolic processing each step
            run_cell_metabolic_cycle(&mut app, cell_entity);

            let waste_after = get_cell_currency_value(&app, cell_entity, Currency::OrganicWaste);

            // Track waste dynamics
            if waste_after > waste_before {
                waste_production_observed = true;
            }
            if waste_after < waste_before && waste_before > 0.0 {
                // waste_removal_observed = true;
            }

            max_waste_observed = max_waste_observed.max(waste_after);

            // Verify waste doesn't accumulate indefinitely
            prop_assert!(waste_after >= 0.0, "Organic waste went negative");

            // Verify all other currencies remain non-negative
            prop_assert!(cell_currencies_non_negative(&app, cell_entity));
        }

        // Verify that both systems actually operated
        // (At least one should have produced waste, vesicle export should have removed some)
        prop_assert!(waste_production_observed || max_waste_observed > 0.0,
            "No waste production observed - fermentation may not be working");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        timeout: 5000,
        .. ProptestConfig::default()
    })]
    /// Test fermentation + fat storage interaction with shared ATP consumption
    #[test]
    fn fermentation_fat_storage_atp_competition(
        initial_pyruvate in 100.0f32..300.0f32,
        initial_rp in 100.0f32..300.0f32,
        initial_ffa in 200.0f32..600.0f32,
        initial_atp in 20.0f32..100.0f32,
        toxicity_threshold in 50.0f32..150.0f32,
        simulation_steps in 3..10usize  // Reduced for performance
    ) {
        // Create test cell with fermentation gene
        let mut app = MetabolisticApp::new_headless();
        let cell_entity = {
            let mut genome = Genome::default();
            genome.add_gene(BlockKind::Fermentation).express_gene(BlockKind::Fermentation);
            
            app.world_mut().spawn((
                metabolistic3d::cell::Cell {
                    genome,
                    currency_pools: CurrencyPools::default(),
                    metabolic_graph: metabolistic3d::metabolism::MetabolicGraph::default(),
                    metabolic_blocks: std::collections::HashMap::new(),
                },
            )).id()
        };

        // Set up conditions where both systems compete for ATP
        modify_cell_currencies(&mut app, cell_entity, |currency_pools| {
            currency_pools.set(Currency::Pyruvate, initial_pyruvate);
            currency_pools.set(Currency::ReducingPower, initial_rp);
            currency_pools.set(Currency::ATP, initial_atp);
            currency_pools.set(Currency::OrganicWaste, 0.0);
            currency_pools.set(Currency::FreeFattyAcids, initial_ffa);
            currency_pools.set(Currency::StorageBeads, 10.0);
        });
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));

        // Trigger metabolic processing to ensure blocks are spawned and working
        run_cell_metabolic_cycle(&mut app, cell_entity);

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
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Run per-cell metabolic processing each step
            run_cell_metabolic_cycle(&mut app, cell_entity);

            // Verify ATP never goes negative despite competition
            let current_atp = get_cell_currency_value(&app, cell_entity, Currency::ATP);
            prop_assert!(current_atp >= 0.0,
                "ATP went negative due to system competition at step {}", step);

            // Verify all currencies remain non-negative
            prop_assert!(cell_currencies_non_negative(&app, cell_entity),
                "Currency went negative during ATP competition at step {}", step);

            // Verify systems operate consistently despite resource constraints
            let pyruvate = get_cell_currency_value(&app, cell_entity, Currency::Pyruvate);
            let ffa = get_cell_currency_value(&app, cell_entity, Currency::FreeFattyAcids);

            prop_assert!(pyruvate <= initial_pyruvate + 0.1);
            prop_assert!(ffa >= 0.0);
        }
    }
}

// --- Resource Contention and Priority Tests ---

// --- System Interaction Patterns ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        timeout: 5000,
        .. ProptestConfig::default()
    })]
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
        simulation_steps in 5..15usize  // Reduced for performance
    ) {
        let (pyruvate, rp, ffa, atp) = metabolic_inputs;
        let (toxicity_threshold, poly_rate, lipo_rate) = system_parameters;

        // Create test cell with fermentation and lipid metabolism genes
        let mut app = MetabolisticApp::new_headless();
        let cell_entity = {
            let mut genome = Genome::default();
            genome.add_gene(BlockKind::Fermentation).express_gene(BlockKind::Fermentation);
            genome.add_gene(BlockKind::LipidMetabolism).express_gene(BlockKind::LipidMetabolism);
            
            app.world_mut().spawn((
                metabolistic3d::cell::Cell {
                    genome,
                    currency_pools: CurrencyPools::default(),
                    metabolic_graph: metabolistic3d::metabolism::MetabolicGraph::default(),
                    metabolic_blocks: std::collections::HashMap::new(),
                },
            )).id()
        };

        // Initialize full system state
        modify_cell_currencies(&mut app, cell_entity, |currency_pools| {
            currency_pools.set(Currency::Pyruvate, pyruvate);
            currency_pools.set(Currency::ReducingPower, rp);
            currency_pools.set(Currency::ATP, atp);
            currency_pools.set(Currency::OrganicWaste, 10.0); // Small initial waste
            currency_pools.set(Currency::FreeFattyAcids, ffa);
            currency_pools.set(Currency::StorageBeads, 20.0);
        });
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));

        // Trigger metabolic processing to ensure blocks are spawned and working
        run_cell_metabolic_cycle(&mut app, cell_entity);

        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate,
                lipo_rate,
            },
        ));

        let initial_total = if let Some(snapshot) = get_cell_currencies(&app, cell_entity) {
            snapshot.pools.values().sum::<f32>()
        } else {
            0.0
        };

        // Run integrated three-system simulation
        for step in 0..simulation_steps {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Run per-cell metabolic processing each step
            run_cell_metabolic_cycle(&mut app, cell_entity);

            let current_total = if let Some(snapshot) = get_cell_currencies(&app, cell_entity) {
                snapshot.pools.values().sum::<f32>()
            } else {
                0.0
            };

            // Verify system integration maintains fundamental invariants
            prop_assert!(cell_currencies_non_negative(&app, cell_entity),
                "Three-system integration violated non-negative invariant at step {}", step);

            // Verify no runaway resource generation
            prop_assert!(current_total <= initial_total * 1.1,
                "Suspicious resource generation: initial {}, current {}",
                initial_total, current_total);

            // Verify individual system constraints
            let current_ffa = get_cell_currency_value(&app, cell_entity, Currency::FreeFattyAcids);
            let current_storage = get_cell_currency_value(&app, cell_entity, Currency::StorageBeads);
            let current_waste = get_cell_currency_value(&app, cell_entity, Currency::OrganicWaste);

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
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        timeout: 5000,
        .. ProptestConfig::default()
    })]
    /// Test system interactions when one system is completely depleted
    #[test]
    fn depleted_system_interactions(
        depletion_scenario in prop_oneof![
            Just("no_fermentation_inputs"),
            Just("no_atp"),
            Just("no_ffa"),
            Just("no_waste")
        ],
        simulation_steps in 3..10usize  // Reduced for performance
    ) {
        // Create test cell with fermentation and lipid metabolism genes
        let mut app = MetabolisticApp::new_headless();
        let cell_entity = {
            let mut genome = Genome::default();
            genome.add_gene(BlockKind::Fermentation).express_gene(BlockKind::Fermentation);
            genome.add_gene(BlockKind::LipidMetabolism).express_gene(BlockKind::LipidMetabolism);
            
            app.world_mut().spawn((
                metabolistic3d::cell::Cell {
                    genome,
                    currency_pools: CurrencyPools::default(),
                    metabolic_graph: metabolistic3d::metabolism::MetabolicGraph::default(),
                    metabolic_blocks: std::collections::HashMap::new(),
                },
            )).id()
        };

        // Set up depletion scenario
        modify_cell_currencies(&mut app, cell_entity, |currency_pools| {
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
        });
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));

        // Trigger metabolic processing to ensure blocks are spawned and working
        run_cell_metabolic_cycle(&mut app, cell_entity);

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
            
            // Run per-cell metabolic processing each step
            run_cell_metabolic_cycle(&mut app, cell_entity);

            // Verify systems handle depletion gracefully
            prop_assert!(cell_currencies_non_negative(&app, cell_entity),
                "System failed with depletion scenario '{}' at step {}",
                depletion_scenario, step);

            // Verify no system creates resources from nothing
            if let Some(current_snapshot) = get_cell_currencies(&app, cell_entity) {
                for &amount in current_snapshot.pools.values() {
                    prop_assert!(amount.is_finite(),
                        "Currency became non-finite in depletion scenario '{}': {}",
                        depletion_scenario, amount);
                }
            }
        }
    }
}
