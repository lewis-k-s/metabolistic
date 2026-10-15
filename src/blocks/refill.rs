//! Blocks that fill one pool up to a target: nucleotide synthesis and pigment
//! synthesis.
//!
//! A refill block is regulated. Its `Drive` is the largest value in [0, 1] that
//! does not take the pool above the target in this tick. So the pool never
//! overshoots the target by `rate x dt`, and the tick length does not matter
//! (as for fat storage).

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, OwnedBy};
use crate::metabolism::{CellTick, CurrencyPools, Drive, FlowmapTimingConfig, FluxProfile, MetabolicNode};
use crate::molecules::Currency;
use crate::proteins::Proteome;

/// A block that fills `POOL` up to `TARGET`.
pub trait RefillBlock: Component {
    const KIND: BlockKind;
    const POOL: Currency;
    /// The pool level at which the block stops.
    const TARGET: f32;
}

/// The drive of a refill block. `capacity` is the pool amount that the block
/// can make per second at drive 1 (enzymes x status x production per enzyme).
pub fn refill_drive(level: f32, target: f32, capacity: f32, dt: f32) -> f32 {
    let gap = (target - level).max(0.0);
    let room = capacity * dt;
    if gap <= 0.0 || room <= 0.0 {
        0.0
    } else {
        (gap / room).min(1.0)
    }
}

/// Sets the drive of each refill block of kind `B` from its cell's pool.
pub fn regulate_refill_system<B: RefillBlock>(
    timing: Res<FlowmapTimingConfig>,
    cells: Query<(&CurrencyPools, &Proteome, &CellTick), With<Cell>>,
    mut blocks: Query<(&OwnedBy, &MetabolicNode, &FluxProfile, &mut Drive), With<B>>,
) {
    for (owner, node, profile, mut drive) in &mut blocks {
        let Ok((pools, proteome, tick)) = cells.get(owner.0) else {
            continue;
        };
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let capacity = proteome.enzymes(B::KIND) as f32
            * node.status.throughput()
            * profile.get_production(B::POOL);
        let wanted = refill_drive(pools.get(B::POOL), B::TARGET, capacity, dt);
        if drive.0 != wanted {
            drive.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drive_stops_at_the_target_without_overshoot() {
        assert_eq!(refill_drive(20.0, 20.0, 1.0, 0.2), 0.0);
        assert_eq!(refill_drive(25.0, 20.0, 1.0, 0.2), 0.0);
        assert_eq!(refill_drive(0.0, 20.0, 1.0, 0.2), 1.0);
        // 0.1 left, the block can make 0.2 in a tick: half drive.
        assert!((refill_drive(19.9, 20.0, 1.0, 0.2) - 0.5).abs() < 1e-5);
        assert_eq!(refill_drive(0.0, 20.0, 0.0, 0.2), 0.0, "no enzymes");
    }
}
