//! Fat storage metabolic block implementation.
//!
//! The fat storage block manages lipid metabolism, polymerizing free fatty acids into
//! storage beads when they exceed toxicity thresholds, and mobilizing them back when needed.
//! This is an active system block that runs polymerization and lipolysis systems each frame.

use crate::cell::Cell;
use crate::metabolism::CurrencyPools;
use crate::molecules::{CellMass, Currency, LipidToxicityThreshold, PolyMer};
use bevy::prelude::*;

/// Component that points back to the owning entity (usually a cell)
/// Used for PolyMer entities that need to find their parent cell
#[derive(Component)]
pub struct OwnedBy(pub Entity);

/// Component marker for fat storage metabolic blocks.
///
/// Fat storage manages two processes:
/// - **Polymerization**: FreeFattyAcids + ATP → StorageBeads (when above toxicity)
/// - **Lipolysis**: StorageBeads → FreeFattyAcids + ATP (when below toxicity)
#[derive(Component)]
pub struct FatStorageBlock;

/// System to polymerize FreeFattyAcids into StorageBeads.
/// This system activates when FreeFattyAcids exceed a LipidToxicityThreshold.
/// It consumes FFA and ATP, produces StorageBeads, and updates CellMass.
/// Runs in parallel for each cell.
pub fn polymerize_beads_system(
    mut query_cells: Query<&mut CurrencyPools, With<Cell>>,
    lipid_toxicity_threshold: Res<LipidToxicityThreshold>,
) {
    // Process each cell in parallel
    query_cells.par_iter_mut().for_each(|mut currency_pools| {
        let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
        if free_fatty_acids > lipid_toxicity_threshold.0 {
            let desired_polymerization: f32 = 20.0; // Desired amount to polymerize
                                                    // Only polymerize what's actually available, up to the desired amount
            let ffa_to_polymerize = desired_polymerization.min(free_fatty_acids);

            // Use safe currency consumption
            if currency_pools.can_consume(Currency::FreeFattyAcids, ffa_to_polymerize) {
                currency_pools.modify(Currency::FreeFattyAcids, -ffa_to_polymerize);
                currency_pools.modify(Currency::StorageBeads, ffa_to_polymerize);
            }
        }
    });
}

/// System to mobilize StorageBeads back into FreeFattyAcids.
/// Only runs when FreeFattyAcids are below the toxicity threshold and there's a need for mobilization.
/// Runs for each cell with PolyMer entities.
pub fn lipolysis_system(
    mut query_cells: Query<&mut CurrencyPools, With<Cell>>,
    lipid_toxicity_threshold: Res<LipidToxicityThreshold>,
    mut query_polymers: Query<(&mut CellMass, &PolyMer, &OwnedBy)>,
) {
    // Process each PolyMer entity and find its owning cell
    for (mut cell_mass, polymer, owned_by) in query_polymers.iter_mut() {
        if let Ok(mut currency_pools) = query_cells.get_mut(owned_by.0) {
            let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
            // Only run lipolysis if we're NOT in a toxic state (i.e., when FFA levels are safe)
            // This prevents lipolysis from interfering with toxicity management
            if free_fatty_acids <= lipid_toxicity_threshold.0 {
                let storage_beads = currency_pools.get(Currency::StorageBeads);
                let beads_to_mobilize = polymer.lipo_rate.min(storage_beads);
                if beads_to_mobilize > 0.0 {
                    currency_pools.modify(Currency::StorageBeads, -beads_to_mobilize);
                    currency_pools.modify(Currency::FreeFattyAcids, beads_to_mobilize);
                    cell_mass.extra -= beads_to_mobilize; // Decrease cell mass as beads are mobilized
                    currency_pools.modify(Currency::ATP, beads_to_mobilize * 0.05);
                    // Example ATP gain
                }
            }
        }
    }
}
