//! Biosynthesis blocks: building material from carbon skeletons.
//!
//! - **Amino acid biosynthesis**: CarbonSkeletons + ATP + ReducingPower -> AminoAcids.
//! - **Lipid metabolism**: CarbonSkeletons + ATP + ReducingPower -> Lipids.
//!
//! Both blocks use carbon skeletons. When carbon is scarce, they share it in
//! proportion to their demand (see `metabolism::solver`).

use bevy::prelude::*;

use crate::metabolism::FluxProfile;
use crate::molecules::Currency;

/// Marker for amino acid biosynthesis blocks.
#[derive(Component)]
pub struct AminoAcidBiosynthesisBlock;

/// Marker for lipid metabolism blocks.
#[derive(Component)]
pub struct LipidMetabolismBlock;

/// Per second, one enzyme at full saturation:
/// 1 CarbonSkeletons + 1 ATP + 0.5 ReducingPower -> 1 AminoAcids.
pub fn amino_acid_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::CarbonSkeletons, 1.0);
    profile.set_demand(Currency::ATP, 1.0);
    profile.set_demand(Currency::ReducingPower, 0.5);
    profile.set_production(Currency::AminoAcids, 1.0);
    profile
}

/// Per second, one enzyme at full saturation:
/// 2 CarbonSkeletons + 1 ATP + 1 ReducingPower -> 1 Lipids.
pub fn lipid_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::CarbonSkeletons, 2.0);
    profile.set_demand(Currency::ATP, 1.0);
    profile.set_demand(Currency::ReducingPower, 1.0);
    profile.set_production(Currency::Lipids, 1.0);
    profile
}
