//! TUI Integration Tests
//!
//! These tests verify that TUI system integrates properly with Bevy app
//! and can be initialized without panicking.

use bevy::prelude::*;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::blocks::genome::{BlockKind, GeneState};
use metabolistic3d::molecules::Currency;
use metabolistic3d::tui::*;

#[test]
fn test_tui_plugin_initialization() {
    // Test that TUI plugin can be added to an app without panicking
    let mut app = MetabolisticApp::new_headless();

    // Add TUI plugin
    app.add_plugins(CellTuiPlugin);

    // Run startup systems
    app.update();

    // Verify that TUI resources are initialized
    assert!(app.world().contains_resource::<TuiState>());
    assert!(app.world().contains_resource::<TuiConfig>());
}

#[test]
fn test_tui_data_collection() {
    let mut app = MetabolisticApp::new_headless();
    app.add_plugins(CellTuiPlugin);

    // Create a test cell
    let _cell_entity = app
        .world_mut()
        .spawn((metabolistic3d::cell::Cell, Name::new("Test Cell")))
        .id();

    // Run systems to collect data
    app.update();

    // Check that TUI update events are being sent
    let events = app.world().resource::<Messages<TuiUpdateEvent>>();
    let mut reader = events.get_cursor();

    // Should have at least one event after update
    let event_count = reader.read(events).count();
    assert!(event_count > 0, "Expected at least one TUI update event");
}

#[test]
fn test_tui_input_handling() {
    let mut app = MetabolisticApp::new_headless();
    app.add_plugins(CellTuiPlugin);

    // Initialize keyboard input
    let mut button_input = ButtonInput::<KeyCode>::default();
    button_input.press(KeyCode::Escape);

    // Insert keyboard input resource
    app.world_mut().insert_resource(button_input);

    // Run systems to handle input
    app.update();

    // Check that TUI input events are being sent
    let events = app.world().resource::<Messages<TuiInputEvent>>();
    let mut reader = events.get_cursor();

    // Should have at least one input event after pressing Escape
    let event_count = reader.read(events).count();
    assert!(event_count > 0, "Expected at least one TUI input event");
}

#[test]
fn test_tui_with_cell_data() {
    let mut app = MetabolisticApp::new_headless();
    app.add_plugins(CellTuiPlugin);

    // Create a cell with some data
    let mut genome = metabolistic3d::blocks::genome::Genome::default();
    genome.add_gene(BlockKind::Fermentation);
    genome.express_gene(BlockKind::Fermentation);
    genome.add_gene(BlockKind::LipidMetabolism);

    let mut pools = metabolistic3d::molecules::CurrencyPools::default();
    pools.set(Currency::ATP, 100.0);
    pools.set(Currency::ReducingPower, 50.0);
    pools.set(Currency::Waste, 10.0);

    app.world_mut().spawn((
        metabolistic3d::cell::Cell,
        genome,
        pools,
        Name::new("Test Cell with Data"),
    ));

    // Run systems to collect data
    app.update();

    // Check that TUI update events contain the cell data
    let events = app.world().resource::<Messages<TuiUpdateEvent>>();
    let mut reader = events.get_cursor();

    for event in reader.read(events) {
        // Verify that the event contains cell data
        assert!(
            !event.cell_data.currencies.is_empty(),
            "Cell data should contain currencies"
        );
        assert!(
            !event.cell_data.genome.is_empty(),
            "Cell data should contain genome data"
        );

        // Check specific values
        if let Some(&atp_amount) = event.cell_data.currencies.get(&Currency::ATP) {
            assert_eq!(atp_amount, 100.0, "ATP should be 100.0");
        }

        if let Some(&reducing_power) = event.cell_data.currencies.get(&Currency::ReducingPower) {
            assert_eq!(reducing_power, 50.0, "ReducingPower should be 50.0");
        }

        // Check genome state
        if let Some(gene_state) = event.cell_data.genome.get(&BlockKind::Fermentation) {
            assert_eq!(
                gene_state,
                &GeneState::Expressed,
                "Fermentation should be expressed"
            );
        }

        if let Some(gene_state) = event.cell_data.genome.get(&BlockKind::LipidMetabolism) {
            assert_eq!(
                gene_state,
                &GeneState::Silent,
                "LipidMetabolism should be silent"
            );
        }
    }
}

#[test]
fn test_tui_config_customization() {
    let mut app = MetabolisticApp::new_headless();
    app.add_plugins(CellTuiPlugin);

    // Insert custom TUI config
    let custom_config = TuiConfig {
        update_interval_ms: 100, // Faster updates
        show_detailed_currencies: false,
        show_genome_descriptions: false,
        show_block_details: false,
        color_enabled: false,
    };

    app.world_mut().insert_resource(custom_config);

    // Run systems
    app.update();

    // Verify that the custom config is preserved
    let config = app.world().resource::<TuiConfig>();
    assert_eq!(
        config.update_interval_ms, 100,
        "Custom update interval should be preserved"
    );
    assert!(
        !config.show_detailed_currencies,
        "Detailed currencies should be disabled"
    );
    assert!(
        !config.show_genome_descriptions,
        "Genome descriptions should be disabled"
    );
    assert!(
        !config.show_block_details,
        "Block details should be disabled"
    );
    assert!(!config.color_enabled, "Color should be disabled");
}
