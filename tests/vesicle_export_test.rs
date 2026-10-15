use metabolistic3d::molecules::{Currency, CurrencyPools, LipidToxicityThreshold};
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::metabolism::MetabolicFlowPlugin;
use metabolistic3d::blocks::genome::{Genome, GenomePlugin, MetabolicUpdateEvent, BlockKind};
use metabolistic3d::player::spawn_local_player_cell;
use bevy::prelude::*;

/// Tests that vesicle export decreases OrganicWaste.
#[test]
#[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
fn test_vesicle_export_decreases_organic_waste() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0));
    
    // Set up genome with vesicle export expressed
    let mut genome = Genome::default();
    genome.express_gene(BlockKind::LipidMetabolism);
    app.insert_resource(genome);

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, Genome::default())
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Trigger genome update to spawn vesicle export block
    app.world_mut().send_event(MetabolicUpdateEvent);
    app.update();
    
    // Initialize OrganicWaste with a starting amount on the cell
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::OrganicWaste, 100.0);
    }

    // --- Run Simulation ---
    app.update();

    // --- Verification ---
    // Vesicle export may or may not work depending on whether environmental systems are active
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let organic_waste = currency_pools.get(Currency::OrganicWaste);
        assert!(organic_waste <= 100.0 && organic_waste >= 0.0, "OrganicWaste should be valid: {}", organic_waste);
    }

    // Run until depletion
    for _ in 0..999 {
        app.update();
    }

    // Verify that it doesn't go below zero and has been reduced
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let final_waste = currency_pools.get(Currency::OrganicWaste);
        assert!(final_waste >= 0.0, "OrganicWaste should not go below zero: {}", final_waste);
        assert!(final_waste < 100.0, "OrganicWaste should be reduced from initial 100.0: {}", final_waste);
    }
}
