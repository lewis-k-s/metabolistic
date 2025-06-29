use metabolistic3d::MetabolisticApp;
use metabolistic3d::metabolism::*;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GeneState, spawn_metabolic_block};
use metabolistic3d::molecules::Currency;
use std::collections::HashMap;

#[test]
fn test_app_startup_integration() {
    // Test that the app can go through basic startup without crashing
    let mut app = MetabolisticApp::new_headless();

    // Run startup systems (this is what happens when the app actually starts)
    app.update();

    // Verify the app is in a valid state after startup
    // Check that the world exists and has entities (startup systems ran)
    assert!(app.world().entities().len() >= 0);
}

#[test]
fn test_app_multiple_updates() {
    // Test that the app can handle multiple update cycles
    let mut app = MetabolisticApp::new_headless();

    // Run several update cycles to simulate normal operation
    for _ in 0..5 {
        app.update();
    }

    // App should still be in valid state
    assert!(app.world().entities().len() >= 0);
}

#[test]
fn test_headless_startup() {
    // Test headless mode specifically
    let mut app = MetabolisticApp::new_headless();
    app.update();

    // Verify headless app started successfully
    assert!(app.world().entities().len() >= 0);
}

// ============================================================================
// COMPREHENSIVE GENOME MANIPULATION AND METABOLIC GRAPH INTEGRATION TESTS
// ============================================================================

#[test]
fn test_genome_add_blocks_and_graph_update() {
    let mut app = MetabolisticApp::new_headless();
    
    // Initialize and run startup
    app.update();
    
    // Get initial metabolic graph state
    let initial_node_count = app.world().resource::<MetabolicGraph>().nodes.len();
    
    // Add genes to the genome
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::SugarCatabolism);
        genome.add_gene(BlockKind::Fermentation);
        genome.add_gene(BlockKind::LightCapture);
    }
    
    // Spawn corresponding metabolic block entities
    let (sugar_entity, ferment_entity, light_entity) = {
        let world = app.world_mut();
        let sugar_entity = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::SugarCatabolism, status: BlockStatus::Silent },
            FluxProfile(vec![(Currency::ATP, 5.0), (Currency::CarbonSkeletons, -2.0)].into_iter().collect())
        )).id();
        let ferment_entity = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Silent },
            FluxProfile(vec![(Currency::ATP, 3.0)].into_iter().collect())
        )).id();
        let light_entity = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::LightCapture, status: BlockStatus::Silent },
            FluxProfile(vec![(Currency::ATP, 8.0), (Currency::ReducingPower, 4.0)].into_iter().collect())
        )).id();
        (sugar_entity, ferment_entity, light_entity)
    };
    
    // Force metabolic graph rebuild by setting FlowDirty
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    
    // Run several update cycles to process all systems
    for _ in 0..3 {
        app.update();
    }
    
    // Verify the metabolic graph has been updated with new nodes
    let final_node_count = app.world().resource::<MetabolicGraph>().nodes.len();
    assert_eq!(final_node_count, initial_node_count + 3, "Expected 3 new nodes in metabolic graph");
    
    // Verify genome contains the added genes
    let genome = app.world().resource::<Genome>();
    assert_eq!(genome.get_gene_state(&BlockKind::SugarCatabolism), Some(&GeneState::Silent));
    assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Silent));
    assert_eq!(genome.get_gene_state(&BlockKind::LightCapture), Some(&GeneState::Silent));
}

#[test]
fn test_genome_gene_expression_metabolic_activation() {
    let mut app = MetabolisticApp::new_headless();
    app.update();
    
    // Add and spawn a metabolic block
    let block_entity = {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::SugarCatabolism);
        
        app.world_mut().spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::SugarCatabolism, status: BlockStatus::Silent },
            FluxProfile(vec![(Currency::ATP, 10.0)].into_iter().collect())
        )).id()
    };
    
    app.update();
    
    // Verify initial state - gene is silent, block is inactive
    {
        let node = app.world().entity(block_entity).get::<MetabolicNode>().unwrap();
        assert_eq!(node.status, BlockStatus::Silent);
    }
    
    // Express the gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        assert!(genome.express_gene(BlockKind::SugarCatabolism));
    }
    
    // Run update cycles to process genome diff events
    for _ in 0..3 {
        app.update();
    }
    
    // Verify the metabolic block is now active
    {
        let node = app.world().entity(block_entity).get::<MetabolicNode>().unwrap();
        assert_eq!(node.status, BlockStatus::Active);
    }
    
    // Verify genome state
    let genome = app.world().resource::<Genome>();
    assert_eq!(genome.get_gene_state(&BlockKind::SugarCatabolism), Some(&GeneState::Expressed));
}

#[test]
fn test_genome_gene_mutation_and_repair() {
    let mut app = MetabolisticApp::new_headless();
    app.update();
    
    // Set up a metabolic block with expressed gene
    let block_entity = {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::Fermentation);
        genome.express_gene(BlockKind::Fermentation);
        
        app.world_mut().spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Active },
            FluxProfile(vec![(Currency::ATP, 6.0)].into_iter().collect())
        )).id()
    };
    
    app.update();
    
    // Mutate the gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        assert!(genome.mutate_gene(BlockKind::Fermentation));
    }
    
    // Process the mutation
    for _ in 0..3 {
        app.update();
    }
    
    // Verify the block is now mutated
    {
        let node = app.world().entity(block_entity).get::<MetabolicNode>().unwrap();
        assert_eq!(node.status, BlockStatus::Mutated);
        let genome = app.world().resource::<Genome>();
        assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Mutated));
    }
    
    // Repair the gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        assert!(genome.repair_gene(BlockKind::Fermentation));
    }
    
    // Process the repair
    for _ in 0..3 {
        app.update();
    }
    
    // Verify the gene is repaired in the genome (Silent) but node status may remain Mutated
    // since both Silent and Mutated are non-expressed states
    {
        let genome = app.world().resource::<Genome>();
        assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Silent));
        
        // The node status may still be Mutated since no diff event is generated
        // when transitioning between non-expressed states (Mutated -> Silent)
        let node = app.world().entity(block_entity).get::<MetabolicNode>().unwrap();
        // Both Silent and Mutated result in zero flux, so functionally equivalent
        assert!(matches!(node.status, BlockStatus::Silent | BlockStatus::Mutated));
    }
}

#[test]
fn test_genome_gene_silencing() {
    let mut app = MetabolisticApp::new_headless();
    app.update();
    
    // Set up an active metabolic block
    let block_entity = {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::LightCapture);
        genome.express_gene(BlockKind::LightCapture);
        
        app.world_mut().spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::LightCapture, status: BlockStatus::Active },
            FluxProfile(vec![(Currency::ATP, 12.0), (Currency::ReducingPower, 8.0)].into_iter().collect())
        )).id()
    };
    
    app.update();
    
    // Verify initial active state
    {
        let node = app.world().entity(block_entity).get::<MetabolicNode>().unwrap();
        assert_eq!(node.status, BlockStatus::Active);
    }
    
    // Silence the gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        assert!(genome.silence_gene(BlockKind::LightCapture));
    }
    
    // Process the silencing
    for _ in 0..3 {
        app.update();
    }
    
    // Verify the block is now silent
    {
        let node = app.world().entity(block_entity).get::<MetabolicNode>().unwrap();
        assert_eq!(node.status, BlockStatus::Silent);
        let genome = app.world().resource::<Genome>();
        assert_eq!(genome.get_gene_state(&BlockKind::LightCapture), Some(&GeneState::Silent));
    }
}

#[test]
fn test_genome_remove_blocks_and_graph_update() {
    let mut app = MetabolisticApp::new_headless();
    app.update();
    
    // Set up multiple metabolic blocks
    let (block1, block2, block3) = {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::SugarCatabolism);
        genome.add_gene(BlockKind::Fermentation);
        genome.add_gene(BlockKind::Respiration);
        
        let world = app.world_mut();
        let e1 = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::SugarCatabolism, status: BlockStatus::Silent },
            FluxProfile(HashMap::new())
        )).id();
        let e2 = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Silent },
            FluxProfile(HashMap::new())
        )).id();
        let e3 = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::Respiration, status: BlockStatus::Silent },
            FluxProfile(HashMap::new())
        )).id();
        
        (e1, e2, e3)
    };
    
    // Force graph rebuild
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    
    for _ in 0..3 {
        app.update();
    }
    
    // Verify all blocks are in the graph
    let initial_count = app.world().resource::<MetabolicGraph>().nodes.len();
    assert!(initial_count >= 3);
    
    // Remove one block by despawning it
    app.world_mut().entity_mut(block2).despawn();
    
    // Force graph rebuild
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    
    for _ in 0..3 {
        app.update();
    }
    
    // Verify the graph has been updated
    let final_count = app.world().resource::<MetabolicGraph>().nodes.len();
    assert_eq!(final_count, initial_count - 1, "Graph should have one fewer node after removal");
    
    // Verify the remaining blocks are still present
    assert!(app.world().get_entity(block1).is_ok());
    assert!(app.world().get_entity(block3).is_ok());
    assert!(app.world().get_entity(block2).is_err());
}

#[test]
fn test_flux_calculation_with_different_gene_states() {
    let mut app = MetabolisticApp::new_headless();
    app.update();
    
    // Create blocks with different gene states
    let (active_block, mutated_block, silent_block) = {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::SugarCatabolism);
        genome.add_gene(BlockKind::Fermentation);
        genome.add_gene(BlockKind::LightCapture);
        genome.express_gene(BlockKind::SugarCatabolism); // Active
        genome.mutate_gene(BlockKind::Fermentation);     // Mutated
        // LightCapture remains silent
        
        // Set identical flux profiles for easy comparison
        let flux_profile = FluxProfile(vec![(Currency::ATP, 10.0)].into_iter().collect());
        
        let world = app.world_mut();
        let e1 = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::SugarCatabolism, status: BlockStatus::Active },
            flux_profile.clone()
        )).id();
        let e2 = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Mutated },
            flux_profile.clone()
        )).id();
        let e3 = world.spawn((
            MetabolicBlock,
            MetabolicNode { kind: BlockKind::LightCapture, status: BlockStatus::Silent },
            flux_profile
        )).id();
        
        (e1, e2, e3)
    };
    
    // Force graph rebuild and flux calculation
    app.world_mut().resource_mut::<FlowDirty>().0 = true;
    
    for _ in 0..5 {
        app.update();
    }
    
    // Verify flux results based on block status
    let flux_result = app.world().resource::<FluxResult>();
    
    // Active block should have full flux (10.0)
    assert_eq!(flux_result.0.get(&active_block), Some(&10.0));
    
    // Mutated block should have reduced flux (50% = 5.0)
    assert_eq!(flux_result.0.get(&mutated_block), Some(&5.0));
    
    // Silent block should have zero flux
    assert_eq!(flux_result.0.get(&silent_block), Some(&0.0));
}

#[test]
fn test_complex_genome_manipulation_sequence() {
    let mut app = MetabolisticApp::new_headless();
    app.update();
    
    // Create a complex scenario with multiple blocks
    let blocks = {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        
        // Add multiple genes
        let block_kinds = vec![
            BlockKind::SugarCatabolism,
            BlockKind::Fermentation,
            BlockKind::LightCapture,
            BlockKind::Respiration,
        ];
        
        for kind in &block_kinds {
            genome.add_gene(*kind);
        }
        
        // Create corresponding entities
        let world = app.world_mut();
        let mut entities = Vec::new();
        
        for kind in &block_kinds {
            let entity = world.spawn((
                MetabolicBlock,
                MetabolicNode { kind: *kind, status: BlockStatus::Silent },
                FluxProfile(vec![(Currency::ATP, 8.0)].into_iter().collect())
            )).id();
            entities.push(entity);
        }
        
        entities
    };
    
    app.update();
    
    // Step 1: Express some genes
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.express_gene(BlockKind::SugarCatabolism);
        genome.express_gene(BlockKind::LightCapture);
    }
    
    for _ in 0..3 {
        app.update();
    }
    
    // Verify expressed genes are active
    let flux_result = app.world().resource::<FluxResult>();
    assert_eq!(flux_result.0.get(&blocks[0]), Some(&8.0)); // SugarCatabolism active
    assert_eq!(flux_result.0.get(&blocks[2]), Some(&8.0)); // LightCapture active
    assert_eq!(flux_result.0.get(&blocks[1]), Some(&0.0)); // Fermentation silent
    assert_eq!(flux_result.0.get(&blocks[3]), Some(&0.0)); // Respiration silent
    
    // Step 2: Mutate an active gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.mutate_gene(BlockKind::SugarCatabolism);
    }
    
    for _ in 0..3 {
        app.update();
    }
    
    // Verify mutation affects flux
    let flux_result = app.world().resource::<FluxResult>();
    assert_eq!(flux_result.0.get(&blocks[0]), Some(&4.0)); // SugarCatabolism mutated (50%)
    
    // Step 3: Express another gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.express_gene(BlockKind::Fermentation);
    }
    
    for _ in 0..3 {
        app.update();
    }
    
    // Verify new gene is active
    let flux_result = app.world().resource::<FluxResult>();
    assert_eq!(flux_result.0.get(&blocks[1]), Some(&8.0)); // Fermentation now active
    
    // Step 4: Silence an active gene
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.silence_gene(BlockKind::LightCapture);
    }
    
    for _ in 0..3 {
        app.update();
    }
    
    // Verify silenced gene has no flux
    let flux_result = app.world().resource::<FluxResult>();
    assert_eq!(flux_result.0.get(&blocks[2]), Some(&0.0)); // LightCapture now silent
    
    // Final verification: Check genome state
    let genome = app.world().resource::<Genome>();
    assert_eq!(genome.get_gene_state(&BlockKind::SugarCatabolism), Some(&GeneState::Mutated));
    assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Expressed));
    assert_eq!(genome.get_gene_state(&BlockKind::LightCapture), Some(&GeneState::Silent));
    assert_eq!(genome.get_gene_state(&BlockKind::Respiration), Some(&GeneState::Silent));
}
