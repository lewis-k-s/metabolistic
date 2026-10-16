//! # Behaviour over time
//!
//! Property tests for long runs: pools stay stable, runs repeat exactly,
//! errors do not collect, oscillations stay small, and the result does not
//! depend on the tick length.

use approx::{assert_abs_diff_eq, assert_relative_eq};
use bevy::prelude::*;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::metabolism::{FlowmapTimingConfig, run_metabolic_tick};
use metabolistic3d::molecules::*;
use proptest::prelude::*;

use crate::common::*;

/// Blocks of a cell that runs on fermentation.
const FERMENTER: [BlockKind; 3] = [
    BlockKind::Fermentation,
    BlockKind::VesicleExport,
    BlockKind::FatStorage,
];

/// A fermenting cell with the given pools and stored lipids.
fn fermenter(
    app: &mut App,
    pyruvate: f32,
    rp: f32,
    lipids: f32,
    atp: f32,
    waste: f32,
    stored: f32,
) -> Entity {
    let cell = spawn_test_cell(
        app,
        &FERMENTER,
        &[
            (Currency::Pyruvate, pyruvate),
            (Currency::ReducingPower, rp),
            (Currency::Lipids, lipids),
            (Currency::ATP, atp),
            (Currency::Waste, waste),
        ],
    );
    set_stored_lipids(app, cell, stored);
    cell
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Pools do not drift far from where they start over a run. (Stored
    /// lipids are not checked: they grow while lipids are above the threshold.)
    #[test]
    fn currency_stabilization_over_time(
        initial_values in (
            100.0f32..300.0f32, // pyruvate
            100.0f32..300.0f32, // reducing power
            200.0f32..600.0f32, // lipids
            50.0f32..150.0f32   // atp
        ),
        long_simulation in 10..30usize
    ) {
        let (pyruvate, rp, lipids, atp) = initial_values;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(100.0));
        let cell = fermenter(&mut app, pyruvate, rp, lipids, atp, 10.0, 30.0);
        let initial = cell_snapshot(&app, cell);

        for step in 0..long_simulation {
            sim_step(&mut app);
            let now = cell_snapshot(&app, cell);
            prop_assert!(now.iter().all(|&v| v >= 0.0), "negative pool at step {}", step);
            for (i, (&a, &b)) in initial.iter().zip(&now).take(Currency::ALL.len()).enumerate() {
                if a > 1.0 {
                    prop_assert!((b - a).abs() / a <= 2.0, "pool {} drifted: {} -> {}", i, a, b);
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Two runs from the same start give the same result.
    #[test]
    fn reproducible_behavior(
        test_conditions in (
            50.0f32..200.0f32,  // pyruvate
            50.0f32..200.0f32,  // rp
            100.0f32..400.0f32, // lipids
            25.0f32..75.0f32    // atp
        ),
        simulation_length in 5..15usize
    ) {
        let (pyruvate, rp, lipids, atp) = test_conditions;
        let mut results = Vec::new();
        for _ in 0..2 {
            let mut app = MetabolisticApp::new_headless();
            app.world_mut().insert_resource(LipidToxicityThreshold(80.0));
            let cell = fermenter(&mut app, pyruvate, rp, lipids, atp, 5.0, 15.0);
            for _ in 0..simulation_length {
                sim_step(&mut app);
            }
            results.push(cell_snapshot(&app, cell));
        }
        for (&a, &b) in results[0].iter().zip(&results[1]) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-6);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Free + stored lipids stay constant over a long run.
    #[test]
    fn long_term_error_accumulation(
        stable_config in (
            200.0f32..500.0f32, // pyruvate
            200.0f32..500.0f32, // rp
            400.0f32..800.0f32, // lipids
            100.0f32..200.0f32  // atp
        ),
        very_long_simulation in 15..40usize
    ) {
        let (pyruvate, rp, lipids, atp) = stable_config;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(200.0));
        let cell = fermenter(&mut app, pyruvate, rp, lipids, atp, 20.0, 50.0);
        let initial_lipids = total_lipids(&app, cell);

        for step in 0..very_long_simulation {
            sim_step(&mut app);
            for (i, &amount) in cell_snapshot(&app, cell).iter().enumerate() {
                prop_assert!(amount.is_finite() && amount >= 0.0,
                    "pool {} bad at step {}: {}", i, step, amount);
            }
            let error = (total_lipids(&app, cell) - initial_lipids).abs();
            prop_assert!(error <= 1e-2, "lipid balance error at step {}: {}", step, error);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// Lipids and waste do not oscillate more and more.
    #[test]
    fn cyclic_behavior_analysis(
        oscillation_config in (
            80.0f32..150.0f32,  // pyruvate
            80.0f32..150.0f32,  // rp
            300.0f32..600.0f32, // lipids (high, so storage starts)
            30.0f32..80.0f32    // atp
        ),
        cycle_observation_length in 10..30usize
    ) {
        let (pyruvate, rp, lipids, atp) = oscillation_config;
        let mut app = MetabolisticApp::new_headless();
        app.world_mut().insert_resource(LipidToxicityThreshold(250.0));
        let cell = fermenter(&mut app, pyruvate, rp, lipids, atp, 0.0, 10.0);

        let mut lipid_history = Vec::new();
        let mut waste_history = Vec::new();
        for _ in 0..cycle_observation_length {
            sim_step(&mut app);
            lipid_history.push(pool(&app, cell, Currency::Lipids));
            waste_history.push(pool(&app, cell, Currency::Waste));
            prop_assert!(cell_currencies_non_negative(&app, cell));
        }

        let range = |v: &[f32]| {
            let max = v.iter().copied().fold(f32::MIN, f32::max);
            let min = v.iter().copied().fold(f32::MAX, f32::min);
            max - min
        };
        prop_assert!(range(&lipid_history) <= lipids * 2.0);
        prop_assert!(waste_history.iter().all(|&w| w <= 500.0));
        let half = lipid_history.len() / 2;
        prop_assert!(range(&lipid_history[half..]) <= range(&lipid_history[..half]) * 2.0 + 1e-3,
            "oscillation grows");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 2,
        .. ProptestConfig::default()
    })]
    /// The same simulated time gives the same result with short or long ticks.
    #[test]
    fn time_scale_consistency(
        base_conditions in (
            100.0f32..250.0f32, // pyruvate
            100.0f32..250.0f32, // rp
            200.0f32..500.0f32, // lipids
            50.0f32..100.0f32   // atp
        ),
        time_scale_factor in prop_oneof![
            Just(0.5f32),
            Just(1.0f32),
            Just(2.0f32),
            Just(4.0f32)
        ]
    ) {
        let (pyruvate, rp, lipids, atp) = base_conditions;
        let mut normal_app = MetabolisticApp::new_headless();
        let mut scaled_app = MetabolisticApp::new_headless();
        let mut cells = Vec::new();
        for app in [&mut normal_app, &mut scaled_app] {
            app.world_mut().insert_resource(LipidToxicityThreshold(120.0));
            cells.push(fermenter(app, pyruvate, rp, lipids, atp, 5.0, 20.0));
        }

        // 40 base ticks; the step count is a whole number for every factor.
        let base_steps = 40;
        let scaled_steps = (base_steps as f32 / time_scale_factor) as usize;
        let base_rate = normal_app.world().resource::<FlowmapTimingConfig>().updates_per_second;
        scaled_app.insert_resource(FlowmapTimingConfig {
            manual: true,
            ..FlowmapTimingConfig::new(base_rate / time_scale_factor)
        });

        for _ in 0..base_steps {
            run_metabolic_tick(normal_app.world_mut());
        }
        for _ in 0..scaled_steps {
            run_metabolic_tick(scaled_app.world_mut());
        }

        let normal = cell_snapshot(&normal_app, cells[0]);
        let scaled = cell_snapshot(&scaled_app, cells[1]);
        for (&n, &s) in normal.iter().zip(&scaled) {
            if n > 1.0 {
                assert_relative_eq!(n, s, epsilon = 0.2);
            } else {
                assert_abs_diff_eq!(n, s, epsilon = 1.0);
            }
        }
    }
}
