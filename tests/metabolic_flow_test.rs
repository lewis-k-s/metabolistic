use bevy::prelude::*;
use metabolistic3d::metabolism::*;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomeDiffEvent};

#[test]
fn metabolic_flow_plugin_adds_resources() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(MetabolicFlowPlugin);
    app.add_event::<GenomeDiffEvent>();

    // Check if resources are added
    assert!(app.world().contains_resource::<FlowDirty>());
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



