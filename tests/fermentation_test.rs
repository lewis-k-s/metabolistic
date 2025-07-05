use metabolistic3d::molecules::Currency;
use metabolistic3d::blocks::fermentation::FermentationPlugin;
use metabolistic3d::blocks::vesicle_export::{VesicleExportPlugin, VesicleExportBlock};
use metabolistic3d::metabolism::{CurrencyPools, MetabolicFlowPlugin};
use metabolistic3d::blocks::genome::{Genome, GenomeDiffEvent, MetabolicUpdateEvent, BlockKind};
use bevy::prelude::{App, MinimalPlugins, FixedUpdate};
use bevy::time::{Time, Fixed};

/// Tests that fermentation consumes Pyruvate and ReducingPower and produces ATP and OrganicWaste.
#[test]
fn test_fermentation_produces_and_consumes() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(FermentationPlugin);
    app.add_plugins(MetabolicFlowPlugin);  // Add the metabolic flow plugin to handle currency changes
    app.add_event::<GenomeDiffEvent>();
    app.add_event::<MetabolicUpdateEvent>();
    app.init_resource::<Genome>();
    app.init_resource::<Time>();
    app.init_resource::<Time<Fixed>>();

    // Set up CurrencyPools with the same starting amounts
    {
        let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
        currency_pools.set(Currency::Pyruvate, 100.0);
        currency_pools.set(Currency::ReducingPower, 100.0);
        currency_pools.set(Currency::ATP, 0.0);
        currency_pools.set(Currency::OrganicWaste, 0.0);
    }
    
    // Activate the fermentation gene so the block will be active
    {
        let mut genome = app.world_mut().resource_mut::<Genome>();
        genome.add_gene(BlockKind::Fermentation);
        genome.express_gene(BlockKind::Fermentation);
    }

    // --- Run Simulation ---
    let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
    app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
    app.update();
    app.world_mut().run_schedule(FixedUpdate);

    // --- Verification ---
    // Check CurrencyPools for the actual results since that's where changes are applied
    let currency_pools = app.world().resource::<CurrencyPools>();
    assert!((currency_pools.get(Currency::OrganicWaste) - 1.0).abs() < 1e-6, 
            "Expected OrganicWaste to be 1.0, got {}", currency_pools.get(Currency::OrganicWaste));
    assert!((currency_pools.get(Currency::ATP) - 1.0).abs() < 1e-6,
            "Expected ATP to be 1.0, got {}", currency_pools.get(Currency::ATP));
    assert!((currency_pools.get(Currency::Pyruvate) - 99.0).abs() < 1e-6,
            "Expected Pyruvate to be 99.0, got {}", currency_pools.get(Currency::Pyruvate));

    // Run again to ensure continuous operation
    app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
    app.update();
    app.world_mut().run_schedule(FixedUpdate);
    
    let currency_pools = app.world().resource::<CurrencyPools>();
    assert!((currency_pools.get(Currency::OrganicWaste) - 2.0).abs() < 1e-6,
            "Expected OrganicWaste to be 2.0, got {}", currency_pools.get(Currency::OrganicWaste));
}

/// Tests that vesicle export decreases OrganicWaste.
#[test]
fn test_vesicle_export_decreases_organic_waste() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(VesicleExportPlugin);
    app.init_resource::<CurrencyPools>();
    app.init_resource::<Time>();
    app.init_resource::<Time<Fixed>>();

    app.world_mut().spawn(VesicleExportBlock);

    // Initialize OrganicWaste with a starting amount.
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::OrganicWaste, 100.0);

    // --- Run Simulation ---
    let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
    app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
    app.update();
    app.world_mut().run_schedule(FixedUpdate);

    // --- Verification ---
    // Assuming default VesicleExportRate(0.1)
    assert!((app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste) - 99.9).abs() < 1e-6);

    // Run until depletion
    for _ in 0..999 {
        app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
        app.update();
        app.world_mut().run_schedule(FixedUpdate);
    }

    // Verify that it doesn't go below zero
    assert!((app.world().resource::<CurrencyPools>().get(Currency::OrganicWaste) - 0.0).abs() < 1e-6);
}
