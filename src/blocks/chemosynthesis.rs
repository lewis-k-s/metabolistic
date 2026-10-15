//! Chemosynthesis metabolic block.
//!
//! The primary energy source at deep-sea vents. The cell oxidizes reduced
//! minerals (for example H2S) with the vent's heat and uses the energy to make
//! ATP and reducing power and to fix carbon. Fe-S clusters (`Metals`) are a
//! cofactor: the block needs them but does not use them up.
//!
//! Minerals are an internal pool, which transporters fill from the grid (see
//! `exchange`). The block takes them as an input, so the solver limits it when
//! the stock is short. Heat is an outside channel (`LocalEnvironment`): the
//! grid level under the cell. Nothing uses heat up.

use bevy::prelude::*;

use crate::environment::Channel;
use crate::metabolism::{FluxProfile, Kinetics};
use crate::molecules::Currency;

/// Marker for chemosynthesis blocks.
#[derive(Component)]
pub struct ChemosynthesisBlock;

/// ATP per second for one enzyme at full saturation.
pub const ATP_RATE: f32 = 2.0;
/// Reducing power per second for one enzyme at full saturation.
pub const REDUCING_POWER_RATE: f32 = 1.0;
/// Carbon skeletons per second for one enzyme at full saturation.
pub const CARBON_RATE: f32 = 1.0;
/// Minerals used for each turnover. **Playtest.**
pub const MINERALS_PER_TURNOVER: f32 = 1.0;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::Minerals, MINERALS_PER_TURNOVER);
    profile.set_production(Currency::ATP, ATP_RATE);
    profile.set_production(Currency::ReducingPower, REDUCING_POWER_RATE);
    profile.set_production(Currency::CarbonSkeletons, CARBON_RATE);
    profile
}

pub fn kinetics() -> Kinetics {
    Kinetics {
        k_half: 2.0,
        cofactors: &[Currency::Metals],
        external: &[Channel::Heat],
    }
}
