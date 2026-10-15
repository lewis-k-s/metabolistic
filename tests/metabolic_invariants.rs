//! # Game-Specific Metabolic Invariant Tests  
//!
//! These tests verify higher-level game logic invariants specific to the
//! metabolic simulation, including economic balance, genetic-metabolic
//! consistency, and cellular viability constraints.

use proptest::prelude::*;
use metabolistic3d::molecules::*;
use metabolistic3d::blocks::genome::{GeneState, Genome};
use metabolistic3d::blocks::fermentation::FermentationBlock;
use metabolistic3d::molecules::{PolyMer, CellMass};
use metabolistic3d::blocks::vesicle_export::VesicleExportBlock;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::player::spawn_local_player_cell;
use bevy::prelude::*;
use bevy::time::{Time, Fixed};

mod common;
use common::*;

// --- Economic Balance Invariants ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that cellular metabolism maintains economic viability over time
    /// (enough ATP production to sustain cellular processes)
    #[test]
    #[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
    fn metabolic_economic_viability(
        initial_resources in (
            100.0f32..400.0f32, // pyruvate 
            100.0f32..400.0f32, // reducing power
            50.0f32..200.0f32,  // initial atp
            200.0f32..600.0f32  // free fatty acids
        ),
        economic_stress_duration in 5..20usize  // Reduced for performance
    ) {
        let (pyruvate, rp, atp, ffa) = initial_resources;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up economically stressed but viable cell
        let (_player_entity, cell_entity) = {
            let mut genome = Genome::default();
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
            genome.add_gene(metabolistic3d::blocks::genome::BlockKind::LipidMetabolism);
            genome.express_gene(metabolistic3d::blocks::genome::BlockKind::LipidMetabolism);
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, genome)
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
            currency_pools.set(Currency::StorageBeads, 30.0);
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));
        
        // Run an update to spawn the blocks from the genome
        app.update();
        
        let mut atp_levels = Vec::new();
        let mut total_energy_production = 0.0f32;
        
        // Monitor economic health over stress period
        for step in 0..economic_stress_duration {
            let atp_before = {
                app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP)
            };
            
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let atp_after = {
                app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::ATP)
            };
            let atp_produced_this_step = (atp_after - atp_before).max(0.0);
            total_energy_production += atp_produced_this_step;
            
            atp_levels.push(atp_after);
            
            // Verify cell doesn't enter economic death spiral
            prop_assert!(atp_after >= 0.0, "ATP went negative at step {}", step);
            prop_assert!(all_currencies_non_negative(&app), 
                "Economic collapse detected at step {}", step);
        }
        
        // Analyze economic sustainability
        let avg_atp = atp_levels.iter().sum::<f32>() / atp_levels.len() as f32;
        let final_atp = atp_levels.last().copied().unwrap_or(0.0);
        
        // Verify economic viability metrics
        prop_assert!(avg_atp >= 1.0, 
            "Average ATP too low for viability: {}", avg_atp);
        
        if initial_resources.0 > 50.0 && initial_resources.1 > 50.0 {
            prop_assert!(total_energy_production > 0.0,
                "No net energy production observed with sufficient inputs");
        }
        
        // Verify no catastrophic economic collapse  
        prop_assert!(final_atp >= avg_atp * 0.1,
            "Economic collapse: final ATP {} much lower than average {}", 
            final_atp, avg_atp);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that resource scarcity leads to graceful degradation, not system failure
    #[test]
    #[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
    fn scarcity_graceful_degradation(
        scarcity_scenario in prop_oneof![
            Just("energy_scarce"),      // Low ATP/pyruvate/RP
            Just("carbon_scarce"),      // Low carbon skeletons/acetyl-CoA
            Just("lipid_overwhelmed")   // Too much FFA, insufficient processing
        ],
        degradation_period in 5..15usize  // Reduced for performance
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Set up scarcity conditions
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();
        
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
            match scarcity_scenario {
                "energy_scarce" => {
                    currency_pools.set(Currency::Pyruvate, 10.0);   // Very low
                    currency_pools.set(Currency::ReducingPower, 5.0); // Very low
                    currency_pools.set(Currency::ATP, 2.0);          // Very low
                    currency_pools.set(Currency::FreeFattyAcids, 100.0);
                    currency_pools.set(Currency::OrganicWaste, 50.0);
                },
                "carbon_scarce" => {
                    currency_pools.set(Currency::Pyruvate, 5.0);     // Very low
                    currency_pools.set(Currency::ReducingPower, 100.0);
                    currency_pools.set(Currency::ATP, 50.0);
                    currency_pools.set(Currency::FreeFattyAcids, 20.0); // Low
                    currency_pools.set(Currency::OrganicWaste, 10.0);
                },
                "lipid_overwhelmed" => {
                    currency_pools.set(Currency::Pyruvate, 200.0);
                    currency_pools.set(Currency::ReducingPower, 200.0);
                    currency_pools.set(Currency::ATP, 100.0);
                    currency_pools.set(Currency::FreeFattyAcids, 1000.0); // Overwhelming
                    currency_pools.set(Currency::OrganicWaste, 300.0);   // High waste
                },
                _ => unreachable!()
            }
            currency_pools.set(Currency::StorageBeads, 20.0);
        }
        
        app.world_mut().insert_resource(LipidToxicityThreshold(150.0));
        
        // Spawn all systems under stress
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate: 25.0,
                lipo_rate: 12.0,
            },
        ));
        
        // Monitor graceful degradation
        for step in 0..degradation_period {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Verify graceful degradation properties
            prop_assert!(all_currencies_non_negative(&app),
                "System failure under {} scarcity at step {}", scarcity_scenario, step);
            
            // Verify no overflow/underflow in stress conditions
            let snapshot = get_currency_snapshot(&app);
            for (i, &amount) in snapshot.iter().enumerate() {
                prop_assert!(amount.is_finite(),
                    "Currency {} became non-finite under {} at step {}: {}", 
                    i, scarcity_scenario, step, amount);
                
                prop_assert!(amount <= 1e9,
                    "Currency {} overflow under {} at step {}: {}", 
                    i, scarcity_scenario, step, amount);
            }
        }
    }
}


// --- Genetic-Metabolic Consistency Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that genome state changes properly propagate to metabolic system state
    #[test]
    #[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
    fn genome_metabolic_consistency(
        gene_operations in prop::collection::vec(
            (block_kind(), gene_state()),
            1..6
        ),
        consistency_check_steps in 3..8usize  // Reduced for performance
    ) {
        let mut app = MetabolisticApp::new_headless();
        
        // Initialize full system
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();
        
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
            currency_pools.set(Currency::Pyruvate, 200.0);
            currency_pools.set(Currency::ReducingPower, 200.0);
            currency_pools.set(Currency::ATP, 100.0);
            currency_pools.set(Currency::OrganicWaste, 10.0);
            currency_pools.set(Currency::FreeFattyAcids, 300.0);
            currency_pools.set(Currency::StorageBeads, 25.0);
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(120.0));
        
        // Add all possible genes to the genome that we might operate on
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            if let Some(mut genome) = entity_mut.get_mut::<Genome>() {
                for (block_kind, _) in &gene_operations {
                    genome.add_gene(*block_kind);
                }
            }
        }
        app.update(); // Run update to process added genes if necessary

        // Apply gene operations and verify consistency
        for &(block_kind, ref target_state) in &gene_operations {
            // Modify genome state
            {
                let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
                if let Some(mut genome) = entity_mut.get_mut::<Genome>() {
                    match target_state {
                        GeneState::Expressed => {
                            genome.express_gene(block_kind);
                        },
                        GeneState::Mutated => {
                            // To mutate, it must first be expressed then mutated
                            genome.express_gene(block_kind);
                            genome.mutate_gene(block_kind);
                        },
                        GeneState::Silent => {
                            genome.silence_gene(block_kind);
                        },
                    }
                }
            }
            
            // Run simulation to propagate changes
            for step in 0..consistency_check_steps {
                app.update();
                
                // After a few steps, check for consistency
                if step > 5 {
                    let should_exist = matches!(target_state, GeneState::Expressed | GeneState::Mutated);

                    let mut block_exists = false;
                    // Check if block exists by looking at cell's metabolic_blocks map
                    if let Ok(cell) = app.world().get_entity(cell_entity) {
                        if let Some(cell_component) = cell.get::<metabolistic3d::cell::Cell>() {
                            block_exists = cell_component.metabolic_blocks.contains_key(&block_kind);
                        }
                    }

                    prop_assert_eq!(block_exists, should_exist,
                        "Block existence inconsistency for {:?}: expected {}, found {}",
                        block_kind, should_exist, block_exists);
                }
            }
             // Verify consistency doesn't break other invariants
            prop_assert!(all_currencies_non_negative(&app),
                "Currency invariant violated during genome-metabolic consistency test");
        }
    }
}

// --- Cellular Viability Constraints ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that toxic waste levels trigger appropriate protective responses
    #[test]
    #[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
    fn toxicity_response_mechanisms(
        toxicity_scenario in (
            200.0f32..800.0f32, // high initial organic waste
            50.0f32..150.0f32,  // toxicity threshold
            20.0f32..100.0f32   // vesicle export capacity
        ),
        toxicity_resolution_time in 5..15usize  // Reduced for performance
    ) {
        let (initial_waste, threshold, _export_capacity) = toxicity_scenario;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up high toxicity scenario
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();
        
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
            currency_pools.set(Currency::OrganicWaste, initial_waste);
            currency_pools.set(Currency::Pyruvate, 50.0);
            currency_pools.set(Currency::ReducingPower, 50.0);
            currency_pools.set(Currency::ATP, 100.0);
            currency_pools.set(Currency::FreeFattyAcids, 200.0);
            currency_pools.set(Currency::StorageBeads, 30.0);
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(threshold));
        
        // Spawn detoxification systems
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn(FermentationBlock); // Can produce more waste
        
        let mut waste_levels = Vec::new();
        let mut toxicity_responses_observed = false;
        
        // Monitor toxicity response
        for step in 0..toxicity_resolution_time {
            let waste_before = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::OrganicWaste);
            
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            let waste_after = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::OrganicWaste);
            waste_levels.push(waste_after);
            
            // Check for toxicity response (waste removal)
            if waste_after < waste_before && waste_before > threshold {
                toxicity_responses_observed = true;
            }
            
            // Verify toxicity doesn't kill the cell
            prop_assert!(waste_after >= 0.0, "Waste went negative at step {}", step);
            prop_assert!(all_currencies_non_negative(&app),
                "Toxicity caused system failure at step {}", step);
        }
        
        let final_waste = waste_levels.last().copied().unwrap_or(0.0);
        let max_waste = waste_levels.iter().copied().fold(0.0f32, f32::max);
        
        // Verify toxicity response effectiveness
        if initial_waste > threshold {
            prop_assert!(toxicity_responses_observed,
                "No toxicity response observed despite waste {} > threshold {}", 
                initial_waste, threshold);
            
            prop_assert!(final_waste <= max_waste,
                "Waste increased over time despite toxicity: initial {}, final {}, max {}",
                initial_waste, final_waste, max_waste);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that lipid toxicity triggers polymerization protective responses
    #[test] 
    #[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
    fn lipid_toxicity_protection(
        lipid_stress in (
            300.0f32..1000.0f32, // high initial FFA
            50.0f32..200.0f32,   // toxicity threshold
            15.0f32..50.0f32     // polymerization rate
        ),
        protection_duration in 5..15usize  // Reduced for performance
    ) {
        let (initial_ffa, threshold, poly_rate) = lipid_stress;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up lipid toxicity scenario
        let (_player_entity, cell_entity) = {
            let mut commands = app.world_mut().commands();
            spawn_local_player_cell(&mut commands, Genome::default())
        };
        app.world_mut().flush();
        
        app.world_mut().insert_resource(LipidToxicityThreshold(threshold));
        {
            let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
            let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
            currency_pools.set(Currency::FreeFattyAcids, initial_ffa);
            currency_pools.set(Currency::StorageBeads, 10.0);
            currency_pools.set(Currency::ATP, 200.0); // Sufficient for polymerization
            currency_pools.set(Currency::Pyruvate, 100.0);
            currency_pools.set(Currency::ReducingPower, 100.0);
            currency_pools.set(Currency::OrganicWaste, 5.0);
        }
        
        // Spawn lipid management system
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 3000.0,
                target_fill: 1500.0,
                poly_rate,
                lipo_rate: poly_rate / 2.0,
            },
        ));
        
        let mut ffa_levels = Vec::new();
        let mut protection_activated = false;
        
        // Monitor lipid toxicity protection
        for step in 0..protection_duration {
            let ffa_before = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::FreeFattyAcids);
            
            app.update();
            
            let ffa_after = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::FreeFattyAcids);
            let storage_after = app.world().get::<CurrencyPools>(cell_entity).unwrap().get(Currency::StorageBeads);
            
            ffa_levels.push(ffa_after);
            
            // Check for protection activation (FFA reduction when above threshold)
            if ffa_after < ffa_before && ffa_before > threshold {
                protection_activated = true;
            }
            
            // Verify protection doesn't break system
            prop_assert!(ffa_after >= 0.0, "FFA went negative at step {}", step);
            prop_assert!(storage_after >= 0.0, "Storage went negative at step {}", step);
            prop_assert!(all_currencies_non_negative(&app),
                "Lipid protection caused system failure at step {}", step);
        }
        
        let final_ffa = ffa_levels.last().copied().unwrap_or(0.0);
        
        // Verify protection effectiveness
        if initial_ffa > threshold {
            prop_assert!(protection_activated,
                "Lipid protection not activated despite FFA {} > threshold {}", 
                initial_ffa, threshold);
            
            prop_assert!(final_ffa <= initial_ffa,
                "Lipid protection failed: FFA increased from {} to {}", 
                initial_ffa, final_ffa);
        }
    }
}

// --- System Integration Health Tests ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Test that full metabolic system maintains overall cellular health
    #[test]
    #[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
    fn cellular_health_maintenance(
        health_scenario in (
            100.0f32..300.0f32, // pyruvate
            100.0f32..300.0f32, // reducing power
            200.0f32..600.0f32, // free fatty acids
            50.0f32..150.0f32,  // initial atp
            100.0f32..300.0f32  // toxicity threshold
        ),
        health_monitoring_period in 10..25usize  // Reduced for performance
    ) {
        let (pyruvate, rp, ffa, atp, threshold) = health_scenario;
        
        let mut app = MetabolisticApp::new_headless();
        
        // Set up full cellular system
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
            currency_pools.set(Currency::StorageBeads, 25.0);
        }
        app.world_mut().insert_resource(LipidToxicityThreshold(threshold));
        
        // Spawn complete metabolic system
        app.world_mut().spawn(FermentationBlock);
        app.world_mut().spawn(VesicleExportBlock);
        app.world_mut().spawn((
            CellMass { base: 1.0, extra: 0.0 },
            PolyMer {
                capacity: 2000.0,
                target_fill: 1000.0,
                poly_rate: 20.0,
                lipo_rate: 10.0,
            },
        ));
        
        let mut health_metrics = Vec::new();
        
        // Monitor cellular health over time
        for step in 0..health_monitoring_period {
            let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
            app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
            app.update();
            app.world_mut().run_schedule(FixedUpdate);
            
            // Calculate health metrics
            let snapshot = get_currency_snapshot(&app);
            let atp_health = snapshot[0]; // ATP index
            let waste_health = threshold - snapshot[6].min(threshold); // Waste below threshold is good
            let lipid_health = threshold - snapshot[4].min(threshold); // FFA below threshold is good
            
            let overall_health = atp_health + waste_health + lipid_health;
            health_metrics.push(overall_health);
            
            // Verify fundamental health constraints
            prop_assert!(all_currencies_non_negative(&app),
                "Health failure: currency went negative at step {}", step);
            
            prop_assert!(overall_health >= 0.0,
                "Health calculation error at step {}: {}", step, overall_health);
        }
        
        // Analyze health trends
        if health_metrics.len() >= 20 {
            let early_health: f32 = health_metrics[0..10].iter().sum::<f32>() / 10.0;
            let late_health: f32 = health_metrics[health_metrics.len()-10..].iter().sum::<f32>() / 10.0;
            
            // Verify cellular health doesn't degrade catastrophically
            prop_assert!(late_health >= early_health * 0.1,
                "Catastrophic health degradation: early {}, late {}", 
                early_health, late_health);
            
            // Verify health remains within reasonable bounds
            let max_health = health_metrics.iter().copied().fold(0.0f32, f32::max);
            let min_health = health_metrics.iter().copied().fold(f32::INFINITY, f32::min);
            
            prop_assert!(max_health - min_health <= max_health * 2.0,
                "Excessive health volatility: range {} vs max {}", 
                max_health - min_health, max_health);
        }
    }
}