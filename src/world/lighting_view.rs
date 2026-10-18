//! # Drawing the light
//!
//! `lighting::render_lightmap` gives one texel for each grid cell. This module
//! keeps that texture in line with the grid and draws the darkness with it:
//! two quads over the field, above the world sprites and below the UI.
//!
//! - **Dark quad**: alpha blending. Colour is the darkest tone of the `WATER`
//!   ramp, alpha is 1 minus the light.
//! - **Tint quad**: additive. It adds the warm (vent) and cool (shaft) colour
//!   of the light.
//!
//! `assets/shaders/lighting.wgsl` snaps each fragment to a sprite pixel, sorts
//! the light into five bands and dithers the band edges, so the screen has no
//! soft gradient. The key `F6` turns the lighting on and off.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d,
    RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

use crate::envgrid::EnvGrid;
use crate::lighting::{Ambient, render_lightmap};
use crate::InRun;

const SHADER_PATH: &str = "shaders/lighting.wgsl";
/// Z of the overlay: above the world sprites (the highest is the plume, 5).
const DARKNESS_Z: f32 = 20.0;
/// The darkest tone of the `WATER` ramp, `#04060d`.
const DARK_COLOR: Color = Color::srgb(0.0157, 0.0235, 0.051);
/// The debug key.
pub const TOGGLE_KEY: KeyCode = KeyCode::F6;

/// Whether the lighting is drawn.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct LightingOn(pub bool);

impl Default for LightingOn {
    fn default() -> Self {
        Self(true)
    }
}

/// The lightmap of the running level.
#[derive(Resource)]
pub struct LightmapImage {
    handle: Handle<Image>,
    ambient: Ambient,
}

/// Marks the quads of the darkness overlay.
#[derive(Component)]
pub struct DarknessQuad;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LightingKey {
    additive: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(LightingKey)]
pub struct LightingMaterial {
    /// Colour of the darkness (linear), w = 1 for the tint quad.
    #[uniform(0)]
    pub dark: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub lightmap: Handle<Image>,
    /// Size of the quad in world units (xy).
    #[uniform(3)]
    pub size: Vec4,
    pub additive: bool,
}

impl From<&LightingMaterial> for LightingKey {
    fn from(material: &LightingMaterial) -> Self {
        Self { additive: material.additive }
    }
}

impl Material2d for LightingMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.additive
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            let add = BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: add, alpha: add });
            }
        }
        Ok(())
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<LightingMaterial>::default())
        .init_resource::<LightingOn>()
        .add_systems(
            Update,
            (
                update_lightmap.run_if(resource_exists_and_changed::<EnvGrid>),
                toggle_lighting.run_if(bevy::input::common_conditions::input_just_pressed(TOGGLE_KEY)),
                apply_visibility,
            )
                .run_if(in_state(InRun)),
        )
        .add_systems(OnExit(InRun), |mut commands: Commands| {
            commands.remove_resource::<LightmapImage>();
        });
}

/// Spawns the lightmap and the two quads over the field. A level with full
/// ambient light (land) gets none.
pub fn spawn_lighting(commands: &mut Commands, grid: &EnvGrid, ambient: Ambient) {
    // Full light everywhere (land): there is nothing to draw.
    if ambient.at(0.0) >= 1.0 && ambient.at(1.0) >= 1.0 {
        return;
    }
    let dims = grid.dims();
    let data = render_lightmap(grid, ambient, &[]);
    let size = grid.world_size();
    let center = grid.origin() + size * 0.5;
    commands.queue(move |world: &mut World| {
        let mut image = Image::new(
            Extent3d { width: dims.width as u32, height: dims.height as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            // Linear: the texels are light values, not colours.
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::linear();
        let handle = world.resource_mut::<Assets<Image>>().add(image);
        world.insert_resource(LightmapImage { handle: handle.clone(), ambient });
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Rectangle::new(size.x, size.y));
        let dark = LinearRgba::from(DARK_COLOR);
        let visible = if world.resource::<LightingOn>().0 { Visibility::Inherited } else { Visibility::Hidden };
        for (name, additive, z) in [("Darkness", false, DARKNESS_Z), ("Light Tint", true, DARKNESS_Z + 0.1)] {
            let material = world.resource_mut::<Assets<LightingMaterial>>().add(LightingMaterial {
                dark: Vec4::new(dark.red, dark.green, dark.blue, if additive { 1.0 } else { 0.0 }),
                lightmap: handle.clone(),
                size: Vec4::new(size.x, size.y, 0.0, 0.0),
                additive,
            });
            world.spawn((
                Name::new(name),
                DarknessQuad,
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material),
                Transform::from_translation(center.extend(z)),
                visible,
                DespawnOnExit(InRun),
            ));
        }
    });
}

fn update_lightmap(
    grid: Res<EnvGrid>,
    lightmap: Option<Res<LightmapImage>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(lightmap) = lightmap else { return };
    if let Some(mut image) = images.get_mut(&lightmap.handle) {
        image.data = Some(render_lightmap(&grid, lightmap.ambient, &[]));
    }
}

fn toggle_lighting(mut on: ResMut<LightingOn>) {
    on.0 = !on.0;
    info!("lighting: {}", if on.0 { "on" } else { "off" });
}

fn apply_visibility(on: Res<LightingOn>, mut quads: Query<&mut Visibility, With<DarknessQuad>>) {
    if !on.is_changed() {
        return;
    }
    for mut visibility in &mut quads {
        *visibility = if on.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
}
