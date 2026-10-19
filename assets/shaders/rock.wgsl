// Rock shapes (`world::shapes_view`): faceted low-poly rock, drawn on the
// sprite pixel grid.
//
// The mesh is in world coordinates. Each triangle is a fan piece of a convex
// part with a raised centre, so it has a face normal of its own (the normal
// attribute). The fragment snaps to a sprite pixel (one world unit), lights the
// facet from the upper left, sorts the light into the bands of the `rock`
// ramp, draws a 1 px outline at the outer edge, sprinkles sulfide and mineral
// specks, and warms the edge near a vent with a dithered rim glow.
//
// uv.x is the distance in pixels to the outer edge of the part, interpolated;
// a very large value marks an edge that is inside the union of the parts.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct RockParams {
    // The ramp: outline, shadow, base, light, highlight (linear colour).
    tones: array<vec4<f32>, 5>,
    // The vent ramp, tones 1 to 3 (linear).
    glow: array<vec4<f32>, 3>,
    // Vents: xy position, z reach of the glow, w unused.
    vents: array<vec4<f32>, 16>,
    // x = vent count.
    counts: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> rock: RockParams;

// World light from the upper left (+y is up in the world).
const LIGHT: vec3<f32> = vec3<f32>(-0.45113, 0.55138, 0.70176);
const SULFIDE: vec3<f32> = vec3<f32>(0.80, 0.55, 0.12);
const MINERAL: vec3<f32> = vec3<f32>(0.05, 0.50, 0.72);

fn bayer4(p: vec2<i32>) -> f32 {
    var m = array<f32, 16>(
        0.5, 8.5, 2.5, 10.5,
        12.5, 4.5, 14.5, 6.5,
        3.5, 11.5, 1.5, 9.5,
        15.5, 7.5, 13.5, 5.5,
    );
    return m[(p.y & 3) * 4 + (p.x & 3)] / 16.0;
}

fn hash2(p: vec2<i32>) -> f32 {
    var x = u32(p.x) * 374761393u + u32(p.y) * 668265263u;
    x = (x ^ (x >> 13u)) * 1274126177u;
    x = x ^ (x >> 16u);
    return f32(x & 0xffffu) / 65536.0;
}

fn finish(c: vec4<f32>) -> vec4<f32> {
    var color = c;
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
    return color;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let wp = in.world_position.xy;
    // The depth at the centre of this sprite pixel: the interpolated value,
    // moved by its screen gradient.
    let pixel = vec2<i32>(floor(wp));
    let snapped = vec2<f32>(pixel) + vec2<f32>(0.5);
    let dx = dpdx(wp);
    let dy = dpdy(wp);
    let dd = vec2<f32>(dpdx(in.uv.x), dpdy(in.uv.x));
    let det = dx.x * dy.y - dx.y * dy.x;
    var depth = in.uv.x;
    if abs(det) > 1e-6 && in.uv.x < 5000.0 {
        let grad = vec2<f32>(
            (dd.x * dy.y - dd.y * dx.y) / det,
            (dd.y * dx.x - dd.x * dy.x) / det,
        );
        depth = in.uv.x + dot(grad, snapped - wp);
    }
    if depth < 0.0 {
        discard;
    }
    if depth < 1.0 {
        return finish(rock.tones[0]);
    }

    let n = normalize(in.world_normal);
    let lam = dot(n, LIGHT);
    // Four bands over the lit range.
    var idx = 1 + i32(lam >= 0.45) + i32(lam >= 0.62) + i32(lam >= 0.80);
    // A fine grain: some pixels step one tone down or up.
    let h = hash2(pixel);
    if h < 0.10 {
        idx = max(idx - 1, 1);
    } else if h > 0.965 {
        idx = min(idx + 1, 4);
    }
    var color = rock.tones[idx];
    // Sulfide and mineral specks.
    let s = hash2(pixel + vec2<i32>(91, 17));
    if s < 0.018 {
        color = vec4<f32>(SULFIDE, 1.0);
    } else if s > 0.992 {
        color = vec4<f32>(MINERAL, 1.0);
    }
    // The warm rim near a vent, strongest at the edge, dithered.
    let count = i32(rock.counts.x);
    var warmth = 0.0;
    for (var i = 0; i < count; i++) {
        let v = rock.vents[i];
        warmth = max(warmth, 1.0 - distance(snapped, v.xy) / v.z);
    }
    let rim = 1.0 - clamp(depth / 7.0, 0.0, 1.0);
    let heat = warmth * (0.35 + 0.65 * rim);
    if heat > bayer4(pixel) * 0.9 + 0.05 {
        color = rock.glow[min(i32(heat * 3.0), 2)];
    }
    return finish(vec4<f32>(color.rgb, 1.0));
}
