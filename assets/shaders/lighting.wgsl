// The darkness over the Run field (`world::lighting_view`).
//
// The lightmap has one texel for each grid cell (RGB light, linear filter).
// Each fragment snaps to a sprite pixel (one world unit), sorts the light into
// five bands and dithers the band edges with the 4x4 Bayer matrix of
// `grid_view.rs`. A sprite pixel is in a band or it is not: no soft gradient.
//
// Two quads use this shader. The dark quad (dark.w = 0) draws the darkest
// water tone with alpha = 1 - banded light, with normal alpha blending. The
// tint quad (dark.w = 1) is additive: it adds the hue of the light (warm near
// a vent, violet in a shaft) in the same bands.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> dark: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var lightmap: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var lightmap_sampler: sampler;
// Size of the quad in world units (xy).
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> size: vec4<f32>;

// Input brightness of each band edge, and the share of the scene that shows
// at that band. The first band keeps a little of the scene, so the player can
// still navigate.
const IN: array<f32, 5> = array<f32, 5>(0.08, 0.25, 0.45, 0.70, 1.0);
const OUT: array<f32, 5> = array<f32, 5>(0.10, 0.30, 0.55, 0.80, 1.0);
// Strength of the additive tint.
const TINT: f32 = 0.45;

fn bayer4(p: vec2<i32>) -> f32 {
    var m = array<f32, 16>(
        0.5, 8.5, 2.5, 10.5,
        12.5, 4.5, 14.5, 6.5,
        3.5, 11.5, 1.5, 9.5,
        15.5, 7.5, 13.5, 5.5,
    );
    return m[(p.y & 3) * 4 + (p.x & 3)] / 16.0;
}

// The banded brightness: one of the five OUT levels, chosen by dither.
fn banded(lum: f32, threshold: f32) -> f32 {
    if lum <= IN[0] {
        return OUT[0];
    }
    for (var i = 0; i < 4; i++) {
        if lum <= IN[i + 1] {
            let f = (lum - IN[i]) / (IN[i + 1] - IN[i]);
            return select(OUT[i], OUT[i + 1], f > threshold);
        }
    }
    return OUT[4];
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The sprite pixel of this fragment, and its centre in the quad's uv.
    let pixel = vec2<i32>(floor(in.world_position.xy));
    let snapped = vec2<f32>(pixel) + vec2<f32>(0.5);
    let shift = snapped - in.world_position.xy;
    // Sample at the pixel centre, so a sprite pixel has one light value.
    let uv = in.uv + vec2<f32>(shift.x, -shift.y) / size.xy;
    let rgb = textureSampleLevel(lightmap, lightmap_sampler, uv, 0.0).rgb;
    let lum = max(rgb.r, max(rgb.g, rgb.b));
    let level = banded(lum, bayer4(pixel));
    if dark.w < 0.5 {
        return vec4<f32>(dark.rgb, 1.0 - level);
    }
    // The hue of the light: what is left when the grey part is taken away.
    let hue = rgb - vec3<f32>(min(rgb.r, min(rgb.g, rgb.b)));
    return vec4<f32>(hue * level * TINT, 0.0);
}
