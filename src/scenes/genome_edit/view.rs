//! src/scenes/genome_edit/view.rs
//!
//! This module is responsible for the visual representation of the genome
//! editing scene. It listens for the creation of logical `GenomeSection`
//! entities and spawns the corresponding visual meshes. It also handles
//! all visual effects, like highlighting and rotation.

use super::{logic, GenomeSceneEntity};
use crate::{
    blocks::genome::{self, BlockKind, GeneState},
    GameState,
};
use bevy::{
    color::palettes::basic::{BLUE, GRAY, GREEN, LIME, MAROON, PURPLE, RED, YELLOW},
    prelude::*,
};
use std::f32::consts::TAU;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::GenomeEditing), setup_view)
            .add_systems(
                Update,
                (
                    spawn_visuals,
                    rotate_genome_ring,
                    update_visuals_from_state,
                )
                    .run_if(in_state(GameState::GenomeEditing)),
            );
    }
}

// --- Component Definitions ---

/// A marker component for the root entity of the entire genome ring.
#[derive(Component)]
struct GenomeRoot;

/// A component added to a visual helix mesh entity. It holds the entity ID
/// of the logical `GenomeSection` it represents, linking the view to the logic.
#[derive(Component)]
struct VisualHelix {
    logical_entity: Entity,
}

// --- Systems ---

/// Sets up the camera, lighting, and the root entity for the genome ring.
fn setup_view(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        GenomeSceneEntity,
    ));
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 100.0,
    });
    commands.spawn((
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 3000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, 0.5, 0.0)),
        GenomeSceneEntity,
    ));
    commands.spawn((
        GenomeRoot,
        GenomeSceneEntity,
        Transform::default(),
        Visibility::default(),
        Name::new("Genome Root"),
    ));
}

/// Spawns a visual representation for each new logical `GenomeSection`.
/// This runs on `Update` and reacts to `Added<logic::GenomeSection>`,
/// making it robust against race conditions during setup.
fn spawn_visuals(
    mut commands: Commands,
    // Query for logical sections that were just added.
    added_query: Query<(Entity, &logic::GenomeSection), Added<logic::GenomeSection>>,
    root_query: Query<Entity, With<GenomeRoot>>,
    state: Res<logic::GenomeEditorState>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    genome: Res<genome::Genome>,
) {
    let Ok(root_entity) = root_query.get_single() else {
        return;
    };

    let num_blocks = state.blocks.len();
    if num_blocks == 0 {
        return;
    }
    // Adjust the radius to provide an offset, preventing overlap.
    let ring_radius = 5.0;
    let helices_per_block = 8;
    let total_helices = num_blocks * helices_per_block;
    let helix_scale = 0.15;

    for (logical_entity, section) in added_query.iter() {
        let state = genome
            .get_gene_state(&section.block_kind)
            .unwrap_or(&GeneState::Silent);
        let color = get_block_color(section.block_kind, state);

        // Each section is a block of 8 helices.
        for i in 0..helices_per_block {
            let helix_index = section.section_index * helices_per_block + i;
            let angle = helix_index as f32 / total_helices as f32 * TAU;
            let x = ring_radius * angle.cos();
            let z = ring_radius * angle.sin();

            let translation = Vec3::new(x, 0.0, z);

            // Calculate a point slightly ahead on the tangent to orient the helix.
            let tangent_point = Vec3::new(
                ring_radius * (angle + 0.1).cos(),
                0.0,
                ring_radius * (angle + 0.1).sin(),
            );

            // Use `looking_at` to point the helix along the ring's curve.
            let mut transform =
                Transform::from_translation(translation).looking_at(tangent_point, Vec3::Y);

            // Add a corrective rotation to align the model's length with the tangent.
            transform.rotate_local_y(std::f32::consts::FRAC_PI_2);
            transform.scale = Vec3::splat(helix_scale);

            commands.entity(root_entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(asset_server.load("gltf/scene.gltf#Mesh0/Primitive0")),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: color,
                        metallic: 0.2,
                        perceptual_roughness: 0.4,
                        ..default()
                    })),
                    transform, // Use the calculated transform
                    VisualHelix { logical_entity },
                    GenomeSceneEntity,
                    Name::new(format!("Visual Helix for {:?}", section.block_kind)),
                ));
            });
        }
    }
}

/// Rotates the entire genome ring by rotating the `GenomeRoot` parent entity.
fn rotate_genome_ring(time: Res<Time>, mut query: Query<&mut Transform, With<GenomeRoot>>) {
    if let Ok(mut transform) = query.get_single_mut() {
        transform.rotate_y(time.delta_secs() * 0.3);
    }
}

/// A single system to update all visual properties of the helices (color, emissiveness)
/// based on the state of the logical entities they represent.
fn update_visuals_from_state(
    mut materials: ResMut<Assets<StandardMaterial>>,
    genome: Res<genome::Genome>,
    // Query for all visual helices and their materials.
    visuals_query: Query<(&MeshMaterial3d<StandardMaterial>, &VisualHelix)>,
    // Query to get data from the logical entities.
    logic_query: Query<(&logic::GenomeSection, Option<&logic::SelectedSection>)>,
) {
    for (material_component, helix_link) in visuals_query.iter() {
        // Get the logical entity that corresponds to this visual helix.
        if let Ok((section, opt_selected)) = logic_query.get(helix_link.logical_entity) {
            // Get the material asset so we can modify it.
            if let Some(mat) = materials.get_mut(&material_component.0) {
                // 1. Update color based on GeneState
                let gene_state = genome
                    .get_gene_state(&section.block_kind)
                    .unwrap_or(&GeneState::Silent);
                mat.base_color = get_block_color(section.block_kind, gene_state);

                // 2. Update emissive based on whether it's selected
                if opt_selected.is_some() {
                    // This helix is part of the selected section, make it glow.
                    mat.emissive = mat.base_color.to_linear() * 1.5;
                } else {
                    // Not selected, no glow.
                    mat.emissive = LinearRgba::BLACK;
                }
            }
        }
    }
}

/// Gets a distinctive color for each block kind, adjusted for its state.
fn get_block_color(block_kind: BlockKind, state: &GeneState) -> Color {
    let base_color = match block_kind {
        BlockKind::LightCapture => YELLOW.into(),
        BlockKind::SugarCatabolism => Color::srgb(1.0, 0.5, 0.0), // Orange
        BlockKind::OrganicAcidOxidation => RED.into(),
        BlockKind::Respiration => BLUE.into(),
        BlockKind::Fermentation => PURPLE.into(),
        BlockKind::NitrogenSulfurAssimilation => GREEN.into(),
        BlockKind::AminoAcidBiosynthesis => Color::srgb(0.0, 1.0, 1.0), // Cyan
        BlockKind::LipidMetabolism => LIME.into(),
        BlockKind::NucleotideCofactorSynthesis => MAROON.into(),
        BlockKind::SecondaryMetabolites => Color::srgb(1.0, 0.5, 0.8), // Pink
        BlockKind::AromaticPrecursorSynthesis => Color::srgb(0.5, 0.8, 1.0), // Light blue
        BlockKind::Polymerization => Color::srgb(0.8, 0.6, 0.4), // Brown
    };

    match state {
        GeneState::Expressed => base_color,
        GeneState::Silent => base_color.with_alpha(0.3),
        GeneState::Mutated => GRAY.into(),
    }
}
