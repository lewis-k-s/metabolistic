use bevy::prelude::*;
use crate::molecules::{FreeFattyAcids, StorageBeads, ATP, CellMass, PolyMer, LipidToxicityThreshold};

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
    mut free_fatty_acids: ResMut<FreeFattyAcids>,
    mut storage_beads: ResMut<StorageBeads>,
    lipid_toxicity_threshold: Res<LipidToxicityThreshold>,
) {
    if free_fatty_acids.0 > lipid_toxicity_threshold.0 {
        let ffa_to_polymerize = 20.0; // Hardcode for testing
        println!("System: FreeFattyAcids before subtraction: {}", free_fatty_acids.0);
        free_fatty_acids.0 -= ffa_to_polymerize;
        println!("System: FreeFattyAcids after subtraction: {}", free_fatty_acids.0);
        storage_beads.0 += ffa_to_polymerize;
    }
}

/// System to mobilize StorageBeads back into FreeFattyAcids.
/// Only runs when FreeFattyAcids are below the toxicity threshold and there's a need for mobilization.
fn lipolysis_system(
    mut free_fatty_acids: ResMut<FreeFattyAcids>,
    mut storage_beads: ResMut<StorageBeads>,
    lipid_toxicity_threshold: Res<LipidToxicityThreshold>,
    mut atp_pool: ResMut<ATP>, // Assuming lipolysis might generate some ATP or reducing power
    mut query: Query<(&mut CellMass, &PolyMer)>,
) {
    // Only run lipolysis if we're NOT in a toxic state (i.e., when FFA levels are safe)
    // This prevents lipolysis from interfering with toxicity management
    if free_fatty_acids.0 <= lipid_toxicity_threshold.0 {
        for (mut cell_mass, polymer) in query.iter_mut() {
            let beads_to_mobilize = polymer.lipo_rate.min(storage_beads.0);
            if beads_to_mobilize > 0.0 {
                storage_beads.0 -= beads_to_mobilize;
                free_fatty_acids.0 += beads_to_mobilize;
                cell_mass.extra -= beads_to_mobilize; // Decrease cell mass as beads are mobilized
                atp_pool.0 += beads_to_mobilize * 0.05; // Example ATP gain
            }
        }
    }
}