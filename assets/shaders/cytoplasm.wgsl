// The cytoplasm behind the metabolic map (`screens::manage::light`).
//
// The light at a sprite pixel is the sum of:
// - a little ambient light;
// - light through the membrane, from the outside (heat is warm, light is
//   cool), that falls with the depth below the membrane;
// - the emitters: active pathway plates and full pools, as boxes with a soft
//   falloff, each with its own colour.
// The sum is cut into five tone bands, with an ordered dither at the band
// edges (docs/art-direction.md, rule 7 and "Light and darkness"). The tint of
// the emitters shows only in the brighter bands. Slow noise moves through the
// light, as the cytoplasm streams, and a few specks drift in it.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct CytoplasmParams {
    // Dark to light (linear).
    ramp: array<vec4<f32>, 5>,
    // Per emitter: centre.xy, half size.xy.
    emit_box: array<vec4<f32>, 48>,
    // Per emitter: colour rgb (linear), strength.
    emit_col: array<vec4<f32>, 48>,
    // The quad: min.xy, size.xy (world).
    rect: vec4<f32>,
    // Time, y of the inner edge of the membrane, emitter count, ambient.
    info: vec4<f32>,
    // Membrane light: warm rgb + strength.
    warm: vec4<f32>,
    // Membrane light: cool rgb + strength.
    cool: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> c: CytoplasmParams;

// Falloff length of an emitter, in pixels.
const REACH: f32 = 26.0;
// Depth over which the membrane light falls to a third.
const DEPTH: f32 = 90.0;

fn hash(n: f32) -> f32 {
    return fract(sin(n * 127.1 + 311.7) * 43758.547);
}

fn hash2(p: vec2<f32>) -> f32 {
    return hash(p.x * 1.731 + p.y * 57.13);
}

// Smooth value noise.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let d = hash2(i + vec2<f32>(0.0, 1.0));
    let e = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(d, e, u.x), u.y);
}

fn bayer4(p: vec2<f32>) -> f32 {
    let x = u32(p.x - 4.0 * floor(p.x / 4.0));
    let y = u32(p.y - 4.0 * floor(p.y / 4.0));
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[y * 4u + x] + 0.5) / 16.0;
}

// Distance from a box (0 inside).
fn box_distance(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let q = abs(p) - half;
    return length(max(q, vec2<f32>(0.0)));
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let local = vec2<f32>(mesh.uv.x, 1.0 - mesh.uv.y) * c.rect.zw + c.rect.xy;
    let p = floor(local) + vec2<f32>(0.5);
    let t = c.info.x;

    // Light through the membrane.
    let depth = max(c.info.y - p.y, 0.0);
    let fall = exp(-depth / DEPTH);
    var light = c.info.w + (c.warm.w + c.cool.w) * fall;
    var tint = (c.warm.rgb * c.warm.w + c.cool.rgb * c.cool.w) * fall;

    // The emitters.
    let n = i32(c.info.z);
    for (var i = 0; i < n; i++) {
        let b = c.emit_box[i];
        let d = box_distance(p - b.xy, b.zw) / REACH;
        let k = c.emit_col[i].w / (1.0 + d * d * 3.0);
        light += k;
        tint += c.emit_col[i].rgb * k;
    }

    // The cytoplasm streams: slow noise drifts through the light.
    let flow = vec2<f32>(t * 3.0, sin(t * 0.21) * 9.0);
    light += (noise((p + flow) / 38.0) - 0.5) * 0.08;

    // Five bands, dithered at the edges.
    let v = clamp(light, 0.0, 1.0) * 4.0 + (bayer4(p) - 0.5) * 0.8;
    let band = clamp(i32(floor(v + 0.5)), 0, 4);
    var color = c.ramp[band];
    // The tint of the light, only in the brighter bands, in steps.
    let strength = clamp(light, 0.0, 1.0);
    if band >= 2 && strength > 0.0 {
        let hue = tint / max(light, 1e-3);
        color = vec4<f32>(mix(color.rgb, color.rgb + hue * 0.18, f32(band - 1) / 3.0), 1.0);
    }

    // Specks that drift with the stream: one pixel each.
    let drift = p + vec2<f32>(floor(t * 4.0), floor(4.0 * sin(t * 0.35 + p.x * 0.02)));
    let cell = floor(drift / 9.0);
    if all(floor(drift) - cell * 9.0 == vec2<f32>(4.0)) && hash2(cell) > 0.9 {
        color = c.ramp[min(band + 1, 4)];
    }

#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
    return color;
}
