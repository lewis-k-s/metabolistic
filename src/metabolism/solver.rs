//! # Pure flux solver
//!
//! `solve_tick` runs one metabolic tick for one cell. It is a pure function:
//! the Bevy systems in `metabolism` fetch the data and apply the result.
//!
//! ## Rate law
//!
//! A block's `FluxProfile` gives its flows per second for one enzyme at full
//! saturation. Its wanted rate (turnovers per second) is:
//!
//! ```text
//! demand = enzymes x throughput(status) x drive x external x min(S / (K + S))
//! ```
//!
//! The minimum is over the block's inputs and cofactors, with the block's
//! half-saturation `K`. This is a simplified Michaelis-Menten rate: enzymes
//! scale the rate linearly, and the substrate supply sets the ceiling.
//!
//! ## Sharing
//!
//! All blocks see the same pools at the start of a step, so the block order has
//! no effect. When the total demand for a pool is more than the pool holds,
//! each consumer gets a share in proportion to its demand. A block runs at the
//! smallest fraction that its shares allow, and all its flows scale by that
//! fraction. Thus each block keeps its stoichiometry, and pools never go
//! negative.
//!
//! ## Tick length
//!
//! Each sub-step uses the midpoint method (see `step_rates`), because the
//! saturation `S / (K + S)` falls fast when a pool drains.
//!
//! `solve_tick` splits a tick into equal sub-steps of at most `MAX_SUBSTEP`
//! seconds. Ticks of 0.1 s, 0.2 s or 0.8 s all use 0.05 s steps, so the result
//! does not depend on the tick length.

use std::collections::HashMap;

use super::{BlockStatus, CurrencyPools, FluxProfile};
use crate::environment::{Channel, saturation};
use crate::molecules::Currency;

/// Largest sub-step of `solve_tick`, in seconds.
pub const MAX_SUBSTEP: f32 = 0.05;

/// One block as the solver sees it.
#[derive(Debug, Clone, Copy)]
pub struct BlockInput<'a> {
    /// Flows per second for one enzyme at full saturation.
    pub profile: &'a FluxProfile,
    /// Number of enzymes allocated to the block.
    pub enzymes: f32,
    /// Gene state of the block.
    pub status: BlockStatus,
    /// Regulation multiplier (0 = off). See `Drive`.
    pub drive: f32,
    /// Half-saturation constant `K` for inputs and cofactors.
    pub k_half: f32,
    /// Pools that the block needs but does not use up.
    pub cofactors: &'a [Currency],
    /// Saturation in [0, 1] from outside channels. 1 if the block has none.
    pub external: f32,
    /// The outside channel with the lowest saturation. Only `block_limit` reads
    /// it, to name the channel. `None` if the block has no channels.
    pub external_channel: Option<Channel>,
}

impl<'a> BlockInput<'a> {
    /// An active block with `enzymes`, no cofactors, and `K = 0`.
    pub fn new(profile: &'a FluxProfile, enzymes: f32) -> Self {
        Self {
            profile,
            enzymes,
            status: BlockStatus::Active,
            drive: 1.0,
            k_half: 0.0,
            cofactors: &[],
            external: 1.0,
            external_channel: None,
        }
    }

    pub fn with_k_half(mut self, k_half: f32) -> Self {
        self.k_half = k_half;
        self
    }

    pub fn with_status(mut self, status: BlockStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_cofactors(mut self, cofactors: &'a [Currency]) -> Self {
        self.cofactors = cofactors;
        self
    }
}

/// Result of one tick for one cell.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TickResult {
    /// Net change of each pool over the tick.
    pub deltas: HashMap<Currency, f32>,
    /// Turnovers of each block over the tick, in the input order.
    /// A block's flow of a pool is `turnover x profile[pool]`.
    pub turnover: Vec<f32>,
}

/// Saturation of a block from its inputs and cofactors: the lowest `S / (K + S)`.
fn internal_saturation(pools: &HashMap<Currency, f32>, block: &BlockInput) -> f32 {
    let level = |c: &Currency| pools.get(c).copied().unwrap_or(0.0);
    block
        .profile
        .0
        .iter()
        .filter(|&(_, &rate)| rate < 0.0)
        .map(|(c, _)| c)
        .chain(block.cofactors)
        .map(|c| saturation(level(c), block.k_half))
        .fold(1.0, f32::min)
}

/// The rate (turnovers per second) that a block wants, before sharing.
pub fn block_demand(pools: &HashMap<Currency, f32>, block: &BlockInput) -> f32 {
    let scale = block.enzymes.max(0.0)
        * block.status.throughput()
        * block.drive.max(0.0)
        * block.external.clamp(0.0, 1.0);
    if scale <= 0.0 {
        return 0.0;
    }
    scale * internal_saturation(pools, block)
}

/// Fraction of the wanted amount that each pool can give in a step of `h`
/// seconds. Only pools that some block wants are in the map.
fn pool_fractions(
    pools: &HashMap<Currency, f32>,
    blocks: &[BlockInput],
    demand: &[f32],
    h: f32,
) -> HashMap<Currency, f32> {
    // Total amount of each pool that the blocks want in this step.
    let mut wanted: HashMap<Currency, f32> = HashMap::new();
    for (block, &d) in blocks.iter().zip(demand) {
        for (&c, &rate) in &block.profile.0 {
            if rate < 0.0 && d > 0.0 {
                *wanted.entry(c).or_insert(0.0) += -rate * d * h;
            }
        }
    }
    wanted
        .into_iter()
        .map(|(c, want)| {
            let have = pools.get(&c).copied().unwrap_or(0.0).max(0.0);
            let f = if want > have { have / want } else { 1.0 };
            (c, f)
        })
        .collect()
}

/// The rates (turnovers per second) of all blocks for a step of `h` seconds,
/// after shared pools are divided in proportion to demand.
pub fn block_rates(pools: &HashMap<Currency, f32>, blocks: &[BlockInput], h: f32) -> Vec<f32> {
    rates_at(pools, pools, blocks, h)
}

/// Like `block_rates`, but the demand is read from `seen` and the pools that
/// the step may spend are `have`. The midpoint step uses this: it asks what
/// the blocks want at the middle of the step, and caps that by the pools at
/// the start of the step.
fn rates_at(
    seen: &HashMap<Currency, f32>,
    have: &HashMap<Currency, f32>,
    blocks: &[BlockInput],
    h: f32,
) -> Vec<f32> {
    let demand: Vec<f32> = blocks.iter().map(|b| block_demand(seen, b)).collect();
    let fraction = pool_fractions(have, blocks, &demand, h);

    blocks
        .iter()
        .zip(&demand)
        .map(|(block, &d)| {
            if d <= 0.0 {
                return 0.0;
            }
            let f = block
                .profile
                .0
                .iter()
                .filter(|&(_, &rate)| rate < 0.0)
                .map(|(c, _)| fraction[c])
                .fold(1.0, f32::min);
            d * f
        })
        .collect()
}

/// Why a block runs below its full rate. See `block_limit`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Limit {
    /// No limit: every factor is at `LIMIT_THRESHOLD` or more.
    None,
    /// The gene is silent, or the drive (regulation, action level) is 0.
    Off,
    /// The cell has no enzymes for the block.
    NoEnzymes,
    /// The outside channel is low (for example no heat).
    External(Channel),
    /// An input pool is low.
    Input(Currency),
    /// A cofactor pool is low. The block needs it but does not use it up.
    Cofactor(Currency),
    /// The pool cannot pay all blocks that want it, so the block gets a share.
    Shared(Currency),
}

/// A factor at or above this value is not a limit.
pub const LIMIT_THRESHOLD: f32 = 0.9;

/// The factors that slow a block: the lowest outside, input and cofactor
/// saturation. Returns the limit of the lowest one, and its value. Does not
/// look at sharing.
fn own_limit(pools: &HashMap<Currency, f32>, block: &BlockInput) -> (Limit, f32) {
    let level = |c: &Currency| pools.get(c).copied().unwrap_or(0.0);
    let mut best = (Limit::None, 1.0_f32);
    if let Some(channel) = block.external_channel
        && block.external < best.1
    {
        best = (Limit::External(channel), block.external);
    }
    let inputs = block
        .profile
        .0
        .iter()
        .filter(|&(_, &rate)| rate < 0.0)
        .map(|(c, _)| *c);
    // A fixed order, so that ties do not depend on HashMap order.
    let mut currencies: Vec<Currency> = inputs.collect();
    currencies.sort();
    currencies.extend(block.cofactors);
    for c in currencies {
        let s = saturation(level(&c), block.k_half);
        if s < best.1 {
            let limit = if block.cofactors.contains(&c) {
                Limit::Cofactor(c)
            } else {
                Limit::Input(c)
            };
            best = (limit, s);
        }
    }
    best
}

/// The reason that one block runs below its full rate, from the pools alone.
/// It uses the terms of `block_demand`: enzymes, status, drive, outside
/// saturation and the lowest input or cofactor saturation. It cannot see
/// other blocks, so it never says `Shared`; use `block_limits` for that.
pub fn block_limit(pools: &HashMap<Currency, f32>, block: &BlockInput) -> Limit {
    if block.status.throughput() <= 0.0 || block.drive <= 0.0 {
        return Limit::Off;
    }
    if block.enzymes <= 0.0 {
        return Limit::NoEnzymes;
    }
    if block.external <= 0.0 && block.external_channel.is_none() {
        return Limit::Off; // an outside factor of 0 with no channel to name
    }
    let (limit, value) = own_limit(pools, block);
    if value < LIMIT_THRESHOLD {
        limit
    } else {
        Limit::None
    }
}

/// The limit of each block, including `Shared`. `h` is the step length of
/// `block_rates` (use `MAX_SUBSTEP`). The weakest factor of a block names its
/// limit: its own saturation, or its share of a pool that other blocks want
/// too. A pool that only this block wants is `Input`, not `Shared`.
pub fn block_limits(pools: &HashMap<Currency, f32>, blocks: &[BlockInput], h: f32) -> Vec<Limit> {
    let demand: Vec<f32> = blocks.iter().map(|b| block_demand(pools, b)).collect();
    let fraction = pool_fractions(pools, blocks, &demand, h);
    blocks
        .iter()
        .zip(&demand)
        .map(|(block, &d)| {
            let own = block_limit(pools, block);
            if d <= 0.0 {
                return own;
            }
            let (_, own_value) = own_limit(pools, block);
            let mut inputs: Vec<Currency> = block.profile.get_demanded_currencies();
            inputs.sort();
            let share = inputs
                .into_iter()
                .map(|c| (c, fraction.get(&c).copied().unwrap_or(1.0)))
                .fold(None::<(Currency, f32)>, |best, (c, f)| match best {
                    Some((_, b)) if b <= f => best,
                    _ => Some((c, f)),
                });
            match share {
                Some((c, f)) if f < LIMIT_THRESHOLD && f < own_value => {
                    let demanders = blocks
                        .iter()
                        .zip(&demand)
                        .filter(|(b, bd)| **bd > 0.0 && b.profile.has_demand_for(c))
                        .count();
                    if demanders > 1 {
                        Limit::Shared(c)
                    } else {
                        Limit::Input(c)
                    }
                }
                _ => own,
            }
        })
        .collect()
}

/// Number of equal sub-steps for a tick of `dt` seconds.
fn substeps(dt: f32) -> u32 {
    // The small margin stops float error from adding a step (0.2 / 0.05 > 4).
    ((dt / MAX_SUBSTEP) - 1e-4).ceil().max(1.0) as u32
}

/// The rates for one sub-step of `h` seconds. The rate law is not linear
/// (`S / (K + S)`), so a rate from the start of the step is too high when a
/// pool falls fast. Thus the step uses the midpoint method: a half step gives
/// the pools at the middle of the step, and the demand there is the rate for
/// the whole step. The error is then second order in `h`, and a long tick and
/// many short ticks agree. Pools never go negative: the shares are capped by
/// the pools at the start of the step.
///
/// If a pool cannot pay the whole step, the step empties it (the share rule
/// sets the rates), and the plain rates are used: the midpoint would only
/// leave a rest in the pool.
fn step_rates(working: &HashMap<Currency, f32>, blocks: &[BlockInput], h: f32) -> Vec<f32> {
    let plain = block_rates(working, blocks, h);
    let exhausts = blocks
        .iter()
        .zip(&plain)
        .any(|(block, &rate)| rate < block_demand(working, block));
    if exhausts {
        return plain;
    }
    let mut middle = working.clone();
    for (block, rate) in blocks.iter().zip(&plain) {
        for (&c, &flow) in &block.profile.0 {
            *middle.entry(c).or_insert(0.0) += flow * rate * h / 2.0;
        }
    }
    rates_at(&middle, working, blocks, h)
}

/// Runs one tick of `dt` seconds over `blocks`. See the module docs.
pub fn solve_tick(pools: &CurrencyPools, blocks: &[BlockInput], dt: f32) -> TickResult {
    let mut working: HashMap<Currency, f32> = pools.pools.clone();
    let mut turnover = vec![0.0; blocks.len()];
    if dt <= 0.0 || blocks.is_empty() {
        return TickResult {
            deltas: HashMap::new(),
            turnover,
        };
    }

    let n = substeps(dt);
    let h = dt / n as f32;
    for _ in 0..n {
        let rates = step_rates(&working, blocks, h);
        for ((block, rate), total) in blocks.iter().zip(&rates).zip(&mut turnover) {
            if *rate <= 0.0 {
                continue;
            }
            *total += rate * h;
            for (&c, &flow) in &block.profile.0 {
                *working.entry(c).or_insert(0.0) += flow * rate * h;
            }
        }
        // The shares never take more than a pool holds. This only removes
        // float error.
        for amount in working.values_mut() {
            *amount = amount.max(0.0);
        }
    }

    let deltas = working
        .iter()
        .filter_map(|(&c, &after)| {
            let delta = after - pools.get(c);
            (delta != 0.0).then_some((c, delta))
        })
        .collect();
    TickResult { deltas, turnover }
}

/// Returns a display order for blocks: producers of a currency before its
/// consumers. Blocks in a cycle keep their input order. The result is a
/// permutation of `0..profiles.len()`. The solver does not use an order.
pub fn solve_order(profiles: &[&FluxProfile]) -> Vec<usize> {
    let n = profiles.len();
    let deps = dependency_lists(profiles);
    let mut order = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut on_stack = vec![false; n];

    fn visit(
        i: usize,
        deps: &[Vec<usize>],
        visited: &mut [bool],
        on_stack: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        if visited[i] || on_stack[i] {
            return; // done, or a cycle: keep input order
        }
        on_stack[i] = true;
        for &d in &deps[i] {
            visit(d, deps, visited, on_stack, order);
        }
        on_stack[i] = false;
        visited[i] = true;
        order.push(i);
    }

    for i in 0..n {
        visit(i, &deps, &mut visited, &mut on_stack, &mut order);
    }
    order
}

/// For each block, the blocks that produce at least one currency it consumes.
pub fn dependency_lists(profiles: &[&FluxProfile]) -> Vec<Vec<usize>> {
    profiles
        .iter()
        .enumerate()
        .map(|(i, consumer)| {
            let inputs = consumer.get_demanded_currencies();
            (0..profiles.len())
                .filter(|&j| j != i && inputs.iter().any(|&c| profiles[j].produces(c)))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(entries: &[(Currency, f32)]) -> FluxProfile {
        FluxProfile(entries.iter().copied().collect())
    }

    fn pools(entries: &[(Currency, f32)]) -> CurrencyPools {
        CurrencyPools {
            pools: entries.iter().copied().collect(),
        }
    }

    #[test]
    fn full_rate_when_saturated_and_inputs_suffice() {
        let p = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let r = solve_tick(
            &pools(&[(Currency::Pyruvate, 10.0)]),
            &[BlockInput::new(&p, 2.0)],
            0.5,
        );
        assert!((r.turnover[0] - 1.0).abs() < 1e-5);
        assert!((r.deltas[&Currency::Pyruvate] + 1.0).abs() < 1e-5);
        assert!((r.deltas[&Currency::ATP] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn saturation_slows_rate_at_low_substrate() {
        let p = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let block = BlockInput::new(&p, 1.0).with_k_half(10.0);
        let start = pools(&[(Currency::Pyruvate, 10.0)]);
        let rate = block_rates(&start.pools, &[block], 0.01)[0];
        assert!((rate - 0.5).abs() < 1e-6, "S = K gives half rate: {rate}");
    }

    #[test]
    fn limiting_input_throttles_all_flows() {
        let p = profile(&[
            (Currency::Pyruvate, -2.0),
            (Currency::ReducingPower, -2.0),
            (Currency::ATP, 2.0),
        ]);
        let start = pools(&[(Currency::Pyruvate, 0.5), (Currency::ReducingPower, 10.0)]);
        let r = solve_tick(&start, &[BlockInput::new(&p, 1.0)], 1.0);
        assert!((r.deltas[&Currency::Pyruvate] + 0.5).abs() < 1e-5);
        assert!((r.deltas[&Currency::ReducingPower] + 0.5).abs() < 1e-5);
        assert!((r.deltas[&Currency::ATP] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn status_scales_throughput() {
        let p = profile(&[(Currency::ATP, 1.0)]);
        let empty = CurrencyPools::default();
        let mutated = solve_tick(
            &empty,
            &[BlockInput::new(&p, 1.0).with_status(BlockStatus::Mutated)],
            1.0,
        );
        let silent = solve_tick(
            &empty,
            &[BlockInput::new(&p, 1.0).with_status(BlockStatus::Silent)],
            1.0,
        );
        assert!((mutated.deltas[&Currency::ATP] - 0.5).abs() < 1e-5);
        assert!(silent.deltas.is_empty());
    }

    #[test]
    fn no_enzymes_no_flux() {
        let p = profile(&[(Currency::ATP, 1.0)]);
        let r = solve_tick(&CurrencyPools::default(), &[BlockInput::new(&p, 0.0)], 1.0);
        assert!(r.deltas.is_empty());
        assert_eq!(r.turnover, vec![0.0]);
    }

    #[test]
    fn cofactor_is_needed_but_not_used_up() {
        let p = profile(&[(Currency::ATP, 1.0)]);
        let cofactors = [Currency::Metals];
        let block = BlockInput::new(&p, 1.0)
            .with_k_half(1.0)
            .with_cofactors(&cofactors);
        let none = solve_tick(&CurrencyPools::default(), &[block], 1.0);
        assert!(none.deltas.is_empty());
        let some = solve_tick(&pools(&[(Currency::Metals, 1.0)]), &[block], 1.0);
        assert!((some.deltas[&Currency::ATP] - 0.5).abs() < 1e-5);
        assert!(!some.deltas.contains_key(&Currency::Metals));
    }

    #[test]
    fn shared_input_is_split_in_proportion_to_enzymes() {
        // Same profile, 3 and 1 enzymes, a scarce shared input.
        let p = profile(&[(Currency::CarbonSkeletons, -1.0), (Currency::ATP, 1.0)]);
        let blocks = [BlockInput::new(&p, 3.0), BlockInput::new(&p, 1.0)];
        let start = pools(&[(Currency::CarbonSkeletons, 1.0)]);
        let r = solve_tick(&start, &blocks, 1.0);
        assert!((r.turnover[0] / r.turnover[1] - 3.0).abs() < 1e-4);
        assert!(r.deltas[&Currency::CarbonSkeletons] >= -1.0 - 1e-5);
    }

    #[test]
    fn block_order_has_no_effect() {
        let a = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let b = profile(&[(Currency::Pyruvate, -2.0), (Currency::Waste, 1.0)]);
        let start = pools(&[(Currency::Pyruvate, 0.3)]);
        let ab = solve_tick(
            &start,
            &[BlockInput::new(&a, 1.0), BlockInput::new(&b, 1.0)],
            1.0,
        );
        let ba = solve_tick(
            &start,
            &[BlockInput::new(&b, 1.0), BlockInput::new(&a, 1.0)],
            1.0,
        );
        // Equal up to float summation order.
        assert!((ab.turnover[0] - ba.turnover[1]).abs() < 1e-6);
        assert!((ab.turnover[1] - ba.turnover[0]).abs() < 1e-6);
    }

    #[test]
    fn long_tick_matches_short_ticks_when_saturation_is_steep() {
        // Waste is only consumed, at S / (K + S) with a small K and a high
        // rate. One Euler step of 0.05 s over-drains the pool; the midpoint
        // step must match three shorter steps.
        let p = profile(&[(Currency::Waste, -3.554)]);
        let block = BlockInput::new(&p, 5.27).with_k_half(1.4);
        let start = pools(&[(Currency::Waste, 0.5589)]);
        let once = start.get(Currency::Waste)
            + solve_tick(&start, &[block], 0.05).deltas[&Currency::Waste];
        let mut stepped = start.clone();
        for _ in 0..3 {
            let r = solve_tick(&stepped, &[block], 0.05 / 3.0);
            stepped.modify(Currency::Waste, r.deltas[&Currency::Waste]);
        }
        let diff = (once - stepped.get(Currency::Waste)).abs();
        // Forward Euler differed by 0.031 here.
        assert!(
            diff < 0.01,
            "one tick {once} vs three ticks {}",
            stepped.get(Currency::Waste)
        );
    }

    #[test]
    fn substeps_are_exact_for_tick_multiples() {
        assert_eq!(substeps(0.05), 1);
        assert_eq!(substeps(0.1), 2);
        assert_eq!(substeps(0.2), 4);
        assert_eq!(substeps(0.8), 16);
        assert_eq!(substeps(0.01), 1);
    }

    #[test]
    fn producers_sort_before_consumers() {
        let consumer = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let producer = profile(&[(Currency::Pyruvate, 1.0)]);
        assert_eq!(solve_order(&[&consumer, &producer]), vec![1, 0]);
    }

    #[test]
    fn cycles_do_not_hang() {
        let a = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let b = profile(&[(Currency::ATP, -1.0), (Currency::Pyruvate, 1.0)]);
        let mut order = solve_order(&[&a, &b]);
        order.sort();
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn pools_never_go_negative() {
        let p = profile(&[(Currency::ATP, -100.0)]);
        let start = pools(&[(Currency::ATP, 3.0)]);
        let r = solve_tick(&start, &[BlockInput::new(&p, 1.0)], 1.0);
        assert!((r.deltas[&Currency::ATP] + 3.0).abs() < 1e-5);
    }
}
