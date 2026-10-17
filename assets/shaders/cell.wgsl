// A cell body as a signed distance field: the run-time port of
// `art/gen_sprites.py` (`blob_sdf`, `Canvas.lit_body`, `Canvas.outline`).
//
// The camera draws at one texel per world pixel (`world::pixel_canvas`), so each
// fragment is one game pixel. All maths is in image coordinates (+y down), as in
// the generator, so the wobble phases and the light match the sprite sheets.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct CellParams {
    // The ramp: outline, shadow, base, light, highlight (linear colour).
    tones: array<vec4<f32>, 5>,
    // Radius in pixels.
    radius: f32,
    // Length change along `angle` (0.1 = 10 % longer); the width shrinks.
    stretch: f32,
    // Direction of the stretch, world radians (+y up).
    angle: f32,
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
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> cell: CellParams;

const TAU: f32 = 6.2831853;
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

// `blob_sdf` with the harmonics of `player_cell`. Negative inside.
fn body_sdf(p: vec2<f32>) -> f32 {
    let q = rest_point(p);
    let ph = TAU * cell.phase;
    let theta = atan2(q.y, q.x);
    let w = cell.wobble;
    let r = cell.radius * (1.0 + 0.012 * w * sin(ph))
        * (1.0 + w * (0.055 * sin(2.0 * theta + 0.6 + ph) + 0.045 * sin(3.0 * theta + 2.1 - ph)));
    return length(q) - r;
}

// The height of the lit ball at a pixel (0 outside).
fn ball_height(p: vec2<f32>) -> f32 {
    let depth = max(-body_sdf(p), 0.0);
    let t = 1.0 - clamp(depth / cell.depth_width, 0.0, 1.0);
    return sqrt(clamp(1.0 - t * t, 0.0, 1.0));
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // The pixel relative to the centre, image coordinates (uv runs +y down).
    let p = (mesh.uv - vec2<f32>(0.5)) * 2.0 * cell.half_extent;
    let sdf = body_sdf(p);
    var color: vec4<f32>;
    if sdf >= 0.0 {
        // `Canvas.outline`: a 1 px ring where a 4-neighbour is inside.
        let edge = body_sdf(p + vec2<f32>(1.0, 0.0)) < 0.0
            || body_sdf(p - vec2<f32>(1.0, 0.0)) < 0.0
            || body_sdf(p + vec2<f32>(0.0, 1.0)) < 0.0
            || body_sdf(p - vec2<f32>(0.0, 1.0)) < 0.0;
        if !edge {
            discard;
        }
        color = cell.tones[0];
    } else {
        // `lit_body`: the normal from the slope of the ball (np.gradient is a
        // central difference over one pixel each side).
        let gx = (ball_height(p + vec2<f32>(1.0, 0.0)) - ball_height(p - vec2<f32>(1.0, 0.0))) * 0.5;
        let gy = (ball_height(p + vec2<f32>(0.0, 1.0)) - ball_height(p - vec2<f32>(0.0, 1.0))) * 0.5;
        let normal = normalize(vec3<f32>(-gx * 2.2, -gy * 2.2, 1.0));
        let lam = dot(normal, LIGHT);
        // np.digitize(lam, [0.55, 0.72, 0.86]): four hard bands.
        let idx = i32(lam >= 0.55) + i32(lam >= 0.72) + i32(lam >= 0.86);
        if -sdf < cell.band {
            color = cell.tones[1 + min(idx + 1, 3)];
        } else {
            color = cell.tones[1 + min(idx, 2)];
        }
        // The specular sparks, placed on the rest shape.
        let q = rest_point(p);
        for (var i = 0; i < 5; i++) {
            let d = abs(q - SPARKS[i] * (cell.radius / 20.0));
            if max(d.x, d.y) < 0.5 {
                color = cell.tones[4];
            }
        }
    }
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
    return color;
}
