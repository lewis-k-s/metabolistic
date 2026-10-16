//! # Flow ledger
//!
//! The solver changes the pools through the blocks. Many other systems change
//! them too: running costs, movement, actions, repair, intake, food. The
//! `FlowLedger` of a cell records every such change with a `FlowSource`, so
//! that the metabolism view can show where a pool goes and where it comes from.
//!
//! A writer calls `apply` (change a pool and record the change) or `record`.
//! The amount recorded is the change that really happened: a pool never goes
//! below 0, so `apply` records the clamped change.
//!
//! Each `(source, currency)` keeps a moving average of the rate per second
//! (time constant `SMOOTH_SECONDS`) and a running total. `MetabolicSet::Record`
//! calls `advance` once per tick, so the average does not flicker.
//!
//! `FlowSource::Upkeep` also holds the maintenance ATP. The solver applies that
//! cost itself (it is a pseudo block), and the solver system records it here
//! with `record`. The metabolism view counts blocks only, so nothing is counted
//! twice.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CellTick, CurrencyPools, FlowmapTimingConfig};
use crate::cell::Cell;
use crate::molecules::Currency;

/// Time constant of the moving average, in seconds.
pub const SMOOTH_SECONDS: f32 = 1.0;

/// Why a pool changed outside a metabolic block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlowSource {
    /// Running costs: maintenance, pigment decay, ROS chemistry.
    Upkeep,
    /// ATP for thrust.
    Movement,
    /// Action slots: pathway levels, bursts, toxin, lysis, induced mutation.
    Action,
    MembraneRepair,
    GenomeRepair,
    /// Minerals and oxygen taken from the grid.
    Intake,
    /// Food and metal pickups.
    Food,
    /// What an engulfed cell gives.
    Engulf,
    /// A recycled protein returns amino acids.
    Recycle,
    /// Oxygen that leaves for the grid.
    Export,
}

#[derive(Debug, Default, Clone, Copy)]
struct Entry {
    /// Change since the last `advance`.
    pending: f32,
    /// Smoothed rate per second.
    rate: f32,
    /// Sum of all changes.
    total: f32,
}

/// Every pool change of a cell outside the solver blocks.
#[derive(Component, Default, Debug, Clone)]
pub struct FlowLedger {
    entries: HashMap<(FlowSource, Currency), Entry>,
}

impl FlowLedger {
    /// Records a change that has already happened.
    pub fn record(&mut self, source: FlowSource, currency: Currency, amount: f32) {
        if amount == 0.0 {
            return;
        }
        let e = self.entries.entry((source, currency)).or_default();
        e.pending += amount;
        e.total += amount;
    }

    /// Changes a pool by `delta` and records the change that really happened.
    /// Returns that change.
    pub fn apply(
        &mut self,
        pools: &mut CurrencyPools,
        source: FlowSource,
        currency: Currency,
        delta: f32,
    ) -> f32 {
        let before = pools.get(currency);
        pools.modify(currency, delta);
        let change = pools.get(currency) - before;
        self.record(source, currency, change);
        change
    }

    /// Sets a pool and records the change that really happened.
    pub fn set(
        &mut self,
        pools: &mut CurrencyPools,
        source: FlowSource,
        currency: Currency,
        amount: f32,
    ) {
        let before = pools.get(currency);
        pools.set(currency, amount);
        self.record(source, currency, pools.get(currency) - before);
    }

    /// Records the difference between two states of the pools.
    pub fn record_diff(&mut self, source: FlowSource, before: &CurrencyPools, after: &CurrencyPools) {
        for currency in Currency::ALL {
            self.record(source, currency, after.get(currency) - before.get(currency));
        }
    }

    /// Closes a tick of `dt` seconds: updates the moving averages.
    pub fn advance(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let alpha = 1.0 - (-dt / SMOOTH_SECONDS).exp();
        for e in self.entries.values_mut() {
            e.rate += (e.pending / dt - e.rate) * alpha;
            e.pending = 0.0;
            if e.rate.abs() < 1e-5 {
                e.rate = 0.0;
            }
        }
    }

    /// Smoothed rate per second of one source and currency.
    pub fn rate(&self, source: FlowSource, currency: Currency) -> f32 {
        self.entries.get(&(source, currency)).map_or(0.0, |e| e.rate)
    }

    /// Sum of all changes of one source and currency.
    pub fn total(&self, source: FlowSource, currency: Currency) -> f32 {
        self.entries.get(&(source, currency)).map_or(0.0, |e| e.total)
    }

    /// Sum of all changes of one currency, over all sources.
    pub fn total_of(&self, currency: Currency) -> f32 {
        self.entries
            .iter()
            .filter(|((_, c), _)| *c == currency)
            .map(|(_, e)| e.total)
            .sum()
    }

    /// Smoothed rates that are not 0, sorted for a stable order.
    pub fn rates(&self) -> Vec<(FlowSource, Currency, f32)> {
        let mut out: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.rate != 0.0)
            .map(|(&(s, c), e)| (s, c, e.rate))
            .collect();
        out.sort_by_key(|&(s, c, _)| (s, c));
        out
    }
}

/// Closes the tick of each due cell's ledger.
pub fn advance_ledgers_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&mut FlowLedger, &CellTick), With<Cell>>,
) {
    for (mut ledger, tick) in &mut cells {
        if tick.due {
            ledger.advance(tick.dt(timing.target_delta()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_records_the_clamped_change() {
        let mut pools = CurrencyPools::default();
        pools.set(Currency::ATP, 2.0);
        let mut ledger = FlowLedger::default();
        let change = ledger.apply(&mut pools, FlowSource::Action, Currency::ATP, -5.0);
        assert_eq!(change, -2.0);
        assert_eq!(ledger.total(FlowSource::Action, Currency::ATP), -2.0);
    }

    #[test]
    fn average_follows_a_steady_rate() {
        let mut ledger = FlowLedger::default();
        for _ in 0..50 {
            ledger.record(FlowSource::Intake, Currency::Minerals, 0.2);
            ledger.advance(0.2); // 1 per second
        }
        let rate = ledger.rate(FlowSource::Intake, Currency::Minerals);
        assert!((rate - 1.0).abs() < 0.01, "{rate}");
        for _ in 0..50 {
            ledger.advance(0.2);
        }
        assert!(ledger.rate(FlowSource::Intake, Currency::Minerals) < 1e-3);
    }

    #[test]
    fn average_does_not_depend_on_tick_length() {
        let mut a = FlowLedger::default();
        let mut b = FlowLedger::default();
        for _ in 0..5 {
            a.record(FlowSource::Food, Currency::CarbonSkeletons, 1.0);
            a.advance(1.0);
        }
        for _ in 0..20 {
            b.record(FlowSource::Food, Currency::CarbonSkeletons, 0.25);
            b.advance(0.25);
        }
        let (ra, rb) = (
            a.rate(FlowSource::Food, Currency::CarbonSkeletons),
            b.rate(FlowSource::Food, Currency::CarbonSkeletons),
        );
        // Same total in the same time; the average differs only by the
        // sampling of the exponential filter.
        assert!((ra - rb).abs() < 0.25, "{ra} {rb}");
    }
}
