// The clouds that cells secrete (`world::cloud_view`, `secretion`).
//
// Two cloud maps have one texel for each grid cell (linear filter). Map A:
// microcin (R), slime (G), AI-2 (B). Map B: free siderophore (R), iron
// complex (G). A level of 1 is the level next to a still producer.
//
// Each fragment snaps to a sprite pixel, so the cloud sits on the pixel grid
// of the sprites. A cloud is a smooth haze: its opacity grows with the level
// and falls to 0 at a soft edge (`EDGE_LO` to `EDGE_HI`), so it stays near
// the producer. Each cloud has its own colour and its own moving pattern, so
// the player can tell them apart:
//
// | Cloud | Colour | Pattern |
// |---|---|---|
// | Microcin | acid green (`toxin`) | wavy diagonal hatch that slides |
// | Slime | pale (`phb`) | gel cells with darker walls, slow wobble |
// | Free siderophore | dark gold (`metal`) | soft swirls (noise) |
// | Iron complex | light gold (`metal`) | round beads that glint |
// | AI-2 | mint (`player`) | rings that move out from the caller |
//
// The clouds are drawn in this order, each over the last: slime, free
// siderophore, iron complex, AI-2, microcin.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::globals

// The field: size (xy) and lower-left corner (zw), in world units.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> size: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var map_a: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var map_a_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var map_b: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var map_b_sampler: sampler;

// The soft edge of a cloud, in level units (0..1): below EDGE_LO nothing,
// above EDGE_HI full opacity.
const EDGE_LO: f32 = 0.25;
const EDGE_HI: f32 = 0.8;
// Largest opacity of a cloud.
const MAX_ALPHA: f32 = 0.7;
// About half a grid cell in world units: the blur of the level samples.
const BLUR: f32 = 12.0;

// Palette tones (linear). See `art/gen_sprites.py`, RAMPS.
const TOXIN_SHADOW: vec3<f32> = vec3<f32>(0.051, 0.188, 0.007);
const TOXIN_BASE: vec3<f32> = vec3<f32>(0.188, 0.527, 0.023);
const TOXIN_LIGHT: vec3<f32> = vec3<f32>(0.456, 0.855, 0.089);
const PHB_SHADOW: vec3<f32> = vec3<f32>(0.352, 0.352, 0.279);
const PHB_BASE: vec3<f32> = vec3<f32>(0.716, 0.716, 0.604);
const PLAYER_SHADOW: vec3<f32> = vec3<f32>(0.045, 0.254, 0.162);
const PLAYER_LIGHT: vec3<f32> = vec3<f32>(0.456, 0.871, 0.479);
const METAL_SHADOW: vec3<f32> = vec3<f32>(0.254, 0.162, 0.014);
const METAL_BASE: vec3<f32> = vec3<f32>(0.658, 0.434, 0.030);
const METAL_LIGHT: vec3<f32> = vec3<f32>(0.905, 0.745, 0.138);

// --- Noise ---

fn hash21(p: vec2<i32>, salt: u32) -> f32 {
    var h = u32(p.x) * 374761393u + u32(p.y) * 668265263u + salt * 2246822519u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffffu) / 16777216.0;
}

// Smooth value noise in 0..1.
fn value_noise(p: vec2<f32>, salt: u32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i, salt);
    let b = hash21(i + vec2<i32>(1, 0), salt);
    let c = hash21(i + vec2<i32>(0, 1), salt);
    let d = hash21(i + vec2<i32>(1, 1), salt);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Distances to the nearest and the second nearest feature point (cells of
// size 1). The points move slowly in small circles.
fn voronoi(p: vec2<f32>, salt: u32, t: f32) -> vec2<f32> {
    let i = vec2<i32>(floor(p));
    var f1 = 9.0;
    var f2 = 9.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let cell = i + vec2<i32>(x, y);
            let phase = 6.2832 * hash21(cell, salt + 5u);
            let jitter = vec2<f32>(hash21(cell, salt), hash21(cell, salt + 1u)) * 0.7 + 0.15
                + 0.12 * vec2<f32>(cos(t + phase), sin(t + phase));
            let d = length(vec2<f32>(cell) + jitter - p);
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    return vec2<f32>(f1, f2);
}

// --- Levels ---

// The levels at `uv`, blurred over about a grid cell, so the haze has no
// corners from the bilinear filter. Returns map A and map B.
fn blurred(uv: vec2<f32>) -> array<vec4<f32>, 2> {
    let du = vec2<f32>(BLUR / size.x, 0.0);
    let dv = vec2<f32>(0.0, BLUR / size.y);
    let offsets = array<vec2<f32>, 4>(du, -du, dv, -dv);
    var a = textureSampleLevel(map_a, map_a_sampler, uv, 0.0) * 2.0;
    var b = textureSampleLevel(map_b, map_b_sampler, uv, 0.0) * 2.0;
    for (var k = 0; k < 4; k++) {
        a += textureSampleLevel(map_a, map_a_sampler, uv + offsets[k], 0.0);
        b += textureSampleLevel(map_b, map_b_sampler, uv + offsets[k], 0.0);
    }
    return array<vec4<f32>, 2>(a / 6.0, b / 6.0);
}

// The opacity of a cloud at a level: 0 at the edge, MAX_ALPHA in the core.
fn coverage(level: f32) -> f32 {
    return smoothstep(EDGE_LO, EDGE_HI, level) * MAX_ALPHA;
}

// Puts a layer (straight colour, alpha) over `under` (premultiplied).
fn over(under: vec4<f32>, color: vec3<f32>, alpha: f32) -> vec4<f32> {
    return vec4<f32>(color * alpha + under.rgb * (1.0 - alpha), alpha + under.a * (1.0 - alpha));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = floor(in.world_position.xy);
    let p = pixel + vec2<f32>(0.5);
    let local = (p - size.zw) / size.xy;
    let uv = vec2<f32>(local.x, 1.0 - local.y);
    let levels = blurred(uv);
    let a = levels[0];
    let b = levels[1];
    let t = globals.time;
    var out = vec4<f32>(0.0);

    // Slime: gel cells. The walls are darker and denser than the inside.
    if a.g > EDGE_LO {
        let v = voronoi(p / 14.0, 53u, t * 0.4);
        let wall = 1.0 - smoothstep(0.03, 0.16, v.y - v.x);
        let color = mix(PHB_BASE, PHB_SHADOW, wall);
        out = over(out, color, coverage(a.g) * mix(0.7, 1.0, wall));
    }
    // Free siderophore: soft swirls that drift.
    if b.r > EDGE_LO {
        let q = p / 26.0 + vec2<f32>(t * 0.08, -t * 0.05);
        let n = 0.65 * value_noise(q, 61u) + 0.35 * value_noise(q * 2.3, 67u);
        let swirl = smoothstep(0.35, 0.75, n);
        out = over(out, mix(METAL_SHADOW, METAL_BASE, 0.4 + 0.6 * swirl), coverage(b.r) * mix(0.4, 0.95, swirl));
    }
    // Iron complex: round beads that glint.
    if b.g > EDGE_LO {
        let v = voronoi(p / 11.0 + vec2<f32>(0.0, t * 0.05), 71u, t * 0.6);
        let bead = 1.0 - smoothstep(0.18, 0.3, v.x);
        let glint = smoothstep(0.6, 1.0, sin(t * 3.0 + 40.0 * v.y));
        let core = 1.0 - smoothstep(0.05, 0.14, v.x);
        let color = mix(METAL_LIGHT, vec3<f32>(1.0, 0.97, 0.8), max(core, glint) * 0.7);
        out = over(out, color, min(1.0, coverage(b.g) * 1.4) * bead);
    }
    // AI-2: rings that move out. A ring follows a line of equal level, so it
    // is centred on the caller.
    if a.b > EDGE_LO {
        let wave = 0.5 + 0.5 * sin(a.b * 26.0 + t * 3.0);
        let ring = smoothstep(0.55, 0.9, wave);
        let color = mix(PLAYER_SHADOW, PLAYER_LIGHT, ring);
        out = over(out, color, coverage(a.b) * mix(0.25, 1.0, ring));
    }
    // Microcin: a wavy diagonal hatch that slides along.
    if a.r > EDGE_LO {
        let s = fract((p.x + p.y + 3.0 * sin(p.y / 11.0 + t)) / 9.0 - t * 0.35);
        let band = smoothstep(0.0, 0.12, s) * (1.0 - smoothstep(0.42, 0.54, s));
        let color = mix(TOXIN_BASE, TOXIN_LIGHT, band * clamp(a.r, 0.0, 1.0));
        out = over(out, color, coverage(a.r) * mix(0.6, 1.0, band));
    }
    if out.a <= 0.001 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(out.rgb / out.a, out.a);
}
