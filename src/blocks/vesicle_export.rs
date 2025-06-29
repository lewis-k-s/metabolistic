use bevy::prelude::*;
use crate::molecules::OrganicWaste;

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
    mut organic_waste: ResMut<OrganicWaste>,
) {
    let amount_to_export = export_rate.0;

    if organic_waste.0 >= amount_to_export {
        organic_waste.0 -= amount_to_export;
        // debug!("VesicleExport: Exported {:.2} OrganicWaste", amount_to_export);
    } else {
        organic_waste.0 = 0.0;
        // debug!("VesicleExport: Exported remaining {:.2} OrganicWaste", organic_waste.0);
    }
}
