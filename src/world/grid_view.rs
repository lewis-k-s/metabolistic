//! # Drawing the grid
//!
//! The grid is one `Image` with nearest-neighbour sampling. It has
//! `TEXELS_PER_CELL` texels along each side of a grid cell, so each texel is a
//! few world pixels. The image is rewritten after each metabolic tick.
//!
//! The grid draws O2 and water, and minerals on land. **Heat and light are not
//! drawn here**: the darkness overlay (`lighting_view`) shows the vents and the UV
//! shafts as light. `heat_glow` and `light_glow` stay, because the Manage
//! screen draws the outside of the cell with their colours. In the vents and
//! in shallow water the minerals are fine cyan specks in the lighting shader:
//! the grid texel is 4 world pixels, too coarse for a dither.
//!
//! Colours follow `docs/art-direction.md`: minerals are cyan, O2 is a sea
//! green. A glow is dithered with an ordered 4x4 pattern: a texel is either
//! fully set or fully clear, never blended. `render_grid` is a pure function
//! of the grid, so the colours and the dither can be tested without a window.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::envgrid::{CELL_SIZE, EnvGrid, bilinear_taps};
use crate::environment::Channel;
use crate::level::Vent;
use crate::InRun;

/// Texels along one side of a grid cell. A texel is `CELL_SIZE / TEXELS_PER_CELL`
/// world pixels.
pub const TEXELS_PER_CELL: usize = 8;

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
#[derive(Debug, PartialEq)]
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
    Band { lo: 1.2, hi: 6.0, color: [0x1d, 0x7a, 0xa8] },
    Band { lo: 5.0, hi: 14.0, color: [0x3e, 0xc0, 0xe0] },
    Band { lo: 14.0, hi: 30.0, color: [0x92, 0xec, 0xff] },
];

/// Light bands: a violet edge, then pale yellow. The bands start where the
/// UV is still small, so the player can see the dim edge.
const LIGHT_BANDS: [Band; 2] = [
    Band { lo: 0.2, hi: 2.5, color: [0x7c, 0x52, 0xcc] },
    Band { lo: 2.0, hi: 7.0, color: [0xff, 0xf4, 0xb4] },
];

/// O2 bands (M7): a pale sea green. The art doc has no O2 colour yet: a
/// stand-in for the **art owner**.
const O2_BANDS: [Band; 2] = [
    Band { lo: 0.4, hi: 3.0, color: [0x2a, 0x8c, 0x6a] },
    Band { lo: 2.5, hi: 8.0, color: [0x9a, 0xf0, 0xcc] },
];

/// Water bands (M8): a deep blue edge, then a pale blue. The art doc has no
/// water-on-land colour yet: a stand-in for the **art owner**.
const WATER_BANDS: [Band; 2] = [
    Band { lo: 0.4, hi: 3.0, color: [0x1d, 0x5a, 0xa8] },
    Band { lo: 2.5, hi: 8.0, color: [0x7a, 0xc4, 0xf4] },
];

/// How strongly a band shows at `level`: 0 before it starts, 1 at its top.
fn band_strength(band: &Band, level: f32) -> f32 {
    let t = ((level - band.lo) / (band.hi - band.lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn glow(bands: &[Band], level: f32) -> Vec<(Color, f32)> {
    bands
        .iter()
        .map(|b| (Color::srgb_u8(b.color[0], b.color[1], b.color[2]), band_strength(b, level)))
        .collect()
}

/// The glow of the Run grid at a heat level: a colour and a strength for each
/// band, outer to inner. The Manage screen draws the outside with it.
pub fn heat_glow(level: f32) -> Vec<(Color, f32)> {
    glow(&HEAT_BANDS, level)
}

/// The same for minerals.
pub fn mineral_glow(level: f32) -> Vec<(Color, f32)> {
    glow(&MINERAL_BANDS, level)
}

/// The same for light.
pub fn light_glow(level: f32) -> Vec<(Color, f32)> {
    glow(&LIGHT_BANDS, level)
}

/// How a level draws its grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridStyle {
    /// Draw the O2 layer. On land the air holds O2 everywhere, so it is not drawn.
    draw_o2: bool,
    /// Draw the water layer (land only).
    draw_water: bool,
    /// Draw the mineral layer. The vents and shallow water draw minerals in
    /// the lighting shader instead, as fine specks (see `lighting_view`).
    draw_minerals: bool,
}

impl GridStyle {
    /// The style of a field.
    pub fn for_field(field: crate::level::FieldKind) -> Self {
        match field {
            crate::level::FieldKind::Vents | crate::level::FieldKind::Shallow => {
                Self { draw_o2: true, draw_water: false, draw_minerals: false }
            }
            crate::level::FieldKind::Land => Self { draw_o2: false, draw_water: true, draw_minerals: true },
        }
    }
}

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

/// The bilinear level at the taps of a position, or 0 if no tap reaches
/// `lowest`. Below `lowest` no band draws, so the result is the same as a full
/// `EnvGrid::sample` for the colours.
fn level_at(taps: &[(usize, f32); 4], layer: &[f32], lowest: f32) -> f32 {
    if taps.iter().all(|&(i, _)| layer[i] < lowest) {
        return 0.0;
    }
    taps.iter().map(|&(i, w)| layer[i] * w).sum()
}

/// Size of the image in texels.
pub fn texture_size(grid: &EnvGrid) -> (usize, usize) {
    let dims = grid.dims();
    (dims.width * TEXELS_PER_CELL, dims.height * TEXELS_PER_CELL)
}

/// RGBA8 pixels of the grid, row 0 at the top. A texel is clear (alpha 0) or
/// fully set (alpha 255). Water is drawn over O2, and O2 over minerals.
pub fn render_grid(grid: &EnvGrid) -> Vec<u8> {
    render_grid_styled(grid, GridStyle::for_field(crate::level::FieldKind::Vents))
}

/// `render_grid` with the style of a level.
pub fn render_grid_styled(grid: &EnvGrid, style: GridStyle) -> Vec<u8> {
    let (width, height) = texture_size(grid);
    let texel = CELL_SIZE / TEXELS_PER_CELL as f32;
    let origin = grid.origin();
    let dims = grid.dims();
    let mut data = vec![0u8; width * height * 4];
    for ty in 0..height {
        // Image rows go down; world y goes up.
        let world_y = origin.y + (height - 1 - ty) as f32 * texel + texel * 0.5;
        for tx in 0..width {
            let world_x = origin.x + tx as f32 * texel + texel * 0.5;
            let pos = Vec2::new(world_x, world_y);
            let threshold = dither(tx, ty);
            // One set of taps for all channels. A channel below its lowest band
            // cannot draw anything here, so it is not interpolated.
            let taps = bilinear_taps(dims, origin, pos);
            let o2 = if style.draw_o2 {
                level_at(&taps, grid.layer(Channel::O2), O2_BANDS[0].lo)
            } else {
                0.0
            };
            let water = if style.draw_water {
                level_at(&taps, grid.layer(Channel::Water), WATER_BANDS[0].lo)
            } else {
                0.0
            };
            let minerals = if style.draw_minerals {
                level_at(&taps, grid.layer(Channel::Minerals), MINERAL_BANDS[0].lo)
            } else {
                0.0
            };
            let color = band_color(&WATER_BANDS, water, threshold)
            .or_else(|| band_color(&O2_BANDS, o2, threshold))
            .or_else(|| band_color(&MINERAL_BANDS, minerals, threshold));
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
pub struct GridImage(pub Handle<Image>, pub GridStyle);

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        update_grid_image
            .run_if(in_state(InRun).and_then(resource_exists_and_changed::<EnvGrid>)),
    );
}

/// Spawns the sprite that shows the grid image.
pub fn spawn_grid_view(commands: &mut Commands, grid: &EnvGrid, style: GridStyle) {
    commands.queue({
        let (width, height) = texture_size(grid);
        let data = render_grid_styled(grid, style);
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
            world.insert_resource(GridImage(handle.clone(), style));
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

/// Z of the sea floor (below the grid image, which is at -10).
const FLOOR_Z: f32 = -20.0;
/// The floor reaches this far past the field, so the camera never sees the
/// clear colour at the edge.
const FLOOR_MARGIN: f32 = 1200.0;

/// Spawns the deep-sea floor: one tiled sprite (`seafloor.png`, 128 x 128)
/// under the grid image. One sprite pixel is one world unit.
pub fn spawn_floor(commands: &mut Commands, asset_server: &AssetServer, origin: Vec2, size: Vec2) {
    let center = origin + size * 0.5;
    commands.spawn((
        Name::new("Sea Floor"),
        Sprite {
            image: asset_server.load("sprites/seafloor.png"),
            custom_size: Some(size + Vec2::splat(2.0 * FLOOR_MARGIN)),
            image_mode: SpriteImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 },
            ..default()
        },
        Transform::from_translation(center.extend(FLOOR_Z)),
        DespawnOnExit(InRun),
    ));
}

/// Z of the vent chimney (below the creatures).
const VENT_Z: f32 = -5.0;
/// Z of the vent plume. It is drawn above creatures (art doc, sprite list).
const PLUME_Z: f32 = 5.0;

/// Spawns a vent: the chimney, its round rock body, and the smoke plume above
/// the creatures. Sprites are drawn at scale 1: one sprite pixel is one world
/// unit, and the camera scale makes it a whole number of screen pixels.
pub fn spawn_vent(commands: &mut Commands, asset_server: &AssetServer, vent: Vent) {
    use avian2d::prelude::*;
    let position = vent.position.round();
    commands.spawn((
        Name::new("Vent"),
        Sprite::from_image(asset_server.load("sprites/vent_chimney_top.png")),
        Transform::from_translation(position.extend(VENT_Z)),
        DespawnOnExit(InRun),
        children![(
            // The rock is smaller than the sprite: the rest of it is glow.
            RigidBody::Static,
            Collider::circle(36.0),
            Transform::default(),
        )],
    ));
    commands.spawn((
        Name::new("Vent Plume"),
        Sprite::from_image(asset_server.load("sprites/vent_plume_top.png")),
        Transform::from_translation(position.extend(PLUME_Z)),
        DespawnOnExit(InRun),
    ));
}

/// Rewrites the grid image after each metabolic tick.
fn update_grid_image(
    grid: Res<EnvGrid>,
    image_handle: Option<Res<GridImage>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(handle) = image_handle else { return };
    if let Some(mut image) = images.get_mut(&handle.0) {
        image.data = Some(render_grid_styled(&grid, handle.1));
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
        // The land style draws the minerals of the vent field.
        let data = render_grid_styled(&grid, GridStyle::for_field(crate::level::FieldKind::Land));
        assert_eq!(data.len(), texture_size(&grid).0 * texture_size(&grid).1 * 4);
        assert!(data.chunks(4).all(|px| px[3] == 0 || px[3] == 255));
        assert!(data.chunks(4).any(|px| px[3] == 255), "something is drawn");
    }

    #[test]
    fn heat_and_light_are_not_drawn() {
        // The lighting overlay shows them. The grid draws neither.
        let mut grid = EnvGrid::centered(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                grid.set_level(Channel::Heat, x, y, 100.0);
                grid.set_level(Channel::Light, x, y, 10.0);
            }
        }
        assert!(render_grid(&grid).chunks(4).all(|px| px[3] == 0));
    }

    #[test]
    fn vents_leave_minerals_to_the_lighting() {
        let mut grid = EnvGrid::centered(8, 8);
        grid.set_level(Channel::Minerals, 4, 4, 30.0);
        assert!(render_grid(&grid).chunks(4).all(|px| px[3] == 0));
    }

    #[test]
    fn minerals_are_cyan() {
        let mut grid = EnvGrid::centered(8, 8);
        let (width, height) = texture_size(&grid);
        grid.set_level(Channel::Minerals, 6, 6, 30.0);
        let data = render_grid_styled(&grid, GridStyle::for_field(crate::level::FieldKind::Land));
        let mut texels = 0;
        for ty in 0..height {
            for tx in 0..width {
                if alpha(&data, width, tx, ty) == 0 {
                    continue;
                }
                let i = (ty * width + tx) * 4;
                assert!(data[i + 2] > data[i], "mineral colours are cyan");
                texels += 1;
            }
        }
        assert!(texels > 10);
    }

    #[test]
    fn a_stronger_glow_sets_more_texels() {
        let count = |minerals: f32| {
            let mut grid = EnvGrid::centered(8, 8);
            for y in 0..8 {
                for x in 0..8 {
                    grid.set_level(Channel::Minerals, x, y, minerals);
                }
            }
            render_grid_styled(&grid, GridStyle::for_field(crate::level::FieldKind::Land)).chunks(4).filter(|px| px[3] == 255).count()
        };
        assert_eq!(count(0.0), 0);
        assert!(count(2.0) < count(4.0));
        assert!(count(4.0) < count(8.0));
        // Above the top band, every texel is set.
        let full = 8 * TEXELS_PER_CELL * 8 * TEXELS_PER_CELL;
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
        // Minerals in the top row of grid cells must set the top rows of texels.
        let mut grid = EnvGrid::centered(4, 4);
        for x in 0..4 {
            grid.set_level(Channel::Minerals, x, 3, 100.0);
        }
        let (width, height) = texture_size(&grid);
        let data = render_grid_styled(&grid, GridStyle::for_field(crate::level::FieldKind::Land));
        assert_eq!(alpha(&data, width, 5, 0), 255, "top row is set");
        assert_eq!(alpha(&data, width, 5, height - 1), 0, "bottom row is clear");
    }
}
