//! # Currency System Integration Tests
//!
//! These tests verify the behavior of the currency system, ensuring that
//! consumption logic works as expected within a minimal Bevy app environment.

use bevy::prelude::*;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::{Currency, CellMass, CurrencyPlugin, LipidToxicityThreshold, PolyMer};
use metabolistic3d::MetabolisticApp;

/// A system that simulates a block consuming a fixed amount of ATP on every update.
fn atp_consuming_system(mut currency_pools: ResMut<CurrencyPools>) {
    // This system represents a metabolic block that requires 10 ATP per cycle.
    if currency_pools.can_consume(Currency::ATP, 10.0) {
        currency_pools.modify(Currency::ATP, -10.0);
    }
}

/// Tests that a currency (ATP) decreases when consumed and never goes below zero.
#[test]
fn test_atp_consumption_decreases_resource() {
    // --- Setup ---
    // Create a headless app with the necessary plugins and systems.
    let mut app = MetabolisticApp::new_headless();
    app.add_systems(Update, atp_consuming_system);

    // Initialize ATP with a starting amount.
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, 100.0);

    // --- Initial State Verification ---
    let initial_atp = app.world().resource::<CurrencyPools>().get(Currency::ATP);
    assert_eq!(initial_atp, 100.0, "Initial ATP should be 100.0");

    // --- Run Simulation (1st update) ---
    app.update();

    // --- Verification after 1st update ---
    let atp_after_1_update = app.world().resource::<CurrencyPools>().get(Currency::ATP);
    assert_eq!(
        atp_after_1_update, 90.0,
        "ATP should decrease by 10 after one update"
    );

    // --- Run Simulation (multiple updates) ---
    // Run the app for 5 more updates.
    for _ in 0..5 {
        app.update();
    }

    // --- Verification after 6 total updates ---
    let atp_after_6_updates = app.world().resource::<CurrencyPools>().get(Currency::ATP);
    assert_eq!(
        atp_after_6_updates, 40.0,
        "ATP should be 40.0 after 6 total updates (100 - 6*10)"
    );

    // --- Run until depletion ---
    // Run the app for 4 more updates to completely deplete the ATP.
    for _ in 0..4 {
        app.update();
    }

    // --- Verification of depletion ---
    let atp_after_10_updates = app.world().resource::<CurrencyPools>().get(Currency::ATP);
    assert_eq!(
        atp_after_10_updates, 0.0,
        "ATP should be fully depleted to 0.0"
    );

    // --- Verification of non-negative currency ---
    // Run the app one more time to ensure ATP doesn't go below zero.
    app.update();
    let atp_after_depletion = app.world().resource::<CurrencyPools>().get(Currency::ATP);
    assert_eq!(
        atp_after_depletion, 0.0,
        "ATP should not become negative after depletion"
    );
}

/// Tests that consumption fails when the requested amount exceeds the available currency.
#[test]
fn test_consumption_fails_on_insufficient_funds() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(CurrencyPlugin);
    app.world_mut().insert_resource(CurrencyPools::with_defaults());
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ReducingPower, 5.0);

    fn test_system(mut currency_pools: ResMut<CurrencyPools>) {
        // This will fail, but shouldn't panic.
        if currency_pools.can_consume(Currency::ReducingPower, 10.0) {
            currency_pools.modify(Currency::ReducingPower, -10.0);
        }
    }
    app.add_systems(Update, test_system);

    // --- Run system ---
    app.update();

    // --- Verification ---
    // Verify that the resource amount remains unchanged.
    let power_after_failed_attempt = app.world().resource::<CurrencyPools>().get(Currency::ReducingPower);
    assert_eq!(
        power_after_failed_attempt, 5.0,
        "ReducingPower amount should not change after a failed consumption"
    );
}


#[test]
fn test_polymerization_on_toxicity_threshold() {
    // --- Setup ---
    let mut app = MetabolisticApp::new_headless();

    // Initialize resources and components
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::FreeFattyAcids, 100.0);
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::StorageBeads, 0.0);
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, 10.0); // Enough ATP for polymerization
    app.world_mut()
        .insert_resource(LipidToxicityThreshold(50.0));

    // --- Initial State Verification ---
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids), 100.0);
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::StorageBeads), 0.0);

    // --- Run Simulation ---
    println!(
        "FreeFattyAcids before update: {}",
        app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids)
    );
    app.update();
    println!(
        "FreeFattyAcids after update: {}",
        app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids)
    );

    // --- Verification ---
    // Expected FFA after polymerization: 100 (initial) - 20 (poly_rate) = 80
    // Expected StorageBeads: 0 (initial) + 20 (poly_rate) = 20
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids), 80.0);
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::StorageBeads), 20.0);
}

#[test]
fn test_polymerization_with_lipolysis_entity() {
    // --- Setup ---
    let mut app = MetabolisticApp::new_headless();

    // Initialize resources
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::FreeFattyAcids, 100.0);
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::StorageBeads, 10.0);
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, 10.0);
    app.world_mut()
        .insert_resource(LipidToxicityThreshold(50.0));

    // Spawn an entity with PolyMer component (this enables lipolysis)
    app.world_mut().spawn((
        CellMass {
            base: 1.0,
            extra: 0.0,
        },
        PolyMer {
            capacity: 100.0,
            target_fill: 50.0,
            poly_rate: 20.0,
            lipo_rate: 5.0, // This will add 5.0 FFA back
        },
    ));

    // --- Initial State Verification ---
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids), 100.0);
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::StorageBeads), 10.0);

    // --- Run Simulation ---
    println!(
        "FreeFattyAcids before update: {}",
        app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids)
    );
    println!(
        "StorageBeads before update: {}",
        app.world().resource::<CurrencyPools>().get(Currency::StorageBeads)
    );
    app.update();
    println!(
        "FreeFattyAcids after update: {}",
        app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids)
    );
    println!(
        "StorageBeads after update: {}",
        app.world().resource::<CurrencyPools>().get(Currency::StorageBeads)
    );

    // --- Expected behavior after fix: Only polymerization runs ---
    // Polymerization: 100 - 20 = 80 FFA, +20 StorageBeads
    // Lipolysis: DOES NOT RUN because FFA (80) > threshold (50)
    // Net result: 80 FFA, 30 StorageBeads

    // This test verifies the fix prevents conflicting system execution
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids), 80.0);
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::StorageBeads), 30.0);
}

#[test]
fn test_lipolysis_when_safe() {
    // --- Setup ---
    let mut app = MetabolisticApp::new_headless();

    // Initialize resources - FFA below threshold
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::FreeFattyAcids, 30.0);
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::StorageBeads, 20.0);
    app.world_mut().resource_mut::<CurrencyPools>().set(Currency::ATP, 10.0);
    app.world_mut()
        .insert_resource(LipidToxicityThreshold(50.0));

    // Spawn an entity with PolyMer component
    app.world_mut().spawn((
        CellMass {
            base: 1.0,
            extra: 10.0,
        },
        PolyMer {
            capacity: 100.0,
            target_fill: 50.0,
            poly_rate: 20.0,
            lipo_rate: 5.0,
        },
    ));

    // --- Initial State Verification ---
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids), 30.0);
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::StorageBeads), 20.0);

    // --- Run Simulation ---
    app.update();

    // --- Verification ---
    // Polymerization: DOES NOT RUN because FFA (30) < threshold (50)
    // Lipolysis: 30 + 5 = 35 FFA, 20 - 5 = 15 StorageBeads
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::FreeFattyAcids), 35.0);
    assert_eq!(app.world().resource::<CurrencyPools>().get(Currency::StorageBeads), 15.0);
}

