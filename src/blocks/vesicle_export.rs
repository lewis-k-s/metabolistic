use bevy::prelude::*;
use crate::molecules::Currency;
use crate::metabolism::CurrencyPools;

#[derive(Component)]
pub struct VesicleExportBlock;

#[derive(Resource)]
pub struct VesicleExportRate(pub f32);

pub struct VesicleExportPlugin;

impl Plugin for VesicleExportPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(VesicleExportRate(0.1)) // Default export rate
            .add_systems(Startup, spawn_vesicle_export_block)
            .add_systems(FixedUpdate, vesicle_export_system);
    }
}

fn spawn_vesicle_export_block(mut commands: Commands) {
    commands.spawn(VesicleExportBlock);
    println!("VesicleExportBlock spawned!");
}

fn vesicle_export_system(
    export_rate: Res<VesicleExportRate>,
    mut currency_pools: ResMut<CurrencyPools>,
) {
    let amount_to_export = export_rate.0;
    let organic_waste = currency_pools.get(Currency::OrganicWaste);

    if organic_waste >= amount_to_export {
        currency_pools.modify(Currency::OrganicWaste, -amount_to_export);
        // debug!("VesicleExport: Exported {:.2} OrganicWaste", amount_to_export);
    } else {
        currency_pools.set(Currency::OrganicWaste, 0.0);
        // debug!("VesicleExport: Exported remaining {:.2} OrganicWaste", organic_waste);
    }
}
