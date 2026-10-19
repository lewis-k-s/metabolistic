// Microbial mat shapes (`world::shapes_view`): a soft patch with swaying
// filaments and dithered edges, on the sprite pixel grid.
//
// The mesh is the facet mesh of the shape; uv.x is the distance in pixels to
// the outer edge (a very large value inside the union of the parts). Near the
// edge a pixel is dropped by the 4x4 Bayer matrix, so the patch fades out in
// dots. The filaments are short vertical strokes that sway with time.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::{view, globals},
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif

struct MatParams {
    // The ramp: dark, shadow, base, light, highlight (linear colour).
    tones: array<vec4<f32>, 5>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> mat: MatParams;

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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let wp = in.world_position.xy;
    let pixel = vec2<i32>(floor(wp));
    let snapped = vec2<f32>(pixel) + vec2<f32>(0.5);
    // Depth at the pixel centre (see rock.wgsl).
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
    // Dithered edge: dots drop out over the outer 9 px.
    if depth < bayer4(pixel) * 9.0 - 1.0 {
        discard;
    }
    let time = globals.time;
    // Body: a mottled base.
    let h = hash2(pixel / 2);
    var tone = 2;
    if h < 0.22 {
        tone = 1;
    } else if h > 0.86 {
        tone = 3;
    }
    if depth < 5.0 {
        tone = max(tone - 1, 1);
    }
    // Filaments: vertical strokes, 3 px apart, that sway.
    let col = floor(snapped.x / 3.0);
    let fh = hash2(vec2<i32>(i32(col), 7));
    let sway = 1.6 * sin(time * 1.3 + snapped.y * 0.17 + fh * 6.2831);
    let along = fract((snapped.y + fh * 40.0) / 17.0);
    if fh < 0.55 && abs(snapped.x - (col * 3.0 + 1.5) - sway) < 0.8 && along < 0.55 {
        tone = select(3, 4, along < 0.12);
    }
    var color = vec4<f32>(mat.tones[tone].rgb, 1.0);
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
    return color;
}
