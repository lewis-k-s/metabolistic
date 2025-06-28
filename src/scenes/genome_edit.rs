use bevy::prelude::*;
use bevy::math::primitives::Cuboid;
use bevy::prelude::{Mesh3d, MeshMaterial3d};
use bevy::color::palettes::basic::YELLOW;
use crate::{GameState, blocks::genome::{self, BlockKind}};

/// Genome editing scene plugin
pub struct GenomeEditPlugin;

impl Plugin for GenomeEditPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(OnEnter(GameState::GenomeEditing), setup_genome_scene)
            .add_systems(Update, (
                navigate_genome,
                highlight_selection,
            ).run_if(in_state(GameState::GenomeEditing)))
            .add_systems(OnExit(GameState::GenomeEditing), cleanup_genome_scene);
    }
}

#[derive(Component)]
struct GenomeSceneEntity;

#[derive(Component)]
struct GenomeBlockVisual {
    index: usize,
}

#[derive(Resource, Default)]
struct GenomeSceneState {
    selected: usize,
    blocks: Vec<BlockKind>,
}

fn setup_genome_scene(
    mut commands: Commands,
    genome: Res<genome::Genome>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GenomeSceneState {
        selected: 0,
        blocks: genome.table.keys().copied().collect(),
    });

    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        GenomeSceneEntity,
    ));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
    });

    let radius = 4.0;
    let count = genome.table.len();
    for (i, block) in genome.table.keys().enumerate() {
        let angle = i as f32 / count as f32 * std::f32::consts::TAU;
        let x = radius * angle.cos();
        let z = radius * angle.sin();
        commands.spawn((
            PbrBundle {
                mesh: Mesh3d(meshes.add(Cuboid::from_size(Vec3::splat(0.5)))),
                material: MeshMaterial3d(materials.add(StandardMaterial::from(Color::WHITE))),
                transform: Transform::from_xyz(x, 1.0, z),
                ..default()
            },
            Name::new(format!("{:?}", block)),
            GenomeBlockVisual { index: i },
            GenomeSceneEntity,
        ));
    }
}

fn navigate_genome(
    input: Res<ButtonInput<KeyCode>>,
    mut scene_state: ResMut<GenomeSceneState>,
) {
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

fn highlight_selection(
    mut materials: ResMut<Assets<StandardMaterial>>,
    query: Query<(&MeshMaterial3d<StandardMaterial>, &GenomeBlockVisual)>,
    scene_state: Res<GenomeSceneState>,
) {
    for (material_handle, visual) in query.iter() {
        if let Some(mat) = materials.get_mut(&material_handle.0) {
            if visual.index == scene_state.selected {
                mat.base_color = YELLOW.into();
            } else {
                mat.base_color = Color::WHITE;
            }
        }
    }
}

fn cleanup_genome_scene(
    mut commands: Commands,
    entities: Query<Entity, With<GenomeSceneEntity>>,
) {
    for entity in entities.iter() {
        commands.entity(entity).despawn_recursive();
    }
    commands.remove_resource::<GenomeSceneState>();
}

