//! # Metabolic flux engine
//!
//! Each cell owns metabolic blocks. Each block has a `FluxProfile`: its flows
//! per second for one enzyme at full saturation. Negative flows consume a
//! pool, positive flows produce one. The cell's `Proteome` sets how many
//! enzymes each block has. See `solver` for the rate law and the sharing rule.
//!
//! The engine runs in `MetabolicSchedule`, one fixed tick at a time. A tick:
//!
//! 1. `MetabolicSet::Sense`: the cell reads the heat of the grid under it.
//! 2. `MetabolicSet::Genome`: random mutations.
//! 3. `MetabolicSet::Regulate`: regulated blocks set their drive or move pools
//!    (for example fat storage and protein synthesis).
//! 4. `MetabolicSet::Solve`: for each cell, `solve_tick` runs all blocks and
//!    the cell's maintenance cost together.
//! 5. `MetabolicSet::Build`: the cell uses the products of the tick. Protein
//!    synthesis advances the synthesis queue, and lipids repair the membrane.
//! 6. `MetabolicSet::Exchange`: the cell and the `EnvGrid` swap minerals, heat
//!    hurts the membrane, movement costs ATP, and the grid runs a step (see
//!    `exchange`).
//!
//! In the game, `run_metabolic_schedule_timed` runs ticks from real time. Tests
//! set `FlowmapTimingConfig::manual` and call `run_metabolic_tick` directly.

pub mod solver;

use std::collections::HashMap;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState};
use crate::cell::{Cell, CellBlocks, CellSize, maintenance_atp_per_sec};
use crate::envgrid::EnvGrid;
use crate::environment::{Channel, LocalEnvironment, external_saturation};
use crate::exchange;
pub use crate::molecules::CurrencyPools; // Re-export for backward compatibility
use crate::molecules::Currency;
use crate::proteins::Proteome;
pub use solver::{
    BlockInput, MAX_SUBSTEP, TickResult, block_demand, block_rates, dependency_lists, solve_order,
    solve_tick,
};

// --- Components ---

/// Marker component for entities that are part of the metabolic system.
#[derive(Component, Default)]
pub struct MetabolicBlock;

/// Flows of a block per second, for one enzyme at full saturation.
/// Negative values consume a currency, positive values produce it.
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

/// Saturation data of a block. See `solver` for the rate law.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Kinetics {
    /// Half-saturation constant `K` for inputs, cofactors and outside channels.
    pub k_half: f32,
    /// Pools that the block needs but does not use up.
    pub cofactors: &'static [Currency],
    /// Outside channels that the block needs (constant supply in M1).
    pub external: &'static [Channel],
}

impl Default for Kinetics {
    fn default() -> Self {
        Self {
            k_half: DEFAULT_K_HALF,
            cofactors: &[],
            external: &[],
        }
    }
}

/// Default half-saturation constant of a block.
pub const DEFAULT_K_HALF: f32 = 5.0;

/// Regulation multiplier of a block: 0 is off, 1 is normal. Regulated blocks
/// set it in `MetabolicSet::Regulate`. M4 action slots (Off / Low / High) will
/// also set it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Drive(pub f32);

impl Default for Drive {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Turnovers of a block in the last metabolic tick.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct BlockTurnover(pub f32);

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

/// The block graph of a cell from the last metabolic tick.
/// Kept on the cell for the flowmap and debug views.
#[derive(Component, Default, Clone, Debug)]
pub struct MetabolicGraph {
    /// Block entities, producers before consumers (see `solve_order`)
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

// --- Schedule ---

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetabolicSchedule;

/// Ordered phases of a metabolic tick.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetabolicSet {
    Sense,
    Genome,
    Regulate,
    Solve,
    Build,
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

/// One block of a cell, as fetched for the solver.
struct OwnedBlock<'a> {
    entity: Entity,
    kind: BlockKind,
    status: BlockStatus,
    profile: &'a FluxProfile,
    kinetics: Kinetics,
    drive: f32,
}

/// Solves one tick for every cell and applies the result to its pools.
#[allow(clippy::type_complexity)]
pub fn solve_cell_flux_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<
        (
            &CellBlocks,
            &Proteome,
            &CellSize,
            &LocalEnvironment,
            &mut CurrencyPools,
            &mut MetabolicGraph,
        ),
        With<Cell>,
    >,
    blocks: Query<(&MetabolicNode, &FluxProfile, &Kinetics, &Drive)>,
    mut turnovers: Query<&mut BlockTurnover>,
) {
    let dt = timing.target_delta();
    for (cell_blocks, proteome, size, env, mut pools, mut graph) in &mut cells {
        // Sort by kind so that the graph does not depend on HashMap order.
        let mut owned: Vec<OwnedBlock> = cell_blocks
            .0
            .values()
            .filter_map(|&entity| {
                let (node, profile, kinetics, drive) = blocks.get(entity).ok()?;
                Some(OwnedBlock {
                    entity,
                    kind: node.kind,
                    status: node.status,
                    profile,
                    kinetics: *kinetics,
                    drive: drive.0,
                })
            })
            .collect();
        owned.sort_by_key(|b| b.kind);

        let maintenance = maintenance_profile(size);
        let mut inputs: Vec<BlockInput> = owned
            .iter()
            .map(|b| BlockInput {
                profile: b.profile,
                enzymes: proteome.enzymes(b.kind) as f32,
                status: b.status,
                drive: b.drive,
                k_half: b.kinetics.k_half,
                cofactors: b.kinetics.cofactors,
                external: external_saturation(env, b.kinetics.external, b.kinetics.k_half),
            })
            .collect();
        // Maintenance competes for ATP like a block, at a fixed rate.
        inputs.push(BlockInput::new(&maintenance, 1.0));

        let result = solve_tick(&pools, &inputs, dt);
        for (currency, delta) in result.deltas {
            pools.modify(currency, delta);
        }
        for (block, turnover) in owned.iter().zip(&result.turnover) {
            if let Ok(mut t) = turnovers.get_mut(block.entity) {
                t.0 = *turnover;
            }
        }

        let profiles: Vec<&FluxProfile> = owned.iter().map(|b| b.profile).collect();
        let order = solve_order(&profiles);
        let deps = dependency_lists(&profiles);
        graph.nodes = order.iter().map(|&i| owned[i].entity).collect();
        graph.dependencies = deps
            .iter()
            .enumerate()
            .map(|(i, d)| {
                (
                    owned[i].entity,
                    d.iter().map(|&j| owned[j].entity).collect(),
                )
            })
            .collect();
    }
}

/// The maintenance cost of a cell as a flux profile: ATP per second.
pub fn maintenance_profile(size: &CellSize) -> FluxProfile {
    let mut profile = FluxProfile::default();
    let atp = maintenance_atp_per_sec(size);
    if atp > 0.0 {
        profile.set_demand(Currency::ATP, atp);
    }
    profile
}

/// Fraction of a pool lost in `dt` seconds at a continuous rate `k` per second.
/// Exact for any tick length, unlike `k * dt`.
pub fn decay_fraction(k: f32, dt: f32) -> f32 {
    1.0 - (-k * dt).exp()
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
                    MetabolicSet::Sense,
                    MetabolicSet::Genome,
                    MetabolicSet::Regulate,
                    MetabolicSet::Solve,
                    MetabolicSet::Build,
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
                exchange::sense_environment_system
                    .in_set(MetabolicSet::Sense)
                    .run_if(resource_exists::<EnvGrid>),
            )
            .add_systems(
                MetabolicSchedule,
                exchange::constant_supply_system
                    .in_set(MetabolicSet::Sense)
                    .run_if(not(resource_exists::<EnvGrid>)),
            )
            .add_systems(
                MetabolicSchedule,
                (
                    exchange::mineral_exchange_system.run_if(resource_exists::<EnvGrid>),
                    exchange::heat_damage_system,
                    exchange::movement_cost_system,
                    exchange::step_grid_system.run_if(resource_exists::<EnvGrid>),
                )
                    .chain()
                    .in_set(MetabolicSet::Exchange),
            )
            .add_systems(Update, run_metabolic_schedule_timed)
            .add_observer(crate::cell::despawn_blocks_of_removed_cell);
    }
}
