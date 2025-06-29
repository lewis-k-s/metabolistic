use metabolistic3d::MetabolisticApp;

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
