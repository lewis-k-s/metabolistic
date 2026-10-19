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
// a vent, violet in a shaft) in the same bands, and the minerals: fine cyan
// specks, one sprite pixel each. A hash of the sprite pixel picks the specks,
// so the spacing is irregular, and the mineral level (the alpha of the
// lightmap) sets how many there are. Each speck glints on its own clock: a
// quick rise and a slow fall to the dim base, and at the peak it grows a plus.
// The light decides how bright a speck is, and how hard a glint hits.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::globals

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> dark: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var lightmap: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var lightmap_sampler: sampler;
// The field: size (xy) and lower-left corner (zw), in world units. The quad
// reaches past the field, and the lightmap clamps to its edge there.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> size: vec4<f32>;

// Input brightness of each band edge, and the share of the scene that shows
// at that band. The first band keeps a little of the scene, so the player can
// still navigate.
const IN: array<f32, 5> = array<f32, 5>(0.08, 0.25, 0.45, 0.70, 1.0);
const OUT: array<f32, 5> = array<f32, 5>(0.20, 0.38, 0.60, 0.82, 1.0);
// Strength of the additive tint.
const TINT: f32 = 0.45;
// Mineral level at which the lightmap alpha is full (`lighting::MINERAL_FULL`).
const MINERAL_FULL: f32 = 30.0;
// Highest share of pixels that a mineral speck layer sets, and the levels
// where the specks start and where they are at that share.
const MINERAL_MAX_DENSITY: f32 = 0.12;
const MINERAL_START: f32 = 2.0;
const MINERAL_SPAN: f32 = 18.0;
// The `mineral` ramp (linear), tones 1 to 4, for low, middle and high levels,
// and for the peak of a glint.
const MINERAL_LOW: vec3<f32> = vec3<f32>(0.012, 0.195, 0.392);
const MINERAL_MID: vec3<f32> = vec3<f32>(0.048, 0.527, 0.745);
const MINERAL_HIGH: vec3<f32> = vec3<f32>(0.287, 0.839, 1.0);
const MINERAL_PEAK: vec3<f32> = vec3<f32>(0.823, 1.0, 1.0);
// How bright a speck is between glints (share of its tone), at no light and at
// full light.
const SPECK_BASE_DARK: f32 = 0.12;
const SPECK_BASE_LIT: f32 = 0.5;
// How hard a glint hits at no light and at full light.
const GLINT_DARK: f32 = 0.35;
const GLINT_LIT: f32 = 1.0;
// Glint: the share of its cycle that a speck glints, and the share of that
// spent rising.
const GLINT_SHARE: f32 = 0.3;
const GLINT_RISE: f32 = 0.25;
// Warm light adds this much of its hue to a glint.
const GLINT_HUE: f32 = 0.6;
// Strength of the mineral specks: they are a hint, not a wash.
const MINERAL_STRENGTH: f32 = 0.7;

// A hash of a sprite pixel and a salt, in 0..1.
fn hash21(p: vec2<i32>, salt: u32) -> f32 {
    var h = u32(p.x) * 374761393u + u32(p.y) * 668265263u + salt * 2246822519u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffffu) / 16777216.0;
}

// The glint of the speck at a sprite pixel: -1 when there is no speck, else 0
// (between glints) to 1 (the peak). The speck has its own rate and phase.
fn speck(pixel: vec2<i32>, minerals: f32) -> f32 {
    let density = clamp((minerals - MINERAL_START) / MINERAL_SPAN, 0.0, 1.0) * MINERAL_MAX_DENSITY;
    if hash21(pixel, 0u) >= density {
        return -1.0;
    }
    let rate = 0.3 + 0.5 * hash21(pixel, 1u);
    let t = fract(globals.time * rate + hash21(pixel, 2u)) / GLINT_SHARE;
    if t >= 1.0 {
        return 0.0;
    }
    return select(1.0 - (t - GLINT_RISE) / (1.0 - GLINT_RISE), t / GLINT_RISE, t < GLINT_RISE);
}

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
    // Sample at the pixel centre, so a sprite pixel has one light value.
    let local = (snapped - size.zw) / size.xy;
    let uv = vec2<f32>(local.x, 1.0 - local.y);
    let sample = textureSampleLevel(lightmap, lightmap_sampler, uv, 0.0);
    let rgb = sample.rgb;
    let lum = max(rgb.r, max(rgb.g, rgb.b));
    let level = banded(lum, bayer4(pixel));
    if dark.w < 0.5 {
        return vec4<f32>(dark.rgb, 1.0 - level);
    }
    // The hue of the light: what is left when the grey part is taken away.
    let hue = rgb - vec3<f32>(min(rgb.r, min(rgb.g, rgb.b)));
    var color = hue * level * TINT;
    // Minerals: glinting specks. A speck is dim in the dark and catches the
    // light: the lit it is, the brighter its base and the harder its glints,
    // and a glint takes some of the warm hue of the light.
    let minerals = sample.a * MINERAL_FULL;
    let lit = clamp((level - OUT[0]) / (OUT[4] - OUT[0]), 0.0, 1.0);
    let base = mix(SPECK_BASE_DARK, SPECK_BASE_LIT, lit);
    let strength = mix(GLINT_DARK, GLINT_LIT, lit);
    var glint = speck(pixel, minerals);
    var brightness = 0.0;
    if glint >= 0.0 {
        brightness = base + (1.0 - base) * glint * strength;
    } else {
        // The arms of a plus round a glint that is at its peak.
        var arm = 0.0;
        var arms = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
        for (var i = 0; i < 4; i++) {
            arm = max(arm, speck(pixel + arms[i], minerals));
        }
        if arm > 0.7 {
            glint = arm;
            brightness = (arm - 0.5) * strength;
        }
    }
    if brightness > 0.0 {
        var tone = select(select(MINERAL_LOW, MINERAL_MID, minerals > 5.0), MINERAL_HIGH, minerals > 14.0);
        if glint > 0.85 {
            tone = MINERAL_PEAK;
        }
        color += (tone + hue * GLINT_HUE * glint) * brightness * MINERAL_STRENGTH;
    }
    return vec4<f32>(color, 0.0);
}
