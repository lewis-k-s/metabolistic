//! Respiration (`BlockKind::Respiration`): the primary energy of land.
//!
//! The block burns carbon with O2: 1 carbon skeleton + 1 O2 + 0.5 reducing
//! power -> 3 ATP. Fermentation makes 1 ATP from 1 pyruvate (and 1 reducing
//! power), so respiration gives about 3 times the ATP per carbon. A real cell
//! gets more (about 15 times); the number is lower for balance. **Playtest.**
//!
//! The O2 comes from the cell's O2 pool. The pool takes O2 from the grid
//! (`oxygen::o2_exchange_system`, exact, mass is conserved; faster in air than in
//! water, `oxygen::AIR_EXCHANGE_RATE`), so the cell needs air or oxygenated water.
//! The uptake sets the ceiling of the block, and carbon sets the long run: one
//! photosynthesis enzyme makes about as much carbon as one respiration enzyme
//! burns, so respiration about doubles the ATP of the same carbon. A small part of the oxygen goes wrong: the block
//! makes some ROS (`ROS_RATE`), so catalase still matters. The O2 pool also
//! makes ROS by itself (`oxygen::ros_from_o2`).
//!
//! The rate law is the same as for every block: the enzymes, times the
//! saturation of the scarcest input (carbon, O2 or reducing power).

use bevy::prelude::*;

use crate::metabolism::{FluxProfile, Kinetics};
use crate::molecules::Currency;

/// Marker for respiration blocks.
#[derive(Component)]
pub struct RespirationBlock;

/// Carbon skeletons used per turnover.
pub const CARBON_PER_TURNOVER: f32 = 1.0;
/// O2 used per turnover.
pub const O2_PER_TURNOVER: f32 = 1.0;
/// Reducing power used per turnover.
pub const REDUCING_POWER_PER_TURNOVER: f32 = 0.5;
/// ATP made per turnover. **Playtest.**
pub const ATP_PER_TURNOVER: f32 = 3.0;
/// ROS made per turnover: the small leak of the electron transport chain.
pub const ROS_PER_TURNOVER: f32 = 0.1;
/// Pool level at which an input saturates to half. **Playtest.**
pub const HALF_SATURATION: f32 = 2.0;

/// ATP per carbon skeleton, for comparison with the other energy blocks.
pub fn atp_per_carbon() -> f32 {
    ATP_PER_TURNOVER / CARBON_PER_TURNOVER
}

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::CarbonSkeletons, CARBON_PER_TURNOVER);
    profile.set_demand(Currency::O2, O2_PER_TURNOVER);
    profile.set_demand(Currency::ReducingPower, REDUCING_POWER_PER_TURNOVER);
    profile.set_production(Currency::ATP, ATP_PER_TURNOVER);
    profile.set_production(Currency::ROS, ROS_PER_TURNOVER);
    profile
}

pub fn kinetics() -> Kinetics {
    Kinetics {
        k_half: HALF_SATURATION,
        cofactors: &[],
        external: &[],
    }
}
