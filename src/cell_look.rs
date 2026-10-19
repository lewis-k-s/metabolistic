//! # The look of a cell body
//!
//! A `CellLook` is the body plan that the cell shader draws
//! (`assets/shaders/cell.wgsl`, `world::shader_cells`): the shape, the ramps,
//! the organelles and the features. It is pure data with no render types, so
//! the headless build has it too. NPCs get theirs at spawn (`npc::spawn_npc`),
//! from the archetype and the signature gene.
//!
//! Lengths are in units of the body radius `R`, the world radius of the cell
//! (`movement::world_radius`), so the body grows with `CellSize` and matches
//! the collider. Angles are world radians (+y up).
//!
//! The looks are ports of the sprite generator (`art/gen_sprites.py`). Plan:
//! `docs/npc-shader-cells-plan.md`.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::npc::Archetype;

/// A 5-tone ramp: outline, shadow, base, light, highlight (sRGB hex codes).
pub type Ramp = [&'static str; 5];

/// The ramps of `RAMPS` in the generator.
pub mod ramps {
    use super::Ramp;
    pub const PLAYER: Ramp = ["#17352f", "#3c8a6e", "#78d49a", "#b4f0b8", "#f0ffe0"];
    pub const PREY: Ramp = ["#1b1f4a", "#3a4aa0", "#6f86e0", "#a9bcff", "#e6edff"];
    pub const PREDATOR: Ramp = ["#3a0f1c", "#8a1f33", "#d1394a", "#f2715f", "#ffc2a0"];
    pub const SYMBIONT: Ramp = ["#4a2e0c", "#b0701c", "#f0b030", "#ffd866", "#fff6c0"];
    pub const GATE: Ramp = ["#0f3a3e", "#1f7a73", "#34b8a0", "#7ae6c0", "#d0fff0"];
    pub const PROTEO: Ramp = ["#3a1030", "#8a2a6e", "#c8509c", "#f08cc8", "#ffe0f2"];
    pub const PIGMENT: Ramp = ["#7a2a0a", "#d0541a", "#f08a28", "#ffc060", "#ffe8a0"];
    pub const NUCLEOID: Ramp = ["#2c1a40", "#6a3f8f", "#a56bc4", "#d6a2ea", "#f6dcff"];
    pub const BEAD: Ramp = ["#5a4a1a", "#c8a838", "#f0dc78", "#fff4b0", "#ffffff"];
    pub const GRANULE: Ramp = ["#10261f", "#2a5c48", "#3c8a6e", "#78d49a", "#b4f0b8"];
    pub const VENT: Ramp = ["#5a1408", "#b02c0c", "#ee6a14", "#ffb030", "#fff0a0"];
}

/// The outline of the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A wobbly disc, with optional spikes.
    Blob = 0,
    /// A capsule along the facing.
    Rod = 1,
}

/// Parts that the shader can draw. Bit flags.
pub mod feature {
    /// A flagellum: a wave that runs back from the rear pole.
    pub const FLAGELLUM: u32 = 1 << 0;
    /// A toothed mouth that faces the heading (the predator of the sprites).
    pub const MOUTH: u32 = 1 << 1;
    /// A dithered glow round the body.
    pub const GLOW: u32 = 1 << 2;
    /// Thylakoid arcs that flow (the vent gate cell).
    pub const STRIPES: u32 = 1 << 3;
    /// Wavy inner folds that drift (the proteobacterium gate cell).
    pub const CRISTAE: u32 = 1 << 4;
}

/// What an organelle disc uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganelleKind {
    /// The `inner` ramp, with an outline (a nucleoid).
    Nucleoid = 1,
    /// The `extra` ramp, no outline (a granule).
    Granule = 2,
    /// The `extra` ramp, with an outline (a bead).
    Bead = 3,
}

/// A disc inside the body. The position is in units of `R`, image axes (+y
/// down), from the centre. On a `Rod` the axes turn with the facing: +x is the
/// front.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Organelle {
    pub kind: OrganelleKind,
    pub at: [f32; 2],
    pub radius: f32,
}

/// The body plan of a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellLook {
    pub shape: Shape,
    /// Bit flags of `feature`.
    pub features: u32,
    /// The body.
    pub tones: Ramp,
    /// The membrane rim and the outline (a pigment rim differs from `tones`).
    pub rim: Ramp,
    /// Nucleoids.
    pub inner: Ramp,
    /// Granules and beads.
    pub extra: Ramp,
    /// The glow.
    pub glow: Ramp,
    /// The body radius as a fraction of `R` (spikes reach past it).
    pub body: f32,
    /// The slow breath, as a fraction of the radius.
    pub breath: f32,
    /// Membrane wobble: (amplitude, phase) of the 2nd and 3rd harmonic.
    pub harmonics: [(f32, f32); 2],
    /// Spike count (0: none) and depth, as in `blob_sdf`.
    pub spikes: u32,
    pub spike_amp: f32,
    /// A rod: half the length of its axis and its radius, in `R`.
    pub rod_half: f32,
    pub rod_radius: f32,
    /// The thickness of the membrane rim, in `R`.
    pub band: f32,
    /// The depth over which the ball shading runs, in `R`.
    pub depth: f32,
    /// The scale of the specular sparks (1 = the player's, at its radius).
    pub sparks: f32,
    /// One pixel of the source sprite, in `R`. The fine parts (flagellum,
    /// mouth, stripes) are drawn in sprite pixels times this.
    pub pixel: f32,
    /// The glow: inner and outer radius in `R`, and its strength.
    pub glow_in: f32,
    pub glow_out: f32,
    pub glow_strength: f32,
    /// How far organelles drift over the idle loop, in `R`.
    pub drift: f32,
    pub organelles: [Option<Organelle>; 4],
    /// The facing when the cell has not moved yet.
    pub rest_facing: f32,
}

/// The look of a cell's body. A cell with none draws as the player.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BodyLook(pub CellLook);

const NONE: [Option<Organelle>; 4] = [None; 4];

fn organelle(kind: OrganelleKind, x: f32, y: f32, radius: f32) -> Option<Organelle> {
    Some(Organelle { kind, at: [x, y], radius })
}

impl CellLook {
    /// The player: `player_cell` (radius 20, depth 15, rim 2.6).
    pub fn player() -> Self {
        CellLook {
            shape: Shape::Blob,
            features: 0,
            tones: ramps::PLAYER,
            rim: ramps::PLAYER,
            inner: ramps::NUCLEOID,
            extra: ramps::BEAD,
            glow: ramps::PLAYER,
            body: 1.0,
            breath: 0.012,
            harmonics: [(0.055, 0.6), (0.045, 2.1)],
            spikes: 0,
            spike_amp: 0.0,
            rod_half: 0.0,
            rod_radius: 0.0,
            band: 2.6 / 20.0,
            depth: 0.75,
            sparks: 1.0,
            pixel: 1.0 / 20.0,
            glow_in: 0.0,
            glow_out: 0.0,
            glow_strength: 0.0,
            drift: 0.0,
            organelles: NONE,
            rest_facing: 0.0,
        }
    }

    /// Prey: `prey_bacillus`, a rod with a flagellum (radius 11.25: half axis
    /// 6.05, rod radius 5.2, depth 5, rim 1.8).
    pub fn prey() -> Self {
        let r = 11.25;
        CellLook {
            shape: Shape::Rod,
            features: feature::FLAGELLUM,
            tones: ramps::PREY,
            rim: ramps::PREY,
            inner: ramps::NUCLEOID,
            extra: ramps::GRANULE,
            breath: 0.03,
            harmonics: [(0.0, 0.0); 2],
            rod_half: 6.05 / r,
            rod_radius: 5.2 / r,
            band: 1.8 / r,
            depth: 5.0 / r,
            pixel: 1.0 / r,
            // The nucleoid sits towards the flagellum, the granule to the front.
            organelles: [
                organelle(OrganelleKind::Nucleoid, -2.0 / r, 0.0, 1.5 / r),
                organelle(OrganelleKind::Granule, 3.3 / r, 0.0, 1.1 / r),
                None,
                None,
            ],
            // `PREY_REST_ANGLE` points at the flagellum (image axes).
            rest_facing: (-6.0_f32).atan2(-10.5),
            ..Self::player()
        }
    }

    /// Predator: `predator_cell`, 7 spikes and a toothed mouth. The sprite core
    /// is 17 px; here the spike tips reach the collider (1.2 x the core).
    pub fn predator() -> Self {
        let core = 1.0 / 1.2;
        let k = core / 17.0; // one sprite pixel, in R
        CellLook {
            features: feature::MOUTH,
            tones: ramps::PREDATOR,
            rim: ramps::PREDATOR,
            inner: ramps::NUCLEOID,
            extra: ramps::VENT,
            body: core,
            breath: 0.015,
            harmonics: [(0.04, 1.0), (0.0, 0.0)],
            spikes: 7,
            spike_amp: 0.42,
            band: 2.4 * k,
            depth: 11.0 * k,
            sparks: 1.2,
            pixel: k,
            organelles: [
                organelle(OrganelleKind::Nucleoid, -5.0 * k, 0.0, 4.6 * k),
                organelle(OrganelleKind::Granule, -9.0 * k, -7.0 * k, 1.4 * k),
                organelle(OrganelleKind::Granule, -9.0 * k, 7.0 * k, 1.4 * k),
                organelle(OrganelleKind::Granule, 0.0, 7.5 * k, 1.2 * k),
            ],
            ..Self::player()
        }
    }

    /// Symbiont: `symbiont_cell`, small and gold, a bead and a glow.
    pub fn symbiont() -> Self {
        let r = 9.5;
        CellLook {
            features: feature::GLOW,
            tones: ramps::SYMBIONT,
            rim: ramps::SYMBIONT,
            inner: ramps::NUCLEOID,
            extra: ramps::BEAD,
            glow: ramps::SYMBIONT,
            breath: 0.03,
            harmonics: [(0.06, 0.2), (0.05, 1.4)],
            band: 2.0 / r,
            depth: 8.0 / r,
            sparks: 1.13,
            pixel: 1.0 / r,
            glow_in: 11.0 / r,
            glow_out: 15.5 / r,
            glow_strength: 0.55,
            drift: 0.6 / r,
            organelles: [organelle(OrganelleKind::Bead, 0.0, 0.5 / r, 3.0 / r), None, None, None],
            ..Self::player()
        }
    }

    /// The vent gate cell: `gate_cell_phototroph`, teal with a carotenoid rim
    /// and thylakoid stripes.
    pub fn gate_phototroph() -> Self {
        let r = 25.0;
        CellLook {
            features: feature::GLOW | feature::STRIPES,
            tones: ramps::GATE,
            rim: ramps::PIGMENT,
            inner: ramps::NUCLEOID,
            extra: ramps::BEAD,
            glow: ramps::VENT,
            breath: 0.008,
            harmonics: [(0.03, 0.5), (0.03, 2.4)],
            band: 3.2 / r,
            depth: 20.0 / r,
            sparks: 1.14,
            pixel: 1.0 / r,
            glow_in: 27.0 / r,
            glow_out: 34.5 / r,
            glow_strength: 0.6,
            drift: 0.8 / r,
            organelles: [
                organelle(OrganelleKind::Nucleoid, 8.0 / r, 3.0 / r, 5.5 / r),
                organelle(OrganelleKind::Bead, 11.0 / r, -8.0 / r, 2.2 / r),
                organelle(OrganelleKind::Granule, 4.0 / r, 12.0 / r, 1.6 / r),
                None,
            ],
            ..Self::player()
        }
    }

    /// The shallow-water gate cell: `gate_cell_proteobacterium`, rose with
    /// cristae folds.
    pub fn gate_proteobacterium() -> Self {
        let r = 25.0;
        CellLook {
            features: feature::GLOW | feature::CRISTAE,
            tones: ramps::PROTEO,
            rim: ramps::PROTEO,
            glow: ramps::PROTEO,
            harmonics: [(0.05, 0.5), (0.025, 2.0)],
            band: 3.0 / r,
            organelles: [
                organelle(OrganelleKind::Nucleoid, 11.0 / r, 4.0 / r, 5.0 / r),
                organelle(OrganelleKind::Bead, 6.0 / r, -10.0 / r, 2.0 / r),
                None,
                None,
            ],
            ..Self::gate_phototroph()
        }
    }

    /// The look of a gate cell from the sprite name that its level picks.
    pub fn gate(sprite: &str) -> Self {
        match sprite {
            "gate_cell_proteobacterium" => Self::gate_proteobacterium(),
            _ => Self::gate_phototroph(),
        }
    }

    pub fn has(&self, flag: u32) -> bool {
        self.features & flag != 0
    }

    /// The farthest point of the body and its parts from the centre, in `R`,
    /// at rest (no stretch).
    pub fn reach(&self) -> f32 {
        let wobble = 1.0 + self.breath + self.harmonics[0].0 + self.harmonics[1].0;
        let mut reach = match self.shape {
            Shape::Blob => {
                let spikes = if self.spikes > 0 { 0.78 + self.spike_amp } else { 1.0 };
                self.body * wobble * spikes
            }
            Shape::Rod => self.rod_half + self.rod_radius * (1.0 + self.breath),
        };
        if self.has(feature::FLAGELLUM) {
            // The wave starts 9.4 sprite pixels out, runs 6.4 and swings 2.
            reach = reach.max((9.4 + 6.4 + 2.0) * self.pixel);
        }
        if self.has(feature::GLOW) {
            reach = reach.max(self.glow_out * 1.06);
        }
        reach
    }
}

/// The look of an NPC. The signature gene picks a variant (later phases; now
/// every variant shares the archetype's look).
pub fn look_for(archetype: Archetype, _signature: Option<BlockKind>) -> CellLook {
    match archetype {
        Archetype::Prey => CellLook::prey(),
        Archetype::Predator => CellLook::predator(),
        Archetype::Symbiont => CellLook::symbiont(),
        Archetype::Gate => CellLook::gate_phototroph(),
    }
}

/// Half the side of the quad that holds the body of `radius` pixels: the reach,
/// the largest move stretch (`MAX_MOVE_STRETCH` is 0.15; 0.2 leaves room), and
/// the outline.
pub fn quad_half_extent(look: &CellLook, radius: f32) -> f32 {
    (radius * look.reach() * 1.2 + 3.0).ceil()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Archetype; 4] = [Archetype::Prey, Archetype::Predator, Archetype::Symbiont, Archetype::Gate];

    #[test]
    fn the_player_quad_is_as_large_as_the_spike_quad() {
        // The spike: (radius * 1.12 * 1.2 + 3).ceil().
        for radius in [20.0, 25.0, 30.0] {
            assert!(quad_half_extent(&CellLook::player(), radius) >= (radius * 1.12 * 1.2 + 3.0).ceil());
        }
    }

    #[test]
    fn the_prey_rod_has_the_sprite_length() {
        let prey = CellLook::prey();
        let tip = (prey.rod_half + prey.rod_radius) * 11.25;
        assert!((tip - 11.25).abs() < 1e-3, "{tip}");
    }

    #[test]
    fn spike_tips_reach_the_collider() {
        let predator = CellLook::predator();
        let tips = predator.body * (0.78 + predator.spike_amp);
        assert!((tips - 1.0).abs() < 0.01, "{tips}");
    }

    #[test]
    fn every_archetype_has_a_look_that_fits_its_quad() {
        for archetype in ALL {
            let look = look_for(archetype, None);
            let radius = crate::npc::spec(archetype).size * crate::movement::PIXELS_PER_SIZE_UNIT;
            let half = quad_half_extent(&look, radius);
            assert!(half >= radius * look.reach() + 1.0, "{archetype:?}");
            for organelle in look.organelles.iter().flatten() {
                let far = (organelle.at[0].hypot(organelle.at[1]) + organelle.radius + look.drift) * radius;
                assert!(far < radius * look.reach(), "{archetype:?} organelle outside");
            }
        }
    }

    #[test]
    fn ramps_are_valid_hex() {
        for look in [
            CellLook::player(),
            CellLook::prey(),
            CellLook::predator(),
            CellLook::symbiont(),
            CellLook::gate_phototroph(),
            CellLook::gate_proteobacterium(),
        ] {
            for ramp in [look.tones, look.rim, look.inner, look.extra, look.glow] {
                for code in ramp {
                    assert_eq!(code.len(), 7);
                    assert!(u32::from_str_radix(&code[1..], 16).is_ok(), "{code}");
                }
            }
        }
    }

    #[test]
    fn the_gate_sprite_picks_the_gate_look() {
        assert_eq!(CellLook::gate("gate_cell_proteobacterium"), CellLook::gate_proteobacterium());
        assert_eq!(CellLook::gate("gate_cell_phototroph"), CellLook::gate_phototroph());
    }
}
