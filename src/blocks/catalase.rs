//! Catalase (`BlockKind::Catalase`): the key innovation of shallow water.
//!
//! The block turns ROS back into O2 for a little ATP: 2 ROS + 0.5 ATP -> 1 O2.
//! Half of the ROS mass comes back as O2 (the rest is the water that the
//! reaction makes, which the model does not track). Enzymes scale the rate like
//! any block. The rate saturates with the ROS level, so a small rest of ROS
//! stays in the cell. The solver never takes a pool below 0, so catalase cannot
//! take ROS below 0.

use bevy::prelude::*;

use crate::metabolism::{FluxProfile, Kinetics};
use crate::molecules::Currency;

/// Marker for catalase blocks.
#[derive(Component)]
pub struct CatalaseBlock;

/// ROS used per turnover.
pub const ROS_PER_TURNOVER: f32 = 2.0;
/// ATP used per turnover. **Playtest.**
pub const ATP_PER_TURNOVER: f32 = 0.5;
/// O2 made per turnover: half of the ROS.
pub const O2_PER_TURNOVER: f32 = 1.0;
/// Pool level at which the block runs at half rate.
pub const HALF_SATURATION: f32 = 4.0;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::ROS, ROS_PER_TURNOVER);
    profile.set_demand(Currency::ATP, ATP_PER_TURNOVER);
    profile.set_production(Currency::O2, O2_PER_TURNOVER);
    profile
}

pub fn kinetics() -> Kinetics {
    Kinetics {
        k_half: HALF_SATURATION,
        cofactors: &[],
        external: &[],
    }
}
