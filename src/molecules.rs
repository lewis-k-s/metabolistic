//! # Currency Molecules & Utilities
//!
//! This module defines the core metabolic "currencies" of the simulation as Bevy `Resources`.
//! These resources act as global pools that different metabolic blocks can draw from and contribute to.
//!
//! Based on the design documents (`AGENTS.md`, `Summary.md`), the primary currencies are:
//! - **ATP**: The main energy currency.
//! - **ReducingPower**: Represents NADH, NADPH, etc., used in biosynthesis.
//! - **AcetylCoA**: A key carbon carrier for lipid synthesis and the TCA cycle.
//! - **CarbonSkeletons**: Precursor molecules for amino acids and nucleotides.
//!
//! This module provides:
//! 1.  The `Resource` structs for each currency.
//! 2.  A `CurrencyPlugin` to initialize these resources in the Bevy app.
//! 3.  A generic `try_consume_currency` utility function for metabolic blocks to safely
//!     request and consume from the currency pools.

use bevy::prelude::*;
use tracing::debug;

// --- Currency Resource Definitions ---

/// **ATP (Adenosine Triphosphate)**
/// The primary energy currency, consumed by almost all metabolic processes.
/// Generated by Light Capture, Respiration, and Fermentation.
#[derive(Resource, Debug, Default)]
pub struct ATP(pub f32);

/// **Reducing Power (NADH, NADPH, etc.)**
/// Represents the pool of electron carriers used for biosynthesis and respiration.
/// Generated by catabolic pathways and light capture.
#[derive(Resource, Debug, Default)]
pub struct ReducingPower(pub f32);

/// **Acetyl-CoA**
/// A central metabolite linking sugar catabolism with the TCA cycle and lipid synthesis.
#[derive(Resource, Debug, Default)]
pub struct AcetylCoA(pub f32);

/// **Carbon Skeletons**
/// A generic pool of precursor molecules (e.g., from glycolysis or the TCA cycle)
/// used for building amino acids, nucleotides, and other complex molecules.
#[derive(Resource, Debug, Default)]
pub struct CarbonSkeletons(pub f32);

/// **Free Fatty Acids**
/// Represents free fatty acids available for various metabolic processes,
/// including storage or oxidation.
#[derive(Resource, Debug, Default)]
pub struct FreeFattyAcids(pub f32);

/// **Storage Beads**
/// Represents fatty acids stored in a compact, inert form (e.g., triacylglycerol beads).
/// Can be mobilized back into FreeFattyAcids.
#[derive(Resource, Debug, Default)]
pub struct StorageBeads(pub f32);

/// Defines the threshold of FreeFattyAcids above which the polymerization system
/// should automatically activate to prevent cellular damage.
#[derive(Resource, Debug, Default)]
pub struct LipidToxicityThreshold(pub f32);

/// **Pyruvate**
/// A key input for fermentation and the TCA cycle.
#[derive(Resource, Debug, Default)]
pub struct Pyruvate(pub f32);

/// **Organic Waste (Ethanol, Lactate, Acetate)**
/// Byproducts of fermentation that can become toxic if not managed.
#[derive(Resource, Debug, Default)]
pub struct OrganicWaste(pub f32);

// --- Components ---

/// Represents the total mass of the cell, affecting physical properties like speed and drag.
#[derive(Component, Debug)]
pub struct CellMass {
    pub base: f32,
    pub extra: f32,
}

/// Manages the polymerization and depolymerization of storage molecules (e.g., fatty acid beads).
#[derive(Component, Debug)]
pub struct PolyMer {
    pub capacity: f32,
    pub target_fill: f32,
    pub poly_rate: f32,
    pub lipo_rate: f32,
}

// --- Currency Trait & Implementations ---

/// An enum representing the different types of metabolic currencies.
/// This is used as a key in `FluxProfile` to define the input/output of each currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Currency {
    ATP,
    ReducingPower,
    AcetylCoA,
    CarbonSkeletons,
    FreeFattyAcids,
    StorageBeads,
    Pyruvate,
    OrganicWaste,
}

// A trait for generic operations on currency resources.
// This allows the `try_consume_currency` function to work with any currency type.
// pub trait CurrencyResource: Resource + Default + std::fmt::Debug {
//     /// Returns the current amount of the currency.
//     fn amount(&self) -> f32;
//     /// Sets the amount of the currency.
//     fn set_amount(&mut self, value: f32);
// }

// impl CurrencyResource for ATP {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// impl CurrencyResource for ReducingPower {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// impl CurrencyResource for AcetylCoA {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// impl CurrencyResource for CarbonSkeletons {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// impl CurrencyResource for FreeFattyAcids {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// impl CurrencyResource for Pyruvate {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// impl CurrencyResource for OrganicWaste {
//     fn amount(&self) -> f32 {
//         self.0
//     }
//     fn set_amount(&mut self, value: f32) {
//         self.0 = value;
//     }
// }

// --- Plugin for Initialization ---

/// A Bevy `Plugin` that initializes all the currency resources.
/// Add this plugin to your `App` to make the currencies available to all systems.
pub struct CurrencyPlugin;

impl Plugin for CurrencyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ATP>()
            .init_resource::<ReducingPower>()
            .init_resource::<AcetylCoA>()
            .init_resource::<CarbonSkeletons>()
            .init_resource::<FreeFattyAcids>()
            .init_resource::<StorageBeads>()
            .init_resource::<LipidToxicityThreshold>()
            .init_resource::<Pyruvate>()
            .init_resource::<OrganicWaste>();

        debug!("CurrencyPlugin loaded: Initialized ATP, ReducingPower, AcetylCoA, CarbonSkeletons, FreeFattyAcids, StorageBeads, LipidToxicityThreshold, Pyruvate, and OrganicWaste resources.");
    }
}

