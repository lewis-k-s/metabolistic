//! # Internal pools (currencies)
//!
//! A cell keeps eleven internal pools. See `docs/gameplay-roadmap.md`, section
//! "Internal pools".
//!
//! | Group | Pools |
//! |---|---|
//! | Energy | ATP, ReducingPower |
//! | Carbon hub | Pyruvate, CarbonSkeletons (includes acetyl-CoA) |
//! | Building | AminoAcids (proteins), Nucleotides (genome repair), Lipids (membrane repair and growth) |
//! | Cofactor | Metals (Fe-S clusters) |
//! | Intake | Minerals (the stock that transporters fill from the grid) |
//! | Waste | Waste |
//! | Screen | Pigments (block UV; they decay slowly) |
//!
//! Lipids have a stored form, `StoredLipids` (lipid beads), on the cell. Fat
//! storage moves lipids between the pool and the store.
//!
//! - **CurrencyPools (Component)**: the pools of one cell.
//!
//! Outside conditions are not pools: the spatial grid (`envgrid`) holds them.

use bevy::prelude::*;
use std::collections::HashMap;
use tracing::debug;

/// One internal pool. A key in `CurrencyPools` and in `FluxProfile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Currency {
    /// The main energy currency.
    ATP,
    /// NADH, NADPH and other electron carriers.
    ReducingPower,
    /// The end of glycolysis, and the input of fermentation.
    Pyruvate,
    /// Precursor carbon for biosynthesis. Includes acetyl-CoA.
    CarbonSkeletons,
    /// Material for proteins (enzymes and membrane proteins).
    AminoAcids,
    /// Material for genome repair.
    Nucleotides,
    /// Free lipids for membrane repair and growth.
    Lipids,
    /// Metal cofactors (Fe-S clusters).
    Metals,
    /// Metabolic byproducts. Toxic when they collect.
    Waste,
    /// Reduced minerals (for example H2S) that the cell took in. Chemosynthesis
    /// uses them. Transporters fill this pool from the `EnvGrid`.
    Minerals,
    /// Pigments that screen UV. They decay slowly (see `genome_damage`).
    Pigments,
}

impl Currency {
    /// All pools, in a fixed order.
    pub const ALL: [Currency; 11] = [
        Currency::ATP,
        Currency::ReducingPower,
        Currency::Pyruvate,
        Currency::CarbonSkeletons,
        Currency::AminoAcids,
        Currency::Nucleotides,
        Currency::Lipids,
        Currency::Metals,
        Currency::Waste,
        Currency::Minerals,
        Currency::Pigments,
    ];
}

/// The internal pools of one cell.
#[derive(Component, Default, Debug, Clone)]
pub struct CurrencyPools {
    pub pools: HashMap<Currency, f32>,
}

impl CurrencyPools {
    /// The amount in a pool. A missing pool holds 0.
    pub fn get(&self, currency: Currency) -> f32 {
        self.pools.get(&currency).copied().unwrap_or(0.0)
    }

    /// Sets a pool. Negative amounts become 0.
    pub fn set(&mut self, currency: Currency, amount: f32) {
        self.pools.insert(currency, amount.max(0.0));
    }

    /// Adds `delta` to a pool (subtracts if negative). The pool stays at 0 or more.
    pub fn modify(&mut self, currency: Currency, delta: f32) {
        let current = self.get(currency);
        self.set(currency, current + delta);
    }

    /// True if the pool holds at least `amount`.
    pub fn can_consume(&self, currency: Currency, amount: f32) -> bool {
        self.get(currency) >= amount
    }

    /// Start amounts for a new cell.
    pub fn with_defaults() -> Self {
        let pools = HashMap::from([
            (Currency::ATP, 100.0),
            (Currency::ReducingPower, 50.0),
            (Currency::Pyruvate, 25.0),
            (Currency::CarbonSkeletons, 30.0),
            (Currency::AminoAcids, 20.0),
            (Currency::Nucleotides, 10.0),
            (Currency::Lipids, 20.0),
            (Currency::Metals, 5.0),
            (Currency::Waste, 0.0),
            (Currency::Minerals, 10.0),
            (Currency::Pigments, 0.0),
        ]);
        Self { pools }
    }
}

/// Lipids that a cell keeps as storage beads. Fat storage moves lipids between
/// this store and the `Lipids` pool. Beads are not toxic.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct StoredLipids(pub f32);

/// Free lipid level above which fat storage stores lipids as beads, to protect
/// the cell from lipid toxicity. Below it, fat storage releases them.
#[derive(Resource, Debug, Clone, Copy)]
pub struct LipidToxicityThreshold(pub f32);

impl Default for LipidToxicityThreshold {
    fn default() -> Self {
        Self(50.0)
    }
}

/// Adds the lipid toxicity threshold. Cell pools are components, not
/// resources.
pub struct CurrencyPlugin;

impl Plugin for CurrencyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LipidToxicityThreshold>();

        debug!("CurrencyPlugin loaded: LipidToxicityThreshold.");
    }
}
