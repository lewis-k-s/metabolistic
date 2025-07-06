use crate::molecules::Currency;
use crate::metabolism::{FluxProfile, MetabolicBlock, MetabolicNode, BlockStatus};
use crate::blocks::genome::BlockKind;
use bevy::prelude::*;

#[derive(Component)]
pub struct FermentationBlock;

pub struct FermentationPlugin;

impl Plugin for FermentationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_fermentation_block);
    }
}

fn spawn_fermentation_block(mut commands: Commands) {
    let mut flux_profile = FluxProfile::default();
    
    // Define the fermentation flux profile using utility methods
    flux_profile.set_demand(Currency::Pyruvate, 1.0);
    flux_profile.set_demand(Currency::ReducingPower, 1.0);
    flux_profile.set_production(Currency::ATP, 1.0);
    flux_profile.set_production(Currency::OrganicWaste, 1.0);
    
    commands.spawn((
        FermentationBlock,
        MetabolicBlock,
        MetabolicNode {
            kind: BlockKind::Fermentation,
            status: BlockStatus::Active,
        },
        flux_profile,
    ));
}