//! # Environment shapes in the world
//!
//! The shapes of the level (`shapes`, `LevelDef::shapes`) are drawn by pixel
//! snapped shaders on meshes, and rock has an exact physics body.
//!
//! - **Rock**: a mesh of facets (`Shape::facets`) with `RockMaterial`
//!   (`assets/shaders/rock.wgsl`), and a static compound collider of the convex
//!   parts. The tiles in the `WallMap` (kind `Rock`) serve the grid and the
//!   creature rules; they have no body of their own (see `walls_view`).
//!
//! The mesh is in world coordinates, so the shader snaps world positions to
//! the sprite pixel grid by itself and needs no low-res canvas.

use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::level::LevelDef;
use crate::shapes::{Facet, Shape, ShapeKind};
use crate::world::Layer;
use crate::InRun;

const ROCK_SHADER: &str = "shaders/rock.wgsl";
/// Z of the shapes: with the wall sprites, below the creatures.
const SHAPE_Z: f32 = -3.5;
/// The rock ramp (`RAMPS["rock"]` of the generator, with a lighter top tone so
/// the facets stand out from the water).
const ROCK_RAMP: [&str; 5] = ["#05070d", "#202a46", "#34415f", "#52617f", "#7f8fb0"];
/// The mat ramp: dark, shadow, base, light, highlight.
const MAT_RAMP: [&str; 5] = ["#10200f", "#24421c", "#3f7a3d", "#6aa650", "#a8d878"];
/// The `head`, `tail` and pore tones of the membrane (see `art/gen_sprites.py`).
const HEAD_TONES: [&str; 2] = ["#ffb0a0", "#fff0e8"];
const TAIL_TONES: [&str; 2] = ["#4a1a34", "#a04a60"];
const PORE_TONES: [&str; 2] = ["#b07a28", "#f2c773"];
/// How far a cell with a transporter opens the pores, in world units.
pub const PORE_REACH: f32 = 110.0;
const MEMBRANE_SHADER: &str = "shaders/membrane.wgsl";
const MAT_SHADER: &str = "shaders/mat.wgsl";
/// The vent ramp, tones 1 to 3.
const GLOW_RAMP: [&str; 3] = ["#b02c0c", "#ee6a14", "#ffb030"];
/// How many vents the rock shader can know.
pub const MAX_VENTS: usize = 16;
/// How far a vent warms the rock, in world units.
const GLOW_REACH: f32 = 260.0;

#[derive(ShaderType, Debug, Clone)]
pub struct RockParams {
    pub tones: [Vec4; 5],
    pub glow: [Vec4; 3],
    pub vents: [Vec4; MAX_VENTS],
    pub counts: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct RockMaterial {
    #[uniform(0)]
    pub params: RockParams,
}

impl Material2d for RockMaterial {
    fn fragment_shader() -> ShaderRef {
        ROCK_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

fn linear<const N: usize>(codes: [&str; N]) -> [Vec4; N] {
    codes.map(|code| {
        let srgb = Srgba::hex(code).unwrap_or(Srgba::WHITE);
        Vec4::from_array(LinearRgba::from(srgb).to_f32_array())
    })
}

/// The shader inputs for a level: the ramps and the vents.
pub fn rock_params(def: &LevelDef) -> RockParams {
    let mut vents = [Vec4::ZERO; MAX_VENTS];
    for (slot, vent) in vents.iter_mut().zip(&def.vents) {
        *slot = Vec4::new(vent.position.x, vent.position.y, GLOW_REACH + vent.radius, 0.0);
    }
    RockParams {
        tones: linear(ROCK_RAMP),
        glow: linear(GLOW_RAMP),
        vents,
        counts: Vec4::new(def.vents.len().min(MAX_VENTS) as f32, 0.0, 0.0, 0.0),
    }
}

/// A mesh of the facets, in world coordinates. `uv.x` is the depth to the
/// outer edge, the normal is the face normal.
pub fn facet_mesh(facets: &[Facet]) -> Mesh {
    let mut positions = Vec::with_capacity(facets.len() * 3);
    let mut normals = Vec::with_capacity(facets.len() * 3);
    let mut uvs = Vec::with_capacity(facets.len() * 3);
    for facet in facets {
        for i in 0..3 {
            positions.push([facet.points[i].x, facet.points[i].y, 0.0]);
            normals.push(facet.normal);
            uvs.push([facet.depth[i], 0.0]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

#[derive(ShaderType, Debug, Clone)]
pub struct MatParams {
    pub tones: [Vec4; 5],
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MatMaterial {
    #[uniform(0)]
    pub params: MatParams,
}

impl Material2d for MatMaterial {
    fn fragment_shader() -> ShaderRef {
        MAT_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

#[derive(ShaderType, Debug, Clone)]
pub struct MembraneParams {
    pub head: [Vec4; 2],
    pub tail: [Vec4; 2],
    pub pore: [Vec4; 2],
    pub tint: Vec4,
    /// x: 0 horizontal run, 1 vertical, 2 both (a corner). y: 1 for a pore.
    pub flags: Vec4,
    /// A cell with a transporter: xy position, z reach, w 1 if there is one.
    pub near: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MembraneMaterial {
    #[uniform(0)]
    pub params: MembraneParams,
}

impl Material2d for MembraneMaterial {
    fn fragment_shader() -> ShaderRef {
        MEMBRANE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

/// The membrane material of a tile: how the run goes (0 horizontal, 1
/// vertical, 2 corner), if it is a pore, and the tint of the wall kind.
pub fn membrane_params(orientation: f32, pore: bool, tint: Color) -> MembraneParams {
    let tint = LinearRgba::from(tint);
    MembraneParams {
        head: linear(HEAD_TONES),
        tail: linear(TAIL_TONES),
        pore: linear(PORE_TONES),
        tint: Vec4::new(tint.red, tint.green, tint.blue, 1.0),
        flags: Vec4::new(orientation, if pore { 1.0 } else { 0.0 }, 0.0, 0.0),
        near: Vec4::new(0.0, 0.0, PORE_REACH, 0.0),
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins((
        Material2dPlugin::<RockMaterial>::default(),
        Material2dPlugin::<MatMaterial>::default(),
        Material2dPlugin::<MembraneMaterial>::default(),
    ))
    .add_systems(Update, update_pore_openers.run_if(in_state(InRun)));
}

/// Tells the membrane shader where a cell with a transporter is, so that the
/// pores near it open wider.
fn update_pore_openers(
    player: Query<&crate::player::ControlledCell, With<crate::player::LocalPlayer>>,
    cells: Query<(&Transform, &crate::proteins::Proteome), With<crate::cell::Cell>>,
    mut materials: ResMut<Assets<MembraneMaterial>>,
) {
    let near = player
        .single()
        .ok()
        .and_then(|c| cells.get(c.0).ok())
        .map(|(t, p)| {
            let has = crate::walls::pore_passable(p);
            Vec4::new(t.translation.x, t.translation.y, PORE_REACH, if has { 1.0 } else { 0.0 })
        })
        .unwrap_or(Vec4::new(0.0, 0.0, PORE_REACH, 0.0));
    let changed = materials.iter().any(|(_, m)| m.params.near != near);
    if !changed {
        return;
    }
    for (_, material) in materials.iter_mut() {
        material.params.near = near;
    }
}

/// Spawns the rock shapes: a mesh and a compound collider each.
pub fn spawn_shapes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    rock_materials: &mut Assets<RockMaterial>,
    mat_materials: &mut Assets<MatMaterial>,
    def: &LevelDef,
) {
    let rock = rock_materials.add(RockMaterial { params: rock_params(def) });
    let mat = mat_materials.add(MatMaterial { params: MatParams { tones: linear(MAT_RAMP) } });
    for (index, shape) in def.shapes.iter().enumerate() {
        match shape.kind {
            ShapeKind::Rock => spawn_rock(commands, meshes, &rock, shape, index),
            ShapeKind::Mat => {
                commands.spawn((
                    Name::new(format!("Mat shape {index}")),
                    Mesh2d(meshes.add(facet_mesh(&shape.facets()))),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, SHAPE_Z - 0.2),
                    Visibility::default(),
                    DespawnOnExit(InRun),
                ));
            }
            // A membrane shape is tiles: `walls_view` draws them.
            ShapeKind::Membrane => {}
        }
    }
}

fn spawn_rock(commands: &mut Commands, meshes: &mut Assets<Mesh>, material: &Handle<RockMaterial>, shape: &Shape, index: usize) {
    let parts: Vec<_> = shape
        .parts()
        .into_iter()
        .filter_map(|part| Collider::convex_hull(part).map(|c| (Vec2::ZERO, 0.0_f32, c)))
        .collect();
    let mut entity = commands.spawn((
        Name::new(format!("Rock shape {index}")),
        Mesh2d(meshes.add(facet_mesh(&shape.facets()))),
        MeshMaterial2d(material.clone()),
        Transform::from_xyz(0.0, 0.0, SHAPE_Z),
        Visibility::default(),
        DespawnOnExit(InRun),
    ));
    if !parts.is_empty() {
        entity.insert((
            RigidBody::Static,
            Collider::compound(parts),
            CollisionLayers::new(Layer::Wall, [Layer::Cell]),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::Geometry;
    use crate::world::cell_layers;
    use std::time::Duration;

    /// A body of radius 20 is pushed east at a rock chimney (a U that opens
    /// to the north): the exact collider stops it at the wall, not at a tile.
    fn push_at_chimney() -> f32 {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::diagnostic::DiagnosticsPlugin,
            PhysicsPlugins::default().with_length_unit(20.0),
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 64.0)));
        let shape = Shape {
            kind: ShapeKind::Rock,
            at: [100.0, 0.0],
            rotation: 0.0,
            scale: [1.0, 1.0],
            geometry: Geometry::Chimney { width: 100.0, height: 100.0, wall: 20.0 },
        };
        let parts: Vec<_> = shape.parts().into_iter().filter_map(|p| Collider::convex_hull(p).map(|c| (Vec2::ZERO, 0.0_f32, c))).collect();
        app.world_mut().spawn((RigidBody::Static, Collider::compound(parts), CollisionLayers::new(Layer::Wall, [Layer::Cell])));
        let body = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::circle(20.0),
                Mass(1.0),
                LinearDamping(3.0),
                LockedAxes::ROTATION_LOCKED,
                ConstantForce(Vec2::new(300.0, 0.0)),
                cell_layers(&crate::proteins::Proteome::default(), false),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        app.finish();
        app.cleanup();
        for _ in 0..140 {
            app.update();
        }
        app.world().get::<Transform>(body).unwrap().translation.x
    }

    #[test]
    fn the_exact_collider_stops_a_cell_at_the_wall_of_a_rock_shape() {
        // The left wall of the chimney spans x 50..70, so a radius 20 body stops at x = 30.
        let x = push_at_chimney();
        assert!((25.0..36.0).contains(&x), "stopped at {x}");
    }


    #[test]
    fn the_mesh_has_three_vertices_for_each_facet() {
        let shape = Shape { kind: ShapeKind::Rock, at: [0.0, 0.0], rotation: 30.0, scale: [1.0, 1.0], geometry: Geometry::Boulder { radius: 50.0, sides: 8, seed: 1 } };
        let facets = shape.facets();
        let mesh = facet_mesh(&facets);
        assert_eq!(mesh.count_vertices(), facets.len() * 3);
    }
}
