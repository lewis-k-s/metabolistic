// Fragment shader of the vent particles (`world::vfx`).
//
// A particle is a square of `in.size` x `in.size` sprite pixels (the vertex shader,
// `vent_particle_vertex.wgsl`, snaps it to whole world units). Each pixel is
// one palette tone, or it is not drawn: there is no alpha blending. The
// particle colour carries two numbers: red is the tone (0..1, in 4 steps) and
// alpha is the fade. A pixel is drawn when the fade is above its BAYER4
// threshold, so particles dissolve with dither.

#import bevy_enoki::particle_vertex_out::{ VertexOutput }

struct Params {
    // The ramp: five tones, dark to light (linear colour).
    tones: array<vec4<f32>, 5>,
    pad2: f32,
    // 0 = a round puff, shaded from the upper left. 1 = a plus-shaped spark.
    shape: f32,
    pad0: f32,
    pad1: f32,
};

@group(1) @binding(0) var<uniform> p: Params;

fn bayer4(c: vec2<i32>) -> f32 {
    var m = array<f32, 16>(
        0.5, 8.5, 2.5, 10.5,
        12.5, 4.5, 14.5, 6.5,
        3.5, 11.5, 1.5, 9.5,
        15.5, 7.5, 13.5, 5.5,
    );
    return m[(c.y & 3) * 4 + (c.x & 3)] / 16.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let cell = vec2<i32>(floor(in.uv * in.size));
    let from_centre = vec2<f32>(cell) + vec2<f32>(0.5) - vec2<f32>(in.size * 0.5);
    if p.shape < 0.5 {
        if length(from_centre) > in.size * 0.5 {
            discard;
        }
    } else if abs(from_centre.x) + abs(from_centre.y) > 1.01 {
        discard;
    }
    if in.color.a <= bayer4(cell) {
        discard;
    }
    var tone = i32(round(in.color.r * 4.0));
    if p.shape < 0.5 {
        // uv.y points down, so the upper left is negative x and negative y.
        let lit = -dot(from_centre / (in.size * 0.5), vec2<f32>(0.6, 0.8));
        tone += select(select(0, -1, lit < -0.45), 1, lit > 0.45);
    }
    tone = clamp(tone, 0, 4);
    return vec4<f32>(p.tones[tone].rgb, 1.0);
}
