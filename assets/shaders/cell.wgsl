// A cell body as a signed distance field: the run-time port of
// `art/gen_sprites.py` (`blob_sdf`, `capsule_sdf`, `Canvas.lit_body`,
// `Canvas.disc`, `Canvas.halo`, `Canvas.outline`).
//
// The camera draws at one texel per world pixel (`world::shader_cells`), so
// each fragment is one game pixel. All maths is in image coordinates (+y down),
// as in the generator, so the wobble phases and the light match the sprites.
// The body plan comes from `cell_look::CellLook`; lengths there are in units of
// the radius, here they are pixels.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct CellParams {
    // Ramps: outline, shadow, base, light, highlight (linear colour).
    tones: array<vec4<f32>, 5>,
    rim: array<vec4<f32>, 5>,
    inner: array<vec4<f32>, 5>,
    extra: array<vec4<f32>, 5>,
    glow: array<vec4<f32>, 5>,
    // Organelles: xy centre (pixels, image axes; body axes on a rod), z radius,
    // w kind (0 none, 1 nucleoid, 2 granule, 3 bead).
    organelles: array<vec4<f32>, 4>,
    // A colour multiplier of the body (the sprite tint: ROS, stun).
    tint: vec4<f32>,
    // 0 blob, 1 rod.
    shape: u32,
    // Bit flags of `cell_look::feature`.
    features: u32,
    spikes: u32,
    moving: u32,
    // Body radius in pixels (spikes reach past it).
    radius: f32,
    // Length change along `angle` (0.1 = 10 % longer); the width shrinks.
    stretch: f32,
    // Direction of the stretch, world radians (+y up).
    angle: f32,
    // Direction the cell faces, world radians.
    facing: f32,
    // Phase of the idle loop, 0..1.
    phase: f32,
    // Thickness of the membrane rim, in pixels.
    band: f32,
    // Depth over which the ball shading goes from rim to centre, in pixels.
    depth_width: f32,
    // Half the side of the quad, in pixels.
    half_extent: f32,
    // Scale of the membrane wobble (1 = the sprite's).
    wobble: f32,
    breath: f32,
    h2_amp: f32,
    h2_phase: f32,
    h3_amp: f32,
    h3_phase: f32,
    spike_amp: f32,
    // A rod: half its axis and its radius, in pixels.
    rod_half: f32,
    rod_radius: f32,
    // Scale of the specular sparks (1 = the player's).
    sparks: f32,
    // Glow: inner and outer radius in pixels, strength.
    glow_in: f32,
    glow_out: f32,
    glow_strength: f32,
    // Organelle drift over the loop, pixels.
    drift: f32,
    // One pixel of the source sprite, in pixels.
    unit: f32,
    // Spawn: 0 at birth, 1 grown. The body grows and dithers in.
    life: f32,
    // Death: 0 alive, 1 gone. The membrane breaks and the contents spill.
    lysis: f32,
    // Membrane integrity, 0..1. Below 1 the rim thins; below 0.5 it has gaps.
    integrity: f32,
    // Extra glow (a symbiont in contact).
    glow_boost: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> cell: CellParams;

const TAU: f32 = 6.2831853;
const PI: f32 = 3.14159265;
// `LIGHT` of the generator, image coordinates.
const LIGHT: vec3<f32> = vec3<f32>(-0.45113, -0.55138, 0.70176);
// The specular sparks of `player_cell`, relative to the centre, at radius 20.
const SPARKS: array<vec2<f32>, 5> = array<vec2<f32>, 5>(
    vec2<f32>(-11.5, -11.0),
    vec2<f32>(-10.5, -12.0),
    vec2<f32>(-9.5, -13.0),
    vec2<f32>(-11.5, -10.0),
    vec2<f32>(-12.5, -9.0),
);
const BAYER4: array<f32, 16> = array<f32, 16>(
    0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0,
    12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0,
    3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0,
    15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0,
);

const FLAGELLUM: u32 = 1u;
const MOUTH: u32 = 2u;
const GLOW: u32 = 4u;
const STRIPES: u32 = 8u;
const CRISTAE: u32 = 16u;

fn has(flag: u32) -> bool {
    return (cell.features & flag) != 0u;
}

// The facing as a unit vector, image axes.
fn facing_dir() -> vec2<f32> {
    let a = -cell.facing;
    return vec2<f32>(cos(a), sin(a));
}

// From image axes to body axes (+x front).
fn to_body(p: vec2<f32>) -> vec2<f32> {
    let d = facing_dir();
    return vec2<f32>(dot(p, d), -p.x * d.y + p.y * d.x);
}

fn from_body(q: vec2<f32>) -> vec2<f32> {
    let d = facing_dir();
    return vec2<f32>(q.x * d.x - q.y * d.y, q.x * d.y + q.y * d.x);
}

// From a pixel (image coordinates, relative to the centre) to the rest shape:
// the inverse of the stretch of `Canvas.set_pose`, with the area kept as
// `player_body::stretch_scale` keeps it.
fn rest_point(p: vec2<f32>) -> vec2<f32> {
    let heading = -cell.angle;
    let d = vec2<f32>(cos(heading), sin(heading));
    let along = 1.0 + max(cell.stretch, 0.0);
    let u = dot(p, d) / along;
    let v = (-p.x * d.y + p.y * d.x) * along;
    return vec2<f32>(u * d.x - v * d.y, u * d.y + v * d.x);
}

// `blob_sdf` with the look's harmonics and spikes. Negative inside.
fn blob_sdf(q: vec2<f32>) -> f32 {
    let ph = TAU * cell.phase;
    let theta = atan2(q.y, q.x);
    let w = cell.wobble;
    var r = cell.radius * (1.0 + cell.breath * w * sin(ph))
        * (1.0 + w * (cell.h2_amp * sin(2.0 * theta + cell.h2_phase + ph)
            + cell.h3_amp * sin(3.0 * theta + cell.h3_phase - ph)));
    if cell.spikes > 0u {
        // Peaks sit either side of the facing, so the mouth has a valley.
        let n = f32(cell.spikes);
        let mouth = -cell.facing;
        let amp = cell.spike_amp + 0.04 * sin(ph + 1.0);
        r = r * (0.78 + amp * pow(abs(cos(n * (theta - mouth + PI / n) / 2.0)), 6.0));
    }
    return length(q) - r;
}

// `capsule_sdf` along the facing. Negative inside.
fn rod_sdf(q: vec2<f32>) -> f32 {
    let b = to_body(q);
    let r = cell.rod_radius * (1.0 + cell.breath * sin(TAU * cell.phase));
    let u = clamp(b.x, -cell.rod_half, cell.rod_half);
    return length(b - vec2<f32>(u, 0.0)) - r;
}

fn body_sdf(p: vec2<f32>) -> f32 {
    let q = rest_point(p);
    if cell.shape == 1u {
        return rod_sdf(q);
    }
    return blob_sdf(q);
}

// The height of the lit ball at a pixel (0 outside).
fn ball_height(p: vec2<f32>) -> f32 {
    let depth = max(-body_sdf(p), 0.0);
    let t = 1.0 - clamp(depth / cell.depth_width, 0.0, 1.0);
    return sqrt(clamp(1.0 - t * t, 0.0, 1.0));
}

// A stable random value in 0..1 for a cell of the integer lattice.
fn hash(n: vec2<f32>) -> f32 {
    return fract(sin(dot(n, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// True where the outline is broken: angular gaps that widen with `amount`.
fn rim_gap(p: vec2<f32>, amount: f32) -> bool {
    if amount <= 0.0 {
        return false;
    }
    let bucket = floor((atan2(p.y, p.x) / TAU + 0.5) * 24.0);
    return hash(vec2<f32>(bucket, 7.0)) < amount;
}

fn bayer(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p)) & vec2<i32>(3);
    return BAYER4[i.y * 4 + i.x];
}

// The centre of an organelle, image axes, with its drift.
fn organelle_centre(i: i32) -> vec2<f32> {
    var c = cell.organelles[i].xy;
    if cell.shape == 1u {
        c = from_body(c);
    }
    let ph = TAU * cell.phase + 2.0 * f32(i);
    return c + cell.drift * vec2<f32>(sin(ph), 0.75 * cos(ph));
}

// `Canvas.disc`: a small ball with a shadow side and a highlight. Its outline
// in the generator goes only on empty pixels, so inside a body it has none.
fn disc(q: vec2<f32>, c: vec2<f32>, r: f32, kind: f32, color: ptr<function, vec4<f32>>) -> bool {
    let d = q - c;
    if length(d) >= r {
        return false;
    }
    var tones: array<vec4<f32>, 5>;
    if kind < 1.5 {
        tones = cell.inner;
    } else {
        tones = cell.extra;
    }
    *color = tones[2];
    if d.x * 0.55 + d.y * 0.65 > 0.25 * r {
        *color = tones[1];
    }
    if r >= 2.0 && length(d - vec2<f32>(-r * 0.35, -r * 0.4)) < max(0.9, r * 0.32) {
        *color = tones[4];
    }
    return true;
}


// The flagellum of `prey_bacillus`: a wave that runs back from the rear pole.
fn on_flagellum(q: vec2<f32>) -> bool {
    let b = to_body(q);
    let unit = cell.unit;
    let start = 9.4 * unit;
    let s = -b.x - start;
    let len = 6.4 * unit;
    if s < 0.0 || s > len {
        return false;
    }
    var beats = 1.0;
    var amp = 1.3 * unit;
    if cell.moving != 0u {
        beats = 2.0;
        amp = 2.0 * unit;
    }
    let grow = min(1.0, s / (2.5 * unit));
    let sl = s / unit;
    let wave = amp * grow * sin(1.3 * sl - beats * TAU * cell.phase);
    return abs(b.y - wave) < 0.6;
}

// The toothed mouth of `predator_cell`. 0: not on it, 1: notch, 2: tooth.
fn mouth(q: vec2<f32>) -> i32 {
    let b = to_body(q) / cell.unit;
    let ph = TAU * cell.phase;
    var opening = 0.65 + 0.22 * sin(ph);
    if cell.moving != 0u {
        opening = 0.9 + 0.1 * sin(ph);
    }
    let width = 0.35 + 0.25 * opening;
    for (var i = 0; i < 3; i++) {
        let tu = 5.0 + 2.5 * f32(i);
        let tv = (tu - 2.0) * width * 0.96;
        if abs(abs(b.y) - tv) < 0.5 && b.x > tu - 0.5 && b.x < tu + 1.0 {
            return 2;
        }
    }
    if b.x > 3.0 && abs(b.y) < (b.x - 2.0) * width {
        return 1;
    }
    return 0;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // The pixel relative to the centre, image coordinates (uv runs +y down).
    let pixel = (mesh.uv - vec2<f32>(0.5)) * 2.0 * cell.half_extent;
    // Spawn: the body grows from 0.4 and dithers in.
    let grown = smoothstep(0.0, 1.0, cell.life);
    if bayer(pixel) > cell.life * 1.6 {
        discard;
    }
    // Lysis: the body swells as it bursts.
    let p = pixel / ((0.4 + 0.6 * grown) * (1.0 + 0.25 * cell.lysis));
    let sdf = body_sdf(p);
    let gaps = max((0.5 - cell.integrity) * 1.4, cell.lysis * 1.6);
    let q = rest_point(p);
    var color: vec4<f32>;
    if sdf >= 0.0 {
        // `Canvas.outline`: a 1 px ring where a 4-neighbour is inside.
        let edge = body_sdf(p + vec2<f32>(1.0, 0.0)) < 0.0
            || body_sdf(p - vec2<f32>(1.0, 0.0)) < 0.0
            || body_sdf(p + vec2<f32>(0.0, 1.0)) < 0.0
            || body_sdf(p - vec2<f32>(0.0, 1.0)) < 0.0;
        if edge && !rim_gap(p, gaps) {
            color = cell.rim[0];
        } else if cell.lysis > 0.0 {
            // The contents spill: specks that fly out and fade.
            let d = length(p);
            let reach = cell.radius * (1.0 + 0.9 * cell.lysis);
            if d < reach && hash(floor(pixel)) > 0.9 && bayer(pixel) > cell.lysis {
                color = cell.tones[2];
            } else {
                discard;
            }
        } else if has(FLAGELLUM) && cell.shape == 1u && on_flagellum(q) {
            color = cell.tones[3];
        } else if has(GLOW) {
            // `Canvas.halo`: dithered, on or off.
            let pulse = 0.5 + 0.5 * sin(TAU * cell.phase);
            let d = length(p);
            let outer = cell.glow_out + (pulse - 0.5) * 2.0 * cell.unit;
            let a = clamp(1.0 - (d - cell.glow_in) / (outer - cell.glow_in), 0.0, 1.0)
                * (cell.glow_strength + 0.18 * (pulse - 0.5) + 0.25 * cell.glow_boost);
            if a <= bayer(p) || d < cell.glow_in * 0.6 {
                discard;
            }
            color = cell.glow[1 + clamp(i32(a * 2.0), 0, 2)];
        } else {
            discard;
        }
    } else {
        // `lit_body`: the normal from the slope of the ball (np.gradient is a
        // central difference over one pixel each side).
        let gx = (ball_height(p + vec2<f32>(1.0, 0.0)) - ball_height(p - vec2<f32>(1.0, 0.0))) * 0.5;
        let gy = (ball_height(p + vec2<f32>(0.0, 1.0)) - ball_height(p - vec2<f32>(0.0, 1.0))) * 0.5;
        let normal = normalize(vec3<f32>(-gx * 2.2, -gy * 2.2, 1.0));
        let lam = dot(normal, LIGHT);
        // np.digitize(lam, [0.55, 0.72, 0.86]): four hard bands.
        let idx = i32(lam >= 0.55) + i32(lam >= 0.72) + i32(lam >= 0.86);
        // Lysis: the inside dithers away.
        if bayer(pixel) < cell.lysis * 1.2 - 0.1 {
            discard;
        }
        if -sdf < cell.band * (0.5 + 0.5 * clamp(cell.integrity, 0.0, 1.0)) {
            color = cell.rim[1 + min(idx + 1, 3)];
        } else {
            color = cell.tones[1 + min(idx, 2)];
        }
        let unit = cell.unit;
        let t = cell.phase;
        // Thylakoid stripes: arcs round a far centre that flow over the loop.
        if has(STRIPES) && -sdf > 6.0 * unit && q.x < 4.0 * unit {
            let d = length(q - vec2<f32>(34.0, 0.0) * unit) / unit;
            if d > 18.0 {
                let on = ((d + 4.0 * t) % 4.0) < 1.15;
                let on_above = ((length(q - vec2<f32>(0.0, 1.0) - vec2<f32>(34.0, 0.0) * unit) / unit + 4.0 * t) % 4.0) < 1.15;
                if on {
                    color = cell.tones[1];
                } else if on_above && q.y < 1.0 {
                    color = cell.tones[3];
                }
            }
        }
        // Cristae: wavy folds that drift.
        if has(CRISTAE) && -sdf > 6.0 * unit && q.x < 10.0 * unit {
            let ys = q.y / unit + 36.0;
            let wave = 2.2 * sin(ys / 3.2 + TAU * t);
            let x = q.x / unit + 36.0;
            if ((x + wave + 6.0 * t) % 6.0) < 1.3 {
                color = cell.tones[1];
            } else if ((x - 1.0 + wave + 6.0 * t) % 6.0) < 1.3 {
                color = cell.tones[3];
            }
        }
        // The mouth: a dark notch with teeth.
        if has(MOUTH) {
            let m = mouth(q);
            if m == 1 {
                color = vec4<f32>(0.0176, 0.0016, 0.0052, 1.0);
            } else if m == 2 {
                color = vec4<f32>(0.922, 0.863, 0.745, 1.0);
            }
        }
        // Organelles, back to front.
        for (var i = 0; i < 4; i++) {
            let o = cell.organelles[i];
            if o.w > 0.5 {
                var c = color;
                if disc(q, organelle_centre(i), o.z, o.w, &c) {
                    color = c;
                }
            }
        }
        // The specular sparks, placed on the rest shape. A rod gets three
        // pixels on the rim that faces the light.
        if cell.shape == 1u {
            let a = facing_dir();
            let to_light = vec2<f32>(-0.6, -0.8);
            var side = to_light - dot(to_light, a) * a;
            side = side / (length(side) + 1e-6);
            let hp = dot(to_light, a) * 3.0 * unit * a + side * 3.3 * unit;
            for (var k = -1; k <= 1; k++) {
                let d = abs(q - (hp + f32(k) * a));
                if max(d.x, d.y) < 0.5 {
                    color = cell.tones[4];
                }
            }
        } else {
            for (var i = 0; i < 5; i++) {
                let d = abs(q - SPARKS[i] * (cell.radius / 20.0) * cell.sparks);
                if max(d.x, d.y) < 0.5 {
                    color = cell.rim[4];
                }
            }
        }
        color = vec4<f32>(color.rgb * cell.tint.rgb, color.a);
    }
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
    return color;
}
