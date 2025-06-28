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
    commands.spawn(FermentationBlock);
    println!("FermentationBlock spawned!");
}