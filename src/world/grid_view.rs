//! # Drawing the grid
//!
//! The grid is one `Image` with nearest-neighbour sampling. It has
//! `TEXELS_PER_CELL` texels along each side of a grid cell, so each texel is a
//! few world pixels. The image is rewritten after each metabolic tick.
//!
//! Colours follow `docs/art-direction.md`: heat is orange and red, minerals are
//! cyan. A glow is dithered with an ordered 4x4 pattern: a texel is either
//! fully set or fully clear, never blended. `render_grid` is a pure function
//! of the grid, so the colours and the dither can be tested without a window.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::envgrid::{CELL_SIZE, EnvGrid};
use crate::environment::Channel;
use crate::level::Vent;
use crate::InRun;

/// Texels along one side of a grid cell. A texel is `CELL_SIZE / TEXELS_PER_CELL`
/// world pixels.
pub const TEXELS_PER_CELL: usize = 4;

/// Ordered 4x4 dither matrix, thresholds in (0, 1). Same as `BAYER4` in
/// `art/gen_sprites.py`.
const BAYER4: [[f32; 4]; 4] = [
    [0.5 / 16.0, 8.5 / 16.0, 2.5 / 16.0, 10.5 / 16.0],
    [12.5 / 16.0, 4.5 / 16.0, 14.5 / 16.0, 6.5 / 16.0],
    [3.5 / 16.0, 11.5 / 16.0, 1.5 / 16.0, 9.5 / 16.0],
    [15.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0, 5.5 / 16.0],
];

/// One glow band: it fades in from `lo` to `hi`, in channel units, and is drawn
/// in `color`.
struct Band {
    lo: f32,
    hi: f32,
    color: [u8; 3],
}

/// Heat bands, outer to inner (the `vent` ramp of the art doc). The last band
/// starts at the membrane damage threshold, so the white-hot area is the burn
/// zone.
const HEAT_BANDS: [Band; 4] = [
    Band { lo: 1.0, hi: 8.0, color: [0xb0, 0x2c, 0x0c] },
    Band { lo: 6.0, hi: 25.0, color: [0xee, 0x6a, 0x14] },
    Band { lo: 25.0, hi: crate::exchange::HEAT_DAMAGE_THRESHOLD, color: [0xff, 0xb0, 0x30] },
    Band { lo: crate::exchange::HEAT_DAMAGE_THRESHOLD, hi: 70.0, color: [0xff, 0xf0, 0xa0] },
];

/// Mineral bands (the `mineral` ramp of the art doc).
const MINERAL_BANDS: [Band; 3] = [
    Band { lo: 0.3, hi: 3.0, color: [0x1d, 0x7a, 0xa8] },
    Band { lo: 2.0, hi: 8.0, color: [0x3e, 0xc0, 0xe0] },
    Band { lo: 8.0, hi: 20.0, color: [0x92, 0xec, 0xff] },
];

/// The dither threshold of a texel.
fn dither(tx: usize, ty: usize) -> f32 {
    BAYER4[ty % 4][tx % 4]
}

/// The colour of the innermost band that is on at this level and texel.
fn band_color(bands: &[Band], level: f32, threshold: f32) -> Option<[u8; 3]> {
    let mut color = None;
    for band in bands {
        let fade = ((level - band.lo) / (band.hi - band.lo)).clamp(0.0, 1.0);
        if fade > threshold {
            color = Some(band.color);
        }
    }
    color
}

/// Size of the image in texels.
pub fn texture_size(grid: &EnvGrid) -> (usize, usize) {
    let dims = grid.dims();
    (dims.width * TEXELS_PER_CELL, dims.height * TEXELS_PER_CELL)
}

/// RGBA8 pixels of the grid, row 0 at the top. A texel is clear (alpha 0) or
/// fully set (alpha 255). Heat is drawn over minerals.
pub fn render_grid(grid: &EnvGrid) -> Vec<u8> {
    let (width, height) = texture_size(grid);
    let texel = CELL_SIZE / TEXELS_PER_CELL as f32;
    let origin = grid.origin();
    let mut data = vec![0u8; width * height * 4];
    for ty in 0..height {
        // Image rows go down; world y goes up.
        let world_y = origin.y + (height - 1 - ty) as f32 * texel + texel * 0.5;
        for tx in 0..width {
            let world_x = origin.x + tx as f32 * texel + texel * 0.5;
            let pos = Vec2::new(world_x, world_y);
            let threshold = dither(tx, ty);
            let color = band_color(&HEAT_BANDS, grid.sample(Channel::Heat, pos), threshold)
                .or_else(|| {
                    band_color(&MINERAL_BANDS, grid.sample(Channel::Minerals, pos), threshold)
                });
            if let Some([r, g, b]) = color {
                let i = (ty * width + tx) * 4;
                data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    data
}

/// The handle of the grid image.
#[derive(Resource)]
pub struct GridImage(pub Handle<Image>);

/// A mineral particle: a sprite that shows where minerals are. It is visible
/// when the minerals under it are above `threshold`. Visual only.
#[derive(Component)]
pub struct MineralParticle {
    pub threshold: f32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (update_grid_image, update_mineral_particles)
            .run_if(in_state(InRun).and_then(resource_exists_and_changed::<EnvGrid>)),
    );
}

/// Spawns the sprite that shows the grid image.
pub fn spawn_grid_view(commands: &mut Commands, grid: &EnvGrid) {
    commands.queue({
        let (width, height) = texture_size(grid);
        let data = render_grid(grid);
        let world_size = grid.world_size();
        let center = grid.origin() + world_size * 0.5;
        move |world: &mut World| {
            let mut image = Image::new(
                Extent3d {
                    width: width as u32,
                    height: height as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                data,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            image.sampler = ImageSampler::nearest();
            let handle = world.resource_mut::<Assets<Image>>().add(image);
            world.insert_resource(GridImage(handle.clone()));
            world.spawn((
                Name::new("Environment Grid"),
                Sprite {
                    image: handle,
                    custom_size: Some(world_size),
                    ..default()
                },
                Transform::from_translation(center.extend(-10.0)),
                DespawnOnExit(InRun),
            ));
        }
    });
}

/// Spawns a vent chimney: a sprite and a round rock body.
pub fn spawn_vent(commands: &mut Commands, asset_server: &AssetServer, vent: Vent) {
    use avian2d::prelude::*;
    let scale = vent.radius / 50.0;
    commands.spawn((
        Name::new("Vent"),
        Sprite::from_image(asset_server.load("sprites/vent_chimney_top.png")),
        Transform::from_translation(vent.position.extend(-5.0)).with_scale(Vec3::splat(scale)),
        DespawnOnExit(InRun),
        children![(
            // The rock is smaller than the sprite: the rest of it is glow.
            RigidBody::Static,
            Collider::circle(36.0),
            Transform::default(),
        )],
    ));
}

/// Spawns mineral particles on a fixed scatter over the field.
pub fn spawn_mineral_particles(
    commands: &mut Commands,
    asset_server: &AssetServer,
    grid: &EnvGrid,
) {
    const COUNT: u32 = 450;
    let image = asset_server.load("sprites/pickup_mineral.png");
    let origin = grid.origin();
    let size = grid.world_size();
    for i in 0..COUNT {
        // A cheap hash gives a fixed scatter and a threshold for each particle.
        let h = |salt: u32| {
            let mut x = i.wrapping_mul(0x9E37_79B1).wrapping_add(salt.wrapping_mul(0x85EB_CA6B));
            x ^= x >> 15;
            x = x.wrapping_mul(0x2C1B_3C6D);
            x ^= x >> 12;
            (x & 0xFFFF) as f32 / 65535.0
        };
        let position = origin + Vec2::new(h(1) * size.x, h(2) * size.y);
        commands.spawn((
            Name::new("Mineral Particle"),
            MineralParticle {
                threshold: 0.3 + h(3) * 4.0,
            },
            Sprite::from_image(image.clone()),
            Transform::from_translation(position.extend(-8.0)),
            Visibility::Hidden,
            DespawnOnExit(InRun),
        ));
    }
}

/// Rewrites the grid image after each metabolic tick.
fn update_grid_image(
    grid: Res<EnvGrid>,
    image_handle: Option<Res<GridImage>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(handle) = image_handle else { return };
    if let Some(mut image) = images.get_mut(&handle.0) {
        image.data = Some(render_grid(&grid));
    }
}

fn update_mineral_particles(
    grid: Res<EnvGrid>,
    mut particles: Query<(&MineralParticle, &Transform, &mut Visibility)>,
) {
    for (particle, transform, mut visibility) in &mut particles {
        let level = grid.sample(Channel::Minerals, transform.translation.truncate());
        *visibility = if level > particle.threshold {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level;

    fn alpha(data: &[u8], width: usize, tx: usize, ty: usize) -> u8 {
        data[(ty * width + tx) * 4 + 3]
    }

    #[test]
    fn the_image_has_no_partial_transparency() {
        let grid = level::vent_field();
        let data = render_grid(&grid);
        assert_eq!(data.len(), texture_size(&grid).0 * texture_size(&grid).1 * 4);
        assert!(data.chunks(4).all(|px| px[3] == 0 || px[3] == 255));
        assert!(data.chunks(4).any(|px| px[3] == 255), "something is drawn");
    }

    #[test]
    fn heat_is_orange_or_red_and_minerals_are_cyan() {
        let mut grid = EnvGrid::centered(8, 8);
        let (width, _) = texture_size(&grid);
        // A hot cell and a mineral cell, far apart.
        grid.set_level(Channel::Heat, 1, 1, 30.0);
        grid.set_level(Channel::Minerals, 6, 6, 30.0);
        let data = render_grid(&grid);
        let mut heat_texels = 0;
        let mut mineral_texels = 0;
        for ty in 0..texture_size(&grid).1 {
            for tx in 0..width {
                if alpha(&data, width, tx, ty) == 0 {
                    continue;
                }
                let i = (ty * width + tx) * 4;
                let [r, b] = [data[i], data[i + 2]];
                if r > b {
                    heat_texels += 1; // red, orange or yellow
                } else {
                    assert!(b > r, "mineral colours are cyan");
                    mineral_texels += 1;
                }
            }
        }
        assert!(heat_texels > 10 && mineral_texels > 10);
    }

    #[test]
    fn a_stronger_glow_sets_more_texels() {
        let count = |heat: f32| {
            let mut grid = EnvGrid::centered(8, 8);
            for y in 0..8 {
                for x in 0..8 {
                    grid.set_level(Channel::Heat, x, y, heat);
                }
            }
            render_grid(&grid).chunks(4).filter(|px| px[3] == 255).count()
        };
        assert_eq!(count(0.0), 0);
        assert!(count(2.0) < count(3.0));
        assert!(count(3.0) < count(5.0));
        // Above the top band, every texel is set.
        let full = 32 * 32;
        assert_eq!(count(100.0), full);
    }

    #[test]
    fn the_dither_is_an_ordered_pattern() {
        // Each 4x4 block holds every threshold once.
        let mut all: Vec<f32> = BAYER4.iter().flatten().copied().collect();
        all.sort_by(f32::total_cmp);
        for (i, v) in all.iter().enumerate() {
            assert!((v - (i as f32 + 0.5) / 16.0).abs() < 1e-6);
        }
    }

    #[test]
    fn the_image_follows_world_y_upwards() {
        // Heat in the top row of grid cells must light the top rows of texels.
        let mut grid = EnvGrid::centered(4, 4);
        for x in 0..4 {
            grid.set_level(Channel::Heat, x, 3, 100.0);
        }
        let (width, height) = texture_size(&grid);
        let data = render_grid(&grid);
        assert_eq!(alpha(&data, width, 5, 0), 255, "top row is set");
        assert_eq!(alpha(&data, width, 5, height - 1), 0, "bottom row is clear");
    }
}
