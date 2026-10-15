//! # Currency conservation invariants
//!
//! Property tests for the basic invariants of the pools:
//! 1. Pools never go below zero.
//! 2. Blocks keep their stoichiometry (mass balance).
//! 3. Transfers are zero-sum.

use approx::assert_relative_eq;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::cell::CellSize;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::*;
use proptest::prelude::*;

mod common;
use common::*;

/// Removes the maintenance cost, so that only the blocks change the pools.
fn no_maintenance(app: &mut bevy::prelude::App, cell: bevy::prelude::Entity) {
    *app.world_mut().get_mut::<CellSize>(cell).unwrap() = CellSize { radius: 0.0 };
}

// --- Invariant 1: pools never go below zero ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        max_shrink_iters: 10,
        .. ProptestConfig::default()
    })]
    /// All pools stay at 0 or more after any sequence of consumption attempts.
    #[test]
    fn currency_pools_never_negative(
        initial_atp in currency_amount(),
        initial_rp in currency_amount(),
        consumption_attempts in prop::collection::vec(consumption_amount(), 1..20)
    ) {
        let mut app = MetabolisticApp::new_headless();
        let cell = spawn_test_cell(&mut app, &[], &[
            (Currency::ATP, initial_atp),
            (Currency::ReducingPower, initial_rp),
        ]);

        for &amount in &consumption_attempts {
            let mut pools = app.world_mut().get_mut::<CurrencyPools>(cell).unwrap();
            for currency in [Currency::ATP, Currency::ReducingPower] {
                if pools.can_consume(currency, amount) {
                    pools.modify(currency, -amount);
                }
                prop_assert!(pools.get(currency) >= 0.0, "{:?} went negative", currency);
            }
        }
        prop_assert!(cell_currencies_non_negative(&app, cell));
    }
}

// --- Invariant 2: mass balance in blocks ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        max_shrink_iters: 5,
        .. ProptestConfig::default()
    })]
    /// Fermentation is 1 Pyruvate + 1 ReducingPower -> 1 ATP + 1 Waste: the
    /// inputs used equal the outputs made.
    #[test]
    fn fermentation_mass_balance(
        initial_pyruvate in 50.0f32..500.0f32,
        initial_rp in 50.0f32..500.0f32,
        simulation_steps in 1..10usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        let cell = spawn_test_cell(&mut app, &[BlockKind::Fermentation], &[
            (Currency::Pyruvate, initial_pyruvate),
            (Currency::ReducingPower, initial_rp),
        ]);
        no_maintenance(&mut app, cell);

        for _ in 0..simulation_steps {
            sim_step(&mut app);
        }

        let pyruvate_used = initial_pyruvate - pool(&app, cell, Currency::Pyruvate);
        let rp_used = initial_rp - pool(&app, cell, Currency::ReducingPower);
        let atp = pool(&app, cell, Currency::ATP);
        let waste = pool(&app, cell, Currency::Waste);

        prop_assert!(pyruvate_used > 0.0);
        assert_relative_eq!(pyruvate_used, rp_used, epsilon = 1e-3);
        assert_relative_eq!(pyruvate_used, atp, epsilon = 1e-3);
        assert_relative_eq!(pyruvate_used, waste, epsilon = 1e-3);
    }
}

// --- Invariant 3: transfers are zero-sum ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        max_shrink_iters: 5,
        .. ProptestConfig::default()
    })]
    /// Fat storage only moves lipids: free + stored lipids stay constant.
    #[test]
    fn fat_storage_zero_sum_transfer(
        initial_lipids in 100.0f32..500.0f32,
        initial_storage in currency_amount(),
        toxicity_threshold in 10.0f32..90.0f32,
        poly_rate in polymer_rate(),
        simulation_steps in 1..5usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        let cell = spawn_test_cell(&mut app, &[BlockKind::FatStorage], &[
            (Currency::Lipids, initial_lipids),
            (Currency::ATP, 100.0),
        ]);
        set_stored_lipids(&mut app, cell, initial_storage);
        set_fat_storage_rates(&mut app, cell, poly_rate, 5.0);
        let initial_total = initial_lipids + initial_storage;

        for _ in 0..simulation_steps {
            sim_step(&mut app);
            assert_relative_eq!(total_lipids(&app, cell), initial_total, epsilon = 0.1);
            prop_assert!(pool(&app, cell, Currency::Lipids) >= 0.0);
            prop_assert!(stored_lipids(&app, cell) >= 0.0);
        }
        prop_assert!(pool(&app, cell, Currency::Lipids) < initial_lipids, "lipids were stored");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        max_shrink_iters: 5,
        .. ProptestConfig::default()
    })]
    /// Vesicle export only removes waste. It makes nothing.
    #[test]
    fn vesicle_export_only_removes(
        initial_waste in 50.0f32..300.0f32,
        simulation_steps in 1..10usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        let cell = spawn_test_cell(&mut app, &[BlockKind::VesicleExport], &[
            (Currency::Waste, initial_waste),
        ]);
        no_maintenance(&mut app, cell);
        // No uptake from the outside pools.
        app.world_mut().remove_resource::<EnvironmentalCurrencyPools>();
        let initial = cell_snapshot(&app, cell);
        let waste_index = Currency::ALL.iter().position(|&c| c == Currency::Waste).unwrap();

        for _ in 0..simulation_steps {
            sim_step(&mut app);
            let now = cell_snapshot(&app, cell);
            for (i, (&before, &after)) in initial.iter().zip(&now).enumerate() {
                if i == waste_index {
                    prop_assert!(after < before, "waste goes down");
                } else {
                    prop_assert!(after <= before + 1e-4, "pool {} grew: {} -> {}", i, before, after);
                }
                prop_assert!(after >= 0.0);
            }
        }
    }
}

// --- Combined blocks ---

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 3,
        .. ProptestConfig::default()
    })]
    /// Fermentation, vesicle export and fat storage together keep all pools at
    /// 0 or more.
    #[test]
    fn multi_system_invariants(
        initial_pyruvate in 100.0f32..300.0f32,
        initial_rp in 100.0f32..300.0f32,
        initial_lipids in 150.0f32..400.0f32,
        toxicity_threshold in 50.0f32..100.0f32,
        simulation_steps in 1..8usize
    ) {
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(toxicity_threshold));
        let cell = spawn_test_cell(
            &mut app,
            &[BlockKind::Fermentation, BlockKind::VesicleExport, BlockKind::FatStorage],
            &[
                (Currency::Pyruvate, initial_pyruvate),
                (Currency::ReducingPower, initial_rp),
                (Currency::ATP, 50.0),
                (Currency::Lipids, initial_lipids),
            ],
        );
        set_stored_lipids(&mut app, cell, 10.0);

        for step in 0..simulation_steps {
            sim_step(&mut app);
            prop_assert!(cell_currencies_non_negative(&app, cell), "negative pool at step {}", step);
        }
    }
}
