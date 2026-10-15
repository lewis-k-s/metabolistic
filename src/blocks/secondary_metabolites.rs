//! Secondary metabolites (`SecondaryMetabolites`): the pigment block.
//!
//! CarbonSkeletons + ATP -> Pigments. Pigments screen UV (shield = P / (K + P),
//! see `genome_damage`) and decay slowly. The block is regulated: it runs while
//! the pigment pool is below `TARGET_PIGMENTS` (see `refill`). The starter
//! genome does not have this gene.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::blocks::refill::RefillBlock;
use crate::metabolism::FluxProfile;
use crate::molecules::Currency;

/// Marker for pigment blocks.
#[derive(Component)]
pub struct PigmentBlock;

/// Pigments at which the block stops (shield 0.8 with K = 10). **Playtest.**
pub const TARGET_PIGMENTS: f32 = 40.0;

impl RefillBlock for PigmentBlock {
    const KIND: BlockKind = BlockKind::SecondaryMetabolites;
    const POOL: Currency = Currency::Pigments;
    const TARGET: f32 = TARGET_PIGMENTS;
}

/// Per second, one enzyme at full saturation: 1 CarbonSkeletons + 1 ATP -> 1 Pigments.
pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::CarbonSkeletons, 1.0);
    profile.set_demand(Currency::ATP, 1.0);
    profile.set_production(Currency::Pigments, 1.0);
    profile
}
