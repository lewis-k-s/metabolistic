use bevy::prelude::*;
use crate::molecules::{ATP, Pyruvate, ReducingPower, OrganicWaste, try_consume_currency};

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
    commands.spawn(FermentationBlock);
    println!("FermentationBlock spawned!");
}

fn fermentation_system(
    fermentation_rate: Res<FermentationRate>,
    pyruvate: ResMut<Pyruvate>,
    reducing_power: ResMut<ReducingPower>,
    mut atp: ResMut<ATP>,
    mut organic_waste: ResMut<OrganicWaste>,
) {
    let rate = fermentation_rate.0;

    // Inputs: Pyruvate + NADH (ReducingPower)
    // Outputs: Small ATP + organic-waste

    // For simplicity, let's assume a 1:1:1:1 ratio for now.
    // Consume Pyruvate and ReducingPower
    let consumed_pyruvate = rate;
    let consumed_reducing_power = rate;

    let produced_atp = rate * 0.5; // Small ATP yield
    let produced_organic_waste = rate;

    if try_consume_currency(pyruvate, consumed_pyruvate, "Fermentation") &&
       try_consume_currency(reducing_power, consumed_reducing_power, "Fermentation")
    {
        atp.0 += produced_atp;
        organic_waste.0 += produced_organic_waste;
        // debug!("Fermentation: Produced {:.2} ATP, {:.2} OrganicWaste", produced_atp, produced_organic_waste);
    }
}
