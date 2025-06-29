//! # Currency System Integration Tests
//!
//! These tests verify the behavior of the currency system, ensuring that
//! consumption logic works as expected within a minimal Bevy app environment.

use metabolistic3d::molecules::{
    try_consume_currency, ATP, CarbonSkeletons, Currency, CurrencyPlugin, ReducingPower, Pyruvate, OrganicWaste
};
use metabolistic3d::MetabolisticApp;
use bevy::prelude::*;










/// A system that simulates a block consuming a fixed amount of ATP on every update.
fn atp_consuming_system(atp: ResMut<ATP>) {
    // This system represents a metabolic block that requires 10 ATP per cycle.
    if try_consume_currency(atp, 10.0, "AtpConsumer") {
        // Successfully consumed ATP
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
    app.world_mut().insert_resource(ATP(100.0));

    // --- Initial State Verification ---
    let initial_atp = app.world().resource::<ATP>().amount();
    assert_eq!(initial_atp, 100.0, "Initial ATP should be 100.0");

    // --- Run Simulation (1st update) ---
    app.update();

    // --- Verification after 1st update ---
    let atp_after_1_update = app.world().resource::<ATP>().amount();
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
    let atp_after_6_updates = app.world().resource::<ATP>().amount();
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
    let atp_after_10_updates = app.world().resource::<ATP>().amount();
    assert_eq!(
        atp_after_10_updates, 0.0,
        "ATP should be fully depleted to 0.0"
    );

    // --- Verification of non-negative currency ---
    // Run the app one more time to ensure ATP doesn't go below zero.
    app.update();
    let atp_after_depletion = app.world().resource::<ATP>().amount();
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
    app.world_mut().insert_resource(ReducingPower(5.0));

    fn test_system(power: ResMut<ReducingPower>) {
        // This will fail, but shouldn't panic. The function returns false.
        let _ = try_consume_currency(power, 10.0, "TestConsumer");
    }
    app.add_systems(Update, test_system);

    // --- Run system ---
    app.update();

    // --- Verification ---
    // Verify that the resource amount remains unchanged.
    let power_after_failed_attempt = app.world().resource::<ReducingPower>().amount();
    assert_eq!(
        power_after_failed_attempt, 5.0,
        "ReducingPower amount should not change after a failed consumption"
    );
}

/// Tests that attempting to consume a negative amount of currency is not allowed.
#[test]
fn test_cannot_consume_negative_currency() {
    // --- Setup ---
    let mut app = App::new();
    app.add_plugins(CurrencyPlugin);
    app.world_mut().insert_resource(CarbonSkeletons(50.0));

    fn test_system(skeletons: ResMut<CarbonSkeletons>) {
        let _ = try_consume_currency(skeletons, -10.0, "NegativeConsumer");
    }
    app.add_systems(Update, test_system);

    // --- Run system ---
    app.update();

    // --- Verification ---
    let amount_after_attempt = app.world().resource::<CarbonSkeletons>().amount();
    assert_eq!(
        amount_after_attempt, 50.0,
        "Currency amount should not change after attempting to consume a negative value"
    );
}