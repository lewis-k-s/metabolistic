//! # Drawing the secreted clouds
//!
//! `secretion::render_cloudmaps` gives two textures, one texel for each grid
//! cell. This module keeps them in line with the grid and draws one quad over
//! the field with `assets/shaders/clouds.wgsl`. The shader draws each cloud as
//! a smooth haze on the sprite pixel grid, with a soft edge near the producer
//! and its own colour and moving pattern (see the shader header). This is the
//! one blended effect of the world, by request: the clouds must read as
//! substances, not as particles. The quad is in the water layer: cells and
//! pickups swim over it, and the scene light falls on it as on the world.
//!
//! Colours: microcin in the `toxin` ramp (hatch), slime in the `phb` ramp
//! (gel cells), AI-2 in the `player` ramp (rings), the siderophore in the
//! `metal` ramp (dark swirls when free, light beads when it holds iron).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::InRun;
use crate::envgrid::EnvGrid;
use crate::secretion::{any_cloud, render_cloudmaps};

const SHADER_PATH: &str = "shaders/clouds.wgsl";
/// Z of the quad: in the water, under the cells (0) and the pickups (-3),
/// over the walls (-4) and the sea floor. The darkness (`lighting_view`, 20)
/// covers it like the rest of the world.
const CLOUD_Z: f32 = -3.5;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CloudMaterial {
    /// The field: size (xy) and lower-left corner (zw), in world units.
    #[uniform(0)]
    pub size: Vec4,
    /// Microcin (R), slime (G), AI-2 (B).
    #[texture(1)]
    #[sampler(2)]
    pub map_a: Handle<Image>,
    /// Free siderophore (R), iron complex (G).
    #[texture(3)]
    #[sampler(4)]
    pub map_b: Handle<Image>,
}

impl Material2d for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// The cloud maps of the running level.
#[derive(Resource)]
struct CloudMaps {
    handles: [Handle<Image>; 2],
    /// True while the maps hold a cloud. When all clouds are gone, one last
    /// write clears them, and then the writes stop.
    drawn: bool,
}

pub fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<CloudMaterial>::default())
        .add_systems(
            Update,
            update_cloudmaps
                .run_if(resource_exists_and_changed::<EnvGrid>)
                .run_if(in_state(InRun)),
        )
        .add_systems(OnExit(InRun), |mut commands: Commands| {
            commands.remove_resource::<CloudMaps>();
        });
}

fn blank_image(width: u32, height: u32) -> Image {
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (width * height * 4) as usize],
        // Linear: the texels are levels, not colours.
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

/// Spawns the cloud maps and the quad over the field.
pub fn spawn_clouds(commands: &mut Commands, grid: &EnvGrid) {
    let dims = grid.dims();
    let size = grid.world_size();
    let origin = grid.origin();
    let center = origin + size * 0.5;
    commands.queue(move |world: &mut World| {
        let (w, h) = (dims.width as u32, dims.height as u32);
        let mut images = world.resource_mut::<Assets<Image>>();
        let handles = [images.add(blank_image(w, h)), images.add(blank_image(w, h))];
        world.insert_resource(CloudMaps { handles: handles.clone(), drawn: false });
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Rectangle::new(size.x, size.y));
        let [map_a, map_b] = handles;
        let material = world.resource_mut::<Assets<CloudMaterial>>().add(CloudMaterial {
            size: Vec4::new(size.x, size.y, origin.x, origin.y),
            map_a,
            map_b,
        });
        world.spawn((
            Name::new("Secreted Clouds"),
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_translation(center.extend(CLOUD_Z)),
            DespawnOnExit(InRun),
        ));
    });
}

fn update_cloudmaps(grid: Res<EnvGrid>, maps: Option<ResMut<CloudMaps>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut maps) = maps else { return };
    let any = any_cloud(&grid);
    if !any && !maps.drawn {
        return;
    }
    for (handle, data) in maps.handles.iter().zip(render_cloudmaps(&grid)) {
        if let Some(mut image) = images.get_mut(handle) {
            image.data = Some(data);
        }
    }
    maps.drawn = any;
}
