//! # Fermentation System Integration Tests
//!
//! These tests verify the behavior of the fermentation and vesicle export systems,
//! ensuring that their consumption and production logic works as expected within
//! a minimal Bevy app environment.

use metabolistic3d::molecules::{ATP, Pyruvate, ReducingPower, OrganicWaste, Currency};
use metabolistic3d::blocks::fermentation::{FermentationPlugin, FermentationBlock};
use metabolistic3d::blocks::vesicle_export::{VesicleExportPlugin, VesicleExportBlock};
use bevy::prelude::{App, MinimalPlugins, FixedUpdate};
use bevy::time::{Time, Fixed};

/// Tests that fermentation consumes Pyruvate and ReducingPower and produces ATP and OrganicWaste.
#[test]
fn test_fermentation_produces_and_consumes() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(FermentationPlugin);
    app.init_resource::<Time>();
    app.init_resource::<Time<Fixed>>();

    app.world_mut().spawn(FermentationBlock);

    // Initialize resources with starting amounts
    app.world_mut().insert_resource(Pyruvate(100.0));
    app.world_mut().insert_resource(ReducingPower(100.0));
    app.world_mut().insert_resource(ATP(0.0));
    app.world_mut().insert_resource(OrganicWaste(0.0));

    // --- Run Simulation ---
    let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
    app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
    app.update();
    app.world_mut().run_schedule(FixedUpdate);

    // --- Verification ---
    assert!((app.world().resource::<OrganicWaste>().amount() - 1.0).abs() < 1e-6);

    // Run again to ensure continuous operation
    app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
    app.update();
    app.world_mut().run_schedule(FixedUpdate);
    assert!((app.world().resource::<OrganicWaste>().amount() - 2.0).abs() < 1e-6);
}

/// Tests that vesicle export decreases OrganicWaste.
#[test]
fn test_vesicle_export_decreases_organic_waste() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(VesicleExportPlugin);
    app.init_resource::<Time>();
    app.init_resource::<Time<Fixed>>();

    app.world_mut().spawn(VesicleExportBlock);

    // Initialize OrganicWaste with a starting amount.
    app.world_mut().insert_resource(OrganicWaste(100.0));

    // --- Run Simulation ---
    let fixed_time_step = app.world().resource::<Time<Fixed>>().delta();
    app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
    app.update();
    app.world_mut().run_schedule(FixedUpdate);

    // --- Verification ---
    // Assuming default VesicleExportRate(0.1)
    assert!((app.world().resource::<OrganicWaste>().amount() - 99.9).abs() < 1e-6);

    // Run until depletion
    for _ in 0..999 {
        app.world_mut().resource_mut::<Time>().advance_by(fixed_time_step);
        app.update();
        app.world_mut().run_schedule(FixedUpdate);
    }

    // Verify that it doesn't go below zero
    assert!((app.world().resource::<OrganicWaste>().amount() - 0.0).abs() < 1e-6);
}
