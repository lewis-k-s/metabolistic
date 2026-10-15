use metabolistic3d::molecules::{Currency, CurrencyPools, LipidToxicityThreshold};
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::metabolism::{MetabolicFlowPlugin, FlowDirty};
use metabolistic3d::blocks::genome::{Genome, GenomePlugin, MetabolicUpdateEvent, BlockKind};
use metabolistic3d::player::spawn_local_player_cell;
use bevy::prelude::*;

#[test]
#[ignore = "pre-pivot Cell model: cells hold CurrencyPools as a field, not a component"]
fn debug_fermentation() {
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
    
    // Check if fermentation block was spawned
    let mut blocks_query = app.world_mut().query::<&metabolistic3d::blocks::fermentation::FermentationBlock>();
    let fermentation_blocks: Vec<_> = blocks_query.iter(app.world()).collect();
    println!("Found {} fermentation blocks", fermentation_blocks.len());
    
    for (i, _block) in fermentation_blocks.iter().enumerate() {
        println!("Fermentation block {}", i);
    }
    
    // Check if cell has CurrencyPools
    if let Ok(Some(_currency_pools)) = app.world().get_entity(cell_entity).and_then(|e| Ok(e.get::<CurrencyPools>())) {
        println!("Cell has CurrencyPools component");
    } else {
        println!("Cell does NOT have CurrencyPools component");
    }
    
    // Initialize currencies for fermentation
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::Pyruvate, 10.0);
        currency_pools.set(Currency::ReducingPower, 10.0);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
        println!("Initialized currencies: Pyruvate=10, ReducingPower=10, ATP=0, OrganicWaste=0");
    }

    // --- Run Simulation ---
    // Trigger metabolic flow calculation
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    println!("Set FlowDirty to true");
    
    // Run multiple updates to ensure all systems execute
    for i in 0..5 {
        app.update();
        println!("Ran update {}", i + 1);
    }

    // --- Verification ---
    // Check that currencies have changed after metabolic flow calculation
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let pyruvate = currency_pools.get(Currency::Pyruvate);
        let reducing_power = currency_pools.get(Currency::ReducingPower);
        let atp = currency_pools.get(Currency::ATP);
        let organic_waste = currency_pools.get(Currency::OrganicWaste);
        
        println!("Final currencies: Pyruvate={}, ReducingPower={}, ATP={}, OrganicWaste={}", 
                 pyruvate, reducing_power, atp, organic_waste);
        
        if pyruvate < 10.0 {
            println!("✓ Pyruvate was consumed");
        } else {
            println!("✗ Pyruvate was NOT consumed");
        }
        
        if reducing_power < 10.0 {
            println!("✓ ReducingPower was consumed");
        } else {
            println!("✗ ReducingPower was NOT consumed");
        }
        
        if atp > 0.0 {
            println!("✓ ATP was produced");
        } else {
            println!("✗ ATP was NOT produced");
        }
        
        if organic_waste > 0.0 {
            println!("✓ OrganicWaste was produced");
        } else {
            println!("✗ OrganicWaste was NOT produced");
        }
    }
}