// The vertex output of the particles: the one of `bevy_enoki` plus the pixel
// size of the particle, so the fragment shader can draw a particle that grows.
#define_import_path bevy_enoki::particle_vertex_out

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) lifetime_frac: f32,
    @location(3) lifetime_total: f32,
    // Side of the particle in whole sprite pixels.
    @location(4) @interpolate(flat) size: f32,
};
