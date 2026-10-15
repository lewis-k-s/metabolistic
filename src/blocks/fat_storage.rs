//! Fat storage metabolic block.
//!
//! A regulated block. Each tick, `regulate_fat_storage_system` moves lipids
//! between the cell's `Lipids` pool and its `StoredLipids` (lipid beads),
//! toward the lipid toxicity threshold:
//! - **Storage**: above the threshold, free lipids become beads.
//! - **Lipolysis**: below the threshold, beads become free lipids.
//!
//! Beads are not an internal pool, so the block has no flux profile. Its rates
//! scale with its enzymes and gene state.
//!
//! The transfer is capped so that one tick never moves past the threshold.
//! Without the cap, the block overshoots by up to `rate * dt`, and the result
//! depends on the tick length. Both directions conserve Lipids + StoredLipids.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, OwnedBy};
use crate::metabolism::{CellTick, CurrencyPools, Drive, FlowmapTimingConfig, MetabolicNode};
use crate::molecules::{Currency, LipidToxicityThreshold, StoredLipids};
use crate::proteins::Proteome;

/// Fat storage block with its rates, per second for one enzyme.
#[derive(Component, Debug, Clone, Copy)]
pub struct FatStorageBlock {
    pub poly_rate: f32,
    pub lipo_rate: f32,
}

impl Default for FatStorageBlock {
    fn default() -> Self {
        Self {
            poly_rate: 20.0,
            lipo_rate: 5.0,
        }
    }
}

/// Lipids to move from the pool into storage in a tick of `dt` seconds
/// (negative: from storage into the pool). `scale` is enzymes x throughput x drive.
pub fn fat_storage_transfer(
    block: &FatStorageBlock,
    lipids: f32,
    stored: f32,
    threshold: f32,
    scale: f32,
    dt: f32,
) -> f32 {
    let scale = scale.max(0.0);
    let gap = lipids - threshold;
    if gap > 0.0 {
        (block.poly_rate * scale * dt).min(gap)
    } else if gap < 0.0 {
        -(block.lipo_rate * scale * dt)
            .min(-gap)
            .min(stored.max(0.0))
    } else {
        0.0
    }
}

/// Moves lipids between each cell's pool and its storage.
pub fn regulate_fat_storage_system(
    threshold: Option<Res<LipidToxicityThreshold>>,
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&mut CurrencyPools, &mut StoredLipids, &Proteome, &CellTick), With<Cell>>,
    blocks: Query<(&FatStorageBlock, &MetabolicNode, &Drive, &OwnedBy)>,
) {
    let threshold = threshold.map(|t| *t).unwrap_or_default();
    for (block, node, drive, owner) in &blocks {
        let Ok((mut pools, mut stored, proteome, tick)) = cells.get_mut(owner.0) else {
            continue;
        };
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let scale =
            proteome.enzymes(BlockKind::FatStorage) as f32 * node.status.throughput() * drive.0;
        let moved = fat_storage_transfer(
            block,
            pools.get(Currency::Lipids),
            stored.0,
            threshold.0,
            scale,
            dt,
        );
        if moved != 0.0 {
            pools.modify(Currency::Lipids, -moved);
            stored.0 = (stored.0 + moved).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regulation_moves_toward_threshold_without_overshoot() {
        let block = FatStorageBlock::default();
        // Far above: full storage rate.
        let m = fat_storage_transfer(&block, 100.0, 0.0, 50.0, 1.0, 0.2);
        assert!((m - block.poly_rate * 0.2).abs() < 1e-5);
        // Two enzymes: twice the rate.
        let m2 = fat_storage_transfer(&block, 100.0, 0.0, 50.0, 2.0, 0.2);
        assert!((m2 - 2.0 * m).abs() < 1e-5);
        // Just above: only the excess.
        let m = fat_storage_transfer(&block, 51.0, 0.0, 50.0, 1.0, 0.2);
        assert!((m - 1.0).abs() < 1e-5);
        // Below: lipolysis, capped by the deficit and by the store.
        let m = fat_storage_transfer(&block, 49.5, 10.0, 50.0, 1.0, 0.2);
        assert!((m + 0.5).abs() < 1e-5);
        let m = fat_storage_transfer(&block, 10.0, 0.3, 50.0, 1.0, 0.2);
        assert!((m + 0.3).abs() < 1e-5);
        // At the threshold, or no enzymes: nothing.
        assert_eq!(
            fat_storage_transfer(&block, 50.0, 10.0, 50.0, 1.0, 0.2),
            0.0
        );
        assert_eq!(
            fat_storage_transfer(&block, 90.0, 10.0, 50.0, 0.0, 0.2),
            0.0
        );
    }
}
