//! Vesicle export metabolic block.
//!
//! The block packs organic waste into vesicles and removes it from the cell.
//! This is a flux-only block: it consumes OrganicWaste and produces nothing
//! inside the cell.

use bevy::prelude::*;

use crate::metabolism::FluxProfile;
use crate::molecules::Currency;

/// Marker for vesicle export blocks.
#[derive(Component)]
pub struct VesicleExportBlock;

/// OrganicWaste removed per second at full rate.
pub const RATE: f32 = 2.0;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::OrganicWaste, RATE);
    profile
}
