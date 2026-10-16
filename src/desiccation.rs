//! # Hydration and desiccation (M8)
//!
//! On land a cell dries out. Each cell has a `Hydration` level in [0, 1]. The
//! land field has a `Channel::Water` layer (puddles, the shore, moist soil
//! pores); the rest is dry ground. The rules are pure functions of the water
//! level under the cell, the cuticle shield and the tick length `dt`:
//!
//! 1. **Wetness** (`wetness`): `W / (K + W)` of the water level `W`. It is the
//!    share of the cell that is "in water".
//! 2. **Hydration** (`hydration_after`): `dH/dt = gain x wet x (1 - H) -
//!    loss x (1 - wet) x (1 - shield) x H`. The wetness is constant within a
//!    tick, so the equation is linear and the system solves it exactly: the
//!    tick length does not change the result. In water the level rises to 1. On
//!    dry ground it falls towards 0. In water the loss term is 0, so a wet cell
//!    never dries.
//! 3. **Damage** (`desiccation_damage`): below `DAMAGE_THRESHOLD` the membrane
//!    takes damage (`DamageSource::Desiccation`), more the drier the cell is. The
//!    damage is the exact integral of the rate over the tick (the tick is split
//!    where the hydration crosses the threshold), so it is tick-length independent.
//! 4. **Cuticle** (`blocks::cuticle`): the shield lowers the loss by a saturating
//!    factor. It is below `MAX_PROTECTION`, so the loss is never 0.
//!
//! Only the player's cell dries out (creatures are land-adapted: **playtest**),
//! and only in a level whose field says so (`FieldKind::dries`).

use bevy::prelude::*;

use crate::blocks::cuticle;
use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellBlocks, DamageSource, DamageTaken};
use crate::environment::{Channel, LocalEnvironment};
use crate::level::CurrentLevel;
use crate::membrane::Membrane;
use crate::metabolism::{BlockTurnover, CellTick, FlowmapTimingConfig, MetabolicSchedule, MetabolicSet};
use crate::npc::Npc;

// --- Tuning. All of these need **playtest**. ---

/// Water level at which a cell is half in water.
pub const WATER_HALF: f32 = 1.5;
/// Hydration gained per second in full water (the rate of approach to 1).
pub const GAIN_PER_SEC: f32 = 0.5;
/// Hydration lost per second on dry ground, with no cuticle (the rate of
/// approach to 0). Half time about 12 s.
pub const LOSS_PER_SEC: f32 = 0.06;
/// Below this hydration the membrane takes damage.
pub const DAMAGE_THRESHOLD: f32 = 0.4;
/// Membrane integrity lost per second at hydration 0. Membrane repair gives
/// back at most `membrane::REPAIR_PER_SEC`.
pub const DAMAGE_PER_SEC: f32 = 3.0;

// --- Components ---

/// Water in the cell, from 0 (dry) to 1 (full).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Hydration(pub f32);

impl Default for Hydration {
    fn default() -> Self {
        Self(1.0)
    }
}

/// The cuticle shield of the cell, from 0 to below 1. The metabolic tick sets it
/// from the running cuticle block (`blocks::cuticle::protection`).
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct CuticleShield(pub f32);

// --- Pure functions ---

/// The share of the cell that is in water, from the water level under it.
pub fn wetness(water: f32) -> f32 {
    if water <= 0.0 { 0.0 } else { water / (WATER_HALF + water) }
}

/// Water loss rate per second on completely dry ground, with the shield. The
/// shield is clamped below 1, so the rate is always above 0.
pub fn water_loss_rate(shield: f32) -> f32 {
    LOSS_PER_SEC * (1.0 - shield.clamp(0.0, cuticle::MAX_PROTECTION))
}

/// Hydration after `dt` seconds at a constant water level and shield. Exact.
pub fn hydration_after(hydration: f32, water: f32, shield: f32, dt: f32) -> f32 {
    let h = hydration.clamp(0.0, 1.0);
    let wet = wetness(water);
    let gain = GAIN_PER_SEC * wet;
    let loss = water_loss_rate(shield) * (1.0 - wet);
    let rate = gain + loss;
    if rate <= 0.0 || dt <= 0.0 {
        return h;
    }
    let balance = gain / rate;
    (balance + (h - balance) * (-rate * dt).exp()).clamp(0.0, 1.0)
}

/// Membrane integrity lost per second at a hydration level.
pub fn damage_rate(hydration: f32) -> f32 {
    (DAMAGE_THRESHOLD - hydration).max(0.0) / DAMAGE_THRESHOLD * DAMAGE_PER_SEC
}

/// Membrane integrity lost in `dt` seconds at a constant water level and shield,
/// from hydration `hydration`. Exact: it integrates `damage_rate` over the same
/// curve as `hydration_after`, `h(t) = balance + (h0 - balance) exp(-rate t)`. The
/// rate is linear in `h` below the threshold, so the tick is split where `h`
/// crosses `DAMAGE_THRESHOLD` (closed form) and the part below it is integrated.
pub fn desiccation_damage(hydration: f32, water: f32, shield: f32, dt: f32) -> f32 {
    let h0 = hydration.clamp(0.0, 1.0);
    let dt = dt.max(0.0);
    let wet = wetness(water);
    let gain = GAIN_PER_SEC * wet;
    let rate = gain + water_loss_rate(shield) * (1.0 - wet);
    if dt == 0.0 {
        return 0.0;
    }
    if rate <= 0.0 {
        return damage_rate(h0) * dt;
    }
    let balance = gain / rate;
    let t = DAMAGE_THRESHOLD;
    // Time at which h reaches the threshold, if it does (else infinity).
    let crossing = if (h0 - balance).abs() > 1e-9 && (t - balance) / (h0 - balance) > 0.0 {
        -((t - balance) / (h0 - balance)).ln() / rate
    } else {
        f32::INFINITY
    };
    let (from, to) = match (h0 < t, balance < t) {
        (true, true) => (0.0, dt),
        (true, false) => (0.0, crossing.min(dt)),
        (false, true) => (crossing.min(dt), dt),
        (false, false) => return 0.0,
    };
    if to <= from {
        return 0.0;
    }
    // Integral of (t - h(s)) over [from, to], scaled to the damage per second.
    let below = (t - balance) * (to - from)
        + (h0 - balance) / rate * ((-rate * to).exp() - (-rate * from).exp());
    (below / t * DAMAGE_PER_SEC).max(0.0)
}

/// Seconds a cell needs on dry ground to fall from full hydration to the damage
/// threshold, with a shield. For the HUD and the tests.
pub fn seconds_to_threshold(shield: f32) -> f32 {
    -DAMAGE_THRESHOLD.ln() / water_loss_rate(shield)
}

// --- Systems ---

/// Sets the cuticle shield from the turnovers of the cuticle block in the tick.
pub fn cuticle_shield_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&CellBlocks, &mut CuticleShield, &CellTick), With<Cell>>,
    turnovers: Query<&BlockTurnover>,
) {
    for (blocks, mut shield, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let strength = blocks
            .get(BlockKind::Cuticle)
            .and_then(|block| turnovers.get(block).ok())
            .map_or(0.0, |t| t.0 / dt);
        let now = cuticle::protection(strength);
        if shield.0 != now {
            shield.0 = now;
        }
    }
}

/// Dries the cell on dry ground, wets it in water, and hurts the membrane when it
/// is dry. Runs only in a level whose field dries cells.
#[allow(clippy::type_complexity)]
pub fn hydration_system(
    timing: Res<FlowmapTimingConfig>,
    level: Option<Res<CurrentLevel>>,
    mut cells: Query<
        (
            &LocalEnvironment,
            &CuticleShield,
            &mut Hydration,
            &mut Membrane,
            &mut DamageTaken,
            &CellTick,
        ),
        (With<Cell>, Without<Npc>),
    >,
) {
    if !level.is_some_and(|l| l.0.field.dries()) {
        return;
    }
    for (env, shield, mut hydration, mut membrane, mut taken, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let before = hydration.0;
        let after = hydration_after(before, env.get(Channel::Water), shield.0, dt);
        if after != before {
            hydration.0 = after;
        }
        let damage = desiccation_damage(before, env.get(Channel::Water), shield.0, dt);
        if damage > 0.0 {
            membrane.damage(damage);
            taken.add(DamageSource::Desiccation, damage);
        }
    }
}

pub struct DesiccationPlugin;

impl Plugin for DesiccationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            MetabolicSchedule,
            (
                cuticle_shield_system.in_set(MetabolicSet::Build),
                hydration_system.in_set(MetabolicSet::Exchange),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn the_hydration_rule_is_exact_for_any_tick_length() {
        for water in [0.0, 0.5, 3.0, 20.0] {
            for shield in [0.0, 0.6] {
                let one = hydration_after(0.7, water, shield, 4.0);
                let mut h = 0.7;
                for _ in 0..8 {
                    h = hydration_after(h, water, shield, 0.5);
                }
                assert_abs_diff_eq!(one, h, epsilon = 1e-5);
            }
        }
    }

    #[test]
    fn a_cell_dries_on_dry_ground_and_never_dries_in_water() {
        assert!(hydration_after(1.0, 0.0, 0.0, 5.0) < 1.0);
        assert!(hydration_after(1.0, 0.0, 0.0, 600.0) < 0.001);
        // In full water the loss term is small and the gain wins: a wet cell
        // rises and a full wet cell stays full.
        assert!(hydration_after(0.2, 50.0, 0.0, 5.0) > 0.2);
        assert!(hydration_after(1.0, 50.0, 0.0, 600.0) > 0.99);
        assert!(hydration_after(1.0, 5.0, 0.0, 60.0) > 0.9);
    }

    #[test]
    fn the_cuticle_lowers_the_loss_but_never_to_zero() {
        assert!(water_loss_rate(0.5) < water_loss_rate(0.0));
        assert!(water_loss_rate(0.9) < water_loss_rate(0.5));
        assert!(water_loss_rate(5.0) > 0.0, "the shield is clamped");
        assert!(seconds_to_threshold(0.5) > 1.9 * seconds_to_threshold(0.0));
        assert!(hydration_after(1.0, 0.0, 0.95, 2000.0) < 0.01, "it dries in the end");
    }

    #[test]
    fn damage_starts_below_the_threshold_and_grows_when_the_cell_is_drier() {
        assert_eq!(damage_rate(1.0), 0.0);
        assert_eq!(damage_rate(DAMAGE_THRESHOLD), 0.0);
        assert!(damage_rate(0.1) > damage_rate(0.3));
        assert_abs_diff_eq!(damage_rate(0.0), DAMAGE_PER_SEC, epsilon = 1e-6);
        assert_eq!(desiccation_damage(0.9, 0.0, 0.0, 0.1), 0.0, "stays above the threshold");
        assert!(desiccation_damage(0.0, 0.0, 0.0, 1.0) > 2.9);
    }

    #[test]
    fn the_damage_is_exact_for_any_tick_length() {
        // Cases: always below, crossing down, crossing up (in water), always above.
        for (h0, water, shield) in [(0.1, 0.0, 0.0), (0.5, 0.0, 0.0), (0.5, 0.0, 0.6), (0.1, 10.0, 0.0), (0.1, 0.5, 0.3), (0.9, 0.0, 0.0)] {
            let one = desiccation_damage(h0, water, shield, 4.0);
            let (mut h, mut many) = (h0, 0.0);
            for _ in 0..20 {
                many += desiccation_damage(h, water, shield, 0.2);
                h = hydration_after(h, water, shield, 0.2);
            }
            assert_abs_diff_eq!(one, many, epsilon = 1e-4);
        }
        assert!(desiccation_damage(0.5, 0.0, 0.0, 30.0) > 0.0, "it crosses the threshold and hurts");
    }
}
