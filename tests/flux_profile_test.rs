use metabolistic3d::metabolism::FluxProfile;
use metabolistic3d::molecules::Currency;

#[test]
fn test_flux_profile_demand_methods() {
    let mut profile = FluxProfile::default();

    // Test setting demand
    profile.set_demand(Currency::Pyruvate, 5.0);
    profile.set_demand(Currency::ATP, 2.0);

    // Test has_demand_for
    assert!(profile.has_demand_for(Currency::Pyruvate));
    assert!(profile.has_demand_for(Currency::ATP));
    assert!(!profile.has_demand_for(Currency::OrganicWaste));

    // Test get_demand
    assert_eq!(profile.get_demand(Currency::Pyruvate), 5.0);
    assert_eq!(profile.get_demand(Currency::ATP), 2.0);
    assert_eq!(profile.get_demand(Currency::OrganicWaste), 0.0);

    // Test get_demanded_currencies
    let demanded = profile.get_demanded_currencies();
    assert_eq!(demanded.len(), 2);
    assert!(demanded.contains(&Currency::Pyruvate));
    assert!(demanded.contains(&Currency::ATP));
}

#[test]
fn test_flux_profile_production_methods() {
    let mut profile = FluxProfile::default();

    // Test setting production
    profile.set_production(Currency::ATP, 10.0);
    profile.set_production(Currency::OrganicWaste, 3.5);

    // Test produces
    assert!(profile.produces(Currency::ATP));
    assert!(profile.produces(Currency::OrganicWaste));
    assert!(!profile.produces(Currency::Pyruvate));

    // Test get_production
    assert_eq!(profile.get_production(Currency::ATP), 10.0);
    assert_eq!(profile.get_production(Currency::OrganicWaste), 3.5);
    assert_eq!(profile.get_production(Currency::Pyruvate), 0.0);

    // Test get_produced_currencies
    let produced = profile.get_produced_currencies();
    assert_eq!(produced.len(), 2);
    assert!(produced.contains(&Currency::ATP));
    assert!(produced.contains(&Currency::OrganicWaste));
}

#[test]
fn test_flux_profile_mixed_operations() {
    let mut profile = FluxProfile::default();

    // Set both demand and production
    profile.set_demand(Currency::Pyruvate, 1.0);
    profile.set_demand(Currency::ReducingPower, 1.0);
    profile.set_production(Currency::ATP, 1.0);
    profile.set_production(Currency::OrganicWaste, 1.0);

    // Verify demands
    assert!(profile.has_demand_for(Currency::Pyruvate));
    assert!(profile.has_demand_for(Currency::ReducingPower));
    assert!(!profile.has_demand_for(Currency::ATP));
    assert!(!profile.has_demand_for(Currency::OrganicWaste));

    // Verify productions
    assert!(!profile.produces(Currency::Pyruvate));
    assert!(!profile.produces(Currency::ReducingPower));
    assert!(profile.produces(Currency::ATP));
    assert!(profile.produces(Currency::OrganicWaste));

    // Test scaling
    profile.scale(2.0);
    assert_eq!(profile.get_demand(Currency::Pyruvate), 2.0);
    assert_eq!(profile.get_production(Currency::ATP), 2.0);

    // Test clearing
    profile.clear();
    assert_eq!(profile.get_demanded_currencies().len(), 0);
    assert_eq!(profile.get_produced_currencies().len(), 0);
}

#[test]
fn test_flux_profile_negative_values_handled_correctly() {
    let mut profile = FluxProfile::default();

    // Test that set_demand handles negative inputs correctly
    profile.set_demand(Currency::Pyruvate, -5.0);
    assert_eq!(profile.get_demand(Currency::Pyruvate), 5.0);

    // Test that set_production handles negative inputs correctly
    profile.set_production(Currency::ATP, -3.0);
    assert_eq!(profile.get_production(Currency::ATP), 3.0);
}
