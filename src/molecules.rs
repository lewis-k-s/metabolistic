//! # Currency System & Metabolic Pools
//!
//! This module defines the core metabolic "currencies" and currency management systems.
//! Unlike traditional global resource pools, this system uses component-based currency pools
//! for individual cells and environmental resource pools for shared/external metabolites.
//!
//! ## Architecture:
//! - **CurrencyPools (Component)**: Per-cell currency management attached to individual cell entities
//! - **EnvironmentalCurrencyPools (Resource)**: Global pools for environmental/external metabolites
//! - **Currency (Enum)**: Type-safe identifiers for different metabolic currencies
//!
//! ## Core Currencies:
//! - **ATP**: The main energy currency
//! - **ReducingPower**: Represents NADH, NADPH, etc., used in biosynthesis
//! - **AcetylCoA**: A key carbon carrier for lipid synthesis and the TCA cycle
//! - **CarbonSkeletons**: Precursor molecules for amino acids and nucleotides
//! - **FreeFattyAcids**: Available fatty acids for various metabolic processes
//! - **Pyruvate**: Key input for fermentation and TCA cycle
//! - **OrganicWaste**: Byproducts that can become toxic if not managed

use bevy::prelude::*;
use std::collections::HashMap;
use tracing::debug;

// --- Currency System Core ---

/// An enum representing the different types of metabolic currencies.
/// This is used as a key in currency pools and flux profiles.
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

/// Central currency pools managed by the metabolic flow system.
/// This is exclusively a Component attached to individual cells.
/// For global/environmental currencies, use EnvironmentalCurrencyPools resource.
#[derive(Component, Default, Debug, Clone)]
pub struct CurrencyPools {
    pub pools: HashMap<Currency, f32>,
}

impl CurrencyPools {
    /// Get the amount of a specific currency
    pub fn get(&self, currency: Currency) -> f32 {
        self.pools.get(&currency).copied().unwrap_or(0.0)
    }

    /// Set the amount of a specific currency
    pub fn set(&mut self, currency: Currency, amount: f32) {
        self.pools.insert(currency, amount.max(0.0)); // Prevent negative currencies
    }

    /// Add to a currency (positive) or subtract (negative)
    pub fn modify(&mut self, currency: Currency, delta: f32) {
        let current = self.get(currency);
        self.set(currency, current + delta);
    }

    /// Check if there's enough of a currency available
    pub fn can_consume(&self, currency: Currency, amount: f32) -> bool {
        self.get(currency) >= amount
    }

    /// Initialize with default starting amounts for a new cell
    pub fn with_defaults() -> Self {
        let mut pools = HashMap::new();
        pools.insert(Currency::ATP, 100.0);
        pools.insert(Currency::ReducingPower, 50.0);
        pools.insert(Currency::AcetylCoA, 20.0);
        pools.insert(Currency::CarbonSkeletons, 30.0);
        pools.insert(Currency::FreeFattyAcids, 10.0);
        pools.insert(Currency::StorageBeads, 0.0);
        pools.insert(Currency::Pyruvate, 25.0);
        pools.insert(Currency::OrganicWaste, 0.0);

        Self { pools }
    }
}

/// Environmental currency pools for shared/external metabolites.
/// These represent currencies that exist in the environment outside of cells,
/// such as waste products, external carbon sources, or shared metabolite pools.
/// Unlike internal CurrencyPools (Component), this is a global resource.
#[derive(Resource, Default, Debug, Clone)]
pub struct EnvironmentalCurrencyPools {
    pub pools: HashMap<Currency, f32>,
}

impl EnvironmentalCurrencyPools {
    /// Get the amount of a specific environmental currency
    pub fn get(&self, currency: Currency) -> f32 {
        self.pools.get(&currency).copied().unwrap_or(0.0)
    }

    /// Set the amount of a specific environmental currency
    pub fn set(&mut self, currency: Currency, amount: f32) {
        self.pools.insert(currency, amount.max(0.0)); // Prevent negative currencies
    }

    /// Add to an environmental currency (positive) or subtract (negative)
    pub fn modify(&mut self, currency: Currency, delta: f32) {
        let current = self.get(currency);
        self.set(currency, current + delta);
    }

    /// Check if there's enough of an environmental currency available
    pub fn can_consume(&self, currency: Currency, amount: f32) -> bool {
        self.get(currency) >= amount
    }

    /// Initialize with default environmental starting amounts
    /// Environmental pools typically start lower since they represent external resources
    pub fn with_defaults() -> Self {
        let mut pools = HashMap::new();
        // Environmental waste accumulation starts at zero
        pools.insert(Currency::OrganicWaste, 0.0);
        // External carbon sources might be limited
        pools.insert(Currency::CarbonSkeletons, 50.0);
        // Environmental ATP/ReducingPower typically minimal (cells produce internally)
        pools.insert(Currency::ATP, 0.0);
        pools.insert(Currency::ReducingPower, 0.0);
        pools.insert(Currency::AcetylCoA, 0.0);
        pools.insert(Currency::FreeFattyAcids, 0.0);
        pools.insert(Currency::StorageBeads, 0.0);
        pools.insert(Currency::Pyruvate, 10.0); // Some external pyruvate sources

        Self { pools }
    }
}

/// Free fatty acid level above which fat storage stores lipids as beads, to
/// protect the cell from lipid toxicity. At or below it, fat storage releases them.
#[derive(Resource, Debug, Clone, Copy)]
pub struct LipidToxicityThreshold(pub f32);

impl Default for LipidToxicityThreshold {
    fn default() -> Self {
        Self(50.0)
    }
}

// --- Plugin for Initialization ---

/// A Bevy `Plugin` that initializes the currency system.
/// This plugin sets up environmental currency pools and system configuration.
/// Individual cell currency pools are managed as components, not global resources.
pub struct CurrencyPlugin;

impl Plugin for CurrencyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EnvironmentalCurrencyPools>()
            .init_resource::<LipidToxicityThreshold>();

        debug!(
            "CurrencyPlugin loaded: Initialized EnvironmentalCurrencyPools and LipidToxicityThreshold. Cell currency pools are managed as components."
        );
    }
}
