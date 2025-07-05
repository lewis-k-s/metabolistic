use bevy::prelude::*;
use metabolistic3d::blocks::genome::{BlockKind, Genome, GenomeDiffEvent, MetabolicUpdateEvent, GeneState, GenomeOperationCosts, poll_genome_diff, apply_genome_diff};

fn setup_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(Genome::default());
    app.insert_resource(GenomeOperationCosts::default());
    app.add_event::<GenomeDiffEvent>();
    app.add_event::<MetabolicUpdateEvent>();
    app.add_systems(PreUpdate, poll_genome_diff);
    app.add_systems(Update, apply_genome_diff);
    app
}

#[test]
fn test_genome_diff_events_expression_and_silencing() {
    let mut app = setup_app();
    let mut genome = app.world_mut().resource_mut::<Genome>();

    // Add a gene
    genome.add_gene(BlockKind::SugarCatabolism);
    genome.add_gene(BlockKind::LightCapture);

    // Initial update to set previous state (all silent)
    app.update();

    // Express a gene
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.express_gene(BlockKind::SugarCatabolism));
    assert_eq!(*genome.get_gene_state(&BlockKind::SugarCatabolism).unwrap(), GeneState::Expressed);

    // Express another gene
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.express_gene(BlockKind::LightCapture));
    assert_eq!(*genome.get_gene_state(&BlockKind::LightCapture).unwrap(), GeneState::Expressed);

    // Run app update to trigger poll_genome_diff and emit event
    app.update();

    // Check for GenomeDiffEvent
    let events = app.world_mut().resource_mut::<Events<GenomeDiffEvent>>();
    let mut reader = events.get_cursor();
    let diff_events: Vec<&GenomeDiffEvent> = reader.read(&events).collect();

    assert_eq!(diff_events.len(), 1);
    let diff = &diff_events[0];
    assert!(diff.enabled.contains(&BlockKind::SugarCatabolism));
    assert!(diff.enabled.contains(&BlockKind::LightCapture));
    assert_eq!(diff.enabled.len(), 2);
    assert!(diff.disabled.is_empty());

    // Clear events before next phase
    app.world_mut().resource_mut::<Events<GenomeDiffEvent>>().clear();

    // Silence a gene
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.silence_gene(BlockKind::SugarCatabolism));
    assert_eq!(*genome.get_gene_state(&BlockKind::SugarCatabolism).unwrap(), GeneState::Silent);

    // Run app update to trigger poll_genome_diff and emit event
    app.update();

    // Check for GenomeDiffEvent (create fresh cursor)
    let events = app.world_mut().resource_mut::<Events<GenomeDiffEvent>>();
    let mut reader = events.get_cursor();
    let diff_events: Vec<&GenomeDiffEvent> = reader.read(&events).collect();

    assert_eq!(diff_events.len(), 1);
    let diff = &diff_events[0];
    assert!(diff.disabled.contains(&BlockKind::SugarCatabolism));
    assert_eq!(diff.disabled.len(), 1);
    assert!(diff.enabled.is_empty());
}

#[test]
fn test_genome_diff_events_mutation_and_repair() {
    let mut app = setup_app();
    let mut genome = app.world_mut().resource_mut::<Genome>();

    // Add and express a gene
    genome.add_gene(BlockKind::Fermentation);
    assert!(genome.express_gene(BlockKind::Fermentation));

    // Initial update to set previous state (Fermentation expressed)
    app.update();

    // Clear all events from initial expression by updating the event system
    app.world_mut().resource_mut::<Events<GenomeDiffEvent>>().clear();

    // Mutate the expressed gene
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.mutate_gene(BlockKind::Fermentation));
    assert_eq!(*genome.get_gene_state(&BlockKind::Fermentation).unwrap(), GeneState::Mutated);

    // Run app update to trigger poll_genome_diff and emit event
    app.update();

    // Check for GenomeDiffEvent
    let events = app.world_mut().resource_mut::<Events<GenomeDiffEvent>>();
    let mut reader = events.get_cursor();
    let diff_events: Vec<&GenomeDiffEvent> = reader.read(&events).collect();

    eprintln!("test_genome_dirf_events_mutatiin_and_repair: Diff Events after mutation: {:?}", diff_events);
    assert_eq!(diff_events.len(), 1);
    let diff = &diff_events[0];
    assert!(diff.disabled.contains(&BlockKind::Fermentation));
    assert_eq!(diff.disabled.len(), 1);
    assert!(diff.enabled.is_empty());

    // Clear events before repair phase
    app.world_mut().resource_mut::<Events<GenomeDiffEvent>>().clear();

    // Repair the mutated gene (it should go back to Silent)
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.repair_gene(BlockKind::Fermentation));
    assert_eq!(*genome.get_gene_state(&BlockKind::Fermentation).unwrap(), GeneState::Silent);

    // Run app update to trigger poll_genome_diff and emit event
    app.update();

    // Check for GenomeDiffEvent (should be empty as it went from Mutated to Silent, which is not an expression change)
    let events = app.world_mut().resource_mut::<Events<GenomeDiffEvent>>();
    let mut reader = events.get_cursor();
    let diff_events: Vec<&GenomeDiffEvent> = reader.read(&events).collect();
    assert!(diff_events.is_empty());

    // Clear events before final expression phase
    app.world_mut().resource_mut::<Events<GenomeDiffEvent>>().clear();

    // Now express the repaired gene
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.express_gene(BlockKind::Fermentation));
    assert_eq!(*genome.get_gene_state(&BlockKind::Fermentation).unwrap(), GeneState::Expressed);

    // Run app update
    app.update();

    // Check for GenomeDiffEvent (should now show Fermentation as enabled)
    let events = app.world_mut().resource_mut::<Events<GenomeDiffEvent>>();
    let mut reader = events.get_cursor();
    let diff_events: Vec<&GenomeDiffEvent> = reader.read(&events).collect();

    assert_eq!(diff_events.len(), 1);
    let diff = &diff_events[0];
    assert!(diff.enabled.contains(&BlockKind::Fermentation));
    assert_eq!(diff.enabled.len(), 1);
}

#[test]
fn test_genome_diff_mutated_to_silent_no_event() {
    let mut app = setup_app();
    let mut genome = app.world_mut().resource_mut::<Genome>();

    // Add and express a gene
    genome.add_gene(BlockKind::Fermentation);
    assert!(genome.express_gene(BlockKind::Fermentation));

    // Initial update to set previous state (Fermentation expressed)
    app.update();

    // Clear events from initial expression
    app.world_mut().resource_mut::<Events<GenomeDiffEvent>>().clear();

    // Mutate the expressed gene
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.mutate_gene(BlockKind::Fermentation));
    assert_eq!(*genome.get_gene_state(&BlockKind::Fermentation).unwrap(), GeneState::Mutated);

    // Run app update to trigger poll_genome_diff and emit event (disabled)
    app.update();

    // Clear events from mutation
    app.world_mut().resource_mut::<Events<GenomeDiffEvent>>().clear();

    // Repair the mutated gene (it should go back to Silent)
    let mut genome = app.world_mut().resource_mut::<Genome>();
    assert!(genome.repair_gene(BlockKind::Fermentation));
    assert_eq!(*genome.get_gene_state(&BlockKind::Fermentation).unwrap(), GeneState::Silent);

    // Run app update to trigger poll_genome_diff and emit event (should be empty)
    app.update();

    // Check for GenomeDiffEvent (should be empty)
    let events = app.world_mut().resource_mut::<Events<GenomeDiffEvent>>();
    let mut reader = events.get_cursor();
    let diff_events: Vec<&GenomeDiffEvent> = reader.read(&events).collect();
    eprintln!("test_genome_diff_mutated_to_silent_no_event: Diff Events after repair: {:?}", diff_events);
    assert!(diff_events.is_empty());
}