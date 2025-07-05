use bevy::prelude::*;
use crate::molecules::{Currency, CellMass, PolyMer, LipidToxicityThreshold};
use crate::metabolism::CurrencyPools;

/// Plugin for the Fat Storage block.
pub struct FatStoragePlugin;

impl Plugin for FatStoragePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            polymerize_beads_system,
            lipolysis_system,
        ));
    }
}

/// System to polymerize FreeFattyAcids into StorageBeads.
/// This system activates when FreeFattyAcids exceed a LipidToxicityThreshold.
/// It consumes FFA and ATP, produces StorageBeads, and updates CellMass.
fn polymerize_beads_system(
    mut currency_pools: ResMut<CurrencyPools>,
    lipid_toxicity_threshold: Res<LipidToxicityThreshold>,
) {
    let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
    if free_fatty_acids > lipid_toxicity_threshold.0 {
        let desired_polymerization: f32 = 20.0; // Desired amount to polymerize
        // Only polymerize what's actually available, up to the desired amount
        let ffa_to_polymerize = desired_polymerization.min(free_fatty_acids);
        
        // Use safe currency consumption
        if currency_pools.can_consume(Currency::FreeFattyAcids, ffa_to_polymerize) {
            currency_pools.modify(Currency::FreeFattyAcids, -ffa_to_polymerize);
            currency_pools.modify(Currency::StorageBeads, ffa_to_polymerize);
            println!("System: Polymerized {:.2} FFA into storage beads", ffa_to_polymerize);
        }
    }
}

/// System to mobilize StorageBeads back into FreeFattyAcids.
/// Only runs when FreeFattyAcids are below the toxicity threshold and there's a need for mobilization.
fn lipolysis_system(
    mut currency_pools: ResMut<CurrencyPools>,
    lipid_toxicity_threshold: Res<LipidToxicityThreshold>,
    mut query: Query<(&mut CellMass, &PolyMer)>,
) {
    let free_fatty_acids = currency_pools.get(Currency::FreeFattyAcids);
    // Only run lipolysis if we're NOT in a toxic state (i.e., when FFA levels are safe)
    // This prevents lipolysis from interfering with toxicity management
    if free_fatty_acids <= lipid_toxicity_threshold.0 {
        for (mut cell_mass, polymer) in query.iter_mut() {
            let storage_beads = currency_pools.get(Currency::StorageBeads);
            let beads_to_mobilize = polymer.lipo_rate.min(storage_beads);
            if beads_to_mobilize > 0.0 {
                currency_pools.modify(Currency::StorageBeads, -beads_to_mobilize);
                currency_pools.modify(Currency::FreeFattyAcids, beads_to_mobilize);
                cell_mass.extra -= beads_to_mobilize; // Decrease cell mass as beads are mobilized
                currency_pools.modify(Currency::ATP, beads_to_mobilize * 0.05); // Example ATP gain
            }
        }
    }
}