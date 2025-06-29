use bevy::prelude::*;
use bevy::ecs::system::SystemState;
use metabolistic3d::metabolism::*;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomeDiffEvent, GeneState};
use metabolistic3d::molecules::Currency;

#[test]
fn metabolic_flow_plugin_adds_resources() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();

    // Check if resources are added
    assert!(app.world().contains_resource::<MetabolicGraph>());
    assert!(app.world().contains_resource::<FlowDirty>());
    assert!(app.world().contains_resource::<FluxResult>());
}

#[test]
fn metabolic_flow_plugin_adds_schedule() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();

    // Check if the schedule is added
    assert!(app.world().resource::<Schedules>().contains(MetabolicSchedule));
    // Check if FixedUpdate is configured
    assert!(app.world().contains_resource::<Time<Fixed>>());
}

#[test]
fn flow_dirty_resource_defaults_to_false() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();

    let flow_dirty = app.world().resource::<FlowDirty>();
    assert!(!flow_dirty.0);
}

#[test]
fn test_rebuild_graph_system() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();
    app.world_mut().insert_resource(Genome::default());

    // Spawn some nodes and edges
    app.world_mut().spawn((MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Active }, MetabolicBlock, FluxProfile::default()));
    app.world_mut().spawn((MetabolicNode { kind: BlockKind::LightCapture, status: BlockStatus::Active }, MetabolicBlock, FluxProfile::default()));
    app.world_mut().spawn(MetabolicEdge);

    // Set FlowDirty to true to trigger rebuild_graph
    app.world_mut().resource_mut::<FlowDirty>().0 = true;

    // Run the MetabolicSchedule directly
    app.world_mut().run_schedule(MetabolicSchedule);

    let metabolic_graph = app.world().resource::<MetabolicGraph>();
    assert_eq!(metabolic_graph.nodes.len(), 2);
    assert_eq!(metabolic_graph.edges.len(), 1);
}

#[test]
fn test_on_genome_diff_system() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_plugins(metabolistic3d::blocks::genome::GenomePlugin);
    app.add_event::<GenomeDiffEvent>();

    // Initialize Genome resource and express the gene
    app.world_mut().insert_resource(Genome::default());
    let node_entity = app.world_mut().spawn((MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Silent }, MetabolicBlock, FluxProfile::default())).id();
    app.world_mut().resource_mut::<Genome>().add_gene(BlockKind::Fermentation);
    app.world_mut().resource_mut::<Genome>().express_gene(BlockKind::Fermentation);

    // Run the app to allow GenomePlugin to update and emit events
    app.update();

    // Run the MetabolicSchedule directly to process the event
    app.world_mut().run_schedule(MetabolicSchedule);

    // Verify node status is updated
    let node = app.world().entity(node_entity).get::<MetabolicNode>().unwrap();
    assert_eq!(node.status, BlockStatus::Active);

    // Verify FlowDirty is set to true
    let flow_dirty = app.world().resource::<FlowDirty>();
    assert!(flow_dirty.0);

    // Silence the gene
    app.world_mut().resource_mut::<Genome>().silence_gene(BlockKind::Fermentation);

    // Run the app again to allow GenomePlugin to update and emit events
    app.update();

    // Run the MetabolicSchedule directly to process the silencing event
    app.world_mut().run_schedule(MetabolicSchedule);

    // Verify node status is updated again
    let node = app.world().entity(node_entity).get::<MetabolicNode>().unwrap();
    assert_eq!(node.status, BlockStatus::Silent);

    // Verify FlowDirty is set to true by the silencing operation
    let flow_dirty = app.world().resource::<FlowDirty>();
    assert!(flow_dirty.0);
}

#[test]
fn test_solve_flux_system() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();
    app.world_mut().insert_resource(Genome::default());

    // Spawn nodes with different statuses
    let active_node = app.world_mut().spawn((MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Active }, MetabolicBlock, FluxProfile(vec![(Currency::ATP, 10.0), (Currency::CarbonSkeletons, -5.0)].into_iter().collect()))).id();
    let mutated_node = app.world_mut().spawn((MetabolicNode { kind: BlockKind::LightCapture, status: BlockStatus::Mutated }, MetabolicBlock, FluxProfile(vec![(Currency::ATP, 8.0)].into_iter().collect()))).id();
    let silent_node = app.world_mut().spawn((MetabolicNode { kind: BlockKind::SugarCatabolism, status: BlockStatus::Silent }, MetabolicBlock, FluxProfile(vec![(Currency::ReducingPower, 2.0)].into_iter().collect()))).id();

    // Manually update MetabolicGraph to include these nodes
    app.world_mut().resource_mut::<MetabolicGraph>().nodes.push(active_node);
    app.world_mut().resource_mut::<MetabolicGraph>().nodes.push(mutated_node);
    app.world_mut().resource_mut::<MetabolicGraph>().nodes.push(silent_node);

    // Run the solve_flux_system directly
    {
        let mut world = app.world_mut();
        let mut system_state: SystemState<(Res<MetabolicGraph>, ResMut<FluxResult>, Query<(&MetabolicNode, &FluxProfile)>)> = SystemState::new(&mut world);
        let (metabolic_graph, mut flux_result, query_blocks) = system_state.get_mut(&mut world);
        solve_flux_system(metabolic_graph, flux_result, query_blocks);
        system_state.apply(&mut world);
    }

    let flux_result = app.world().resource::<FluxResult>();

    assert_eq!(flux_result.0.get(&active_node), Some(&5.0));
    assert_eq!(flux_result.0.get(&mutated_node), Some(&4.0));
    assert_eq!(flux_result.0.get(&silent_node), Some(&0.0));
}

#[test]
fn test_apply_flux_results_system() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();
    app.world_mut().insert_resource(Genome::default());

    // Spawn a node
    let node_entity = app.world_mut().spawn((MetabolicNode { kind: BlockKind::Fermentation, status: BlockStatus::Active }, MetabolicBlock, FluxProfile(vec![(Currency::ATP, 5.5)].into_iter().collect()))).id();

    // Manually set FluxResult
    app.world_mut().resource_mut::<FluxResult>().0.insert(node_entity, 5.5);

    // Run the apply_flux_results_system directly
    {
        let mut world = app.world_mut();
        let mut system_state: SystemState<(Res<FluxResult>, Query<(&MetabolicNode, &FluxProfile)>)> = SystemState::new(&mut world);
        let (flux_result, query_blocks) = system_state.get_mut(&mut world);
        apply_flux_results_system(flux_result, query_blocks);
        system_state.apply(&mut world);
    }

    // For now, apply_flux_results_system only logs, so we can't directly assert changes to components.
    // In a real scenario, this test would assert changes to other components or resources based on flux.
    // For demonstration, we can check if the system ran without panicking.
    assert!(true); // Placeholder assertion
}