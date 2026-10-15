//! Vesicle export metabolic block.
//!
//! The block packs waste into vesicles and removes it from the cell. This is a
//! flux-only block: it consumes Waste and produces nothing inside the cell.

use bevy::prelude::*;

use crate::metabolism::FluxProfile;
use crate::molecules::Currency;

/// Marker for vesicle export blocks.
#[derive(Component)]
pub struct VesicleExportBlock;

/// Waste removed per second by one enzyme at full saturation.
pub const RATE: f32 = 2.0;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::Waste, RATE);
    profile
}
