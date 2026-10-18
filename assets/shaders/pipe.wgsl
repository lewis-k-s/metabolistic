// A pipe of the metabolic map (`screens::manage::diagram`): one material for
// the whole route, drawn on a quad for each segment. Each fragment finds its
// place along the route, so corners join and the flow runs on without a seam.
//
// The pipe is a lit tube: a 1 px dark wall, a core in the pipe colour, and a
// light line on the side that faces the light (upper left). Packets of a
// lighter tone move along it at the speed of the molecules. A pipe that limits
// its pathway shows a red pulse that runs backwards (the jam). A cofactor is a
// dotted pin: needed, not used up.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct PipeParams {
    // Per point: x, y, arc length at the point, unused.
    points: array<vec4<f32>, 16>,
    // The pipe colour (linear).
    color: vec4<f32>,
    // The jam colour (linear).
    bad: vec4<f32>,
    // Point count, width of the core in pixels, flux (0 = still), time.
    info: vec4<f32>,
    // Limiting (0/1), cofactor (0/1), packet speed in px/s, glow (0..1).
    state: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> pipe: PipeParams;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let p = floor(mesh.world_position.xy) + vec2<f32>(0.5);
    let n = i32(pipe.info.x);
    // The nearest segment: arc length, the offset across, and which side.
    var best = 1e6;
    var s = 0.0;
    var across = 0.0;
    for (var i = 0; i + 1 < n; i++) {
        let a = pipe.points[i];
        let b = pipe.points[i + 1];
        let d = b.xy - a.xy;
        let len = max(length(d), 1e-3);
        let dir = d / len;
        let raw = dot(p - a.xy, dir);
        let u = clamp(raw, 0.0, len);
        let q = a.xy + dir * u;
        // Box distance: square ends, so corners stay square.
        let over = max(max(-raw, raw - len), 0.0);
        let perp = abs(dir.x * (p.y - a.y) - dir.y * (p.x - a.x));
        let dist = max(perp, over);
        if dist < best {
            best = dist;
            s = a.z + u;
            // Across: + is up for a horizontal pipe and left for a vertical one.
            if abs(dir.x) > abs(dir.y) {
                across = p.y - q.y;
            } else {
                across = q.x - p.x;
            }
        }
    }
    let half = pipe.info.y * 0.5;
    let wall = half + 1.0;
    if best > wall + 0.01 {
        discard;
    }
    let t = pipe.info.w;
    let flux = pipe.info.z;
    let base = pipe.color.rgb;
    var color: vec3<f32>;
    let still = flux <= 0.0;
    if pipe.state.y > 0.5 {
        // A cofactor pin: dotted, dim.
        if fract(s / 3.0) >= 0.5 || best > half + 0.01 {
            discard;
        }
        color = base * 0.55;
    } else if best > half {
        // The wall.
        color = base * 0.3;
    } else {
        var k = select(0.95, 0.45, still);
        // The lit side: a light line on the upper (or left) edge of a wide pipe.
        if pipe.info.y >= 2.0 && across > half - 1.0 {
            k += 0.35;
        }
        color = base * k;
        if !still {
            // Packets move with the molecules.
            let spacing = 9.0;
            let phase = fract((s - t * pipe.state.z) / spacing);
            if phase < 0.34 {
                color = mix(color, vec3<f32>(1.0), 0.28 + 0.2 * pipe.state.w);
            }
        }
    }
    if pipe.state.x > 0.5 {
        // The jam: a red pulse that runs backwards, against the flow.
        let jam = fract((s + t * 28.0) / 22.0);
        if jam < 0.35 {
            color = mix(color, pipe.bad.rgb, 0.65);
        }
    }
    var out = vec4<f32>(color, 1.0);
#ifdef TONEMAP_IN_SHADER
    out = tonemapping::tone_mapping(out, view.color_grading);
#endif
    return out;
}
