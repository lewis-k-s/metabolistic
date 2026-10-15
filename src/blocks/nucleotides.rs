//! Nucleotide and cofactor synthesis (`NucleotideCofactorSynthesis`).
//!
//! CarbonSkeletons + ATP + ReducingPower -> Nucleotides. Genome repair needs
//! nucleotides (see `genome_damage`). The block is regulated: it runs while the
//! pool is below `TARGET_NUCLEOTIDES` (see `refill`).

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::blocks::refill::RefillBlock;
use crate::metabolism::FluxProfile;
use crate::molecules::Currency;

/// Marker for nucleotide synthesis blocks.
#[derive(Component)]
pub struct NucleotideSynthesisBlock;

/// Nucleotides at which the block stops. **Playtest.**
pub const TARGET_NUCLEOTIDES: f32 = 20.0;

impl RefillBlock for NucleotideSynthesisBlock {
    const KIND: BlockKind = BlockKind::NucleotideCofactorSynthesis;
    const POOL: Currency = Currency::Nucleotides;
    const TARGET: f32 = TARGET_NUCLEOTIDES;
}

/// Per second, one enzyme at full saturation:
/// 1 CarbonSkeletons + 1 ATP + 0.5 ReducingPower -> 0.5 Nucleotides.
pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::CarbonSkeletons, 1.0);
    profile.set_demand(Currency::ATP, 1.0);
    profile.set_demand(Currency::ReducingPower, 0.5);
    profile.set_production(Currency::Nucleotides, 0.5);
    profile
}
