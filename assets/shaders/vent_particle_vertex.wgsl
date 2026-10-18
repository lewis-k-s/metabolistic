// Vertex shader of the particles: it replaces the one of `bevy_enoki`, so that
// each particle is a square on the sprite pixel grid (one world unit is one
// sprite pixel). The size is rounded to whole pixels and the corner is
// snapped. There is no rotation.

#import bevy_render::view::View
#import bevy_enoki::particle_vertex_out::{ VertexOutput }

@group(0) @binding(0) var<uniform> view: View;

struct VertexIn {
    @builtin(vertex_index) index: u32,
    @location(0) i_transform: vec4<f32>,
    @location(1) i_scale_lifetime: vec4<f32>,
    @location(2) i_color: vec4<f32>,
};

@vertex
fn vertex(in: VertexIn) -> VertexOutput {
    var out: VertexOutput;

    let corner = vec2<f32>(f32(in.index & 0x1u), f32((in.index & 0x2u) >> 1u));
    let size = max(round(in.i_scale_lifetime.xy), vec2<f32>(1.0));
    let origin = floor(in.i_transform.xy - size * 0.5 + vec2<f32>(0.5));
    let world_position = vec3<f32>(origin + corner * size, in.i_transform.z);

    out.clip_position = view.clip_from_world * vec4<f32>(world_position, 1.0);
    out.color = in.i_color;
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    out.lifetime_frac = in.i_scale_lifetime.z;
    out.lifetime_total = in.i_scale_lifetime.w;
    out.size = size.x;
    return out;
}
