use crate::{
    blocks::genome::{self, BlockKind, GeneState},
    GameState,
};
use bevy::color::palettes::basic::{BLUE, GRAY, GREEN, LIME, MAROON, PURPLE, RED, YELLOW};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Genome editing scene plugin
pub struct GenomeEditPlugin;

impl Plugin for GenomeEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::GenomeEditing), setup_genome_scene)
            .add_systems(
                Update,
                (
                    navigate_genome,
                    highlight_selection,
                    rotate_genome_ring,
                ).run_if(in_state(GameState::GenomeEditing)),
            )
            .add_systems(OnExit(GameState::GenomeEditing), cleanup_genome_scene);
    }
}

// --- Component Definitions ---

/// A marker component for any entity that is part of the genome editing scene.
/// Used for easy cleanup.
#[derive(Component)]
struct GenomeSceneEntity;

/// A marker component for the root entity of the entire genome ring.
/// This allows us to easily query for and manipulate the entire structure.
#[derive(Component)]
struct GenomeRoot;

/// Component to store data about a specific section (a group of helices) of the genome.
#[derive(Component)]
struct GenomeSection {
    block_kind: BlockKind,
    section_index: usize,
}

// --- Resource Definition ---

/// A resource to hold the state of the genome editing scene, like the currently selected section.
#[derive(Resource, Default)]
struct GenomeSceneState {
    selected: usize,
    blocks: Vec<BlockKind>,
}

// --- Systems and Functions ---

/// Gets a distinctive color for each block kind, adjusted for its state (expressed, silent, mutated).
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

    // Modify color based on gene state
    match state {
        GeneState::Expressed => base_color,
        GeneState::Silent => base_color.with_alpha(0.3), // Dimmed using alpha
        GeneState::Mutated => GRAY.into(),
    }
}

/// Sets up the entire genome editing scene, including the camera, lighting, and the genome ring itself.
fn setup_genome_scene(
    mut commands: Commands,
    genome: Res<genome::Genome>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let blocks: Vec<BlockKind> = genome.table.keys().copied().collect();

    commands.insert_resource(GenomeSceneState {
        selected: 0,
        blocks: blocks.clone(),
    });

    // --- Setup camera and lighting ---
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 3.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
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

    // --- Create the Genome Ring using a Parent-Child Hierarchy ---
    // First, spawn a single parent entity that will act as the root of the entire genome ring.
    // It has a transform, but no mesh or material itself.
    commands
        .spawn((
            GenomeRoot, // Mark this as the root
            GenomeSceneEntity,
            Transform::default(), // Provides a Transform at origin
            Visibility::default(), // Required for spatial entities
            Name::new("Genome Root"),
        ))
        .with_children(|parent| {
            // Now, spawn all the individual helices as children of the `GenomeRoot` entity.
            // Their transforms will be relative to the parent's transform.
            let num_blocks = blocks.len();
            let ring_radius = 4.0;
            let helices_per_block = 8;
            let total_helices = num_blocks * helices_per_block;
            let helix_scale = 0.15;

            for helix_index in 0..total_helices {
                let block_index = helix_index / helices_per_block;
                let block_kind = blocks[block_index];

                // Calculate the LOCAL position for this helix relative to the parent's center (0,0,0).
                let angle = helix_index as f32 / total_helices as f32 * TAU;
                let x = ring_radius * angle.cos();
                let z = ring_radius * angle.sin();

                let state = genome
                    .get_gene_state(&block_kind)
                    .unwrap_or(&GeneState::Silent);
                let color = get_block_color(block_kind, state);

                // Spawn the child helix entity using mesh and material components.
                parent.spawn((
                    Mesh3d(asset_server.load("gltf/scene.gltf#Mesh0/Primitive0")),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: color,
                        metallic: 0.2,
                        perceptual_roughness: 0.4,
                        ..default()
                    })),
                    Transform {
                        translation: Vec3::new(x, 0.0, z),
                        rotation: Quat::from_rotation_y(angle + std::f32::consts::FRAC_PI_2),
                        scale: Vec3::splat(helix_scale),
                    },
                    GenomeSection {
                        block_kind,
                        section_index: block_index,
                    },
                    Name::new(format!("Genome Helix {}: {:?}", helix_index, block_kind)),
                ));
            }
        });
}

/// Rotates the entire genome ring by rotating only the `GenomeRoot` parent entity.
/// Bevy's transform propagation handles the rest automatically.
fn rotate_genome_ring(
    time: Res<Time>,
    mut query: Query<&mut Transform, With<GenomeRoot>>, // Query for the single parent
) {
    // There should only be one GenomeRoot, so get_single_mut is appropriate.
    if let Ok(mut transform) = query.get_single_mut() {
        transform.rotate_y(time.delta_secs() * 0.3); // Rotate the whole ring
    }
}

/// System to handle keyboard navigation for selecting genome sections.
fn navigate_genome(input: Res<ButtonInput<KeyCode>>, mut scene_state: ResMut<GenomeSceneState>) {
    if input.just_pressed(KeyCode::ArrowRight) {
        scene_state.selected = (scene_state.selected + 1) % scene_state.blocks.len();
    } else if input.just_pressed(KeyCode::ArrowLeft) {
        if scene_state.selected == 0 {
            scene_state.selected = scene_state.blocks.len() - 1;
        } else {
            scene_state.selected -= 1;
        }
    }
}

/// System to update the material properties of genome sections based on the current selection.
fn highlight_selection(
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Query for the material handle and section data of each visible helix.
    query: Query<(&MeshMaterial3d<StandardMaterial>, &GenomeSection)>,
    scene_state: Res<GenomeSceneState>,
    genome: Res<genome::Genome>,
) {
    for (material_handle, section) in query.iter() {
        // Get a mutable reference to the material asset itself from the handle.
        if let Some(mat) = materials.get_mut(&material_handle.0) {
            let state = genome
                .get_gene_state(&section.block_kind)
                .unwrap_or(&GeneState::Silent);
            let base_color = get_block_color(section.block_kind, state);

            if section.section_index == scene_state.selected {
                // Brighten the selected section and make it emissive for a glow effect.
                mat.base_color = base_color.with_alpha(1.0); // Ensure it's fully opaque
                mat.emissive = base_color.to_linear() * 0.5; // Make it glow
            } else {
                // Revert non-selected sections to their standard appearance.
                mat.base_color = base_color;
                mat.emissive = LinearRgba::BLACK;
            }
        }
    }
}

/// Cleans up all entities created for the genome scene upon exiting the state.
fn cleanup_genome_scene(mut commands: Commands, entities: Query<Entity, With<GenomeSceneEntity>>) {
    for entity in &entities {
        commands.entity(entity).despawn_recursive();
    }
    commands.remove_resource::<GenomeSceneState>();
}
