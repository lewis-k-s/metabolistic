//! Vesicle export metabolic block implementation.
//! 
//! The vesicle export block simulates cellular waste removal through vesicle transport.
//! This is an active system block that continuously removes organic waste from cells
//! at a configurable rate.

use bevy::prelude::*;
use crate::molecules::Currency;
use crate::metabolism::CurrencyPools;
use crate::cell::Cell;

/// Component marker for vesicle export metabolic blocks.
/// 
/// Vesicle export removes organic waste from cells, preventing toxic buildup.
/// The export rate is controlled by the `VesicleExportRate` resource.
#[derive(Component)]
pub struct VesicleExportBlock;

/// Resource controlling the rate of organic waste export via vesicles.
/// 
/// The value represents the amount of organic waste removed per frame.
/// Default value is 0.1.
#[derive(Resource)]
pub struct VesicleExportRate(pub f32);

/// System that exports organic waste from cells via vesicles.
/// Runs in parallel for each cell.
pub fn vesicle_export_system(
    export_rate: Res<VesicleExportRate>,
    mut query_cells: Query<&mut CurrencyPools, With<Cell>>,
) {
    let amount_to_export = export_rate.0;
    
    // Process each cell in parallel
    query_cells.par_iter_mut().for_each(|mut currency_pools| {
        let organic_waste = currency_pools.get(Currency::OrganicWaste);

        if organic_waste >= amount_to_export {
            currency_pools.modify(Currency::OrganicWaste, -amount_to_export);
            // debug!("VesicleExport: Exported {:.2} OrganicWaste", amount_to_export);
        } else {
            currency_pools.set(Currency::OrganicWaste, 0.0);
            // debug!("VesicleExport: Exported remaining {:.2} OrganicWaste", organic_waste);
        }
    });
}
