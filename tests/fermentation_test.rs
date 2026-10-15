use metabolistic3d::molecules::{Currency, CurrencyPools, LipidToxicityThreshold};
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::metabolism::{MetabolicFlowPlugin, FlowDirty};
use metabolistic3d::blocks::genome::{Genome, GenomePlugin, MetabolicUpdateEvent, BlockKind};
use metabolistic3d::player::spawn_local_player_cell;
use bevy::prelude::*;

/// Tests that fermentation converts Pyruvate and ReducingPower into ATP and OrganicWaste.
#[test]
#[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
fn test_fermentation_basic_flow() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0));

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        // Create a genome with fermentation expressed for the cell
        let mut cell_genome = Genome::default();
        cell_genome.add_gene(BlockKind::Fermentation);
        cell_genome.express_gene(BlockKind::Fermentation);
        spawn_local_player_cell(&mut commands, cell_genome)
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Force initial metabolic update since poll_genome_diff won't detect per-cell genomes
    app.world_mut().send_event(MetabolicUpdateEvent);
    app.update();
    
    // Run another update to ensure block spawning completes  
    app.update();
    
    // Initialize currencies for fermentation
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::Pyruvate, 10.0);
        currency_pools.set(Currency::ReducingPower, 10.0);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
    }

    // --- Run Simulation ---
    // Trigger metabolic flow calculation
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    app.update();
    
    // Run one more update to ensure all systems execute
    app.update();

    // --- Verification ---
    // Check that currencies have changed after metabolic flow calculation
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let pyruvate = currency_pools.get(Currency::Pyruvate);
        let reducing_power = currency_pools.get(Currency::ReducingPower);
        let atp = currency_pools.get(Currency::ATP);
        let organic_waste = currency_pools.get(Currency::OrganicWaste);
        
        // Should have consumed some inputs
        assert!(pyruvate < 10.0, "Pyruvate should be consumed: {}", pyruvate);
        assert!(reducing_power < 10.0, "ReducingPower should be consumed: {}", reducing_power);
        
        // Should have produced some outputs
        assert!(atp > 0.0, "ATP should be produced: {}", atp);
        assert!(organic_waste > 0.0, "OrganicWaste should be produced: {}", organic_waste);
    }
}

/// Tests that fermentation doesn't occur without required inputs.
#[test]
#[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
fn test_fermentation_requires_inputs() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0));

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        let mut cell_genome = Genome::default();
        cell_genome.add_gene(BlockKind::Fermentation);
        cell_genome.express_gene(BlockKind::Fermentation);
        spawn_local_player_cell(&mut commands, cell_genome)
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Trigger genome update to spawn fermentation block
    app.world_mut().send_event(MetabolicUpdateEvent);
    app.update();
    
    // Initialize with no inputs
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::Pyruvate, 0.0);
        currency_pools.set(Currency::ReducingPower, 0.0);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
    }

    // --- Run Simulation ---
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    app.update();

    // --- Verification ---
    // Check that no currencies changed without inputs
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        assert_eq!(currency_pools.get(Currency::ATP), 0.0, "ATP should remain at 0");
        assert_eq!(currency_pools.get(Currency::OrganicWaste), 0.0, "OrganicWaste should remain at 0");
    }
}

/// Tests that fermentation respects the 1:1:1:1 stoichiometry.
#[test]
#[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
fn test_fermentation_stoichiometry() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0));

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        let mut cell_genome = Genome::default();
        cell_genome.add_gene(BlockKind::Fermentation);
        cell_genome.express_gene(BlockKind::Fermentation);
        spawn_local_player_cell(&mut commands, cell_genome)
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Trigger genome update to spawn fermentation block
    app.world_mut().send_event(MetabolicUpdateEvent);
    app.update();
    
    // Initialize with specific amounts
    let initial_pyruvate = 5.0;
    let initial_reducing = 8.0;
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::Pyruvate, initial_pyruvate);
        currency_pools.set(Currency::ReducingPower, initial_reducing);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
    }

    // --- Run Simulation ---
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    app.update();

    // --- Verification ---
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let pyruvate_consumed = initial_pyruvate - currency_pools.get(Currency::Pyruvate);
        let reducing_consumed = initial_reducing - currency_pools.get(Currency::ReducingPower);
        let atp_produced = currency_pools.get(Currency::ATP);
        let waste_produced = currency_pools.get(Currency::OrganicWaste);
        
        // All should be equal due to 1:1:1:1 stoichiometry
        assert!((pyruvate_consumed - reducing_consumed).abs() < 0.01, 
            "Pyruvate and ReducingPower consumption should be equal: {} vs {}", 
            pyruvate_consumed, reducing_consumed);
        assert!((pyruvate_consumed - atp_produced).abs() < 0.01, 
            "Input consumption should equal ATP production: {} vs {}", 
            pyruvate_consumed, atp_produced);
        assert!((pyruvate_consumed - waste_produced).abs() < 0.01, 
            "Input consumption should equal waste production: {} vs {}", 
            pyruvate_consumed, waste_produced);
        
        // Should be limited by the smaller input (pyruvate = 5.0)
        assert!((pyruvate_consumed - initial_pyruvate).abs() < 0.01, 
            "Should consume all available pyruvate: {}", pyruvate_consumed);
    }
}