//! # The land level (M8)
//!
//! The third stage: the cell leaves the water. Respiration burns carbon with
//! the O2 of the air for much more ATP, but the cell dries out (`desiccation`),
//! and dry ground is heavy to cross. The key innovation is the **cuticle**.
//!
//! - **Water** (`Channel::Water`): a shore along the west edge, puddles, and
//!   moist soil pores. Water decays fast on dry ground, so a wet zone is local.
//! - **Air**: O2 everywhere (`AIR_O2`). A cell takes it in (`oxygen`); a cell that
//!   makes O2 puts it back.
//! - **Light**: from above, the same everywhere (`LIGHT`). UV is half of it
//!   (`UV_FRACTION`). Photosynthesis still works and so does UV damage.
//! - **Drag** (`drag_multiplier`): `DRY_DRAG` times the water value on dry ground,
//!   1 in a wet zone, in between at the edge of a zone.
//! - **Shelters**: four moist soil pores (`SHELTERS`). Each also has a small wet
//!   zone, so a cell that rests there is wet.
//! - **Tissue**: every wall kind so far. A tough line, a plain wall with pores, a
//!   crawl-through mat band (a moss crust), and a new **waxy hedge** (plant-like:
//!   solid, stops water, lets O2 through). The gate chamber is waxy, with a wet
//!   room inside.
//! - **Key gene**: three prey that carry the cuticle wait at a puddle near the start
//!   (`CUTICLE_PREY`); they are topped up while the player lacks it.
//! - **Creatures**: the old archetypes. Creatures do not dry out (**playtest**).
//!   The prey that make energy photosynthesise, so they put O2 into the air.
//!   **Art owner:** land needs its own palette (dry earth ramp for the ground,
//!   green and ochre for the plant-like tissue, a pale blue for water).
//! - **Gate**: a placeholder. The gate cell gives `Multicellularity` and winning
//!   completes the run.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::envgrid::FieldParams;
use crate::environment::Channel;
use crate::level::{
    Chamber, FIELD_HEIGHT, FIELD_WIDTH, FieldKind, KeyGene, LIGHT_DECAY, LevelDef, NpcSpawn, Shaft,
    SurfaceLight, WallLine, WetSpot,
};
use crate::npc::Archetype;
use crate::shelter::Shelter;
use crate::walls::WallKind;

/// O2 level of the air. **Playtest.**
pub const AIR_O2: f32 = 4.0;
/// Light level, the same everywhere. Low enough that the start is in weak UV.
/// **Playtest.**
pub const LIGHT: f32 = 2.8;
/// Share of the light that is UV. **Playtest.**
pub const UV_FRACTION: f32 = 0.5;
/// Drag of dry ground, as a multiple of the drag in water. **Playtest.**
pub const DRY_DRAG: f32 = 2.5;
/// Water level on the shore (a long edge: the level is the steady state).
pub const WATER_LEVEL: f32 = 10.0;
/// Source level of a puddle. A puddle is small, so it loses water to the dry
/// ground round it, and the level in its middle is about 9. **Playtest.**
pub const PUDDLE_LEVEL: f32 = 18.0;
/// Source level of a moist soil pore and of the gate room (about 6 inside).
pub const PORE_LEVEL: f32 = 12.0;
/// The farthest that a hop from one wet zone to the next may be, edge to edge
/// (world units). The layout validator checks it. **Playtest.**
pub const MAX_DRY_HOP: f32 = 350.0;

/// The start: on the dry ground just east of the shore.
pub const START_POSITION: Vec2 = Vec2::new(-780.0, 30.0);
/// The centre of the zone where the cuticle prey wait: a puddle.
pub const CUTICLE_PREY: Vec2 = Vec2::new(-470.0, 150.0);
/// The cuticle prey needed while the player has no cuticle.
pub const CUTICLE_PREY_TARGET: usize = 3;
/// The gate cell: in the middle of the waxy chamber.
pub const GATE_POSITION: Vec2 = Vec2::new(800.0, 80.0);
/// The sprite of the gate cell. A stand-in: the phototroph gate. **Art owner:**
/// a multicellular colony (see the art doc).
pub const GATE_SPRITE: &str = "gate_cell_phototroph";

/// The moist soil pores (shelters).
pub const SHELTERS: [Shelter; 4] = [
    Shelter { position: Vec2::new(-700.0, -90.0), radius: 48.0 },
    Shelter { position: Vec2::new(-110.0, 60.0), radius: 48.0 },
    Shelter { position: Vec2::new(160.0, -260.0), radius: 48.0 },
    Shelter { position: Vec2::new(620.0, 330.0), radius: 48.0 },
];

/// The gate chamber: interior grid cells, walls round it.
pub const CHAMBER_X: std::ops::RangeInclusive<usize> = 54..=59;
pub const CHAMBER_Y: std::ops::RangeInclusive<usize> = 24..=28;
pub const CHAMBER_PORES: [usize; 2] = [54, 58];
/// The plain wall line: grid row, over grid x.
pub const WALL_LINE: (usize, std::ops::RangeInclusive<usize>) = (20, 14..=30);
/// The moss crust: grid column, over grid y.
pub const MAT_STRIP: (usize, std::ops::RangeInclusive<usize>) = (24, 24..=40);
/// The tough line: grid column, over grid y, and its pores.
pub const TOUGH_LINE: (usize, std::ops::RangeInclusive<usize>) = (40, 12..=36);
pub const TOUGH_PORES: [usize; 2] = [18, 30];
/// The waxy hedge: grid column, over grid y, and its pores.
pub const WAXY_LINE: (usize, std::ops::RangeInclusive<usize>) = (47, 6..=26);
pub const WAXY_PORES: [usize; 2] = [10, 22];

/// How the channels move on land. Water decays fast (it dries) and spreads a
/// little, so a wet zone has a halo of about one grid cell.
pub fn field_params(channel: Channel) -> FieldParams {
    match channel {
        // No vents on land: heat and minerals stay at 0.
        Channel::Heat | Channel::Minerals => FieldParams::STILL,
        Channel::Light => FieldParams {
            diffusion: 0.25,
            decay: LIGHT_DECAY,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        Channel::O2 => FieldParams {
            diffusion: 0.8,
            decay: 0.2,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        Channel::Water => FieldParams {
            diffusion: 0.5,
            decay: 0.5,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        // Clouds that cells secrete: no current on land.
        Channel::Microcin
        | Channel::Slime
        | Channel::Siderophore
        | Channel::FeSiderophore
        | Channel::Autoinducer => crate::secretion::cloud_params(channel, Vec2::ZERO),
    }
}

/// The factor on the drag at a water level: `DRY_DRAG` on dry ground, 1 in
/// water, and a blend (by wetness) between them.
pub fn drag_multiplier(water: f32) -> f32 {
    let wet = crate::desiccation::wetness(water);
    DRY_DRAG + (1.0 - DRY_DRAG) * wet
}

fn npc(archetype: Archetype, x: f32, y: f32, signature: Option<BlockKind>) -> NpcSpawn {
    NpcSpawn { archetype, position: Vec2::new(x, y), signature }
}

/// The creatures at the start. The cuticle prey are slots 0 to 2.
pub fn npc_spawns() -> Vec<NpcSpawn> {
    let cuticle = Some(BlockKind::Cuticle);
    let photo = Some(BlockKind::LightCapture);
    vec![
        npc(Archetype::Prey, -490.0, 175.0, cuticle),
        npc(Archetype::Prey, -440.0, 140.0, cuticle),
        npc(Archetype::Prey, -480.0, 110.0, cuticle),
        // Photosynthesising prey in the wet zones: they put O2 into the air.
        npc(Archetype::Prey, -150.0, -330.0, photo),
        npc(Archetype::Prey, 120.0, 200.0, photo),
        npc(Archetype::Prey, 390.0, -330.0, photo),
        npc(Archetype::Prey, 700.0, 400.0, photo),
        npc(Archetype::Prey, 380.0, 460.0, photo),
        // Other prey.
        npc(Archetype::Prey, -300.0, -330.0, None),
        npc(Archetype::Prey, 20.0, 330.0, None),
        // Predators, far from the start.
        npc(Archetype::Predator, 420.0, 250.0, None),
        npc(Archetype::Predator, -100.0, -520.0, None),
        // Symbionts.
        npc(Archetype::Symbiont, -620.0, -20.0, None),
        npc(Archetype::Symbiont, -100.0, 120.0, None),
        npc(Archetype::Symbiont, 190.0, -150.0, None),
    ]
}

/// The wet zones: the shore, the puddles, the pores and the gate room.
pub fn wet_spots() -> Vec<WetSpot> {
    let mut spots = vec![
        // The shore along the west edge.
        WetSpot::Rect { min: Vec2::new(-1024.0, -768.0), max: Vec2::new(-872.0, 768.0), level: WATER_LEVEL },
    ];
    for (centre, radius) in [
        (CUTICLE_PREY, 70.0),
        (Vec2::new(-150.0, -330.0), 90.0),
        (Vec2::new(120.0, 200.0), 70.0),
        (Vec2::new(390.0, -330.0), 80.0),
        (Vec2::new(380.0, 460.0), 60.0),
        (Vec2::new(700.0, 400.0), 70.0),
        (Vec2::new(760.0, -260.0), 80.0),
    ] {
        spots.push(WetSpot::Disc { centre, radius, level: PUDDLE_LEVEL });
    }
    // The moist soil pores.
    for shelter in SHELTERS {
        spots.push(WetSpot::Disc { centre: shelter.position, radius: 40.0, level: PORE_LEVEL });
    }
    // The room of the gate cell.
    spots.push(WetSpot::Disc { centre: GATE_POSITION, radius: 60.0, level: PORE_LEVEL });
    spots
}

impl LevelDef {
    /// The hand-made base layout of the land level.
    pub fn land_base() -> Self {
        Self {
            name: "Land",
            seed: 0,
            width: FIELD_WIDTH,
            height: FIELD_HEIGHT,
            field: FieldKind::Land,
            vents: Vec::new(),
            shafts: Vec::<Shaft>::new(),
            shaft_level: 0.0,
            surface_light: Some(SurfaceLight { level: LIGHT, depth_scale: 1.0e9 }),
            uv_fraction: UV_FRACTION,
            wet: wet_spots(),
            air_o2: AIR_O2,
            shelters: SHELTERS.to_vec(),
            chamber: Chamber {
                x: CHAMBER_X,
                y: CHAMBER_Y,
                pores: CHAMBER_PORES.to_vec(),
                wall: WallKind::Waxy,
                lit: false,
            },
            lines: vec![
                WallLine {
                    kind: WallKind::Wall,
                    vertical: false,
                    at: WALL_LINE.0,
                    span: WALL_LINE.1,
                    pores: vec![18, 26],
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
                WallLine {
                    kind: WallKind::Waxy,
                    vertical: true,
                    at: WAXY_LINE.0,
                    span: WAXY_LINE.1,
                    pores: WAXY_PORES.to_vec(),
                },
            ],
            npcs: npc_spawns(),
            viruses: vec![
                Vec2::new(-300.0, -270.0),
                Vec2::new(60.0, 40.0),
                Vec2::new(390.0, 210.0),
                Vec2::new(600.0, -170.0),
            ],
            // Carbon is scarce on dry ground: the food is in the wet zones.
            food_zones: vec![
                (CUTICLE_PREY, 90.0),
                (Vec2::new(-150.0, -330.0), 100.0),
                (Vec2::new(120.0, 200.0), 90.0),
                (Vec2::new(390.0, -330.0), 90.0),
                (Vec2::new(700.0, 400.0), 80.0),
            ],
            metal_zones: vec![
                (Vec2::new(-350.0, -330.0), 60.0),
                (Vec2::new(200.0, -100.0), 50.0),
                (Vec2::new(620.0, 120.0), 40.0),
            ],
            initial_food: 18,
            initial_metals: 8,
            gate_position: GATE_POSITION,
            gate_gene: BlockKind::Multicellularity,
            gate_sprite: GATE_SPRITE,
            key_gene: Some(KeyGene {
                gene: BlockKind::Cuticle,
                centre: CUTICLE_PREY,
                count: CUTICLE_PREY_TARGET,
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
    fn the_drag_is_heavy_on_dry_ground_and_light_in_water() {
        assert_eq!(drag_multiplier(0.0), DRY_DRAG);
        assert!(drag_multiplier(WATER_LEVEL) < 1.3);
        assert!(drag_multiplier(1.0) < drag_multiplier(0.1), "wetter is lighter");
        assert!(drag_multiplier(1.0) > 1.0);
    }

    #[test]
    fn the_cuticle_prey_are_the_first_three_slots() {
        let def = LevelDef::land_base();
        let carriers: Vec<_> = def
            .npcs
            .iter()
            .enumerate()
            .filter(|(_, n)| n.signature == Some(BlockKind::Cuticle))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(carriers, vec![0, 1, 2]);
    }
}
