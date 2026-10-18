//! # Vent particles
//!
//! Smoke and sparkles over each vent, made with `bevy_enoki` (full builds
//! only). See `docs/art-direction.md`, "Particles".
//!
//! - **Smoke**: dark grey-brown puffs (the `waste` ramp) that rise from the
//!   crater and drift with the heat channel of the grid. They are drawn in the
//!   lit layer (below the darkness), so the dark hides them far from light.
//! - **Sparkles**: small light-orange and yellow plus-shaped specks (the `vent`
//!   ramp) that rise and twinkle out. They are emissive: drawn above the
//!   darkness.
//!
//! The look rules hold: each particle is a square on the sprite pixel grid,
//! each pixel is one palette tone, and a particle fades out with a dither
//! (`assets/shaders/vent_particle.wgsl`), never with alpha blending. The
//! particle colour curves carry a tone (red) and a fade (alpha), not a colour.
//! The effect parameters are pure functions (`smoke_effect`, `spark_effect`).

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use bevy::asset::uuid_handle;
use bevy_enoki::prelude::*;

use crate::envgrid::CELL_SIZE;
use crate::level::Vent;
use crate::InRun;

const FRAGMENT_PATH: &str = "shaders/vent_particle.wgsl";
const VERTEX_SOURCE: &str = include_str!("../../assets/shaders/vent_particle_vertex.wgsl");
/// The vertex shader of `bevy_enoki` (its internal handle). We replace it.
const ENOKI_VERTEX: Handle<Shader> = uuid_handle!("57c98346-305c-461a-8cdc-7b3fac8be0ca");

/// Z of the smoke: above the plume sprite (5), below the darkness (20).
const SMOKE_Z: f32 = 6.0;
/// Z of the sparkles: above the darkness and its tint (20.1).
const SPARK_Z: f32 = 30.0;

/// The `waste` ramp of the generator: murky olive-grey, for the smoke.
const SMOKE_RAMP: [&str; 5] = ["#1a1612", "#403a30", "#6a6252", "#948a74", "#c0b8a0"];
/// The `vent` ramp of the generator, for the sparkles.
const VENT_RAMP: [&str; 5] = ["#5a1408", "#b02c0c", "#ee6a14", "#ffb030", "#fff0a0"];

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct PixelParams {
    pub tones: [Vec4; 5],
    pub size: f32,
    pub shape: f32,
    pub pad0: f32,
    pub pad1: f32,
}

/// The particle material: a palette, a size and a shape.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PixelParticleMaterial {
    #[uniform(0)]
    pub params: PixelParams,
}

impl Particle2dMaterial for PixelParticleMaterial {
    fn fragment_shader() -> ShaderRef {
        FRAGMENT_PATH.into()
    }
}

fn tones(codes: [&str; 5]) -> [Vec4; 5] {
    codes.map(|code| {
        let srgb = Srgba::hex(code).unwrap_or(Srgba::WHITE);
        Vec4::from_array(LinearRgba::from(srgb).to_f32_array())
    })
}

pub fn plugin(app: &mut App) {
    app.add_plugins(EnokiPlugin)
        .add_plugins(Particle2dMaterialPlugin::<PixelParticleMaterial>::default());
    // Snap every particle to the sprite pixel grid.
    let _ = app
        .world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(&ENOKI_VERTEX, Shader::from_wgsl(VERTEX_SOURCE, "vent_particle_vertex.wgsl"));
}

/// A colour of a particle curve: `tone` in 0..=4 (a ramp index) and a fade.
fn tone_color(tone: f32, fade: f32) -> LinearRgba {
    LinearRgba::new(tone / 4.0, 0.0, 0.0, fade)
}

/// The smoke of one vent. `size` is the side of a puff in sprite pixels;
/// `drift` is the heat drift of the grid in grid cells per second.
pub fn smoke_effect(size: f32, drift: Vec2, rate: f32, amount: u32) -> Particle2dEffect {
    let drift_dir = drift.normalize_or_zero();
    let drift_speed = drift.length() * CELL_SIZE;
    Particle2dEffect {
        spawn_rate: rate,
        spawn_amount: amount,
        emission_shape: EmissionShape::Circle(14.0),
        lifetime: Rval::new(6.0, 0.3),
        linear_speed: Some(Rval::new(20.0, 0.4)),
        direction: Some(Rval::new(Vec2::Y, 0.12)),
        scale: Some(Rval::new(size, 0.0)),
        gravity_direction: Some(Rval::new(drift_dir, 0.0)),
        gravity_speed: Some(Rval::new(drift_speed * 2.0, 0.25)),
        linear_damp: Some(Rval::new(0.4, 0.0)),
        color_curve: Some(
            MultiCurve::new()
                .with_point(tone_color(2.0, 0.0), 0.0, None)
                .with_point(tone_color(2.0, 0.9), 0.12, None)
                .with_point(tone_color(2.0, 0.8), 0.5, None)
                .with_point(tone_color(1.0, 0.0), 1.0, None),
        ),
        ..default()
    }
}

/// The sparkles of one vent: they rise fast and twinkle out.
pub fn spark_effect(drift: Vec2) -> Particle2dEffect {
    let drift_dir = drift.normalize_or_zero();
    Particle2dEffect {
        spawn_rate: 0.09,
        spawn_amount: 2,
        emission_shape: EmissionShape::Circle(10.0),
        lifetime: Rval::new(1.5, 0.45),
        linear_speed: Some(Rval::new(42.0, 0.6)),
        direction: Some(Rval::new(Vec2::Y, 0.3)),
        scale: Some(Rval::new(3.0, 0.0)),
        gravity_direction: Some(Rval::new(drift_dir, 0.0)),
        gravity_speed: Some(Rval::new(drift.length() * CELL_SIZE, 0.0)),
        color_curve: Some(
            MultiCurve::new()
                .with_point(tone_color(4.0, 1.0), 0.0, None)
                .with_point(tone_color(4.0, 0.3), 0.2, None)
                .with_point(tone_color(3.0, 1.0), 0.35, None)
                .with_point(tone_color(3.0, 0.4), 0.55, None)
                .with_point(tone_color(2.0, 0.9), 0.7, None)
                .with_point(tone_color(2.0, 0.0), 1.0, None),
        ),
        ..default()
    }
}

/// Spawns the smoke and the sparkles of one vent. `drift` is the heat drift of
/// the grid, in grid cells per second.
pub fn spawn_vent_particles(commands: &mut Commands, vent: Vent, drift: Vec2) {
    commands.queue(move |world: &mut World| {
        let spawn = |world: &mut World, name: &'static str, effect: Particle2dEffect, params: PixelParams, z: f32| {
            let effect = world.resource_mut::<Assets<Particle2dEffect>>().add(effect);
            let material = world
                .resource_mut::<Assets<PixelParticleMaterial>>()
                .add(PixelParticleMaterial { params });
            world.spawn((
                Name::new(name),
                ParticleSpawner(material),
                ParticleEffectHandle(effect),
                Transform::from_translation(vent.position.round().extend(z)),
                DespawnOnExit(InRun),
            ));
        };
        let smoke = |size: f32| PixelParams { tones: tones(SMOKE_RAMP), size, shape: 0.0, pad0: 0.0, pad1: 0.0 };
        // Dense smoke: big slow puffs and many small ones.
        spawn(world, "Vent Smoke", smoke_effect(24.0, drift, 0.12, 1), smoke(24.0), SMOKE_Z);
        spawn(world, "Vent Smoke Small", smoke_effect(11.0, drift, 0.05, 2), smoke(11.0), SMOKE_Z);
        spawn(
            world,
            "Vent Sparkles",
            spark_effect(drift),
            PixelParams { tones: tones(VENT_RAMP), size: 3.0, shape: 1.0, pad0: 0.0, pad1: 0.0 },
            SPARK_Z,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_drifts_with_the_grid() {
        let effect = smoke_effect(12.0, Vec2::new(0.3, 0.12), 0.1, 1);
        let dir = effect.gravity_direction.unwrap().0;
        assert!(dir.x > 0.0 && dir.y > 0.0);
        assert!((dir.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn particle_colors_carry_a_tone_and_a_fade() {
        let effect = spark_effect(Vec2::ZERO);
        let curve = effect.color_curve.unwrap();
        // Ends faded out, starts at the lightest tone.
        assert_eq!(curve.lerp(0.0).red, 1.0);
        assert_eq!(curve.lerp(1.5).alpha, 0.0);
    }
}
