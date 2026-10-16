//! # Membrane integrity
//!
//! The membrane of a cell has an integrity value. Hazards (UV, heat, toxins)
//! lower it. The cell repairs it with lipids from its `Lipids` pool. Thus
//! defence is connected to metabolism. The cell dies when integrity reaches 0
//! (see `cell::is_alive`).

use bevy::prelude::*;

use crate::cell::Cell;
use crate::metabolism::{CellTick, CurrencyPools, FlowLedger, FlowSource, FlowmapTimingConfig};
use crate::molecules::Currency;

/// Membrane integrity of a cell.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Membrane {
    /// Current integrity. 0 is a broken membrane.
    pub integrity: f32,
    /// Integrity of an undamaged membrane.
    pub max: f32,
}

impl Default for Membrane {
    fn default() -> Self {
        Self {
            integrity: 100.0,
            max: 100.0,
        }
    }
}

impl Membrane {
    /// Lowers integrity by `amount`. Integrity stays at 0 or more.
    pub fn damage(&mut self, amount: f32) {
        self.integrity = (self.integrity - amount.max(0.0)).max(0.0);
    }
}

/// Most integrity points repaired per second. **Playtest.**
pub const REPAIR_PER_SEC: f32 = 2.0;
/// Lipids used for each integrity point repaired. **Playtest.**
pub const LIPIDS_PER_POINT: f32 = 0.5;

/// Integrity points to repair in a tick of `dt` seconds, with `lipids` in the
/// pool. The repair stops at `max` and when lipids run out, so the result does
/// not depend on the tick length.
pub fn repair_points(membrane: &Membrane, lipids: f32, dt: f32) -> f32 {
    let gap = (membrane.max - membrane.integrity).max(0.0);
    let affordable = lipids.max(0.0) / LIPIDS_PER_POINT;
    (REPAIR_PER_SEC * dt).min(gap).min(affordable).max(0.0)
}

/// Repairs each cell's membrane with lipids.
pub fn membrane_repair_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&mut Membrane, &mut CurrencyPools, &mut FlowLedger, &CellTick), With<Cell>>,
) {
    for (mut membrane, mut pools, mut ledger, tick) in &mut cells {
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let points = repair_points(&membrane, pools.get(Currency::Lipids), dt);
        if points > 0.0 {
            membrane.integrity += points;
            ledger.apply(
                &mut pools,
                FlowSource::MembraneRepair,
                Currency::Lipids,
                -points * LIPIDS_PER_POINT,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_is_capped_by_rate_gap_and_lipids() {
        let mut m = Membrane::default();
        assert_eq!(repair_points(&m, 100.0, 1.0), 0.0, "intact");
        m.damage(10.0);
        assert_eq!(repair_points(&m, 100.0, 1.0), REPAIR_PER_SEC);
        assert_eq!(repair_points(&m, 100.0, 100.0), 10.0, "stops at max");
        assert_eq!(repair_points(&m, 0.5, 1.0), 0.5 / LIPIDS_PER_POINT);
        assert_eq!(repair_points(&m, 0.0, 1.0), 0.0);
    }

    #[test]
    fn damage_stops_at_zero() {
        let mut m = Membrane::default();
        m.damage(1000.0);
        assert_eq!(m.integrity, 0.0);
    }
}
