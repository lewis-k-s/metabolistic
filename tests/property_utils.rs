//! # Property Testing Utilities
//!
//! This module provides proptest strategies for generating test data for the metabolic simulation.
//! It includes generators for currency values, metabolic states, and system configurations.

use proptest::prelude::*;
use metabolistic3d::molecules::*;
use metabolistic3d::blocks::genome::{BlockKind, GeneState};
use metabolistic3d::metabolism::{BlockStatus, FluxProfile, CurrencyPools};
use metabolistic3d::MetabolisticApp;
use bevy::prelude::*;
use std::collections::HashMap;

// --- Currency Value Strategies ---

/// Generates reasonable currency values (0.0 to 1000.0)
/// Avoids extreme values that could cause overflow or precision issues
pub fn currency_amount() -> impl Strategy<Value = f32> {
    0.0f32..1000.0f32
}

/// Generates small currency amounts for precise operations (0.0 to 100.0)
pub fn small_currency_amount() -> impl Strategy<Value = f32> {
    0.0f32..100.0f32
}

/// Generates currency consumption amounts (positive values only)
pub fn consumption_amount() -> impl Strategy<Value = f32> {
    0.1f32..50.0f32
}

/// Generates currency production amounts (positive values only)
pub fn production_amount() -> impl Strategy<Value = f32> {
    0.1f32..100.0f32
}

// --- Metabolic State Strategies ---

/// Generates different BlockKind variants for testing
pub fn block_kind() -> impl Strategy<Value = BlockKind> {
    prop_oneof![
        Just(BlockKind::LightCapture),
        Just(BlockKind::SugarCatabolism),
        Just(BlockKind::Respiration),
        Just(BlockKind::Fermentation),
        Just(BlockKind::LipidMetabolism),
        Just(BlockKind::Polymerization),
    ]
}

/// Generates different BlockStatus variants for testing
pub fn block_status() -> impl Strategy<Value = BlockStatus> {
    prop_oneof![
        Just(BlockStatus::Active),
        Just(BlockStatus::Mutated),
        Just(BlockStatus::Silent),
    ]
}

/// Generates different GeneState variants for testing
pub fn gene_state() -> impl Strategy<Value = GeneState> {
    prop_oneof![
        Just(GeneState::Expressed),
        Just(GeneState::Mutated),
        Just(GeneState::Silent),
    ]
}

/// Generates Currency enum variants for testing
pub fn currency_type() -> impl Strategy<Value = Currency> {
    prop_oneof![
        Just(Currency::ATP),
        Just(Currency::ReducingPower),
        Just(Currency::AcetylCoA),
        Just(Currency::CarbonSkeletons),
    ]
}

// --- FluxProfile Strategies ---

/// Generates a simple FluxProfile with one currency
pub fn simple_flux_profile() -> impl Strategy<Value = FluxProfile> {
    (currency_type(), -100.0f32..100.0f32)
        .prop_map(|(currency, amount)| {
            let mut profile = HashMap::new();
            profile.insert(currency, amount);
            FluxProfile(profile)
        })
}

/// Generates a complex FluxProfile with multiple currencies
pub fn complex_flux_profile() -> impl Strategy<Value = FluxProfile> {
    prop::collection::hash_map(currency_type(), -100.0f32..100.0f32, 1..=4)
        .prop_map(|profile| FluxProfile(profile))
}

// --- System Configuration Strategies ---

/// Generates a reasonable lipid toxicity threshold
pub fn lipid_toxicity_threshold() -> impl Strategy<Value = f32> {
    10.0f32..200.0f32
}

/// Generates polymer rates for testing
pub fn polymer_rate() -> impl Strategy<Value = f32> {
    1.0f32..50.0f32
}

/// Generates lipolysis rates for testing
pub fn lipo_rate() -> impl Strategy<Value = f32> {
    0.5f32..25.0f32
}

// --- App Configuration Strategies ---

/// Creates a headless app with specific initial currency amounts
pub fn app_with_currencies(
    atp: f32,
    reducing_power: f32,
    acetyl_coa: f32,
    carbon_skeletons: f32,
    free_fatty_acids: f32,
    pyruvate: f32,
    organic_waste: f32,
) -> App {
    let mut app = MetabolisticApp::new_headless();
    
    let mut currency_pools = app.world_mut().resource_mut::<CurrencyPools>();
    currency_pools.set(Currency::ATP, atp);
    currency_pools.set(Currency::ReducingPower, reducing_power);
    currency_pools.set(Currency::AcetylCoA, acetyl_coa);
    currency_pools.set(Currency::CarbonSkeletons, carbon_skeletons);
    currency_pools.set(Currency::FreeFattyAcids, free_fatty_acids);
    currency_pools.set(Currency::Pyruvate, pyruvate);
    currency_pools.set(Currency::OrganicWaste, organic_waste);
    
    app
}

/// Strategy for generating an app with random currency amounts
pub fn app_with_random_currencies() -> impl Strategy<Value = App> {
    (
        currency_amount(),
        currency_amount(),
        currency_amount(),
        currency_amount(),
        currency_amount(),
        currency_amount(),
        currency_amount(),
    ).prop_map(|(atp, rp, acetyl, carbon, ffa, pyruvate, waste)| {
        app_with_currencies(atp, rp, acetyl, carbon, ffa, pyruvate, waste)
    })
}

// --- Metabolic Block Spawning Helpers ---

/// Spawns a complete fermentation block with all required components for testing
pub fn spawn_complete_fermentation_block(app: &mut App) {
    // Express fermentation gene to enable the block
    {
        let mut genome = app.world_mut().resource_mut::<metabolistic3d::blocks::genome::Genome>();
        genome.add_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
        genome.express_gene(metabolistic3d::blocks::genome::BlockKind::Fermentation);
    }
    
    // Spawn fermentation block with complete component architecture
    app.world_mut().spawn((
        metabolistic3d::blocks::fermentation::FermentationBlock,
        metabolistic3d::metabolism::MetabolicBlock,
        metabolistic3d::metabolism::MetabolicNode {
            kind: metabolistic3d::blocks::genome::BlockKind::Fermentation,
            status: metabolistic3d::metabolism::BlockStatus::Active,
        },
        {
            let mut flux_profile = metabolistic3d::metabolism::FluxProfile::default();
            flux_profile.0.insert(Currency::Pyruvate, -1.0);
            flux_profile.0.insert(Currency::ReducingPower, -1.0);
            flux_profile.0.insert(Currency::ATP, 1.0);
            flux_profile.0.insert(Currency::OrganicWaste, 1.0);
            flux_profile
        },
    ));
    
    // Trigger metabolic graph rebuild
    app.world_mut().resource_mut::<metabolistic3d::metabolism::FlowDirty>().0 = true;
}

// --- Utility Functions ---

/// Calculates the total currency pool across all currencies in an app
pub fn total_currency_pool(app: &App) -> f32 {
    let currency_pools = app.world().resource::<CurrencyPools>();
    currency_pools.pools.values().sum()
}

/// Checks if all currencies in an app are non-negative
pub fn all_currencies_non_negative(app: &App) -> bool {
    let currency_pools = app.world().resource::<CurrencyPools>();
    currency_pools.pools.values().all(|&v| v >= 0.0)
}

/// Gets all currency amounts as a vector for easy comparison
pub fn get_currency_snapshot(app: &App) -> Vec<f32> {
    let currency_pools = app.world().resource::<CurrencyPools>();
    vec![
        currency_pools.get(Currency::ATP),
        currency_pools.get(Currency::ReducingPower),
        currency_pools.get(Currency::AcetylCoA),
        currency_pools.get(Currency::CarbonSkeletons),
        currency_pools.get(Currency::FreeFattyAcids),
        currency_pools.get(Currency::Pyruvate),
        currency_pools.get(Currency::OrganicWaste),
        currency_pools.get(Currency::StorageBeads),
    ]
}

// --- Test Macros ---

/// Macro to create a proptest that verifies a property holds across multiple app updates
#[macro_export]
macro_rules! proptest_over_time {
    ($name:ident, $strategy:expr, $updates:expr, $property:expr) => {
        proptest! {
            #[test]
            fn $name(mut app in $strategy) {
                for _ in 0..$updates {
                    let initial_state = get_currency_snapshot(&app);
                    app.update();
                    prop_assert!($property(&app, &initial_state));
                }
            }
        }
    };
}