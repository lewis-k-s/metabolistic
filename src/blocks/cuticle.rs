//! Cuticle (`BlockKind::Cuticle`): the key innovation of land.
//!
//! **Why a gene (block) and not a membrane protein.** A key gene is a
//! `BlockKind`: the key-gene rule (`npc::KeyGeneRule`), engulfing, the codex and
//! the gate cell all move genes of that type. A membrane protein would need its
//! own path through all of them. A block also gives the running cost for free:
//! it uses Lipids every second, like a cuticle that the cell must keep up.
//!
//! The block makes no pool. It only uses Lipids (`LIPIDS_PER_TURNOVER` for each
//! turnover). The turnovers per second (the *strength*) set the shield
//! (`protection`): `s / (K + s)`, like the pigment shield for UV. The shield
//! lowers water loss (`desiccation::water_loss_rate`) and adds a little drag
//! (`movement::cuticle_drag_factor`). It never reaches 1, so a cell on dry
//! ground always dries out in the end. Without Lipids, the block stops and the
//! shield falls.

use bevy::prelude::*;

use crate::metabolism::{FluxProfile, Kinetics};
use crate::molecules::Currency;

/// Marker for cuticle blocks.
#[derive(Component)]
pub struct CuticleBlock;

/// Lipids used per turnover. One enzyme at full rate costs 0.05 per second.
/// **Playtest.**
pub const LIPIDS_PER_TURNOVER: f32 = 0.05;
/// Pool level at which the Lipids input saturates to half.
pub const HALF_SATURATION: f32 = 2.0;
/// Strength at which the shield is 0.5 (enzymes running at full rate).
/// **Playtest.**
pub const SHIELD_HALF_STRENGTH: f32 = 2.0;
/// The shield is never more than this, so water loss is never 0.
pub const MAX_PROTECTION: f32 = 0.95;

pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::Lipids, LIPIDS_PER_TURNOVER);
    profile
}

pub fn kinetics() -> Kinetics {
    Kinetics {
        k_half: HALF_SATURATION,
        cofactors: &[],
        external: &[],
    }
}

/// The shield from the strength (turnovers per second): 0 without a cuticle, and
/// always below `MAX_PROTECTION`.
pub fn protection(strength: f32) -> f32 {
    if strength <= 0.0 {
        return 0.0;
    }
    (strength / (SHIELD_HALF_STRENGTH + strength)).min(MAX_PROTECTION)
}
