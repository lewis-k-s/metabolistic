//! Fermentation metabolic block.
//!
//! Fermentation is an anaerobic pathway: it oxidizes reducing power back so
//! that ATP production can continue without oxygen. This is a flux-only block.

use bevy::prelude::*;

use crate::metabolism::FluxProfile;
use crate::molecules::Currency;

/// Marker for fermentation blocks.
#[derive(Component)]
pub struct FermentationBlock;

/// Full rate, per second: 1 Pyruvate + 1 ReducingPower -> 1 ATP + 1 OrganicWaste.
pub const RATE: f32 = 1.0;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::Pyruvate, RATE);
    profile.set_demand(Currency::ReducingPower, RATE);
    profile.set_production(Currency::ATP, RATE);
    profile.set_production(Currency::OrganicWaste, RATE);
    profile
}
