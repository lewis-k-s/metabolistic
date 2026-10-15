//! Fat storage metabolic block.
//!
//! A regulated block. Each tick, `regulate_fat_storage_system` sets its flux
//! profile from the owning cell's pools. It moves FreeFattyAcids toward the
//! lipid toxicity threshold:
//! - **Polymerization**: above the threshold, FreeFattyAcids become StorageBeads.
//! - **Lipolysis**: below the threshold, StorageBeads become FreeFattyAcids.
//!
//! The rate is capped so that one tick never moves past the threshold. Without
//! the cap, the block overshoots by up to `rate * dt`, and the result depends on
//! the tick length. Both directions conserve FreeFattyAcids + StorageBeads.

use bevy::prelude::*;

use crate::cell::{Cell, OwnedBy};
use crate::metabolism::{CurrencyPools, FlowmapTimingConfig, FluxProfile};
use crate::molecules::{Currency, LipidToxicityThreshold};

/// Fat storage block with its rates, per second.
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

/// The flux profile (per second) for the current pools and a tick of `dt` seconds.
pub fn fat_storage_profile(
    block: &FatStorageBlock,
    free_fatty_acids: f32,
    threshold: f32,
    dt: f32,
) -> FluxProfile {
    let mut profile = FluxProfile::default();
    let gap = free_fatty_acids - threshold;
    if gap > 0.0 {
        let rate = block.poly_rate.min(gap / dt);
        profile.set_demand(Currency::FreeFattyAcids, rate);
        profile.set_production(Currency::StorageBeads, rate);
    } else if gap < 0.0 {
        let rate = block.lipo_rate.min(-gap / dt);
        profile.set_demand(Currency::StorageBeads, rate);
        profile.set_production(Currency::FreeFattyAcids, rate);
    }
    profile
}

/// Sets each fat storage block's profile from its cell's pools.
pub fn regulate_fat_storage_system(
    threshold: Option<Res<LipidToxicityThreshold>>,
    timing: Res<FlowmapTimingConfig>,
    cells: Query<&CurrencyPools, With<Cell>>,
    mut blocks: Query<(&FatStorageBlock, &OwnedBy, &mut FluxProfile)>,
) {
    let threshold = threshold.map(|t| *t).unwrap_or_default();
    let dt = timing.target_delta();
    for (block, owner, mut profile) in &mut blocks {
        if let Ok(pools) = cells.get(owner.0) {
            *profile =
                fat_storage_profile(block, pools.get(Currency::FreeFattyAcids), threshold.0, dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regulation_moves_toward_threshold_without_overshoot() {
        let block = FatStorageBlock::default();
        // Far above: full poly rate.
        let p = fat_storage_profile(&block, 100.0, 50.0, 0.2);
        assert_eq!(p.get_demand(Currency::FreeFattyAcids), block.poly_rate);
        // Just above: only the excess, spread over the tick.
        let p = fat_storage_profile(&block, 51.0, 50.0, 0.2);
        assert!((p.get_demand(Currency::FreeFattyAcids) * 0.2 - 1.0).abs() < 1e-5);
        // Below: lipolysis, capped by the deficit.
        let p = fat_storage_profile(&block, 49.5, 50.0, 0.2);
        assert!((p.get_production(Currency::FreeFattyAcids) * 0.2 - 0.5).abs() < 1e-5);
        // At the threshold: nothing.
        assert!(fat_storage_profile(&block, 50.0, 50.0, 0.2).0.is_empty());
    }
}
