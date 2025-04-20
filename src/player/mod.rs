use bevy::prelude::*;
use avian3d::prelude::*;
use leafwing_input_manager::prelude::*;
use std::f32::consts::PI;
pub mod controller;

#[derive(Component)]
pub struct Player;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app
        .add_plugins(controller::CharacterControllerPlugin)
        .add_systems(Startup, spawn_player);
    }
}

pub fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let player_transform = Transform::from_xyz(0.0, 1.0, 0.0);
    let radius = 0.5;

    commands.spawn((
        Player,
        Mesh3d(meshes.add(Sphere::new(radius).mesh())),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
        player_transform,
        InputManagerBundle::with_map(controller::Action::input_map()),
        controller::CharacterControllerBundle::new(Collider::sphere(radius)).with_movement(
            30.0,
            0.9,
            7.0,
            PI * 0.45,
        ),
    )).with_children(|parent| {
        parent.spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 1.5, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    });
}
