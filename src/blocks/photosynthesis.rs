//! Photosynthesis (`BlockKind::LightCapture`).
//!
//! The primary energy source of shallow water. Light is an outside channel
//! (`LocalEnvironment`): the block saturates with the light under the cell, like
//! chemosynthesis does with heat, and nothing uses the light up. One turnover
//! makes ATP, reducing power and carbon, and it puts O2 into the cell. A small
//! part of the oxygen chemistry goes wrong at once: the block also makes some
//! ROS (reactive oxygen species). See `oxygen` for what O2 and ROS do next.
//!
//! This is the Great Oxidation Event in small: the block that feeds the cell is
//! also the block that poisons it, unless the cell has catalase.

use bevy::prelude::*;

use crate::environment::Channel;
use crate::metabolism::{FluxProfile, Kinetics};
use crate::molecules::Currency;

/// Marker for photosynthesis blocks.
#[derive(Component)]
pub struct PhotosynthesisBlock;

/// ATP per turnover. **Playtest.**
pub const ATP_RATE: f32 = 3.0;
/// Reducing power per turnover.
pub const REDUCING_POWER_RATE: f32 = 1.5;
/// Carbon skeletons per turnover.
pub const CARBON_RATE: f32 = 1.0;
/// O2 per turnover.
pub const O2_RATE: f32 = 1.5;
/// ROS per turnover: the small leak of the light reactions.
pub const ROS_RATE: f32 = 0.2;
/// Light level at which the block runs at half rate.
pub const LIGHT_HALF_SATURATION: f32 = 3.0;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_production(Currency::ATP, ATP_RATE);
    profile.set_production(Currency::ReducingPower, REDUCING_POWER_RATE);
    profile.set_production(Currency::CarbonSkeletons, CARBON_RATE);
    profile.set_production(Currency::O2, O2_RATE);
    profile.set_production(Currency::ROS, ROS_RATE);
    profile
}

pub fn kinetics() -> Kinetics {
    Kinetics {
        k_half: LIGHT_HALF_SATURATION,
        cofactors: &[],
        external: &[Channel::Light],
    }
}
