use crate::molecules::Currency;
use crate::metabolism::{CurrencyPools, FluxProfile, MetabolicBlock, MetabolicNode, BlockStatus};
use crate::blocks::genome::BlockKind;
use bevy::prelude::*;

#[derive(Component)]
pub struct FermentationBlock;

#[derive(Resource)]
pub struct FermentationRate(pub f32);

pub struct FermentationPlugin;

impl Plugin for FermentationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FermentationRate(1.0)) // Default rate
            .add_systems(Startup, spawn_fermentation_block)
            .add_systems(FixedUpdate, fermentation_system);
    }
}

fn spawn_fermentation_block(mut commands: Commands) {
    let mut flux_profile = FluxProfile::default();
    // Define the fermentation flux profile: consumes Pyruvate and ReducingPower, produces ATP and OrganicWaste
    flux_profile.0.insert(Currency::Pyruvate, -1.0);       // Consumes 1 unit of Pyruvate
    flux_profile.0.insert(Currency::ReducingPower, -1.0);  // Consumes 1 unit of ReducingPower
    flux_profile.0.insert(Currency::ATP, 1.0);             // Produces 1 unit of ATP
    flux_profile.0.insert(Currency::OrganicWaste, 1.0);    // Produces 1 unit of OrganicWaste
    
    commands.spawn((
        FermentationBlock,
        MetabolicBlock,
        MetabolicNode {
            kind: BlockKind::Fermentation,
            status: BlockStatus::Silent, // Will be updated by genome system
        },
        flux_profile,
    ));
    println!("FermentationBlock spawned with FluxProfile!");
}

fn fermentation_system(
    fermentation_rate: Res<FermentationRate>,
    currency_pools: Res<CurrencyPools>,
    mut query_fermentation: Query<&mut FluxProfile, (With<FermentationBlock>, With<MetabolicNode>)>,
) {
    let rate = fermentation_rate.0;

    for mut flux_profile in query_fermentation.iter_mut() {
        // Check resource availability before setting flux profile
        let pyruvate_available = currency_pools.get(Currency::Pyruvate);
        let reducing_power_available = currency_pools.get(Currency::ReducingPower);
        
        // Calculate actual rate based on resource availability
        let consumed_pyruvate = rate;
        let consumed_reducing_power = rate;
        
        // Scale rate down if not enough resources
        let actual_rate = if pyruvate_available >= consumed_pyruvate && reducing_power_available >= consumed_reducing_power {
            rate
        } else {
            // Scale down based on most limiting resource
            let pyruvate_ratio = if consumed_pyruvate > 0.0 { pyruvate_available / consumed_pyruvate } else { 1.0 };
            let reducing_power_ratio = if consumed_reducing_power > 0.0 { reducing_power_available / consumed_reducing_power } else { 1.0 };
            rate * pyruvate_ratio.min(reducing_power_ratio).min(1.0)
        };
        
        // Update flux profile based on actual rate
        if actual_rate > 0.0 {
            flux_profile.0.insert(Currency::Pyruvate, -actual_rate);
            flux_profile.0.insert(Currency::ReducingPower, -actual_rate);  
            flux_profile.0.insert(Currency::ATP, actual_rate);
            flux_profile.0.insert(Currency::OrganicWaste, actual_rate);
        } else {
            // No resources available, clear flux profile
            flux_profile.0.clear();
        }
    }
}
