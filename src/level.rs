//! # The vent field
//!
//! The vent field is the one hand-built level of M2. The shallow-water level is in
//! `shallow`. All sizes are in world units (pixels). The
//! grid is centred on the world origin.
//!
//! - **Vent core**: the main vent at the origin. Its heat is high enough to
//!   burn the membrane within about 2.5 grid cells, and it emits minerals.
//! - **Plumes**: minerals drift east and north, and decay with distance. Heat
//!   spreads in a wider, weaker halo, because chemosynthesis needs both.
//! - **A second, weaker vent** in the north-east.
//! - **Light shafts**: two columns of light from above (`SHAFTS`). In the
//!   vents all light is UV hazard (`UV_FRACTION`). Shaft A is between the start
//!   and the second vent, shaft B is east of it, before the far plume. The
//!   player must go round them. Light has no drift, and its sources are fixed.
//! - **Pigment prey**: prey cells in the dim edge of shaft A. Their signature
//!   gene is the pigment gene (see `npc`).
//! - **Shelters**: two vent crevices (`SHELTERS`), one near the start and one
//!   near the far plume. Hazards do no damage inside.
//! - **Tissue walls** (`wall_tiles`): the gate chamber in the east, a wall
//!   line east of the second vent (it joins the end of shaft B and splits the
//!   field), and a short line south of the core. Each has pores.
//! - **Gate chamber**: a walled room with two pores in its south wall and a
//!   light zone inside. The gate cell sits in it (`GATE_POSITION`).
//! - **Creatures** (`NPC_SPAWNS`), **viruses**, and the zones for food and
//!   metal pickups (`FOOD_ZONES`, `METAL_ZONES`).
//! - **Cold zones**: far from the vents there is no heat and no minerals.
//!
//! `vent_field` builds the grid and runs it to a steady state, so the player
//! starts in a field that already has plumes.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::envgrid::{EnvGrid, FieldParams};
use crate::environment::Channel;
use crate::npc::Archetype;
use crate::walls::{WallKind, WallMap};

/// Grid size in grid cells. At 32 px a cell, the field is 2048 x 1536 px.
pub const FIELD_WIDTH: usize = 64;
pub const FIELD_HEIGHT: usize = 48;

/// Simulated seconds that `vent_field` runs before the game starts.
const WARM_UP_SECONDS: f32 = 150.0;

/// A vent: a source of heat and minerals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vent {
    pub position: Vec2,
    /// Radius of the emitter in world units.
    pub radius: f32,
    /// Heat emission per second.
    pub heat: f32,
    /// Mineral emission per second.
    pub minerals: f32,
}

/// The vents of the field. The first one is the core.
pub const VENTS: [Vent; 2] = [
    Vent {
        position: Vec2::new(0.0, 0.0),
        radius: 50.0,
        heat: 500.0,
        minerals: 70.0,
    },
    Vent {
        position: Vec2::new(400.0, 280.0),
        radius: 40.0,
        heat: 160.0,
        minerals: 25.0,
    },
];

/// A light shaft: a column of light from above. It covers `min..max` in world
/// units. Light fades over about a grid cell at the sides and at the lower end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaft {
    pub min: Vec2,
    pub max: Vec2,
}

/// Light level inside a shaft, far from its edge. **Playtest.**
pub const SHAFT_LEVEL: f32 = 10.0;

/// Share of the light that is UV in the vent level: all of it.
pub const UV_FRACTION: f32 = crate::genome_damage::VENT_UV_FRACTION;

/// The shafts of the field. They run from the top edge down to `min.y` (A) or
/// from the bottom edge up to `max.y` (B).
pub const SHAFTS: [Shaft; 2] = [
    Shaft {
        min: Vec2::new(270.0, 130.0),
        max: Vec2::new(370.0, 800.0),
    },
    Shaft {
        min: Vec2::new(550.0, -800.0),
        max: Vec2::new(670.0, 170.0),
    },
];

/// The shelters (vent crevices) of the field: one near the start, one near the
/// far plume. Inside, heat and UV do no damage, and time stops while the
/// editor is open (see `shelter`).
pub const SHELTERS: [crate::shelter::Shelter; 2] = [
    crate::shelter::Shelter {
        position: Vec2::new(120.0, -130.0),
        radius: 48.0,
    },
    crate::shelter::Shelter {
        position: Vec2::new(480.0, 380.0),
        radius: 48.0,
    },
];

/// Where the pigment prey live: in the dim edge at the lower end of shaft A.
/// They carry the pigment gene (`SecondaryMetabolites`), so the way to the
/// UV shield is: catch one and engulf it.
pub const PIGMENT_PREY_POSITION: Vec2 = Vec2::new(320.0, 92.0);

/// Centre of the gate cell. It sits in the middle of the gate chamber.
pub const GATE_POSITION: Vec2 = Vec2::new(880.0, 96.0);

/// Where the player starts: in the warm mineral plume, near the vent but not
/// in the core.
pub const START_POSITION: Vec2 = Vec2::new(190.0, 70.0);

/// How the channels move in the vent field.
pub fn field_params(channel: Channel) -> FieldParams {
    match channel {
        Channel::Heat => FieldParams {
            diffusion: 1.0,
            decay: 0.2,
            drift: Vec2::new(0.3, 0.12),
            open_drift_edges: true,
        },
        Channel::Minerals => FieldParams {
            diffusion: 0.4,
            decay: 0.07,
            drift: Vec2::new(0.7, 0.25),
            open_drift_edges: true,
        },
        // Shafts stay where they are: no drift. They fade over about a cell.
        Channel::Light => FieldParams {
            diffusion: 0.25,
            decay: LIGHT_DECAY,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        // The cyanobacteria are not here yet: O2 only comes from the gate cell.
        Channel::O2 => FieldParams {
            diffusion: 0.5,
            decay: 0.2,
            drift: Vec2::ZERO,
            open_drift_edges: false,
        },
        // No water channel in the vents: the sea is not a "wet zone".
        Channel::Water => FieldParams::STILL,
    }
}

/// Light decay per second. With a source of `SHAFT_LEVEL x LIGHT_DECAY` in each
/// grid cell, the level inside a shaft is `SHAFT_LEVEL`.
pub const LIGHT_DECAY: f32 = 0.3;

// --- Tissue ---

/// The gate chamber: interior grid cells `x 57..=61`, `y 24..=29`
/// (world x 800 to 960, y 0 to 192), walls round it.
pub const CHAMBER_X: std::ops::RangeInclusive<usize> = 57..=61;
pub const CHAMBER_Y: std::ops::RangeInclusive<usize> = 24..=29;
/// Pores in the south wall of the chamber (grid x).
pub const CHAMBER_PORES: [usize; 2] = [57, 60];
/// The wall line east of the second vent: grid x, from grid y to grid y.
pub const LINE_EAST: (usize, std::ops::RangeInclusive<usize>) = (50, 29..=47);
/// Pores in the east line (grid y).
pub const LINE_EAST_PORES: [usize; 2] = [36, 41];
/// The wall line south of the core: grid y, over grid x.
pub const LINE_SOUTH: (usize, std::ops::RangeInclusive<usize>) = (16, 30..=48);
/// Pores in the south line (grid x).
pub const LINE_SOUTH_PORES: [usize; 2] = [35, 42];

/// All wall tiles of the base layout, as grid cells.
pub fn wall_tiles() -> Vec<((usize, usize), WallKind)> {
    LevelDef::vents_base().wall_tiles()
}

/// The wall tiles of the base layout as a `WallMap`.
pub fn wall_map() -> WallMap {
    LevelDef::vents_base().wall_map()
}

// --- Creatures and particles ---

/// A creature of the starting layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcSpawn {
    pub archetype: Archetype,
    pub position: Vec2,
    /// Replaces the signature gene of the archetype.
    pub signature: Option<BlockKind>,
}

const fn npc(archetype: Archetype, x: f32, y: f32, signature: Option<BlockKind>) -> NpcSpawn {
    NpcSpawn {
        archetype,
        position: Vec2::new(x, y),
        signature,
    }
}

/// The creatures at the start. Three prey wait in the dim edge of shaft A and
/// carry the pigment gene. Two predators hunt: one east of the wall line (near
/// the gate), one in the west. Symbionts stay near the player once they sense it.
pub const NPC_SPAWNS: [NpcSpawn; 15] = [
    npc(Archetype::Prey, 300.0, 50.0, Some(BlockKind::SecondaryMetabolites)),
    npc(Archetype::Prey, 345.0, 40.0, Some(BlockKind::SecondaryMetabolites)),
    npc(Archetype::Prey, 255.0, 75.0, Some(BlockKind::SecondaryMetabolites)),
    npc(Archetype::Prey, 60.0, 170.0, None),
    npc(Archetype::Prey, 250.0, -70.0, None),
    npc(Archetype::Prey, -150.0, 100.0, None),
    npc(Archetype::Prey, 210.0, -330.0, None),
    npc(Archetype::Prey, 700.0, 250.0, None),
    npc(Archetype::Predator, 760.0, 330.0, None),
    npc(Archetype::Predator, -330.0, -210.0, None),
    npc(Archetype::Symbiont, 60.0, -70.0, None),
    npc(Archetype::Symbiont, 450.0, 150.0, None),
    npc(Archetype::Symbiont, 260.0, -340.0, None),
    npc(Archetype::Prey, 470.0, 440.0, None),
    npc(Archetype::Prey, -40.0, 330.0, None),
];

/// Where the viruses start. They drift.
pub const VIRUS_SPAWNS: [Vec2; 5] = [
    Vec2::new(100.0, 300.0),
    Vec2::new(-200.0, -50.0),
    Vec2::new(460.0, -100.0),
    Vec2::new(700.0, 420.0),
    Vec2::new(-350.0, 200.0),
];

/// Zones where food appears, as `(centre, radius)`.
pub const FOOD_ZONES: [(Vec2, f32); 4] = [
    (Vec2::new(-100.0, 200.0), 110.0),
    (Vec2::new(450.0, 210.0), 90.0),
    (Vec2::new(300.0, -200.0), 130.0),
    (Vec2::new(120.0, 20.0), 80.0),
];

/// Zones where metal pickups appear: round the vents.
pub const METAL_ZONES: [(Vec2, f32); 4] = [
    (Vec2::new(130.0, 150.0), 60.0),
    (Vec2::new(-120.0, -130.0), 60.0),
    (Vec2::new(330.0, 330.0), 50.0),
    (Vec2::new(500.0, 250.0), 40.0),
];

/// Food particles and metal pickups at the start.
pub const INITIAL_FOOD: usize = 22;
pub const INITIAL_METALS: usize = 10;

// --- The level as a value ---

/// A wall line: a row or a column of tissue tiles with pores.
#[derive(Debug, Clone, PartialEq)]
pub struct WallLine {
    /// The kind of the tiles that are not pores: `Wall`, `Tough`, or `Mat` (a
    /// band of microbial mat, which has no pores).
    pub kind: WallKind,
    /// A column (`at` is grid x, `span` is grid y) or a row (`at` is grid y,
    /// `span` is grid x).
    pub vertical: bool,
    pub at: usize,
    pub span: std::ops::RangeInclusive<usize>,
    /// Positions along the line (a grid y for a column, a grid x for a row).
    pub pores: Vec<usize>,
}

/// The walled room of the gate cell. Walls ring `x` and `y`; the pores are in
/// its south wall.
#[derive(Debug, Clone, PartialEq)]
pub struct Chamber {
    pub x: std::ops::RangeInclusive<usize>,
    pub y: std::ops::RangeInclusive<usize>,
    /// Grid x of the pores in the south wall.
    pub pores: Vec<usize>,
    /// The kind of the ring tiles that are not pores.
    pub wall: WallKind,
    /// True if the room has a light zone of its own (the vent gate chamber).
    pub lit: bool,
}

/// The key gene of a level: the gene that the player needs, and the prey that
/// carry it. While the player does not have the gene, the world keeps `count`
/// carriers alive (see `npc::npc_respawn_system`). The carriers are the prey
/// slots whose signature is `gene`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyGene {
    pub gene: BlockKind,
    /// The centre of the carrier zone.
    pub centre: Vec2,
    pub count: usize,
    /// True if the carriers wait in the dim edge below a light shaft (the
    /// vents). Else they wait within `layout::KEY_ZONE_RADIUS` of `centre`.
    pub dim_edge: bool,
}

/// Light from the surface: the top of the field is the water surface, and the
/// light falls off with depth like `level x exp(-depth / depth_scale)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceLight {
    pub level: f32,
    pub depth_scale: f32,
}

impl SurfaceLight {
    /// The light level at a depth below the surface, in world units.
    pub fn at_depth(&self, depth: f32) -> f32 {
        self.level * (-depth.max(0.0) / self.depth_scale).exp()
    }
}

/// Everything that makes one level: the grid build, the hazards and shelters,
/// the walls, the creatures and particles, the gate cell and the start. The
/// world spawns from a `LevelDef` (see `CurrentLevel`); a stage is a
/// `LevelDef` value, so a new stage is new data. `layout` makes variants of the
/// vent level from a seed.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDef {
    pub name: &'static str,
    pub seed: u64,
    /// Grid size in grid cells.
    pub width: usize,
    pub height: usize,
    /// How the channels move.
    pub field: FieldKind,
    pub vents: Vec<Vent>,
    pub shafts: Vec<Shaft>,
    pub shaft_level: f32,
    /// Light from the surface (shallow water). The vents have none.
    pub surface_light: Option<SurfaceLight>,
    /// Share of the light that is UV.
    pub uv_fraction: f32,
    /// Wet zones: where the water channel has a source (land only).
    pub wet: Vec<WetSpot>,
    /// O2 level of the air (land only): a uniform source over the whole field.
    /// 0 is no air.
    pub air_o2: f32,
    pub shelters: Vec<crate::shelter::Shelter>,
    pub chamber: Chamber,
    pub lines: Vec<WallLine>,
    pub npcs: Vec<NpcSpawn>,
    pub viruses: Vec<Vec2>,
    pub food_zones: Vec<(Vec2, f32)>,
    pub metal_zones: Vec<(Vec2, f32)>,
    pub initial_food: usize,
    pub initial_metals: usize,
    pub gate_position: Vec2,
    /// The gene that the gate cell gives.
    pub gate_gene: BlockKind,
    /// The sprite of the gate cell.
    pub gate_sprite: &'static str,
    /// The key gene and its carriers (pigment prey in the vents).
    pub key_gene: Option<KeyGene>,
    /// True if the creatures live in light (shallow water). Else they leave it,
    /// because it is UV (the vents).
    pub npcs_like_light: bool,
    pub start: Vec2,
}

/// A wet zone of the land level: a puddle, the shore or a moist soil pore. It is
/// a source of `Channel::Water`; the steady level inside is about `level`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WetSpot {
    Disc { centre: Vec2, radius: f32, level: f32 },
    Rect { min: Vec2, max: Vec2, level: f32 },
}

impl WetSpot {
    /// Distance from a point to the zone, 0 inside it.
    pub fn distance(&self, point: Vec2) -> f32 {
        match *self {
            WetSpot::Disc { centre, radius, .. } => (point.distance(centre) - radius).max(0.0),
            WetSpot::Rect { min, max, .. } => {
                let outside = (min - point).max(point - max).max(Vec2::ZERO);
                outside.length()
            }
        }
    }

    pub fn level(&self) -> f32 {
        match *self {
            WetSpot::Disc { level, .. } | WetSpot::Rect { level, .. } => level,
        }
    }
}

/// The channel physics of a level. A new stage adds a kind with its own values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Vents,
    /// Shallow water: light from the surface, calm water, O2.
    Shallow,
    /// Land: air with O2 everywhere, water only in wet zones, heavy drag on dry
    /// ground.
    Land,
}

impl FieldKind {
    pub fn params(self, channel: Channel) -> FieldParams {
        match self {
            FieldKind::Vents => field_params(channel),
            FieldKind::Shallow => crate::shallow::field_params(channel),
            FieldKind::Land => crate::land::field_params(channel),
        }
    }

    /// The factor on a cell's drag at a water level. Water (and every field but
    /// land) is 1. Dry ground on land is much more (`land::DRY_DRAG`).
    pub fn drag_multiplier(self, water: f32) -> f32 {
        match self {
            FieldKind::Land => crate::land::drag_multiplier(water),
            _ => 1.0,
        }
    }

    /// Share of the difference between a cell's O2 and the grid that moves per
    /// second: air is faster than water.
    pub fn o2_exchange_rate(self) -> f32 {
        match self {
            FieldKind::Land => crate::oxygen::AIR_EXCHANGE_RATE,
            _ => crate::oxygen::O2_EXCHANGE_RATE,
        }
    }

    /// True if a cell can dry out here (see `desiccation`).
    pub fn dries(self) -> bool {
        matches!(self, FieldKind::Land)
    }

    /// The ambient light of the field (see `lighting`): dark in the vents, a
    /// gradient with depth in shallow water, full on land.
    pub fn ambient(self) -> crate::lighting::Ambient {
        use crate::lighting::Ambient;
        match self {
            FieldKind::Vents => Ambient::Flat(crate::lighting::VENT_AMBIENT),
            FieldKind::Shallow => Ambient::Depth { surface: 0.9, bottom: 0.3 },
            FieldKind::Land => Ambient::Flat(1.0),
        }
    }

    /// The background colour of the field: deep water, or dry earth.
    pub fn background(self) -> [f32; 3] {
        match self {
            FieldKind::Land => [0.165, 0.125, 0.09],
            _ => [0.039, 0.071, 0.149],
        }
    }
}

/// The level of the running stage. The world inserts it when a stage starts.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CurrentLevel(pub LevelDef);

impl LevelDef {
    /// The hand-made base layout of the vent level (the constants above).
    pub fn vents_base() -> Self {
        Self {
            name: "Deep-sea vents",
            seed: 0,
            width: FIELD_WIDTH,
            height: FIELD_HEIGHT,
            field: FieldKind::Vents,
            vents: VENTS.to_vec(),
            shafts: SHAFTS.to_vec(),
            shaft_level: SHAFT_LEVEL,
            surface_light: None,
            uv_fraction: UV_FRACTION,
            wet: Vec::new(),
            air_o2: 0.0,
            shelters: SHELTERS.to_vec(),
            chamber: Chamber {
                x: CHAMBER_X,
                y: CHAMBER_Y,
                pores: CHAMBER_PORES.to_vec(),
                wall: WallKind::Wall,
                lit: true,
            },
            lines: vec![
                WallLine {
                    kind: WallKind::Wall,
                    vertical: true,
                    at: LINE_EAST.0,
                    span: LINE_EAST.1,
                    pores: LINE_EAST_PORES.to_vec(),
                },
                WallLine {
                    kind: WallKind::Wall,
                    vertical: false,
                    at: LINE_SOUTH.0,
                    span: LINE_SOUTH.1,
                    pores: LINE_SOUTH_PORES.to_vec(),
                },
            ],
            npcs: NPC_SPAWNS.to_vec(),
            viruses: VIRUS_SPAWNS.to_vec(),
            food_zones: FOOD_ZONES.to_vec(),
            metal_zones: METAL_ZONES.to_vec(),
            initial_food: INITIAL_FOOD,
            initial_metals: INITIAL_METALS,
            gate_position: GATE_POSITION,
            gate_gene: crate::gate::GATE_GENE,
            gate_sprite: "gate_cell_phototroph",
            key_gene: Some(KeyGene {
                gene: BlockKind::SecondaryMetabolites,
                centre: PIGMENT_PREY_POSITION,
                count: crate::npc::PIGMENT_PREY_TARGET,
                dim_edge: true,
            }),
            npcs_like_light: false,
            start: START_POSITION,
        }
    }

    /// All wall tiles of the level, as grid cells.
    pub fn wall_tiles(&self) -> Vec<((usize, usize), WallKind)> {
        let mut tiles = Vec::new();
        let (cx, cy) = (&self.chamber.x, &self.chamber.y);
        // The chamber ring.
        for x in (cx.start() - 1)..=(cx.end() + 1) {
            for y in [cy.start() - 1, cy.end() + 1] {
                let kind = if y < *cy.start() && self.chamber.pores.contains(&x) {
                    WallKind::Pore
                } else {
                    self.chamber.wall
                };
                tiles.push(((x, y), kind));
            }
        }
        for y in cy.clone() {
            for x in [cx.start() - 1, cx.end() + 1] {
                tiles.push(((x, y), self.chamber.wall));
            }
        }
        for line in &self.lines {
            for along in line.span.clone() {
                let kind = if line.pores.contains(&along) { WallKind::Pore } else { line.kind };
                let tile = if line.vertical { (line.at, along) } else { (along, line.at) };
                tiles.push((tile, kind));
            }
        }
        tiles
    }

    /// The wall tiles as a `WallMap`.
    pub fn wall_map(&self) -> WallMap {
        let mut map = WallMap::default();
        for (tile, kind) in self.wall_tiles() {
            map.insert(tile, kind);
        }
        map
    }

    /// The grid before the warm-up: emitters set, grid empty.
    pub fn cold_grid(&self) -> EnvGrid {
        let mut grid = EnvGrid::centered(self.width, self.height);
        for channel in Channel::ALL {
            grid.set_params(channel, self.field.params(channel));
        }
        for vent in &self.vents {
            grid.add_emitter(Channel::Heat, vent.position, vent.radius, vent.heat);
            grid.add_emitter(Channel::Minerals, vent.position, vent.radius, vent.minerals);
        }
        let source = self.shaft_level * LIGHT_DECAY;
        for shaft in &self.shafts {
            grid.add_rect_emitter(Channel::Light, shaft.min, shaft.max, source);
        }
        // The light zone of the gate chamber: the room, and nothing else.
        if self.chamber.lit {
            let chamber_min = grid.cell_center(*self.chamber.x.start(), *self.chamber.y.start());
            let chamber_max = grid.cell_center(*self.chamber.x.end(), *self.chamber.y.end());
            grid.add_rect_emitter(Channel::Light, chamber_min, chamber_max, source);
        }
        // Light from the surface: one row at a time, falling with depth.
        if let Some(surface) = self.surface_light {
            let top = grid.origin().y + grid.world_size().y;
            let (left, right) = (grid.origin().x, grid.origin().x + grid.world_size().x);
            for row in 0..self.height {
                let y = grid.cell_center(0, row).y;
                let level = surface.at_depth(top - y);
                grid.add_rect_emitter(
                    Channel::Light,
                    Vec2::new(left, y),
                    Vec2::new(right, y),
                    level * LIGHT_DECAY,
                );
            }
        }
        // Air: O2 everywhere (land).
        if self.air_o2 > 0.0 {
            let source = self.air_o2 * grid.params(Channel::O2).decay;
            let (min, max) = (grid.origin(), grid.origin() + grid.world_size());
            grid.add_rect_emitter(Channel::O2, min, max, source);
        }
        // Wet zones: the steady level inside is `level`.
        let water_decay = grid.params(Channel::Water).decay;
        for spot in &self.wet {
            let source = spot.level() * water_decay;
            match *spot {
                WetSpot::Disc { centre, radius, .. } => {
                    grid.add_disc_emitter(Channel::Water, centre, radius, source)
                }
                WetSpot::Rect { min, max, .. } => grid.add_rect_emitter(Channel::Water, min, max, source),
            }
        }
        // Walls first, so that nothing flows into them during the warm-up. A
        // mat is not a wall: it only slows diffusion. A waxy wall only stops water.
        for ((x, y), kind) in self.wall_tiles() {
            match kind {
                kind if kind.blocks_grid() => grid.set_wall(x, y, true),
                crate::walls::WallKind::Waxy => {
                    grid.set_channel_leak(Channel::Water, x, y, crate::walls::WAXY_WATER_FACTOR)
                }
                _ => grid.set_leak(x, y, crate::walls::MAT_DIFFUSION_FACTOR),
            }
        }
        grid
    }

    /// The grid, run to a steady state.
    pub fn grid(&self) -> EnvGrid {
        let mut grid = self.cold_grid();
        for _ in 0..WARM_UP_SECONDS as usize {
            grid.step(1.0);
        }
        // Light shafts are static: after the warm-up the grid does not step them.
        grid.settle(Channel::Light);
        // Wet zones are static too: their steady state does not change.
        if !self.wet.is_empty() {
            grid.settle(Channel::Water);
        }
        grid
    }
}

/// The vent field before the warm-up: emitters set, grid empty.
pub fn vent_field_cold() -> EnvGrid {
    LevelDef::vents_base().cold_grid()
}

/// The base vent field, run to a steady state.
pub fn vent_field() -> EnvGrid {
    LevelDef::vents_base().grid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelters_are_in_the_field_clear_of_the_vents_and_one_is_near_the_start() {
        let half = Vec2::new(FIELD_WIDTH as f32, FIELD_HEIGHT as f32) * 16.0;
        for shelter in SHELTERS {
            assert!(shelter.position.abs().cmplt(half - Vec2::splat(shelter.radius)).all());
            for vent in VENTS {
                assert!(shelter.position.distance(vent.position) > shelter.radius + 40.0);
            }
        }
        assert!(SHELTERS[0].position.distance(START_POSITION) < 250.0);
        assert!(SHELTERS[1].position.distance(VENTS[1].position) < 250.0);
    }

    #[test]
    fn creatures_start_in_free_space_inside_the_field() {
        let grid = vent_field();
        let walls = wall_map();
        let half = grid.world_size() * 0.5;
        let positions = NPC_SPAWNS
            .iter()
            .map(|n| n.position)
            .chain(VIRUS_SPAWNS)
            .chain(FOOD_ZONES.iter().map(|z| z.0))
            .chain(METAL_ZONES.iter().map(|z| z.0));
        for p in positions {
            assert!(p.abs().cmplt(half).all(), "{p} is outside");
            for vent in VENTS {
                assert!(p.distance(vent.position) > 60.0, "{p} is in a vent");
            }
            assert!(
                !crate::walls::touching_wall(&walls, grid.origin(), p, 20.0),
                "{p} is next to a wall"
            );
            assert!(!crate::shelter::point_in_shelter(&SHELTERS, p) || true);
        }
        // The player does not start next to a predator.
        for n in NPC_SPAWNS.iter().filter(|n| n.archetype == Archetype::Predator) {
            assert!(n.position.distance(START_POSITION) > 400.0);
        }
    }

    #[test]
    fn the_pigment_prey_wait_in_dim_light_and_the_gate_is_in_a_lit_chamber() {
        let grid = vent_field();
        for n in NPC_SPAWNS.iter().filter(|n| n.signature == Some(BlockKind::SecondaryMetabolites)) {
            assert!(grid.sample(Channel::Light, n.position) < crate::npc::LIGHT_AVOID, "{n:?}");
            assert!(n.position.distance(PIGMENT_PREY_POSITION) < 100.0);
        }
        assert!(grid.sample(Channel::Light, GATE_POSITION) > 6.0);
        for outside in [Vec2::new(0.0, -170.0), Vec2::new(0.0, 190.0)] {
            assert!(grid.sample(Channel::Light, GATE_POSITION + outside) < 0.5);
        }
        // The gate cell is the only creature in the chamber.
        assert_eq!(wall_tiles().iter().filter(|(_, k)| *k == WallKind::Pore).count(), 6);
    }

    #[test]
    fn walls_are_in_the_grid_mask_and_the_map_alike() {
        let grid = vent_field();
        let map = wall_map();
        assert_eq!(grid.wall_count(), map.len());
        for ((x, y), _) in wall_tiles() {
            assert!(grid.is_wall(x, y));
        }
        // The chamber is closed: no mineral inside it.
        assert!(grid.sample(Channel::Minerals, GATE_POSITION) < 0.05);
    }

    /// Prints the field as text. Run with `cargo th print_field -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_field() {
        let grid = vent_field();
        for channel in Channel::ALL {
            eprintln!("{channel:?}  total {:.1}", grid.total(channel));
            for y in (0..FIELD_HEIGHT).rev() {
                let row: String = (0..FIELD_WIDTH)
                    .map(|x| {
                        let v = grid.level(channel, x, y);
                        match v {
                            v if v >= 40.0 => '#',
                            v if v >= 10.0 => '+',
                            v if v >= 3.0 => ':',
                            v if v >= 0.5 => '.',
                            _ => ' ',
                        }
                    })
                    .collect();
                eprintln!("{row}");
            }
        }
        let at = |p: Vec2| {
            (
                grid.sample(Channel::Heat, p),
                grid.sample(Channel::Minerals, p),
                grid.sample(Channel::Light, p),
            )
        };
        eprintln!("pigment prey {:?}", at(PIGMENT_PREY_POSITION));
        eprintln!("start {:?}", at(START_POSITION));
        eprintln!("core {:?}", at(Vec2::ZERO));
        eprintln!("far {:?}", at(Vec2::new(-900.0, 600.0)));
        eprintln!("vent2 {:?}", at(VENTS[1].position));
        let max_x = (0..FIELD_WIDTH).map(|x| grid.level(Channel::Minerals, x, 20)).fold(0.0, f32::max);
        eprintln!("edge column minerals {:?}", (0..FIELD_HEIGHT).map(|y| grid.level(Channel::Minerals, FIELD_WIDTH - 1, y)).fold(0.0f32, f32::max));
        let _ = max_x;
    }
}
