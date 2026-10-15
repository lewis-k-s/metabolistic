//! # Exchange between the grid and a cell
//!
//! Each metabolic tick, a cell and the `EnvGrid` under it swap these:
//!
//! 1. **Sense** (start of the tick): the heat and light of the grid at the
//!    cell's position become `LocalEnvironment` heat and light. They are
//!    conditions: nothing uses them up. UV damage reads the light (see
//!    `genome_damage`).
//! 2. **Minerals** (end of the tick): chemosynthesis has already used minerals
//!    from the `Minerals` pool in the solver, so a short stock limits the
//!    block. Then the cell's transporters take minerals from the grid into the
//!    pool. Every unit that the grid loses, the pool gains, so mass is
//!    conserved (`ExchangeStats` counts what went in and what was used).
//! 3. **Heat damage**: heat above `HEAT_DAMAGE_THRESHOLD` lowers membrane
//!    integrity. This is how a cell burns near the vent core.
//! 4. **Movement cost**: ATP for the thrust used since the last tick.
//! 5. **Grid step**: the grid runs its diffusion, drift, sources and decay.
//!
//! Without an `EnvGrid` resource (the M1 headless tests), the `Minerals` pool
//! is topped up to the constant supply in `LocalEnvironment`, and only steps 3
//! and 4 run.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellBlocks, DamageSource, DamageTaken};
use crate::blocks::chemosynthesis::MINERALS_PER_TURNOVER;
use crate::envgrid::EnvGrid;
use crate::environment::{Channel, LocalEnvironment};
use crate::membrane::Membrane;
use crate::metabolism::{BlockTurnover, CellTick, CurrencyPools, FlowmapTimingConfig};
use crate::molecules::Currency;
use crate::movement::{ThrustEffort, move_atp_cost};
use crate::shelter::InShelter;
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};

/// Minerals that the cell can hold. **Playtest.**
pub const MINERAL_STOCK_CAP: f32 = 20.0;
/// Mineral uptake per second with no transporters: slow, but not zero.
/// **Playtest.**
pub const BASE_UPTAKE_PER_SEC: f32 = 0.6;
/// Mineral uptake per second that each transporter adds. **Playtest.**
pub const TRANSPORTER_UPTAKE_PER_SEC: f32 = 1.5;
/// Heat level above which the membrane takes damage. **Playtest.**
pub const HEAT_DAMAGE_THRESHOLD: f32 = 40.0;
/// Integrity points per second for each heat unit above the threshold.
/// Membrane repair gives back at most `membrane::REPAIR_PER_SEC`. **Playtest.**
pub const HEAT_DAMAGE_PER_UNIT_SEC: f32 = 0.1;

/// What a cell has taken in from the grid, and used, since it was spawned.
/// For tests and debug views.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct ExchangeStats {
    /// Minerals that transporters took from the grid.
    pub minerals_taken: f32,
    /// Minerals that chemosynthesis used up.
    pub minerals_used: f32,
}

// --- Pure functions ---

/// The most minerals per second that the cell can take in. It grows with the
/// number of transporters.
pub fn uptake_capacity(transporters: u32) -> f32 {
    BASE_UPTAKE_PER_SEC + transporters as f32 * TRANSPORTER_UPTAKE_PER_SEC
}

/// Minerals that `turnover` turnovers of chemosynthesis used up. The solver
/// takes exactly this from the `Minerals` pool.
pub fn minerals_used(turnover: f32) -> f32 {
    turnover.max(0.0) * MINERALS_PER_TURNOVER
}

/// Minerals that the cell asks the grid for in a tick of `dt` seconds: the
/// transporter limit, and not more than fits in the stock.
pub fn uptake_request(stock: f32, transporters: u32, dt: f32) -> f32 {
    let room = (MINERAL_STOCK_CAP - stock).max(0.0);
    (uptake_capacity(transporters) * dt.max(0.0)).min(room)
}

/// Integrity points lost in `dt` seconds at a heat level. The level is
/// constant within a tick, so the result does not depend on the tick length.
pub fn heat_damage(heat: f32, dt: f32) -> f32 {
    (heat - HEAT_DAMAGE_THRESHOLD).max(0.0) * HEAT_DAMAGE_PER_UNIT_SEC * dt.max(0.0)
}

// --- Systems ---

/// Sets the heat and light of each cell from the grid under it.
pub fn sense_environment_system(
    grid: Res<EnvGrid>,
    mut cells: Query<(&Transform, &mut LocalEnvironment), With<Cell>>,
) {
    for (transform, mut env) in &mut cells {
        let heat = grid.sample(Channel::Heat, transform.translation.truncate());
        env.set(Channel::Heat, heat);
        let light = grid.sample(Channel::Light, transform.translation.truncate());
        env.set(Channel::Light, light);
        // The water's O2 level: the HUD shows it. The O2 exchange samples it
        // again (see `oxygen`).
        let o2 = grid.sample(Channel::O2, transform.translation.truncate());
        env.set(Channel::O2, o2);
    }
}

/// Transporters refill the `Minerals` pool from the grid. The statistics count
/// what chemosynthesis used in the tick that just ran.
pub fn mineral_exchange_system(
    timing: Res<FlowmapTimingConfig>,
    mut grid: ResMut<EnvGrid>,
    mut cells: Query<
        (
            &Transform,
            &CellBlocks,
            &Proteome,
            &mut CurrencyPools,
            &mut ExchangeStats,
            &CellTick,
        ),
        With<Cell>,
    >,
    turnovers: Query<&BlockTurnover>,
) {
    for (transform, blocks, proteome, mut pools, mut stats, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let turnover = blocks
            .get(BlockKind::Chemosynthesis)
            .and_then(|block| turnovers.get(block).ok())
            .map_or(0.0, |t| t.0);
        stats.minerals_used += minerals_used(turnover);

        let stock = pools.get(Currency::Minerals);
        let transporters = proteome.count(ProteinTarget::Membrane(MembraneProtein::Transporter));
        let request = uptake_request(stock, transporters, dt);
        // The grid gives what it has. The cell gets exactly that.
        let taken = grid.take(Channel::Minerals, transform.translation.truncate(), request);
        pools.modify(Currency::Minerals, taken);
        stats.minerals_taken += taken;
    }
}

/// Without a grid, a constant supply: the `Minerals` pool is topped up to the
/// `Minerals` level of `LocalEnvironment`. This is a test-only source (M1
/// headless tests): it makes minerals from nothing.
pub fn constant_supply_system(mut cells: Query<(&LocalEnvironment, &mut CurrencyPools), With<Cell>>) {
    for (env, mut pools) in &mut cells {
        let supply = env.get(Channel::Minerals);
        if supply > pools.get(Currency::Minerals) {
            pools.set(Currency::Minerals, supply);
        }
    }
}

/// Heat above the threshold damages the membrane.
pub fn heat_damage_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<
        (&LocalEnvironment, &mut Membrane, &mut DamageTaken, &InShelter, &CellTick),
        With<Cell>,
    >,
) {
    for (env, mut membrane, mut taken, sheltered, tick) in &mut cells {
        let dt = tick.dt(timing.target_delta());
        if sheltered.0 || !tick.due {
            continue;
        }
        let damage = heat_damage(env.get(Channel::Heat), dt);
        if damage > 0.0 {
            membrane.damage(damage);
            taken.add(DamageSource::Heat, damage);
        }
    }
}

/// Moving costs ATP for the thrust used since the last tick.
pub fn movement_cost_system(
    mut cells: Query<(&mut ThrustEffort, &mut CurrencyPools, &CellTick), With<Cell>>,
) {
    for (mut effort, mut pools, tick) in &mut cells {
        if tick.due && effort.0 > 0.0 {
            pools.modify(Currency::ATP, -move_atp_cost(effort.0));
            effort.0 = 0.0;
        }
    }
}

/// Runs the grid for one tick.
pub fn step_grid_system(timing: Res<FlowmapTimingConfig>, mut grid: ResMut<EnvGrid>) {
    grid.step(timing.target_delta());
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn uptake_is_slow_without_transporters_and_grows_with_them() {
        assert!(uptake_capacity(0) > 0.0, "slow, but not zero");
        assert!(uptake_capacity(1) > uptake_capacity(0));
        assert_abs_diff_eq!(
            uptake_capacity(4) - uptake_capacity(3),
            TRANSPORTER_UPTAKE_PER_SEC,
            epsilon = 1e-6
        );
    }

    #[test]
    fn uptake_request_stops_at_the_stock_cap() {
        assert_abs_diff_eq!(uptake_request(0.0, 0, 0.2), 0.2 * BASE_UPTAKE_PER_SEC);
        assert_eq!(uptake_request(MINERAL_STOCK_CAP, 5, 1.0), 0.0);
        assert_abs_diff_eq!(uptake_request(MINERAL_STOCK_CAP - 0.1, 5, 1.0), 0.1, epsilon = 1e-5);
    }

    #[test]
    fn minerals_used_follows_turnover() {
        assert_eq!(minerals_used(0.0), 0.0);
        assert_eq!(minerals_used(0.4), 0.4 * MINERALS_PER_TURNOVER);
        assert_eq!(minerals_used(-1.0), 0.0);
    }

    #[test]
    fn heat_damage_starts_above_the_threshold() {
        assert_eq!(heat_damage(0.0, 1.0), 0.0);
        assert_eq!(heat_damage(HEAT_DAMAGE_THRESHOLD, 1.0), 0.0);
        assert!(heat_damage(HEAT_DAMAGE_THRESHOLD + 10.0, 1.0) > 0.0);
        // Twice the time, twice the damage.
        assert_abs_diff_eq!(
            heat_damage(70.0, 2.0),
            2.0 * heat_damage(70.0, 1.0),
            epsilon = 1e-6
        );
        // The core out-damages the membrane repair.
        assert!(heat_damage(100.0, 1.0) > crate::membrane::REPAIR_PER_SEC);
    }
}
