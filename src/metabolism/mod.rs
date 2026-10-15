//! # Metabolic flux engine
//!
//! Each cell owns metabolic blocks. Each block has a `FluxProfile`: the rate (per
//! second) at which it consumes (negative) and produces (positive) currencies at
//! full activity.
//!
//! The engine runs in `MetabolicSchedule`, one fixed tick at a time. A tick:
//!
//! 1. `MetabolicSet::Genome`: random mutations.
//! 2. `MetabolicSet::Regulate`: regulated blocks (for example fat storage) set
//!    their flux profile from the cell's current pools.
//! 3. `MetabolicSet::Solve`: for each cell, `solve_tick` runs the blocks in
//!    dependency order. Each block runs at the largest activity in [0, 1] that its
//!    inputs allow. Mass is conserved per block, and pools never go negative.
//! 4. `MetabolicSet::Exchange`: the cell exchanges waste and nutrients with the
//!    environment.
//!
//! In the game, `run_metabolic_schedule_timed` runs ticks from real time. Tests
//! set `FlowmapTimingConfig::manual` and call `run_metabolic_tick` directly.

use std::collections::HashMap;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState};
use crate::cell::{Cell, CellBlocks};
pub use crate::molecules::CurrencyPools; // Re-export for backward compatibility
use crate::molecules::{Currency, EnvironmentalCurrencyPools};

// --- Components ---

/// Marker component for entities that are part of the metabolic system.
#[derive(Component, Default)]
pub struct MetabolicBlock;

/// Defines the flux profile (production/consumption) of a metabolic block.
/// Keys are Currency types, values are flux amounts (positive for production, negative for consumption).
#[derive(Component, Default, Debug, Clone)]
pub struct FluxProfile(pub HashMap<Currency, f32>);

impl FluxProfile {
    /// Set demand (consumption) for a specific currency
    /// Demand is represented as a negative value in the flux profile
    pub fn set_demand(&mut self, currency: Currency, amount: f32) {
        self.0.insert(currency, -amount.abs());
    }

    /// Set production for a specific currency
    /// Production is represented as a positive value in the flux profile
    pub fn set_production(&mut self, currency: Currency, amount: f32) {
        self.0.insert(currency, amount.abs());
    }

    /// Check if this profile has demand (consumption) for a given currency
    /// Returns true if the currency exists and has a negative value
    pub fn has_demand_for(&self, currency: Currency) -> bool {
        self.0.get(&currency).is_some_and(|&amount| amount < 0.0)
    }

    /// Get the demand amount for a specific currency
    /// Returns the absolute value of consumption, or 0.0 if no demand
    pub fn get_demand(&self, currency: Currency) -> f32 {
        self.0
            .get(&currency)
            .map(|&amount| if amount < 0.0 { -amount } else { 0.0 })
            .unwrap_or(0.0)
    }

    /// Check if this profile produces a given currency
    /// Returns true if the currency exists and has a positive value
    pub fn produces(&self, currency: Currency) -> bool {
        self.0.get(&currency).is_some_and(|&amount| amount > 0.0)
    }

    /// Get the production amount for a specific currency
    /// Returns the production amount, or 0.0 if no production
    pub fn get_production(&self, currency: Currency) -> f32 {
        self.0
            .get(&currency)
            .map(|&amount| if amount > 0.0 { amount } else { 0.0 })
            .unwrap_or(0.0)
    }

    /// Get all currencies this profile has demand for
    pub fn get_demanded_currencies(&self) -> Vec<Currency> {
        self.0
            .iter()
            .filter_map(
                |(&currency, &amount)| {
                    if amount < 0.0 { Some(currency) } else { None }
                },
            )
            .collect()
    }

    /// Get all currencies this profile produces
    pub fn get_produced_currencies(&self) -> Vec<Currency> {
        self.0
            .iter()
            .filter_map(
                |(&currency, &amount)| {
                    if amount > 0.0 { Some(currency) } else { None }
                },
            )
            .collect()
    }

    /// Clear all flux values
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Scale all flux values by a factor
    pub fn scale(&mut self, factor: f32) {
        for amount in self.0.values_mut() {
            *amount *= factor;
        }
    }
}

// --- Resources ---

/// Configuration for metabolic tick timing
#[derive(Resource, Clone)]
pub struct FlowmapTimingConfig {
    /// Metabolic ticks per second (default: 5.0). Each tick simulates
    /// `target_delta()` seconds.
    pub updates_per_second: f32,
    /// Accumulated time since last metabolic update
    pub accumulator: f32,
    /// When true, real time does not drive ticks. Call `run_metabolic_tick`
    /// instead. Tests use this for deterministic results.
    pub manual: bool,
}

impl Default for FlowmapTimingConfig {
    fn default() -> Self {
        Self {
            updates_per_second: 5.0, // 5 updates per second
            accumulator: 0.0,
            manual: false,
        }
    }
}

impl FlowmapTimingConfig {
    /// Create config with custom update rate
    pub fn new(updates_per_second: f32) -> Self {
        Self {
            updates_per_second: updates_per_second.max(0.1), // Minimum 0.1 Hz
            ..default()
        }
    }

    /// Config for tests and tools: ticks run only on `run_metabolic_tick`.
    pub fn manual() -> Self {
        Self {
            manual: true,
            ..default()
        }
    }

    /// Get the target delta time between updates
    pub fn target_delta(&self) -> f32 {
        1.0 / self.updates_per_second
    }

    /// Check if enough time has accumulated for an update
    pub fn should_update(&self) -> bool {
        self.accumulator >= self.target_delta()
    }

    /// Consume accumulated time for one update cycle
    pub fn consume_update(&mut self) {
        self.accumulator -= self.target_delta();
        // Keep any remaining time for next cycle
        self.accumulator = self.accumulator.max(0.0);
    }

    /// Add time to the accumulator
    pub fn add_time(&mut self, delta: f32) {
        self.accumulator += delta;
    }

    /// Set new update rate
    pub fn set_rate(&mut self, updates_per_second: f32) {
        self.updates_per_second = updates_per_second.max(0.1);
    }
}

/// The solve order of a cell's blocks from the last metabolic tick.
/// Kept on the cell for the flowmap and debug views.
#[derive(Component, Default, Clone, Debug)]
pub struct MetabolicGraph {
    /// Block entities in the order the solver ran them
    pub nodes: Vec<Entity>,
    /// block -> blocks that produce something it consumes
    pub dependencies: HashMap<Entity, Vec<Entity>>,
}

/// Status of a metabolic block, derived from genome expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockStatus {
    Active,
    Mutated,
    Silent,
}

impl BlockStatus {
    /// Fraction of full throughput that a block with this status can run at.
    pub fn throughput(self) -> f32 {
        match self {
            BlockStatus::Active => 1.0,
            BlockStatus::Mutated => 0.5,
            BlockStatus::Silent => 0.0,
        }
    }
}

impl From<GeneState> for BlockStatus {
    fn from(gene_state: GeneState) -> Self {
        match gene_state {
            GeneState::Expressed => BlockStatus::Active,
            GeneState::Mutated => BlockStatus::Mutated,
            GeneState::Silent => BlockStatus::Silent,
        }
    }
}

/// Component for a node in the metabolic graph.
#[derive(Component, Debug, Clone, Copy)]
pub struct MetabolicNode {
    pub kind: BlockKind,
    pub status: BlockStatus,
}

// --- Pure solver ---

/// Returns the order in which to run blocks: producers of a currency before its
/// consumers. Blocks in a cycle keep their input order. The result is a
/// permutation of `0..profiles.len()`.
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

/// Result of one metabolic tick for one cell.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TickResult {
    /// Net change of each currency over the tick
    pub deltas: HashMap<Currency, f32>,
    /// Activity in [0, 1] of each block, in the same order as the input
    pub activity: Vec<f32>,
}

/// Runs one tick of `dt` seconds over `blocks`, in the order given.
///
/// Each block runs at `status.throughput()` times its full rate, reduced to the
/// largest fraction that the current pools can supply. All of a block's flows
/// scale by the same fraction, so each block conserves its stoichiometry. Blocks
/// later in the order see the products of earlier blocks.
pub fn solve_tick(
    pools: &CurrencyPools,
    blocks: &[(BlockStatus, &FluxProfile)],
    dt: f32,
) -> TickResult {
    let mut working: HashMap<Currency, f32> = pools.pools.clone();
    let mut activity = Vec::with_capacity(blocks.len());

    for (status, profile) in blocks {
        let scale = status.throughput() * dt;
        if scale <= 0.0 {
            activity.push(0.0);
            continue;
        }

        let mut fraction = 1.0f32;
        for (currency, &rate) in &profile.0 {
            if rate < 0.0 {
                let need = -rate * scale;
                let available = working.get(currency).copied().unwrap_or(0.0).max(0.0);
                fraction = fraction.min(available / need);
            }
        }
        let fraction = fraction.clamp(0.0, 1.0);
        activity.push(fraction);
        if fraction == 0.0 {
            continue;
        }

        for (currency, &rate) in &profile.0 {
            let entry = working.entry(*currency).or_insert(0.0);
            // Guard against float error taking a pool below zero.
            *entry = (*entry + rate * scale * fraction).max(0.0);
        }
    }

    let mut deltas = HashMap::new();
    for (currency, &after) in &working {
        let before = pools.get(*currency);
        let delta = after - before;
        if delta != 0.0 {
            deltas.insert(*currency, delta);
        }
    }
    TickResult { deltas, activity }
}

// --- Schedule ---

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetabolicSchedule;

/// Ordered phases of a metabolic tick.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetabolicSet {
    Genome,
    Regulate,
    Solve,
    Exchange,
}

/// Runs one metabolic tick now. Use with `FlowmapTimingConfig::manual()`.
pub fn run_metabolic_tick(world: &mut World) {
    world.run_schedule(MetabolicSchedule);
}

/// Most ticks to run in one frame. Stops a slow frame from causing a spiral.
const MAX_TICKS_PER_FRAME: u32 = 4;

/// Runs metabolic ticks at `FlowmapTimingConfig::updates_per_second` of real time.
fn run_metabolic_schedule_timed(world: &mut World) {
    let delta_time = world.resource::<Time>().delta_secs();
    let mut ticks = 0;
    {
        let mut timing = world.resource_mut::<FlowmapTimingConfig>();
        if timing.manual {
            return;
        }
        timing.add_time(delta_time);
        while timing.should_update() && ticks < MAX_TICKS_PER_FRAME {
            timing.consume_update();
            ticks += 1;
        }
    }
    for _ in 0..ticks {
        world.run_schedule(MetabolicSchedule);
    }
}

// --- Systems ---

/// Solves one tick for every cell and applies the result to its pools.
pub fn solve_cell_flux_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&CellBlocks, &mut CurrencyPools, &mut MetabolicGraph), With<Cell>>,
    blocks: Query<(&MetabolicNode, &FluxProfile)>,
) {
    let dt = timing.target_delta();
    for (cell_blocks, mut pools, mut graph) in &mut cells {
        // Sort by kind so the result does not depend on HashMap order.
        let mut owned: Vec<(Entity, BlockStatus, &FluxProfile, BlockKind)> = cell_blocks
            .0
            .values()
            .filter_map(|&e| blocks.get(e).ok().map(|(n, p)| (e, n.status, p, n.kind)))
            .collect();
        owned.sort_by_key(|b| b.3);

        let profiles: Vec<&FluxProfile> = owned.iter().map(|b| b.2).collect();
        let order = solve_order(&profiles);
        let deps = dependency_lists(&profiles);
        let ordered: Vec<(BlockStatus, &FluxProfile)> =
            order.iter().map(|&i| (owned[i].1, owned[i].2)).collect();

        let result = solve_tick(&pools, &ordered, dt);
        for (currency, delta) in result.deltas {
            pools.modify(currency, delta);
        }

        graph.nodes = order.iter().map(|&i| owned[i].0).collect();
        graph.dependencies = deps
            .iter()
            .enumerate()
            .map(|(i, d)| (owned[i].0, d.iter().map(|&j| owned[j].0).collect()))
            .collect();
    }
}

// Exchange rates with the environment. Placeholder tuning values, per second.
const WASTE_LEAK_THRESHOLD: f32 = 20.0;
const WASTE_LEAK_FRACTION_PER_SEC: f32 = 0.5;
const CARBON_ABSORB_BELOW: f32 = 10.0;
const CARBON_ABSORB_RATE: f32 = 10.0;
const PYRUVATE_ABSORB_BELOW: f32 = 5.0;
const PYRUVATE_ABSORB_RATE: f32 = 5.0;
const ENV_WASTE_DECAY_FRACTION_PER_SEC: f32 = 0.1;
const ENV_WASTE_TO_CARBON: f32 = 0.3;
const ENV_CARBON_CAP: f32 = 100.0;
const ENV_CARBON_REPLENISH: f32 = 5.0;
const ENV_PYRUVATE_CAP: f32 = 20.0;
const ENV_PYRUVATE_REPLENISH: f32 = 2.5;

/// Fraction of a pool lost in `dt` seconds at a continuous rate `k` per second.
/// Exact for any tick length, unlike `k * dt`.
fn decay_fraction(k: f32, dt: f32) -> f32 {
    1.0 - (-k * dt).exp()
}

/// Cells leak organic waste into the environment when it builds up.
pub fn cell_waste_export_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<&mut CurrencyPools, With<Cell>>,
    mut env_pools: ResMut<EnvironmentalCurrencyPools>,
) {
    let dt = timing.target_delta();
    for mut pools in &mut cells {
        let waste = pools.get(Currency::OrganicWaste);
        if waste > WASTE_LEAK_THRESHOLD {
            let amount = waste * decay_fraction(WASTE_LEAK_FRACTION_PER_SEC, dt);
            pools.modify(Currency::OrganicWaste, -amount);
            env_pools.modify(Currency::OrganicWaste, amount);
        }
    }
}

/// Cells take up carbon skeletons and pyruvate from the environment when low.
pub fn cell_nutrient_absorption_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<&mut CurrencyPools, With<Cell>>,
    mut env_pools: ResMut<EnvironmentalCurrencyPools>,
) {
    let dt = timing.target_delta();
    for mut pools in &mut cells {
        for (currency, below, rate) in [
            (
                Currency::CarbonSkeletons,
                CARBON_ABSORB_BELOW,
                CARBON_ABSORB_RATE,
            ),
            (
                Currency::Pyruvate,
                PYRUVATE_ABSORB_BELOW,
                PYRUVATE_ABSORB_RATE,
            ),
        ] {
            let gap = below - pools.get(currency);
            if gap > 0.0 {
                // Cap at the gap so the result does not depend on tick length.
                let amount = (rate * dt).min(gap).min(env_pools.get(currency));
                if amount > 0.0 {
                    pools.modify(currency, amount);
                    env_pools.modify(currency, -amount);
                }
            }
        }
    }
}

/// Environmental waste decays into carbon, and outside nutrients replenish slowly.
pub fn environmental_currency_processing_system(
    timing: Res<FlowmapTimingConfig>,
    mut env_pools: ResMut<EnvironmentalCurrencyPools>,
) {
    let dt = timing.target_delta();
    let waste = env_pools.get(Currency::OrganicWaste);
    if waste > 0.0 {
        let decay = waste * decay_fraction(ENV_WASTE_DECAY_FRACTION_PER_SEC, dt);
        env_pools.modify(Currency::OrganicWaste, -decay);
        env_pools.modify(Currency::CarbonSkeletons, decay * ENV_WASTE_TO_CARBON);
    }
    if env_pools.get(Currency::CarbonSkeletons) < ENV_CARBON_CAP {
        env_pools.modify(Currency::CarbonSkeletons, ENV_CARBON_REPLENISH * dt);
    }
    if env_pools.get(Currency::Pyruvate) < ENV_PYRUVATE_CAP {
        env_pools.modify(Currency::Pyruvate, ENV_PYRUVATE_REPLENISH * dt);
    }
}

// --- Plugin ---

pub struct MetabolicFlowPlugin;

impl Plugin for MetabolicFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlowmapTimingConfig>()
            .init_schedule(MetabolicSchedule)
            .configure_sets(
                MetabolicSchedule,
                (
                    MetabolicSet::Genome,
                    MetabolicSet::Regulate,
                    MetabolicSet::Solve,
                    MetabolicSet::Exchange,
                )
                    .chain(),
            )
            .add_systems(
                MetabolicSchedule,
                solve_cell_flux_system.in_set(MetabolicSet::Solve),
            )
            .add_systems(
                MetabolicSchedule,
                (
                    cell_waste_export_system,
                    cell_nutrient_absorption_system,
                    environmental_currency_processing_system,
                )
                    .chain()
                    .in_set(MetabolicSet::Exchange)
                    .run_if(resource_exists::<EnvironmentalCurrencyPools>),
            )
            .add_systems(Update, run_metabolic_schedule_timed)
            .add_observer(crate::cell::despawn_blocks_of_removed_cell);
    }
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
    fn full_activity_when_inputs_suffice() {
        let p = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let r = solve_tick(
            &pools(&[(Currency::Pyruvate, 10.0)]),
            &[(BlockStatus::Active, &p)],
            0.5,
        );
        assert_eq!(r.activity, vec![1.0]);
        assert_eq!(r.deltas[&Currency::Pyruvate], -0.5);
        assert_eq!(r.deltas[&Currency::ATP], 0.5);
    }

    #[test]
    fn limiting_input_throttles_all_flows() {
        let p = profile(&[
            (Currency::Pyruvate, -2.0),
            (Currency::ReducingPower, -2.0),
            (Currency::ATP, 2.0),
        ]);
        let start = pools(&[(Currency::Pyruvate, 0.5), (Currency::ReducingPower, 10.0)]);
        let r = solve_tick(&start, &[(BlockStatus::Active, &p)], 1.0);
        assert!((r.activity[0] - 0.25).abs() < 1e-6);
        assert!((r.deltas[&Currency::Pyruvate] + 0.5).abs() < 1e-6);
        assert!((r.deltas[&Currency::ReducingPower] + 0.5).abs() < 1e-6);
        assert!((r.deltas[&Currency::ATP] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn status_scales_throughput() {
        let p = profile(&[(Currency::ATP, 1.0)]);
        let empty = CurrencyPools::default();
        let mutated = solve_tick(&empty, &[(BlockStatus::Mutated, &p)], 1.0);
        let silent = solve_tick(&empty, &[(BlockStatus::Silent, &p)], 1.0);
        assert_eq!(mutated.deltas[&Currency::ATP], 0.5);
        assert!(silent.deltas.is_empty());
    }

    #[test]
    fn producers_run_before_consumers() {
        let consumer = profile(&[(Currency::Pyruvate, -1.0), (Currency::ATP, 1.0)]);
        let producer = profile(&[(Currency::Pyruvate, 1.0)]);
        let order = solve_order(&[&consumer, &producer]);
        assert_eq!(order, vec![1, 0]);

        // With the producer first, the consumer can use its product in the same tick.
        let ordered: Vec<(BlockStatus, &FluxProfile)> = order
            .iter()
            .map(|&i| (BlockStatus::Active, [&consumer, &producer][i]))
            .collect();
        let r = solve_tick(&CurrencyPools::default(), &ordered, 1.0);
        assert_eq!(r.deltas[&Currency::ATP], 1.0);
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
        let r = solve_tick(&start, &[(BlockStatus::Active, &p)], 1.0);
        assert_eq!(r.deltas[&Currency::ATP], -3.0);
    }
}
