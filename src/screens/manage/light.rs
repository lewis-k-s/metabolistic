//! The cytoplasm and its light, behind the metabolic map.
//!
//! One shader quad (`CytoplasmMaterial`, `assets/shaders/cytoplasm.wgsl`)
//! fills the inside of the cell from the membrane down, past the map, to the
//! genome below it (see `codex::genome_view`). It draws the light of the
//! cell's inside:
//!
//! - light through the membrane from the outside: warm with heat, cool with
//!   light (`LocalEnvironment`);
//! - the emitters (`emitters`): a pathway that runs glows in its own colour,
//!   and breathes faster when it runs harder; a pool glows a little in its
//!   colour when it is full.
//!
//! The light is cut into tone bands with a dithered edge, so the map stays
//! pixel art (`docs/art-direction.md`, "Light and darkness"). The light is for
//! the view only: it changes nothing in the simulation.

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use super::diagram::{accent, currency_color, gauge_fraction};
use super::layout::{self, Layout};
use super::{MapEntity, MapLayout, local_cell, map_layer};
use crate::GameState;
use crate::cell::Cell;
use crate::environment::{Channel, LocalEnvironment};
use crate::metabolism::BlockStatus;
use crate::metabolism::view::{LocalMetabolismView, MetabolismView};
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};

const SHADER_PATH: &str = "shaders/cytoplasm.wgsl";
/// The most emitters that the shader takes.
pub const MAX_EMITTERS: usize = 48;
/// The quad reaches this far past the sides of the map, and this far below it
/// (the genome lies there).
const SIDE_MARGIN: f32 = 320.0;
const BOTTOM_MARGIN: f32 = 900.0;
/// Light everywhere in the cell, in the light units of the shader (one band is
/// 0.25). It is in the middle of band 1, so the calm cytoplasm is flat and the
/// dither shows only where the light changes.
const AMBIENT: f32 = 0.25;
/// Membrane light at full heat or full light.
const MEMBRANE_LIGHT: f32 = 0.32;
/// Outside levels that give full membrane light.
const FULL_HEAT: f32 = 5.0;
const FULL_LIGHT: f32 = 6.0;
const Z_CYTOPLASM: f32 = -10.0;

/// The tones of the cytoplasm, dark to light. The second is the old flat
/// colour of the map.
const RAMP: [&str; 5] = ["#090c17", "#0d1220", "#141c32", "#1d2946", "#29395f"];
const WARM: &str = "#ff8a3a";
const COOL: &str = "#a8b8ff";

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<CytoplasmMaterial>::default()).add_systems(
        Update,
        (spawn_cytoplasm.run_if(resource_exists_and_changed::<MapLayout>), update_light)
            .chain()
            .after(super::fit_map)
            .run_if(in_state(GameState::Manage)),
    );
}

#[derive(ShaderType, Debug, Clone)]
pub struct CytoplasmParams {
    ramp: [Vec4; 5],
    emit_box: [Vec4; MAX_EMITTERS],
    emit_col: [Vec4; MAX_EMITTERS],
    rect: Vec4,
    info: Vec4,
    warm: Vec4,
    cool: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CytoplasmMaterial {
    #[uniform(0)]
    params: CytoplasmParams,
}

impl Material2d for CytoplasmMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

fn linear(color: Color) -> Vec3 {
    let l = LinearRgba::from(color);
    Vec3::new(l.red, l.green, l.blue)
}

fn hex(code: &str) -> Color {
    Color::Srgba(Srgba::hex(code).unwrap_or(Srgba::WHITE))
}

/// One light of the map: a box with a colour and a strength.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub centre: Vec2,
    pub half: Vec2,
    pub color: Color,
    pub strength: f32,
}

/// The light of a pathway that runs at `activity` (0..1) at time `t`. It
/// breathes: faster and deeper when it runs harder. 0 when it does not run.
pub fn plate_glow(activity: f32, phase: f32, t: f32) -> f32 {
    if activity <= 0.01 {
        return 0.0;
    }
    let speed = 1.2 + 3.0 * activity;
    let breath = 0.5 + 0.5 * (t * speed + phase).sin();
    activity.sqrt() * (0.32 + 0.14 * breath)
}

/// The light of a pool at `level`: a pool that is full glows a little.
pub fn pool_glow(level: f32) -> f32 {
    0.16 * gauge_fraction(level)
}

/// The emitters of the map at time `t`: the pathways that run, and the pools.
pub fn emitters(view: &MetabolismView, layout: &Layout, t: f32) -> Vec<Emitter> {
    let mut out = Vec::new();
    for (i, p) in view.pathways.iter().enumerate() {
        let Some(place) = layout.plate_place(p.kind) else { continue };
        if !p.owned {
            continue;
        }
        let strength = plate_glow(p.activity * p.throttle.clamp(0.0, 1.0), i as f32 * 1.7, t);
        if strength <= 0.0 {
            continue;
        }
        // A mutated gene gives a sick, red light.
        let color = match p.state {
            Some(BlockStatus::Mutated) => accent(p.kind).mix(&Color::srgb(0.9, 0.2, 0.2), 0.6),
            _ => accent(p.kind),
        };
        out.push(Emitter { centre: place.pos, half: place.size / 2.0, color, strength });
    }
    for currency in Currency::ALL {
        let level = view.pool(currency).map_or(0.0, |p| p.level);
        let strength = pool_glow(level);
        if strength > 0.0 {
            let place = layout.pool_place(currency);
            out.push(Emitter { centre: place.pos, half: place.size / 2.0, color: currency_color(currency), strength });
        }
    }
    // The brightest first, if there are more than the shader takes.
    out.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    out.truncate(MAX_EMITTERS);
    out
}

/// The light through the membrane: (warm, cool) strengths from the outside.
pub fn membrane_light(heat: f32, light: f32) -> (f32, f32) {
    (
        MEMBRANE_LIGHT * (heat / FULL_HEAT).clamp(0.0, 1.0),
        MEMBRANE_LIGHT * (light / FULL_LIGHT).clamp(0.0, 1.0),
    )
}

/// The quad of the cytoplasm, from the membrane down.
fn cytoplasm_rect(layout: &Layout) -> Rect {
    let top = layout.outside_bottom;
    Rect::new(
        -layout.size.x / 2.0 - SIDE_MARGIN,
        -layout.size.y / 2.0 - BOTTOM_MARGIN,
        layout.size.x / 2.0 + SIDE_MARGIN,
        top,
    )
}

#[derive(Component)]
struct Cytoplasm(Handle<CytoplasmMaterial>);

fn spawn_cytoplasm(
    layout: Res<MapLayout>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CytoplasmMaterial>>,
    mut commands: Commands,
) {
    let rect = cytoplasm_rect(&layout);
    let mut ramp = [Vec4::ZERO; 5];
    for (i, code) in RAMP.iter().enumerate() {
        ramp[i] = linear(hex(code)).extend(1.0);
    }
    let material = materials.add(CytoplasmMaterial {
        params: CytoplasmParams {
            ramp,
            emit_box: [Vec4::ZERO; MAX_EMITTERS],
            emit_col: [Vec4::ZERO; MAX_EMITTERS],
            rect: Vec4::new(rect.min.x, rect.min.y, rect.width(), rect.height()),
            info: Vec4::new(0.0, layout.band_y - layout::BAND_HEIGHT / 2.0, 0.0, AMBIENT),
            warm: linear(hex(WARM)).extend(0.0),
            cool: linear(hex(COOL)).extend(0.0),
        },
    });
    commands.spawn((
        Name::new("Cytoplasm"),
        Cytoplasm(material.clone()),
        MapEntity,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(material),
        Transform::from_translation(rect.center().extend(Z_CYTOPLASM)).with_scale(rect.size().extend(1.0)),
        map_layer(),
        Pickable::IGNORE,
        DespawnOnExit(GameState::Manage),
    ));
}


/// Sets the time, the emitters and the membrane light each frame.
fn update_light(
    time: Res<Time<Real>>,
    view: Res<LocalMetabolismView>,
    layout: Res<MapLayout>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&LocalEnvironment, With<Cell>>,
    quads: Query<&Cytoplasm>,
    mut materials: ResMut<Assets<CytoplasmMaterial>>,
) {
    let t = time.elapsed_secs_wrapped();
    let lights = view.0.as_ref().map(|v| emitters(v, &layout, t)).unwrap_or_default();
    let (warm, cool) = local_cell(&players)
        .and_then(|c| cells.get(c).ok())
        .map_or((0.0, 0.0), |env| membrane_light(env.get(Channel::Heat), env.get(Channel::Light)));
    for quad in &quads {
        let Some(mut material) = materials.get_mut(&quad.0) else { continue };
        let p = &mut material.params;
        p.info.x = t;
        p.info.z = lights.len() as f32;
        p.warm.w = warm;
        p.cool.w = cool;
        for (i, e) in lights.iter().enumerate() {
            p.emit_box[i] = Vec4::new(e.centre.x, e.centre.y, e.half.x, e.half.y);
            p.emit_col[i] = linear(e.color).extend(e.strength);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plate_glows_more_when_it_runs_harder_and_not_at_all_when_still() {
        assert_eq!(plate_glow(0.0, 0.0, 3.0), 0.0);
        for t in [0.0, 0.7, 2.9] {
            assert!(plate_glow(1.0, 0.0, t) > plate_glow(0.2, 0.0, t));
        }
        // It breathes: the light changes over time.
        let samples: Vec<f32> = (0..20).map(|i| plate_glow(0.8, 0.0, i as f32 * 0.1)).collect();
        let (lo, hi) = samples.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(hi - lo > 0.05);
    }

    #[test]
    fn membrane_light_follows_the_outside_and_is_capped() {
        assert_eq!(membrane_light(0.0, 0.0), (0.0, 0.0));
        let (warm, cool) = membrane_light(100.0, 100.0);
        assert_eq!((warm, cool), (MEMBRANE_LIGHT, MEMBRANE_LIGHT));
        assert!(membrane_light(2.0, 0.0).0 < warm);
    }

    #[test]
    fn a_full_pool_glows_and_an_empty_one_does_not() {
        assert_eq!(pool_glow(0.0), 0.0);
        assert!(pool_glow(50.0) > pool_glow(5.0));
    }

    #[test]
    fn the_cytoplasm_covers_the_map_and_the_genome_below_it() {
        let layout = Layout::new(layout::MIN_SIZE);
        let rect = cytoplasm_rect(&layout);
        assert!(rect.contains(Vec2::new(0.0, -layout.size.y / 2.0 - 500.0)));
        assert!(rect.max.y <= layout.outside_bottom + 1e-3);
        assert!(rect.min.x < -layout.size.x / 2.0);
    }
}
