//! # The shallow-water level (M7)
//!
//! The second stage: the Great Oxidation Event, with a little body horror. The
//! top of the field is the water surface. Light comes from the surface and falls
//! with depth (`SurfaceLight`), so the best light is where UV and ROS are worst.
//! The cell lives on photosynthesis, which makes O2 and a little ROS, and the
//! ROS grows with the O2 in the cell. The key innovation is **catalase**.
//!
//! - **Light**: `SURFACE_LIGHT` at the surface, `exp(-depth / DEPTH_SCALE)` below.
//!   UV is `UV_FRACTION` of it, lower than in the vents (the water screens it).
//! - **Vent**: one weak vent on the sea floor (`VENT`): a dim fallback for
//!   chemosynthesis, and a place for metals.
//! - **Shelters**: three microbial mats (`SHELTERS`). The mat tiles (`mat band`)
//!   are crawl-through tissue.
//! - **Tissue**: a microbial mat band across the upper water (any cell crawls
//!   through it slowly), a tough wall line (it fights back when lysed) and the
//!   gate chamber with tough walls on the sea floor.
//! - **Key gene**: three prey that carry catalase wait west of the start
//!   (`CATALASE_PREY`). While the player lacks catalase, they are topped up.
//! - **Creatures**: cyanobacteria-like prey (they photosynthesise, so they make
//!   O2), grazers (predators), symbionts.
//! - **Gate**: the alpha-proteobacterium in the chamber (`GATE_POSITION`). It gives
//!   `Respiration` (the block comes in M8).

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::envgrid::FieldParams;
use crate::environment::Channel;
use crate::level::{
    Chamber, FIELD_HEIGHT, FIELD_WIDTH, FieldKind, KeyGene, LIGHT_DECAY, LevelDef, NpcSpawn, Shaft,
    SurfaceLight, Vent, WallLine,
};
use crate::npc::Archetype;
use crate::shelter::Shelter;
use crate::walls::WallKind;

/// Light at the surface. **Playtest.**
pub const SURFACE_LIGHT: f32 = 10.0;
/// Depth (world units) over which light falls by a factor `e`. **Playtest.**
pub const DEPTH_SCALE: f32 = 560.0;
/// Share of the light that is UV. The vents have 1.0. **Playtest.**
pub const UV_FRACTION: f32 = 0.3;

/// Catalase prey needed while the player has no catalase.
pub const CATALASE_PREY_TARGET: usize = 3;
/// The sprite of the gate cell: the alpha-proteobacterium (`art/gen_sprites.py`).
/// Its victory clip is the phototroph gate's (`gate_cell_victory`): **art owner**
/// may draw one for it.
pub const GATE_SPRITE: &str = "gate_cell_proteobacterium";

/// The weak vent on the sea floor.
pub const VENT: Vent = Vent {
    position: Vec2::new(-200.0, -640.0),
    radius: 40.0,
    heat: 90.0,
    minerals: 30.0,
};

/// The shelters: microbial mats.
pub const SHELTERS: [Shelter; 3] = [
    Shelter { position: Vec2::new(-650.0, -30.0), radius: 48.0 },
    Shelter { position: Vec2::new(-80.0, 400.0), radius: 48.0 },
    Shelter { position: Vec2::new(610.0, 250.0), radius: 48.0 },
];

/// The start: west, in dim light, near a shelter.
pub const START_POSITION: Vec2 = Vec2::new(-760.0, 60.0);
/// The centre of the zone where the catalase prey wait.
pub const CATALASE_PREY: Vec2 = Vec2::new(-440.0, 60.0);
/// The gate cell: in the middle of the chamber on the sea floor.
pub const GATE_POSITION: Vec2 = Vec2::new(800.0, -528.0);

/// The gate chamber: interior grid cells, walls round it.
pub const CHAMBER_X: std::ops::RangeInclusive<usize> = 54..=59;
pub const CHAMBER_Y: std::ops::RangeInclusive<usize> = 5..=9;
pub const CHAMBER_PORES: [usize; 2] = [54, 58];
/// The microbial mat band: grid row, over grid x.
pub const MAT_BAND: (usize, std::ops::RangeInclusive<usize>) = (31, 14..=30);
/// A second, vertical mat strip: grid column, over grid y.
pub const MAT_STRIP: (usize, std::ops::RangeInclusive<usize>) = (38, 8..=26);
/// The tough wall line: grid column, over grid y, and its pores.
pub const TOUGH_LINE: (usize, std::ops::RangeInclusive<usize>) = (46, 4..=30);
pub const TOUGH_PORES: [usize; 2] = [12, 20];

/// How the channels move in shallow water: calm, with a surface that lets O2 go.
pub fn field_params(channel: Channel) -> FieldParams {
    match channel {
        Channel::Heat => FieldParams {
            diffusion: 1.0,
            decay: 0.3,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        Channel::Minerals => FieldParams {
            diffusion: 0.4,
            decay: 0.07,
            drift: Vec2::new(0.2, 0.1),
            open_drift_edges: true,
        },
        Channel::Light => FieldParams {
            diffusion: 0.25,
            decay: LIGHT_DECAY,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        Channel::O2 => FieldParams {
            diffusion: 0.6,
            decay: 0.06,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        Channel::Water => FieldParams::STILL,
        // Clouds that cells secrete drift with the water, like the minerals.
        Channel::Microcin
        | Channel::Slime
        | Channel::Siderophore
        | Channel::FeSiderophore
        | Channel::Autoinducer => crate::secretion::cloud_params(channel, Vec2::new(0.2, 0.1)),
    }
}

fn npc(archetype: Archetype, x: f32, y: f32, signature: Option<BlockKind>) -> NpcSpawn {
    NpcSpawn { archetype, position: Vec2::new(x, y), signature }
}

/// The creatures at the start.
pub fn npc_spawns() -> Vec<NpcSpawn> {
    let cyano = Some(BlockKind::LightCapture);
    let catalase = Some(BlockKind::Catalase);
    vec![
        // The catalase prey: first, so that they are slots 0 to 2.
        npc(Archetype::Prey, -450.0, 95.0, catalase),
        npc(Archetype::Prey, -410.0, 45.0, catalase),
        npc(Archetype::Prey, -480.0, 35.0, catalase),
        // Cyanobacteria: they photosynthesise, in the light.
        npc(Archetype::Prey, -500.0, 420.0, cyano),
        npc(Archetype::Prey, -300.0, 560.0, cyano),
        npc(Archetype::Prey, -100.0, 480.0, cyano),
        npc(Archetype::Prey, 200.0, 600.0, cyano),
        npc(Archetype::Prey, 350.0, 450.0, cyano),
        npc(Archetype::Prey, -650.0, 300.0, cyano),
        // Deep prey: they carry the siderophore gene (`entF`).
        npc(Archetype::Prey, -300.0, -200.0, Some(BlockKind::Siderophore)),
        npc(Archetype::Prey, 100.0, -300.0, Some(BlockKind::Siderophore)),
        // Grazers.
        npc(Archetype::Predator, 340.0, 100.0, None),
        npc(Archetype::Predator, -100.0, -350.0, None),
        // Symbionts: they carry the AI-2 gene (`luxS`).
        npc(Archetype::Symbiont, -600.0, -60.0, Some(BlockKind::Autoinducer)),
        npc(Archetype::Symbiont, 100.0, -100.0, Some(BlockKind::Autoinducer)),
        npc(Archetype::Symbiont, 340.0, -300.0, Some(BlockKind::Autoinducer)),
    ]
}

impl LevelDef {
    /// The hand-made base layout of the shallow-water level.
    pub fn shallow_base() -> Self {
        Self {
            name: "Shallow water".into(),
            seed: 0,
            width: FIELD_WIDTH,
            height: FIELD_HEIGHT,
            field: FieldKind::Shallow,
            field_overrides: Vec::new(),
            vents: vec![VENT],
            shafts: Vec::<Shaft>::new(),
            shaft_level: 0.0,
            surface_light: Some(SurfaceLight { level: SURFACE_LIGHT, depth_scale: DEPTH_SCALE }),
            uv_fraction: UV_FRACTION,
            wet: Vec::new(),
            air_o2: 0.0,
            shelters: SHELTERS.to_vec(),
            chamber: Chamber {
                x: CHAMBER_X,
                y: CHAMBER_Y,
                pores: CHAMBER_PORES.to_vec(),
                wall: WallKind::Tough,
                lit: false,
            },
            lines: vec![
                WallLine {
                    kind: WallKind::Mat,
                    vertical: false,
                    at: MAT_BAND.0,
                    span: MAT_BAND.1,
                    pores: Vec::new(),
                },
                WallLine {
                    kind: WallKind::Mat,
                    vertical: true,
                    at: MAT_STRIP.0,
                    span: MAT_STRIP.1,
                    pores: Vec::new(),
                },
                WallLine {
                    kind: WallKind::Tough,
                    vertical: true,
                    at: TOUGH_LINE.0,
                    span: TOUGH_LINE.1,
                    pores: TOUGH_PORES.to_vec(),
                },
            ],
            tiles: Vec::new(),
            shapes: Vec::new(),
            npcs: npc_spawns(),
            viruses: vec![
                Vec2::new(-200.0, 60.0),
                Vec2::new(200.0, 350.0),
                Vec2::new(0.0, -100.0),
                Vec2::new(560.0, 520.0),
            ],
            food_zones: vec![
                (Vec2::new(-500.0, 380.0), 100.0),
                (Vec2::new(0.0, 420.0), 110.0),
                (Vec2::new(300.0, -100.0), 100.0),
                (Vec2::new(-300.0, -300.0), 100.0),
            ],
            metal_zones: vec![
                (Vec2::new(-130.0, -540.0), 60.0),
                (Vec2::new(100.0, -500.0), 50.0),
                (Vec2::new(560.0, -480.0), 50.0),
            ],
            initial_food: 22,
            initial_metals: 10,
            gate_position: GATE_POSITION,
            gate_gene: BlockKind::Respiration,
            gate_sprite: GATE_SPRITE,
            key_gene: Some(KeyGene {
                gene: BlockKind::Catalase,
                centre: CATALASE_PREY,
                count: CATALASE_PREY_TARGET,
                dim_edge: false,
            }),
            npcs_like_light: true,
            start: START_POSITION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_falls_with_depth_and_uv_is_lower_than_in_the_vents() {
        let surface = SurfaceLight { level: SURFACE_LIGHT, depth_scale: DEPTH_SCALE };
        assert_eq!(surface.at_depth(0.0), SURFACE_LIGHT);
        assert!(surface.at_depth(400.0) < surface.at_depth(100.0));
        assert!(surface.at_depth(1500.0) > 0.0);
        assert!(UV_FRACTION < crate::level::UV_FRACTION);
    }

    #[test]
    fn the_catalase_prey_are_the_first_three_slots() {
        let def = LevelDef::shallow_base();
        let carriers: Vec<_> = def
            .npcs
            .iter()
            .enumerate()
            .filter(|(_, n)| n.signature == Some(BlockKind::Catalase))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(carriers, vec![0, 1, 2]);
        assert_eq!(def.key_gene.unwrap().count, CATALASE_PREY_TARGET);
    }
}
