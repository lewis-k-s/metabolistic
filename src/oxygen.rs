//! # Oxygen and reactive oxygen species
//!
//! Photosynthesis (`blocks::photosynthesis`) puts O2 into the cell. The cell
//! has two pools for it (`Currency::O2`, `Currency::ROS`) and four rules. All
//! rules are pure functions of the pools and the tick length `dt`, and they are
//! exact, so the tick length does not change the result.
//!
//! 1. **O2 leaves the cell.** The O2 pool and the grid level under the cell
//!    move towards each other: a fraction `1 - exp(-k dt)` of the difference
//!    changes hands (`o2_exchange`). The cell never goes past the grid level
//!    (no overshoot). What the pool loses, the grid gains, and the other way
//!    round, so mass is conserved. A cell in clean water loses its O2 slowly. A
//!    cell in water that other phototrophs filled loses it more slowly, or
//!    takes some in.
//! 2. **O2 turns into ROS.** A part of the internal O2 becomes ROS every second
//!    (`ros_from_o2`). The more O2 in the cell, the more ROS.
//! 3. **ROS decays.** The cell's own antioxidants remove ROS slowly
//!    (`ros_after_decay`). Catalase (`blocks::catalase`) removes it fast.
//! 4. **ROS hurts.** ROS lowers the membrane (`DamageSource::Ros`) and the
//!    genome integrity, and adds to the mutation load. A shelter does not
//!    help: the damage is inside the cell.
//!
//! Without catalase, a photosynthesising cell in full light dies of ROS in
//! about a minute. With a few catalase enzymes, ROS stays low.

use bevy::prelude::*;

use crate::cell::{Cell, DamageSource, DamageTaken};
use crate::envgrid::EnvGrid;
use crate::environment::Channel;
use crate::genome_damage::{GenomeIntegrity, MutationLoad};
use crate::membrane::Membrane;
use crate::metabolism::{
    CellTick, CurrencyPools, FlowmapTimingConfig, MetabolicSchedule, MetabolicSet, decay_fraction,
};
use crate::molecules::Currency;

// --- Tuning. All of these need **playtest**. ---

/// Share of the difference between the O2 pool and the grid level that moves
/// per second.
pub const O2_EXCHANGE_RATE: f32 = 0.15;
/// Share of the internal O2 that turns into ROS per second.
pub const ROS_FROM_O2_RATE: f32 = 0.02;
/// Decay of ROS per second from the cell's own antioxidants.
pub const ROS_DECAY_RATE: f32 = 0.03;
/// Membrane integrity points lost per second for each ROS unit.
pub const ROS_MEMBRANE_PER_SEC: f32 = 0.3;
/// Genome integrity lost per second for each ROS unit.
pub const ROS_GENOME_PER_SEC: f32 = 0.0012;
/// Mutation load gained per second for each ROS unit.
pub const ROS_MUTATION_PER_SEC: f32 = 0.006;
/// ROS level at which the body-horror tint is full (see `world::player_body`).
pub const ROS_TINT_FULL: f32 = 25.0;

// --- Pure functions ---

/// O2 that leaves the cell in `dt` seconds, with `pool` in the cell and `level`
/// in the grid under it. Negative if the grid has more: the cell takes in that
/// much. It is a fraction of the difference, so it never passes the grid level.
pub fn o2_exchange(pool: f32, level: f32, dt: f32) -> f32 {
    (pool.max(0.0) - level.max(0.0)) * decay_fraction(O2_EXCHANGE_RATE, dt.max(0.0))
}

/// ROS made from the internal O2 in `dt` seconds. It is also the O2 that is lost.
pub fn ros_from_o2(o2: f32, dt: f32) -> f32 {
    o2.max(0.0) * decay_fraction(ROS_FROM_O2_RATE, dt.max(0.0))
}

/// ROS left after `dt` seconds of the cell's own antioxidants.
pub fn ros_after_decay(ros: f32, dt: f32) -> f32 {
    ros.max(0.0) * (1.0 - decay_fraction(ROS_DECAY_RATE, dt.max(0.0)))
}

/// Membrane integrity points lost in `dt` seconds. The level is constant within
/// a tick, so the result does not depend on the tick length.
pub fn ros_membrane_damage(ros: f32, dt: f32) -> f32 {
    ros.max(0.0) * ROS_MEMBRANE_PER_SEC * dt.max(0.0)
}

/// Genome integrity lost in `dt` seconds.
pub fn ros_genome_damage(ros: f32, dt: f32) -> f32 {
    ros.max(0.0) * ROS_GENOME_PER_SEC * dt.max(0.0)
}

/// Mutation load gained in `dt` seconds.
pub fn ros_mutation_load(ros: f32, dt: f32) -> f32 {
    ros.max(0.0) * ROS_MUTATION_PER_SEC * dt.max(0.0)
}

/// How far the body-horror tint goes, from 0 to 1, at a ROS level.
pub fn ros_tint(ros: f32) -> f32 {
    (ros / ROS_TINT_FULL).clamp(0.0, 1.0)
}

// --- Systems ---

/// ROS chemistry: part of the O2 becomes ROS, then the cell's antioxidants
/// remove some ROS.
pub fn ros_chemistry_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&mut CurrencyPools, &CellTick), With<Cell>>,
) {
    for (mut pools, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let o2 = pools.get(Currency::O2);
        let ros = pools.get(Currency::ROS);
        if o2 <= 0.0 && ros <= 0.0 {
            continue;
        }
        let made = ros_from_o2(o2, dt);
        pools.set(Currency::O2, o2 - made);
        pools.set(Currency::ROS, ros_after_decay(ros, dt) + made);
    }
}

/// O2 moves between the cell and the grid under it. Without a grid, the water
/// holds no O2 and the cell's O2 just leaves.
pub fn o2_exchange_system(
    timing: Res<FlowmapTimingConfig>,
    mut grid: Option<ResMut<EnvGrid>>,
    mut cells: Query<(&Transform, &mut CurrencyPools, &CellTick), With<Cell>>,
) {
    for (transform, mut pools, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let position = transform.translation.truncate();
        let pool = pools.get(Currency::O2);
        let Some(grid) = grid.as_deref_mut() else {
            if pool > 0.0 {
                pools.set(Currency::O2, pool - o2_exchange(pool, 0.0, dt));
            }
            continue;
        };
        let level = grid.sample(Channel::O2, position);
        let flow = o2_exchange(pool, level, dt);
        if flow > 0.0 {
            // The grid gets exactly what the cell loses.
            let given = grid.deposit(Channel::O2, position, flow);
            pools.set(Currency::O2, pool - given);
        } else if flow < 0.0 {
            // The cell gets exactly what the grid gives.
            let taken = grid.take(Channel::O2, position, -flow);
            pools.set(Currency::O2, pool + taken);
        }
    }
}

/// ROS damages the membrane and the genome.
#[allow(clippy::type_complexity)]
pub fn ros_damage_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<
        (
            &CurrencyPools,
            &mut Membrane,
            &mut DamageTaken,
            &mut GenomeIntegrity,
            &mut MutationLoad,
            &CellTick,
        ),
        With<Cell>,
    >,
) {
    for (pools, mut membrane, mut taken, mut integrity, mut load, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let ros = pools.get(Currency::ROS);
        if ros <= 0.0 {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let damage = ros_membrane_damage(ros, dt);
        if damage > 0.0 {
            membrane.damage(damage);
            taken.add(DamageSource::Ros, damage);
        }
        integrity.0 = (integrity.0 - ros_genome_damage(ros, dt)).max(0.0);
        load.0 += ros_mutation_load(ros, dt);
    }
}

pub struct OxygenPlugin;

impl Plugin for OxygenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            MetabolicSchedule,
            (
                ros_chemistry_system.in_set(MetabolicSet::Build),
                (
                    ros_damage_system,
                    o2_exchange_system.before(crate::exchange::step_grid_system),
                )
                    .in_set(MetabolicSet::Exchange),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn o2_moves_towards_the_grid_level_and_never_past_it() {
        // Surplus leaves, deficit comes in, equal levels stay.
        assert!(o2_exchange(10.0, 2.0, 0.2) > 0.0);
        assert!(o2_exchange(1.0, 6.0, 0.2) < 0.0);
        assert_eq!(o2_exchange(3.0, 3.0, 1.0), 0.0);
        // Even a very long tick stops at the level.
        for dt in [0.2, 5.0, 1e4] {
            let left = 10.0 - o2_exchange(10.0, 2.0, dt);
            assert!(left >= 2.0 - 1e-4, "dt {dt}: {left}");
        }
        assert_abs_diff_eq!(10.0 - o2_exchange(10.0, 2.0, 1e4), 2.0, epsilon = 1e-3);
    }

    #[test]
    fn the_exchange_is_the_same_for_one_long_tick_and_two_short_ones() {
        let one = o2_exchange(20.0, 4.0, 0.4);
        let first = o2_exchange(20.0, 4.0, 0.2);
        let second = o2_exchange(20.0 - first, 4.0, 0.2);
        assert_abs_diff_eq!(one, first + second, epsilon = 1e-4);
    }

    #[test]
    fn ros_from_o2_is_tick_length_independent() {
        let one = ros_from_o2(30.0, 1.0);
        let first = ros_from_o2(30.0, 0.5);
        let second = ros_from_o2(30.0 - first, 0.5);
        assert_abs_diff_eq!(one, first + second, epsilon = 1e-4);
        assert_abs_diff_eq!(ros_from_o2(30.0, 0.2) / 30.0, 1.0 - (-ROS_FROM_O2_RATE * 0.2).exp(), epsilon = 1e-6);
        // More O2, more ROS.
        assert!(ros_from_o2(40.0, 0.2) > ros_from_o2(10.0, 0.2));
        assert_eq!(ros_from_o2(-1.0, 0.2), 0.0);
    }

    #[test]
    fn ros_decay_is_exact_and_ros_damage_follows_the_level() {
        let two = ros_after_decay(ros_after_decay(8.0, 0.5), 0.5);
        assert_abs_diff_eq!(two, ros_after_decay(8.0, 1.0), epsilon = 1e-5);
        assert_abs_diff_eq!(ros_membrane_damage(10.0, 2.0), 2.0 * ros_membrane_damage(10.0, 1.0), epsilon = 1e-6);
        assert!(ros_membrane_damage(20.0, 1.0) > ros_membrane_damage(10.0, 1.0));
        assert_eq!(ros_membrane_damage(0.0, 1.0), 0.0);
        assert!(ros_genome_damage(10.0, 1.0) > 0.0 && ros_mutation_load(10.0, 1.0) > 0.0);
    }

    #[test]
    fn the_tint_goes_from_none_to_full() {
        assert_eq!(ros_tint(0.0), 0.0);
        assert_eq!(ros_tint(ROS_TINT_FULL * 3.0), 1.0);
        assert!(ros_tint(ROS_TINT_FULL * 0.5) > 0.4 && ros_tint(ROS_TINT_FULL * 0.5) < 0.6);
    }
}
