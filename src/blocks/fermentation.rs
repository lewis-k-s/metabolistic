//! Fermentation metabolic block implementation.
//!
//! Fermentation is an anaerobic metabolic pathway that converts pyruvate and reducing power
//! into ATP and organic waste. This is a flux-only block with no active systems.

use bevy::prelude::*;

/// Component marker for fermentation metabolic blocks.
///
/// Fermentation follows the stoichiometry:
/// - Inputs: 1 Pyruvate + 1 ReducingPower
/// - Outputs: 1 ATP + 1 OrganicWaste
#[derive(Component)]
pub struct FermentationBlock;
