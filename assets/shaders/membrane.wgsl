// Tissue membrane (`world::shapes_view`, one quad for each wall tile): a lipid
// bilayer drawn on the sprite pixel grid.
//
// Two wavy lines of lipid heads (round dots) with the tails between them. The
// heads drift along the line, and the layer breathes. The waves use world
// coordinates, so the tiles of a wall join into one membrane at any angle of
// the run. A pore tile has a gap with two channel proteins that pulse; the gap
// opens wider when a cell with a transporter is near.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::{view, globals},
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct MembraneParams {
    // Lipid head tones (base, light), tail tones (dark, mid), channel protein
    // tones (dark, light), linear colour.
    head: array<vec4<f32>, 2>,
    tail: array<vec4<f32>, 2>,
    pore: array<vec4<f32>, 2>,
    // Multiplies the colours: the kind of wall.
    tint: vec4<f32>,
    // x: 0 horizontal, 1 vertical, 2 both. y: 1 for a pore tile.
    flags: vec4<f32>,
    // A cell with a transporter: xy position, z reach, w 1 if there is one.
    near: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> m: MembraneParams;

const TAU: f32 = 6.2831853;

// One run of membrane. s runs along it, t across, in sprite pixels; sw is the
// world coordinate along it (for continuous waves); open is the pore opening
// in 0..1 and local_s the distance along from the tile centre.
fn bilayer(s: f32, t: f32, sw: f32, local_s: f32, open: f32, is_pore: bool) -> vec4<f32> {
    let time = globals.time;
    let breath = 1.0 + 0.12 * sin(time * 0.9 + sw * 0.02);
    if is_pore {
        let gap = mix(3.0, 10.0, open);
        let a = abs(local_s);
        if a < gap {
            return vec4<f32>(0.0);
        }
        // The channel proteins stand at both ends of the gap.
        if a < gap + 3.5 && abs(t) < 8.5 {
            let pulse = 0.5 + 0.5 * sin(time * 2.2 + step(0.0, local_s) * 1.4);
            let edge = a > gap + 2.0 || abs(t) > 7.0;
            if edge || pulse < 0.5 {
                return m.pore[0];
            }
            return m.pore[1];
        }
    }
    for (var k = -1; k <= 1; k += 2) {
        let fk = f32(k);
        let wave = 1.3 * sin(sw * 0.15 + time * 1.2 + fk * 1.9);
        let centre = fk * 4.5 * breath + wave;
        // Heads every 4 px, drifting slowly (the two layers drift apart).
        let drift = fk * 0.6 * time;
        let u = sw - drift;
        let i = floor(u / 4.0 + 0.5);
        let head_s = i * 4.0 + drift - (sw - s);
        let dist = distance(vec2<f32>(s, t), vec2<f32>(head_s, centre));
        if dist < 1.7 {
            // The light falls from the upper left: the head's upper edge is lighter.
            return select(m.head[0], m.head[1], (t - centre) * fk * -1.0 > 0.2 || dist < 0.9);
        }
        // The two tails of the head, running to the middle.
        if abs(s - head_s) < 0.75 && (t - centre) * fk < -1.0 && (t - centre) * fk > -4.2 {
            return select(m.tail[0], m.tail[1], abs(s - head_s) < 0.3);
        }
    }
    return vec4<f32>(0.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let wp = in.world_position.xy;
    // Position in the tile (the quad is 32 x 32), snapped to a sprite pixel.
    let local = vec2<f32>((in.uv.x - 0.5) * 32.0, (0.5 - in.uv.y) * 32.0);
    let lp = floor(local) + vec2<f32>(0.5);
    let snapped = (wp - local) + lp;
    let centre = wp - local;
    var open = 0.0;
    if m.near.w > 0.5 {
        open = 1.0 - smoothstep(m.near.z * 0.4, m.near.z, distance(centre, m.near.xy));
    }
    let is_pore = m.flags.y > 0.5;
    var color = vec4<f32>(0.0);
    if m.flags.x < 0.5 || m.flags.x > 1.5 {
        color = bilayer(lp.x, lp.y, snapped.x, lp.x, open, is_pore);
    }
    if color.a < 0.5 && m.flags.x > 0.5 {
        color = bilayer(lp.y, lp.x, snapped.y, lp.y, open, is_pore);
    }
    if color.a < 0.5 {
        discard;
    }
    var out = vec4<f32>(color.rgb * m.tint.rgb, 1.0);
#ifdef TONEMAP_IN_SHADER
    out = tonemapping::tone_mapping(out, view.color_grading);
#endif
    return out;
}
