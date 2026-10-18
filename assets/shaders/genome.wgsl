// The genome of the codex: a circular chromosome, folded into rows, drawn as a
// double helix. The geometry is `genome_map::GenomeLayout`: the same segments,
// the same wobble and the same helix phase, so the icons that the game places
// on the CPU stay on the strand.
//
// The quad covers the layout bounds. Each fragment snaps to the centre of its
// sprite pixel, so the strand is pixel art at any camera scale: hard tone
// bands, a 1 px outline, dithered glow (see docs/art-direction.md).

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct GenomeParams {
    // Look::COUNT ramps of four tones: outline, dark, base, light (linear).
    tones: array<vec4<f32>, 32>,
    // Per segment: kind (0 line, 1 arc), start arc length, length, sweep.
    seg_a: array<vec4<f32>, 16>,
    // A line: from.xy, to.xy. An arc: centre.xy, radius, start angle.
    seg_b: array<vec4<f32>, 16>,
    // Per locus: s0, s1, look, flags.
    loci: array<vec4<f32>, 40>,
    // The quad: bounds min.xy, size.xy (layout units).
    rect: vec4<f32>,
    // Loop length, short-wave count, helix turns, time in seconds.
    loop_info: vec4<f32>,
    // Segment count, locus count, genome integrity (0..1), unused.
    counts: vec4<f32>,
    // ori.xy, ter.xy.
    marks: vec4<f32>,
    // The nucleoid region behind the strand.
    nucleoid: vec4<f32>,
    // Selection brackets and the ori bead.
    accent: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> g: GenomeParams;

const TAU: f32 = 6.2831853;
// `genome_map` constants.
const AMP: f32 = 3.0;
const WOBBLE_AMP: vec2<f32> = vec2<f32>(1.3, 0.9);
const WOBBLE_SPEED: vec2<f32> = vec2<f32>(0.9, 0.45);
const HELIX_SPIN: f32 = 0.35;
// Look indices.
const LINKER: i32 = 0;
const MUTATED: i32 = 3;
const SILENT: i32 = 4;
const IN_CODEX: i32 = 5;
const UNKNOWN: i32 = 6;
// Flag bits.
const ACTIVE: u32 = 1u;
const DIMMED: u32 = 2u;
const HOVERED: u32 = 4u;
const SELECTED: u32 = 8u;

fn seg_point(i: i32, u: f32) -> vec2<f32> {
    let a = g.seg_a[i];
    let b = g.seg_b[i];
    if a.x < 0.5 {
        return b.xy + normalize(b.zw - b.xy) * u;
    }
    let ang = b.w + sign(a.w) * u / b.z;
    return b.xy + b.z * vec2<f32>(cos(ang), sin(ang));
}

fn seg_normal(i: i32, u: f32) -> vec2<f32> {
    let a = g.seg_a[i];
    let b = g.seg_b[i];
    var t: vec2<f32>;
    if a.x < 0.5 {
        t = normalize(b.zw - b.xy);
    } else {
        let ang = b.w + sign(a.w) * u / b.z;
        t = sign(a.w) * vec2<f32>(-sin(ang), cos(ang));
    }
    return vec2<f32>(-t.y, t.x);
}

// Nearest point of the strand at rest: (arc length, signed distance along the
// left normal, distance).
fn closest(p: vec2<f32>) -> vec3<f32> {
    var best = vec3<f32>(0.0, 1e6, 1e6);
    let n = i32(g.counts.x);
    for (var i = 0; i < n; i++) {
        let a = g.seg_a[i];
        let b = g.seg_b[i];
        var u: f32;
        if a.x < 0.5 {
            let d = normalize(b.zw - b.xy);
            u = clamp(dot(p - b.xy, d), 0.0, a.z);
        } else {
            let ang = atan2(p.y - b.y, p.x - b.x);
            var along: f32;
            if a.w > 0.0 {
                along = ang - b.w;
            } else {
                along = b.w - ang;
            }
            along = along - TAU * floor(along / TAU);
            if along <= abs(a.w) {
                u = along * b.z;
            } else if distance(p, seg_point(i, 0.0)) <= distance(p, seg_point(i, a.z)) {
                u = 0.0;
            } else {
                u = a.z;
            }
        }
        let q = seg_point(i, u);
        let dist = distance(p, q);
        if dist < best.z {
            best = vec3<f32>(a.y + u, dot(p - q, seg_normal(i, u)), dist);
        }
    }
    return best;
}

fn wobble(s: f32, t: f32) -> f32 {
    let x = s / g.loop_info.x;
    return WOBBLE_AMP.x * sin(TAU * g.loop_info.y * x - WOBBLE_SPEED.x * t)
        + WOBBLE_AMP.y * sin(TAU * 2.0 * x + WOBBLE_SPEED.y * t);
}

fn hash(n: f32) -> f32 {
    return fract(sin(n * 127.1 + 311.7) * 43758.547);
}

fn hash2(p: vec2<f32>) -> f32 {
    return hash(p.x * 1.731 + p.y * 57.13);
}

// The ordered 4x4 threshold of a pixel, in (0, 1).
fn bayer4(p: vec2<f32>) -> f32 {
    let x = u32(p.x - 4.0 * floor(p.x / 4.0));
    let y = u32(p.y - 4.0 * floor(p.y / 4.0));
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[y * 4u + x] + 0.5) / 16.0;
}

fn tone(look: i32, k: i32) -> vec4<f32> {
    return g.tones[look * 4 + clamp(k, 0, 3)];
}

fn finish(c: vec4<f32>) -> vec4<f32> {
    var color = c;
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
    return color;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // The sprite pixel of this fragment (uv runs +y down, layout +y up).
    let local = vec2<f32>(mesh.uv.x, 1.0 - mesh.uv.y) * g.rect.zw + g.rect.xy;
    let p = floor(local) + vec2<f32>(0.5);
    let t = g.loop_info.w;

    let near = closest(p);
    let s = near.x;

    // The locus at this arc length.
    var look = LINKER;
    var fl = 0u;
    var s0 = -1.0;
    var s1 = -1.0;
    let count = i32(g.counts.y);
    for (var i = 0; i < count; i++) {
        let l = g.loci[i];
        if s >= l.x && s < l.y {
            look = i32(l.z + 0.5);
            fl = u32(l.w + 0.5);
            s0 = l.x;
            s1 = l.y;
        }
    }
    let in_locus = s0 >= 0.0;
    let dimmed = (fl & DIMMED) != 0u;
    let hovered = (fl & HOVERED) != 0u;
    let selected = (fl & SELECTED) != 0u;
    let glowing = (fl & ACTIVE) != 0u && !dimmed;
    var ramp = look;
    if dimmed {
        ramp = LINKER;
    }
    // Hover lifts every tone one step.
    var lift = 0;
    if hovered {
        lift = 1;
    }

    var off = near.y - wobble(s, t);
    // A mutated gene is kinked.
    if look == MUTATED && !dimmed {
        off += (hash(floor(s / 2.0)) - 0.5) * 1.6;
    }
    // A silent gene is wound tight: a flatter helix.
    var amp = AMP;
    if look == SILENT || look == UNKNOWN {
        amp = 2.0;
    }
    let phase = TAU * (g.loop_info.z * s / g.loop_info.x + HELIX_SPIN * t);
    let y1 = amp * sin(phase);
    let front1 = cos(phase) >= 0.0;
    let d1 = abs(off - y1);
    let d2 = abs(off + y1);

    // UV damage: single-strand nicks first, then breaks of both strands.
    let damage = 1.0 - g.counts.z;
    let cell = floor(s / 4.0);
    let nick1 = hash(cell) < damage * 0.45;
    let nick2 = hash(cell + 91.7) < damage * 0.45 - 0.2;
    // The ghost of a sequence in the codex: a dashed strand.
    let ghost = look == IN_CODEX && !dimmed;
    let dash = !ghost || fract(s / 4.0) < 0.5;
    let on1 = d1 < 0.9 && !nick1 && dash;
    let on2 = d2 < 0.9 && !nick2 && dash;
    let rungs = in_locus && look != UNKNOWN && look != SILENT && !ghost;
    let on_rung = rungs && abs(off) < abs(y1) - 0.6 && fract(s / 3.0) < 0.34 && !(nick1 && nick2);

    // Back strand, rungs, front strand.
    var color = vec4<f32>(0.0);
    var hit = false;
    let back_on = select(on1, on2, front1);
    let front_on = select(on2, on1, front1);
    let front_y = select(-y1, y1, front1);
    if back_on {
        color = tone(ramp, 1 + lift);
        hit = true;
    }
    if on_rung {
        color = tone(ramp, 2 + lift);
        hit = true;
    }
    if front_on {
        // The crest of the front strand catches the light (upper left).
        var k = 2;
        if front_y > amp * 0.5 {
            k = 3;
        }
        color = tone(ramp, k + lift);
        hit = true;
    }
    if !hit {
        var edge = 9.0;
        if !nick1 && dash {
            edge = min(edge, d1);
        }
        if !nick2 && dash {
            edge = min(edge, d2);
        }
        if edge < 1.9 || (on_rung && abs(off) < amp + 1.0) {
            color = tone(ramp, 0);
            hit = true;
        }
    }

    // The ori and ter beads.
    if distance(p, g.marks.xy) < 3.0 {
        color = g.accent;
        hit = true;
    } else if distance(p, g.marks.xy) < 4.2 {
        color = tone(LINKER, 0);
        hit = true;
    }
    if distance(p, g.marks.zw) < 2.6 {
        color = tone(LINKER, 3);
        hit = true;
    } else if distance(p, g.marks.zw) < 3.8 {
        color = tone(LINKER, 0);
        hit = true;
    }

    // The selected locus: gold brackets at both ends, and a dotted bar under it.
    if selected && !hit {
        let at_end = min(abs(s - s0), abs(s - s1)) < 1.0;
        if at_end && abs(off) < amp + 5.0 {
            color = g.accent;
            hit = true;
        } else if abs(off + amp + 5.0) < 0.6 && fract(s / 2.0) < 0.5 {
            color = g.accent;
            hit = true;
        }
    }

    // A gene with a product glows: a dithered halo that breathes.
    if glowing && !hit && in_locus && abs(off) < amp + 4.0 {
        let pulse = 0.5 + 0.5 * sin(2.2 * t - s * 0.05);
        let fall = 1.0 - (abs(off) - amp) / 4.0;
        if bayer4(p) < (0.12 + 0.18 * pulse) * fall {
            color = tone(ramp, 3);
            hit = true;
        }
    }
    if hit {
        return finish(color);
    }

    // The nucleoid: a calm region round the loop, with a dithered edge, and a
    // few specks that drift in it.
    let dist = near.z;
    let inside = clamp((46.0 - dist) / 14.0, 0.0, 1.0);
    if inside <= 0.0 {
        discard;
    }
    let drift = p + vec2<f32>(floor(t * 2.0), floor(3.0 * sin(t * 0.3 + p.y * 0.01)));
    let speck_cell = floor(drift / 7.0);
    let at_centre = all(floor(drift) - speck_cell * 7.0 == vec2<f32>(3.0));
    if at_centre && hash2(speck_cell) > 0.93 && inside > 0.5 {
        return finish(tone(LINKER, 1));
    }
    if bayer4(p) < inside {
        return finish(g.nucleoid);
    }
    discard;
}
