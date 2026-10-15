use metabolistic3d::molecules::{Currency, CurrencyPools, LipidToxicityThreshold, PolyMer, CellMass};
use metabolistic3d::blocks::MetabolicBlocksPlugin;
use metabolistic3d::metabolism::MetabolicFlowPlugin;
use metabolistic3d::blocks::genome::{Genome, GenomePlugin, MetabolicUpdateEvent};
use metabolistic3d::player::spawn_local_player_cell;
use metabolistic3d::blocks::fat_storage::OwnedBy;
use bevy::prelude::*;

/// Tests that fat storage polymerizes FreeFattyAcids into StorageBeads when above toxicity threshold.
#[test]
fn test_fat_storage_polymerization() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0)); // Set toxicity threshold

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, Genome::default())
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Initialize FreeFattyAcids above toxicity threshold
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::FreeFattyAcids, 80.0); // Above threshold of 50.0
        currency_pools.set(Currency::ATP, 10.0); // Provide ATP for polymerization
    }

    // --- Run Simulation ---
    app.update();

    // --- Verification ---
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
        let storage_beads = currency_pools.get(Currency::StorageBeads);
        
        // Should have polymerized some FreeFattyAcids
        assert!(free_fatty_acids < 80.0, "FreeFattyAcids should be reduced from 80.0: {}", free_fatty_acids);
        assert!(storage_beads > 0.0, "StorageBeads should be produced: {}", storage_beads);
        
        // Conservation check (20:20 ratio)
        assert_eq!(80.0 - free_fatty_acids, storage_beads, "FreeFattyAcids consumed should equal StorageBeads produced");
    }
}

/// Tests that lipolysis mobilizes StorageBeads back to FreeFattyAcids when below toxicity threshold.
#[test]
fn test_fat_storage_lipolysis() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0)); // Set toxicity threshold

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, Genome::default())
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Initialize currencies with low FreeFattyAcids and high StorageBeads
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::FreeFattyAcids, 10.0); // Below threshold of 50.0
        currency_pools.set(Currency::StorageBeads, 40.0); // Available for lipolysis
    }
    
    // Spawn a PolyMer entity for lipolysis
    let polymer_entity = app.world_mut().spawn((
        PolyMer { 
            capacity: 100.0,
            target_fill: 0.8,
            poly_rate: 10.0,
            lipo_rate: 5.0 
        },
        CellMass { 
            base: 10.0,
            extra: 40.0 
        },
        OwnedBy(cell_entity),
    )).id();

    // --- Run Simulation ---
    app.update();

    // --- Verification ---
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
        let storage_beads = currency_pools.get(Currency::StorageBeads);
        let atp = currency_pools.get(Currency::ATP);
        
        // Should have mobilized some StorageBeads
        assert!(free_fatty_acids > 10.0, "FreeFattyAcids should increase from 10.0: {}", free_fatty_acids);
        assert!(storage_beads < 40.0, "StorageBeads should decrease from 40.0: {}", storage_beads);
        assert!(atp > 0.0, "ATP should be produced during lipolysis: {}", atp);
        
        // Check that lipolysis rate was applied correctly (up to 5.0)
        assert_eq!(free_fatty_acids - 10.0, 5.0, "Should mobilize exactly lipo_rate amount");
    }
    
    // Check cell mass was updated
    {
        let cell_mass = app.world().get_entity(polymer_entity).unwrap().get::<CellMass>().unwrap();
        assert_eq!(cell_mass.extra, 35.0, "Cell mass should decrease by amount mobilized");
    }
}

/// Tests that polymerization doesn't occur below toxicity threshold.
#[test]
fn test_no_polymerization_below_threshold() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(GenomePlugin);
    app.add_plugins(MetabolicBlocksPlugin);
    app.add_plugins(MetabolicFlowPlugin);
    app.insert_resource(LipidToxicityThreshold(50.0)); // Set toxicity threshold

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, Genome::default())
    };
    
    // Apply commands to actually create the entities
    app.world_mut().flush();
    
    // Initialize FreeFattyAcids below toxicity threshold
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::FreeFattyAcids, 30.0); // Below threshold of 50.0
        currency_pools.set(Currency::ATP, 10.0);
    }

    // --- Run Simulation ---
    app.update();

    // --- Verification ---
    {
        let currency_pools = app.world().get_entity(cell_entity).unwrap().get::<CurrencyPools>().unwrap();
        let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
        let storage_beads = currency_pools.get(Currency::StorageBeads);
        
        // Should not have polymerized
        assert_eq!(free_fatty_acids, 30.0, "FreeFattyAcids should remain at 30.0: {}", free_fatty_acids);
        assert_eq!(storage_beads, 0.0, "StorageBeads should remain at 0.0: {}", storage_beads);
    }
}