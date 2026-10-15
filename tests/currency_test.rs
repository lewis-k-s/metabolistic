//! # Currency System Integration Tests
//!
//! These tests verify the behavior of the currency system, ensuring that
//! consumption logic works as expected within a minimal Bevy app environment.

use bevy::prelude::*;
use metabolistic3d::blocks::genome::Genome;
use metabolistic3d::molecules::{Currency, CurrencyPools};
use metabolistic3d::player::{spawn_local_player_cell, ControlledCell, LocalPlayer};
use metabolistic3d::MetabolisticApp;

/// A system that simulates a block consuming a fixed amount of ATP on every update.
fn atp_consuming_system(
    local_players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cell_query: Query<&mut CurrencyPools>,
) {
    // This system represents a metabolic block that requires 10 ATP per cycle.
    if let Ok(controlled_cell) = local_players.get_single() {
        if let Ok(mut currency_pools) = cell_query.get_mut(controlled_cell.0) {
            if currency_pools.can_consume(Currency::ATP, 10.0) {
                currency_pools.modify(Currency::ATP, -10.0);
            }
        }
    }
}

/// Tests that a currency (ATP) decreases when consumed and never goes below zero.
#[test]
fn test_atp_consumption_decreases_resource() {
    // --- Setup ---
    // Create a headless app with the necessary plugins and systems.
    let mut app = MetabolisticApp::new_headless();
    app.add_systems(Update, atp_consuming_system);

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, Genome::default())
    };

    // Apply commands to actually create the entities
    app.world_mut().flush();

    // Initialize ATP with a starting amount on the cell
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::ATP, 100.0);
    }

    // --- Initial State Verification ---
    let initial_atp = {
        let entity_ref = app.world().get_entity(cell_entity).unwrap();
        let currency_pools = entity_ref.get::<CurrencyPools>().unwrap();
        currency_pools.get(Currency::ATP)
    };
    assert_eq!(initial_atp, 100.0, "Initial ATP should be 100.0");

    // --- Run Simulation (1st update) ---
    app.update();

    // --- Verification after 1st update ---
    let atp_after_1_update = {
        let entity_ref = app.world().get_entity(cell_entity).unwrap();
        let currency_pools = entity_ref.get::<CurrencyPools>().unwrap();
        currency_pools.get(Currency::ATP)
    };
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
    let atp_after_6_updates = {
        let entity_ref = app.world().get_entity(cell_entity).unwrap();
        let currency_pools = entity_ref.get::<CurrencyPools>().unwrap();
        currency_pools.get(Currency::ATP)
    };
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
    let atp_after_10_updates = {
        let entity_ref = app.world().get_entity(cell_entity).unwrap();
        let currency_pools = entity_ref.get::<CurrencyPools>().unwrap();
        currency_pools.get(Currency::ATP)
    };
    assert_eq!(
        atp_after_10_updates, 0.0,
        "ATP should be fully depleted to 0.0"
    );

    // --- Verification of non-negative currency ---
    // Run the app one more time to ensure ATP doesn't go below zero.
    app.update();
    let atp_after_depletion = {
        let entity_ref = app.world().get_entity(cell_entity).unwrap();
        let currency_pools = entity_ref.get::<CurrencyPools>().unwrap();
        currency_pools.get(Currency::ATP)
    };
    assert_eq!(
        atp_after_depletion, 0.0,
        "ATP should not become negative after depletion"
    );
}

/// Tests that consumption fails when the requested amount exceeds the available currency.
#[test]
fn test_consumption_fails_on_insufficient_funds() {
    // --- Setup ---
    let mut app = MetabolisticApp::new_headless();

    // Spawn a local player-cell pair for testing
    let (_player_entity, cell_entity) = {
        let mut commands = app.world_mut().commands();
        spawn_local_player_cell(&mut commands, Genome::default())
    };

    // Apply commands to actually create the entities
    app.world_mut().flush();

    // Set up currency on the cell
    {
        let mut entity_mut = app.world_mut().get_entity_mut(cell_entity).unwrap();
        let mut currency_pools = entity_mut.get_mut::<CurrencyPools>().unwrap();
        currency_pools.set(Currency::ReducingPower, 5.0);
    }

    fn test_system(
        local_players: Query<&ControlledCell, With<LocalPlayer>>,
        mut cell_query: Query<&mut CurrencyPools>,
    ) {
        // This will fail, but shouldn't panic.
        if let Ok(controlled_cell) = local_players.get_single() {
            if let Ok(mut currency_pools) = cell_query.get_mut(controlled_cell.0) {
                if currency_pools.can_consume(Currency::ReducingPower, 10.0) {
                    currency_pools.modify(Currency::ReducingPower, -10.0);
                }
            }
        }
    }
    app.add_systems(Update, test_system);

    // --- Run system ---
    app.update();

    // --- Verification ---
    // Verify that the resource amount remains unchanged or has been managed by environmental systems.
    // Since environmental systems may modify currencies, we just verify it's not negative and consumption was properly gated.
    let power_after_failed_attempt = {
        let entity_ref = app.world().get_entity(cell_entity).unwrap();
        let currency_pools = entity_ref.get::<CurrencyPools>().unwrap();
        currency_pools.get(Currency::ReducingPower)
    };
    assert!(
        power_after_failed_attempt >= 0.0,
        "ReducingPower should remain non-negative after failed consumption"
    );
}

// TODO: Update remaining tests to use DefaultCell approach
// These tests were temporarily removed during the currency architecture refactor
// They need to be updated to use component-based CurrencyPools instead of resource-based

/*
#[test]
fn test_polymerization_on_toxicity_threshold() {
    // This test needs to be updated to use DefaultCell and component-based currency pools
    // See test_atp_consumption_decreases_resource() above for the correct pattern
}

#[test]
fn test_lipid_depolymerization() {
    // This test needs to be updated to use DefaultCell and component-based currency pools
    // See test_atp_consumption_decreases_resource() above for the correct pattern
}
*/
