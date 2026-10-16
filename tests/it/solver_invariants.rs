//! # Flux solver invariants
//!
//! Property tests for the pure solver (`metabolism::solver`), on random blocks
//! and pools:
//! - each block keeps its stoichiometry (mass is conserved per block);
//! - pools never go negative;
//! - the result does not depend on the tick length;
//! - more enzymes never lower a block's rate;
//! - blocks share a scarce input in proportion to their enzymes.

use metabolistic3d::metabolism::{
    BlockInput, BlockStatus, CurrencyPools, FluxProfile, block_rates, solve_tick,
};
use metabolistic3d::molecules::Currency;
use proptest::prelude::*;

/// A random block: flows on up to four pools, at least one input.
#[derive(Debug, Clone)]
struct RandomBlock {
    profile: FluxProfile,
    enzymes: f32,
    k_half: f32,
    mutated: bool,
}

fn currency() -> impl Strategy<Value = Currency> {
    prop::sample::select(Currency::ALL.to_vec())
}

fn random_block() -> impl Strategy<Value = RandomBlock> {
    (
        currency(),
        0.1f32..5.0,
        prop::collection::vec((currency(), -5.0f32..5.0), 0..4),
        0.0f32..6.0,
        0.0f32..20.0,
        any::<bool>(),
    )
        .prop_map(|(input, need, others, enzymes, k_half, mutated)| {
            let mut profile = FluxProfile::default();
            for (c, flow) in others {
                profile.0.insert(c, flow);
            }
            profile.set_demand(input, need);
            RandomBlock {
                profile,
                enzymes,
                k_half,
                mutated,
            }
        })
}

fn random_pools() -> impl Strategy<Value = CurrencyPools> {
    prop::collection::vec(0.0f32..200.0, Currency::ALL.len()).prop_map(|amounts| CurrencyPools {
        pools: Currency::ALL.iter().copied().zip(amounts).collect(),
    })
}

fn inputs(blocks: &[RandomBlock]) -> Vec<BlockInput<'_>> {
    blocks
        .iter()
        .map(|b| {
            let status = if b.mutated {
                BlockStatus::Mutated
            } else {
                BlockStatus::Active
            };
            BlockInput::new(&b.profile, b.enzymes)
                .with_k_half(b.k_half)
                .with_status(status)
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    /// The net change of each pool is the sum over blocks of turnover x flow.
    /// So every block moves its pools in the ratio of its stoichiometry.
    #[test]
    fn mass_conserved_per_block(
        blocks in prop::collection::vec(random_block(), 1..5),
        pools in random_pools(),
        dt in 0.01f32..1.0,
    ) {
        let result = solve_tick(&pools, &inputs(&blocks), dt);
        for c in Currency::ALL {
            let expected: f32 = blocks
                .iter()
                .zip(&result.turnover)
                .map(|(b, t)| t * b.profile.0.get(&c).copied().unwrap_or(0.0))
                .sum();
            let actual = result.deltas.get(&c).copied().unwrap_or(0.0);
            let scale = 1.0 + pools.get(c) + expected.abs();
            prop_assert!((actual - expected).abs() <= 1e-4 * scale,
                "{:?}: delta {} vs sum of block flows {}", c, actual, expected);
        }
        prop_assert!(result.turnover.iter().all(|&t| t >= 0.0));
    }

    /// No pool goes below zero, whatever the demand.
    #[test]
    fn no_negative_pools(
        blocks in prop::collection::vec(random_block(), 1..5),
        pools in random_pools(),
        dt in 0.01f32..2.0,
    ) {
        let result = solve_tick(&pools, &inputs(&blocks), dt);
        for c in Currency::ALL {
            let after = pools.get(c) + result.deltas.get(&c).copied().unwrap_or(0.0);
            prop_assert!(after >= 0.0, "{:?} went negative: {}", c, after);
        }
    }

    /// One long tick and many short ticks over the same time give nearly the
    /// same pools.
    #[test]
    fn time_scale_consistency(
        blocks in prop::collection::vec(random_block(), 1..4),
        pools in random_pools(),
        total in 0.05f32..1.0,
        splits in 2usize..8,
    ) {
        let input = inputs(&blocks);
        let once = solve_tick(&pools, &input, total);
        let mut stepped = pools.clone();
        for _ in 0..splits {
            let r = solve_tick(&stepped, &input, total / splits as f32);
            for (c, d) in r.deltas {
                stepped.modify(c, d);
            }
        }
        for c in Currency::ALL {
            let a = pools.get(c) + once.deltas.get(&c).copied().unwrap_or(0.0);
            let b = stepped.get(c);
            // Sub-steps differ in size by at most MAX_SUBSTEP, so only the
            // integration error of one sub-step remains.
            let tolerance = 0.02 * (1.0 + pools.get(c).max(a));
            prop_assert!((a - b).abs() <= tolerance, "{:?}: one tick {} vs {} ticks {}", c, a, splits, b);
        }
    }

    /// Adding enzymes to one block never lowers that block's rate.
    #[test]
    fn more_enzymes_never_lower_rate(
        blocks in prop::collection::vec(random_block(), 1..5),
        pools in random_pools(),
        pick in any::<prop::sample::Index>(),
        extra in 0.0f32..10.0,
        h in 0.01f32..0.5,
    ) {
        let i = pick.index(blocks.len());
        let before = block_rates(&pools.pools, &inputs(&blocks), h)[i];
        let mut more = blocks.clone();
        more[i].enzymes += extra;
        let after = block_rates(&pools.pools, &inputs(&more), h)[i];
        prop_assert!(after >= before * (1.0 - 1e-5) - 1e-6, "rate fell from {} to {}", before, after);
    }

    /// Two blocks with the same profile share a scarce input in proportion to
    /// their enzymes.
    #[test]
    fn shared_input_split_by_enzymes(
        a in 0.5f32..10.0,
        b in 0.5f32..10.0,
        supply in 0.01f32..5.0,
        k_half in 0.0f32..10.0,
    ) {
        let mut profile = FluxProfile::default();
        profile.set_demand(Currency::CarbonSkeletons, 2.0);
        profile.set_production(Currency::AminoAcids, 1.0);
        let blocks = [
            BlockInput::new(&profile, a).with_k_half(k_half),
            BlockInput::new(&profile, b).with_k_half(k_half),
        ];
        let pools = CurrencyPools {
            pools: [(Currency::CarbonSkeletons, supply)].into_iter().collect(),
        };
        let r = solve_tick(&pools, &blocks, 1.0);
        prop_assume!(r.turnover[1] > 1e-6);
        let ratio = r.turnover[0] / r.turnover[1];
        prop_assert!((ratio - a / b).abs() <= 1e-3 * (a / b), "ratio {} vs enzymes {}", ratio, a / b);
    }
}
