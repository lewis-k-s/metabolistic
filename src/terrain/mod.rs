use bevy::prelude::*;

// Generating terrain procedurally

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, generate_terrain);
    }
}

fn generate_terrain(mut commands: Commands) {
    commands.spawn(TerrainBundle::default());
}