//! # Level files
//!
//! A level file is a JSON description of one map of a stage. It holds what the
//! world needs to build the level (`LevelDef`), and the design notes that only
//! people and the level tools read: a summary, the design pillars, a catalog of
//! the creatures with their labels, and notes on the map.
//!
//! Files are in `assets/levels/<stage>/<name>.json`. The id of a level is the
//! path below `assets/levels` without `.json`, for example `vents/classic`. A
//! stage can have many levels.
//!
//! ## Tools
//!
//! - `cargo levels list` lists the levels.
//! - `cargo levels check` checks that every level parses and passes
//!   `layout::validate`.
//! - `cargo levels bake` writes `target/levels/viewer.html` (all levels, open it
//!   in a browser) and one PNG per level.
//! - `cargo levels ascii vents/classic heat` prints a channel as text.
//! - `cargo levels probe vents/classic 120 -40` prints the channels at a point.
//!
//! To play a level: `METABOLISTIC_LEVEL=vents/chimney_forest cargo run`, or put
//! `"level": "vents/chimney_forest"` in a scenario.
//!
//! ## Format
//!
//! Points are world units `[x, y]`: the origin is the centre of the field and y
//! goes up. Tile positions are grid cells `[x, y]` (32 px a cell, `[0, 0]` is
//! the bottom-left cell). Ranges are inclusive `[first, last]`.
//!
//! | Field | Meaning |
//! |---|---|
//! | `name`, `stage` | Display name, and `Vents`, `ShallowWater` or `Land`. |
//! | `summary`, `pillars`, `proposals` | Design text: what the map is, how the environment shapes it, and mechanics it would need that the engine does not have yet. The game ignores them. |
//! | `size` | `[width, height]` in grid cells. |
//! | `currents` | Per channel (`Heat`, `Minerals`, `Light`, `O2`, `Water`): `diffusion`, `decay`, `drift` `[x, y]` (cells per second) and `open_edges`. A missing value keeps the stage value. |
//! | `vents` | `{ "at", "radius", "heat", "minerals" }`. |
//! | `shafts` | Light columns `{ "min", "max" }`. |
//! | `shelters` | `{ "at", "radius" }`. |
//! | `chamber` | The gate room: `{ "x", "y", "pores", "wall", "lit" }` (grid cells, pores in the south wall). |
//! | `lines` | Wall lines `{ "kind", "vertical", "at", "span", "pores" }`. |
//! | `tiles` | Free tiles as text, one string per row, top row first. `.` open, `#` tough rock, `W` wall, `o` pore, `~` mat, `x` waxy wall. Exactly `height` rows of `width` characters. |
//! | `mobs` | The creature catalog: id -> `{ "label", "archetype", "signature", "sprite", "proposed", "notes" }`. `archetype` (`Prey`, `Predator`, `Symbiont`) is what the engine runs now; `proposed` is the behaviour the creature should get. |
//! | `spawns` | `{ "mob", "at" }`: one creature of a catalog entry. |
//! | `viruses` | Start points of viruses. |
//! | `food_zones`, `metal_zones` | `{ "at", "radius" }`. |
//! | `gate` | `{ "at", "gene", "sprite" }`. The gene and the sprite default to the stage's. |
//! | `key_gene` | `{ "gene", "centre", "count", "dim_edge" }`. |
//! | `start` | Where the player starts. |
//! | `notes` | Map notes for the viewer: `{ "text", "at", "to", "radius", "rect", "proposed" }`. |
//!
//! Optional stage values (`shaft_level`, `uv_fraction`, `surface_light`,
//! `air_o2`, `wet`, `npcs_like_light`, `initial_food`, `initial_metals`)
//! default to the base level of the stage.
//!
//! The functions here are pure, except `load` and `list`, which read files.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::blocks::genome::BlockKind;
use crate::environment::Channel;
use crate::envgrid::FieldParams;
use crate::level::{Chamber, KeyGene, LevelDef, NpcSpawn, Shaft, SurfaceLight, Vent, WallLine, WetSpot};
use crate::npc::Archetype;
use crate::shelter::Shelter;
use crate::stage::Stage;
use crate::walls::WallKind;

/// The folder of the level files, below the assets folder.
pub const LEVEL_DIR: &str = "levels";
/// Environment variable that names a level to play.
pub const LEVEL_VAR: &str = "METABOLISTIC_LEVEL";
/// Command line option that names a level to play.
pub const LEVEL_ARG: &str = "--level";

/// Sprites that a gate cell can use. The world keeps sprite names as static
/// text, so a file can only pick one of these.
pub const GATE_SPRITES: [&str; 2] = ["gate_cell_phototroph", "gate_cell_proteobacterium"];

type Point = [f32; 2];
type Span = [usize; 2];

/// One level, as it is in the file. See the module docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelFile {
    pub name: String,
    pub stage: Stage,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pillars: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<String>,
    #[serde(default)]
    pub seed: u64,
    pub size: [usize; 2],
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub currents: BTreeMap<Channel, FlowFile>,
    #[serde(default)]
    pub vents: Vec<VentFile>,
    #[serde(default)]
    pub shafts: Vec<RectFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaft_level: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_fraction: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_light: Option<SurfaceLightFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_o2: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wet: Option<Vec<WetFile>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npcs_like_light: Option<bool>,
    #[serde(default)]
    pub shelters: Vec<DiscFile>,
    pub chamber: ChamberFile,
    #[serde(default)]
    pub lines: Vec<LineFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiles: Vec<String>,
    #[serde(default)]
    pub mobs: BTreeMap<String, MobFile>,
    #[serde(default)]
    pub spawns: Vec<SpawnFile>,
    #[serde(default)]
    pub viruses: Vec<Point>,
    #[serde(default)]
    pub food_zones: Vec<DiscFile>,
    #[serde(default)]
    pub metal_zones: Vec<DiscFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_food: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_metals: Option<usize>,
    pub gate: GateFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_gene: Option<KeyGeneFile>,
    pub start: Point,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<NoteFile>,
}

/// Changes to how one channel moves. A missing value keeps the stage value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffusion: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_edges: Option<bool>,
}

impl FlowFile {
    /// The stage values with this file's changes.
    pub fn apply(&self, base: FieldParams) -> FieldParams {
        FieldParams {
            diffusion: self.diffusion.unwrap_or(base.diffusion),
            decay: self.decay.unwrap_or(base.decay),
            drift: self.drift.map(Vec2::from).unwrap_or(base.drift),
            open_drift_edges: self.open_edges.unwrap_or(base.open_drift_edges),
        }
    }

    /// Every value of `params`, written out.
    pub fn full(params: FieldParams) -> Self {
        Self {
            diffusion: Some(params.diffusion),
            decay: Some(params.decay),
            drift: Some(params.drift.to_array()),
            open_edges: Some(params.open_drift_edges),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VentFile {
    pub at: Point,
    pub radius: f32,
    pub heat: f32,
    pub minerals: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RectFile {
    pub min: Point,
    pub max: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscFile {
    pub at: Point,
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceLightFile {
    pub level: f32,
    pub depth_scale: f32,
}

/// A wet zone: a disc (`at`, `radius`) or a rectangle (`min`, `max`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WetFile {
    Disc { at: Point, radius: f32, level: f32 },
    Rect { min: Point, max: Point, level: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChamberFile {
    pub x: Span,
    pub y: Span,
    pub pores: Vec<usize>,
    #[serde(default = "default_wall")]
    pub wall: WallKind,
    #[serde(default)]
    pub lit: bool,
}

fn default_wall() -> WallKind {
    WallKind::Wall
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineFile {
    #[serde(default = "default_wall")]
    pub kind: WallKind,
    pub vertical: bool,
    pub at: usize,
    pub span: Span,
    #[serde(default)]
    pub pores: Vec<usize>,
}

/// A kind of creature in the level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobFile {
    pub label: String,
    /// What the engine runs now.
    pub archetype: Archetype,
    /// Replaces the signature gene of the archetype.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<BlockKind>,
    /// Sprite for the viewer. Default: the archetype's sprite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
    /// The behaviour this creature should get, if the engine does not have it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnFile {
    pub mob: String,
    pub at: Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateFile {
    pub at: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gene: Option<BlockKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyGeneFile {
    pub gene: BlockKind,
    pub centre: Point,
    pub count: usize,
    #[serde(default)]
    pub dim_edge: bool,
}

/// A note on the map. With `to` it is an arrow, with `radius` a circle, with
/// `rect` (`[min x, min y, max x, max y]`) a box, else a label at `at`.
/// `proposed` marks a mechanic that the engine does not have yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteFile {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proposed: bool,
}

/// The tile character of a wall kind (see the module docs).
pub fn tile_char(kind: WallKind) -> char {
    match kind {
        WallKind::Tough => '#',
        WallKind::Wall => 'W',
        WallKind::Pore => 'o',
        WallKind::Mat => '~',
        WallKind::Waxy => 'x',
    }
}

/// The wall kind of a tile character. `None` for an open tile.
pub fn tile_kind(c: char) -> Result<Option<WallKind>, String> {
    Ok(Some(match c {
        '.' | ' ' => return Ok(None),
        '#' => WallKind::Tough,
        'W' => WallKind::Wall,
        'o' => WallKind::Pore,
        '~' => WallKind::Mat,
        'x' => WallKind::Waxy,
        other => return Err(format!("unknown tile character {other:?}")),
    }))
}

/// The tiles of a text map. Row 0 of the text is the top row of the field.
pub fn parse_tiles(rows: &[String], width: usize, height: usize) -> Result<Vec<((usize, usize), WallKind)>, String> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    if rows.len() != height {
        return Err(format!("tiles has {} rows, the field has {height}", rows.len()));
    }
    let mut tiles = Vec::new();
    for (row, text) in rows.iter().enumerate() {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() != width {
            return Err(format!("tiles row {row} has {} characters, the field is {width} wide", chars.len()));
        }
        let y = height - 1 - row;
        for (x, c) in chars.into_iter().enumerate() {
            if let Some(kind) = tile_kind(c).map_err(|e| format!("tiles row {row}: {e}"))? {
                tiles.push(((x, y), kind));
            }
        }
    }
    Ok(tiles)
}

/// The text map of a list of tiles. Empty if there are no tiles.
pub fn paint_tiles(tiles: &[((usize, usize), WallKind)], width: usize, height: usize) -> Vec<String> {
    if tiles.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![vec!['.'; width]; height];
    for &((x, y), kind) in tiles {
        if x < width && y < height {
            rows[height - 1 - y][x] = tile_char(kind);
        }
    }
    rows.into_iter().map(|r| r.into_iter().collect()).collect()
}

/// The base level of a stage: the defaults for the values a file leaves out.
pub fn stage_base(stage: Stage) -> LevelDef {
    match stage {
        Stage::Vents => LevelDef::vents_base(),
        Stage::ShallowWater => LevelDef::shallow_base(),
        Stage::Land => LevelDef::land_base(),
    }
}

/// The catalog id and entry of a creature of a `LevelDef` (for export).
fn default_mob(archetype: Archetype, signature: Option<BlockKind>) -> (String, MobFile) {
    let base = match archetype {
        Archetype::Prey => "prey",
        Archetype::Predator => "predator",
        Archetype::Symbiont => "symbiont",
        Archetype::Gate => "gate",
    };
    let id = match signature {
        Some(gene) => format!("{base}_{gene:?}").to_lowercase(),
        None => base.to_string(),
    };
    let label = match (archetype, signature) {
        (Archetype::Prey, Some(BlockKind::SecondaryMetabolites)) => "Pigmented bacillus".to_string(),
        (Archetype::Prey, Some(BlockKind::Catalase)) => "Catalase bacillus".to_string(),
        (Archetype::Prey, Some(gene)) => format!("Bacillus ({gene:?})"),
        (Archetype::Prey, None) => "Bacillus".to_string(),
        (Archetype::Predator, _) => "Engulfer".to_string(),
        (Archetype::Symbiont, _) => "Symbiont".to_string(),
        (Archetype::Gate, _) => "Gate cell".to_string(),
    };
    (id, MobFile { label, archetype, signature, sprite: None, proposed: None, notes: String::new() })
}

/// The default sprite of an archetype.
pub fn archetype_sprite(archetype: Archetype) -> &'static str {
    crate::npc::spec(archetype).sprite
}

fn rect(r: &RectFile) -> (Vec2, Vec2) {
    (Vec2::from(r.min), Vec2::from(r.max))
}

fn span(s: Span) -> std::ops::RangeInclusive<usize> {
    s[0]..=s[1]
}

impl LevelFile {
    /// Parses a level from JSON text.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The level as JSON text.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The level that the world builds. Fails on a bad tile map, an unknown
    /// creature id or an unknown gate sprite.
    pub fn to_def(&self) -> Result<LevelDef, String> {
        let base = stage_base(self.stage);
        let [width, height] = self.size;
        if width < 8 || height < 8 {
            return Err(format!("the field must be at least 8 x 8, not {width} x {height}"));
        }
        let field = base.field;
        let field_overrides = self
            .currents
            .iter()
            .map(|(channel, flow)| (*channel, flow.apply(field.params(*channel))))
            .collect();
        let mut npcs = Vec::new();
        for spawn in &self.spawns {
            let mob = self.mobs.get(&spawn.mob).ok_or_else(|| format!("unknown mob {:?}", spawn.mob))?;
            if mob.archetype == Archetype::Gate {
                return Err(format!("mob {:?}: the gate cell is `gate`, not a spawn", spawn.mob));
            }
            npcs.push(NpcSpawn { archetype: mob.archetype, position: Vec2::from(spawn.at), signature: mob.signature });
        }
        let gate_sprite = match &self.gate.sprite {
            None => base.gate_sprite,
            Some(name) => GATE_SPRITES
                .iter()
                .copied()
                .find(|s| s == name)
                .ok_or_else(|| format!("unknown gate sprite {name:?}; use one of {GATE_SPRITES:?}"))?,
        };
        let wet = match &self.wet {
            None => base.wet.clone(),
            Some(spots) => spots
                .iter()
                .map(|w| match *w {
                    WetFile::Disc { at, radius, level } => WetSpot::Disc { centre: Vec2::from(at), radius, level },
                    WetFile::Rect { min, max, level } => WetSpot::Rect { min: Vec2::from(min), max: Vec2::from(max), level },
                })
                .collect(),
        };
        let chamber = &self.chamber;
        if chamber.x[0] == 0 || chamber.y[0] == 0 || chamber.x[1] + 1 >= width || chamber.y[1] + 1 >= height {
            return Err("the chamber and its walls must be inside the field".into());
        }
        Ok(LevelDef {
            name: self.name.clone(),
            seed: self.seed,
            width,
            height,
            field,
            field_overrides,
            vents: self
                .vents
                .iter()
                .map(|v| Vent { position: Vec2::from(v.at), radius: v.radius, heat: v.heat, minerals: v.minerals })
                .collect(),
            shafts: self
                .shafts
                .iter()
                .map(|s| {
                    let (min, max) = rect(s);
                    Shaft { min, max }
                })
                .collect(),
            shaft_level: self.shaft_level.unwrap_or(base.shaft_level),
            surface_light: match self.surface_light {
                Some(s) => Some(SurfaceLight { level: s.level, depth_scale: s.depth_scale }),
                None => base.surface_light,
            },
            uv_fraction: self.uv_fraction.unwrap_or(base.uv_fraction),
            wet,
            air_o2: self.air_o2.unwrap_or(base.air_o2),
            shelters: self.shelters.iter().map(|s| Shelter { position: Vec2::from(s.at), radius: s.radius }).collect(),
            chamber: Chamber {
                x: span(chamber.x),
                y: span(chamber.y),
                pores: chamber.pores.clone(),
                wall: chamber.wall,
                lit: chamber.lit,
            },
            lines: self
                .lines
                .iter()
                .map(|l| WallLine { kind: l.kind, vertical: l.vertical, at: l.at, span: span(l.span), pores: l.pores.clone() })
                .collect(),
            tiles: parse_tiles(&self.tiles, width, height)?,
            npcs,
            viruses: self.viruses.iter().copied().map(Vec2::from).collect(),
            food_zones: self.food_zones.iter().map(|z| (Vec2::from(z.at), z.radius)).collect(),
            metal_zones: self.metal_zones.iter().map(|z| (Vec2::from(z.at), z.radius)).collect(),
            initial_food: self.initial_food.unwrap_or(base.initial_food),
            initial_metals: self.initial_metals.unwrap_or(base.initial_metals),
            gate_position: Vec2::from(self.gate.at),
            gate_gene: self.gate.gene.unwrap_or(base.gate_gene),
            gate_sprite,
            key_gene: self.key_gene.map(|k| KeyGene {
                gene: k.gene,
                centre: Vec2::from(k.centre),
                count: k.count,
                dim_edge: k.dim_edge,
            }),
            npcs_like_light: self.npcs_like_light.unwrap_or(base.npcs_like_light),
            start: Vec2::from(self.start),
        })
    }

    /// A level file that builds exactly `def`. Every stage value is written
    /// out, so the file does not depend on the stage defaults.
    pub fn from_def(def: &LevelDef, stage: Stage) -> Self {
        let mut mobs = BTreeMap::new();
        let mut spawns = Vec::new();
        for npc in &def.npcs {
            let (id, mob) = default_mob(npc.archetype, npc.signature);
            mobs.entry(id.clone()).or_insert(mob);
            spawns.push(SpawnFile { mob: id, at: npc.position.to_array() });
        }
        let disc = |(at, radius): &(Vec2, f32)| DiscFile { at: at.to_array(), radius: *radius };
        Self {
            name: def.name.clone(),
            stage,
            summary: String::new(),
            pillars: Vec::new(),
            proposals: Vec::new(),
            seed: def.seed,
            size: [def.width, def.height],
            currents: def.field_overrides.iter().map(|(c, p)| (*c, FlowFile::full(*p))).collect(),
            vents: def
                .vents
                .iter()
                .map(|v| VentFile { at: v.position.to_array(), radius: v.radius, heat: v.heat, minerals: v.minerals })
                .collect(),
            shafts: def.shafts.iter().map(|s| RectFile { min: s.min.to_array(), max: s.max.to_array() }).collect(),
            shaft_level: Some(def.shaft_level),
            uv_fraction: Some(def.uv_fraction),
            surface_light: def.surface_light.map(|s| SurfaceLightFile { level: s.level, depth_scale: s.depth_scale }),
            air_o2: Some(def.air_o2),
            wet: Some(
                def.wet
                    .iter()
                    .map(|w| match *w {
                        WetSpot::Disc { centre, radius, level } => WetFile::Disc { at: centre.to_array(), radius, level },
                        WetSpot::Rect { min, max, level } => WetFile::Rect { min: min.to_array(), max: max.to_array(), level },
                    })
                    .collect(),
            ),
            npcs_like_light: Some(def.npcs_like_light),
            shelters: def.shelters.iter().map(|s| DiscFile { at: s.position.to_array(), radius: s.radius }).collect(),
            chamber: ChamberFile {
                x: [*def.chamber.x.start(), *def.chamber.x.end()],
                y: [*def.chamber.y.start(), *def.chamber.y.end()],
                pores: def.chamber.pores.clone(),
                wall: def.chamber.wall,
                lit: def.chamber.lit,
            },
            lines: def
                .lines
                .iter()
                .map(|l| LineFile {
                    kind: l.kind,
                    vertical: l.vertical,
                    at: l.at,
                    span: [*l.span.start(), *l.span.end()],
                    pores: l.pores.clone(),
                })
                .collect(),
            tiles: paint_tiles(&def.tiles, def.width, def.height),
            mobs,
            spawns,
            viruses: def.viruses.iter().map(|v| v.to_array()).collect(),
            food_zones: def.food_zones.iter().map(disc).collect(),
            metal_zones: def.metal_zones.iter().map(disc).collect(),
            initial_food: Some(def.initial_food),
            initial_metals: Some(def.initial_metals),
            gate: GateFile {
                at: def.gate_position.to_array(),
                gene: Some(def.gate_gene),
                sprite: Some(def.gate_sprite.to_string()),
            },
            key_gene: def.key_gene.map(|k| KeyGeneFile {
                gene: k.gene,
                centre: k.centre.to_array(),
                count: k.count,
                dim_edge: k.dim_edge,
            }),
            start: def.start.to_array(),
            notes: Vec::new(),
        }
    }

    /// The creature catalog with a count for each entry, in catalog order.
    pub fn roster(&self) -> Vec<(&str, &MobFile, usize)> {
        self.mobs
            .iter()
            .map(|(id, mob)| (id.as_str(), mob, self.spawns.iter().filter(|s| s.mob == *id).count()))
            .collect()
    }
}

/// The folder of the level files.
pub fn level_dir(assets_dir: &Path) -> PathBuf {
    assets_dir.join(LEVEL_DIR)
}

/// The file of a level id (`vents/classic`). A name that ends in `.json` is a path.
pub fn resolve_path(id: &str, assets_dir: &Path) -> PathBuf {
    if id.ends_with(".json") {
        PathBuf::from(id)
    } else {
        level_dir(assets_dir).join(format!("{id}.json"))
    }
}

/// Reads and parses a level file. The error text names the file.
pub fn load(path: &Path) -> Result<LevelFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read level {}: {e}", path.display()))?;
    LevelFile::from_json(&text).map_err(|e| format!("cannot parse level {}: {e}", path.display()))
}

/// Reads a level file and builds its `LevelDef`.
pub fn load_def(id: &str, assets_dir: &Path) -> Result<LevelDef, String> {
    let path = resolve_path(id, assets_dir);
    load(&path)?.to_def().map_err(|e| format!("level {}: {e}", path.display()))
}

/// Every level id below the level folder, sorted: `vents/classic`, ...
pub fn list(assets_dir: &Path) -> Vec<(String, PathBuf)> {
    let root = level_dir(assets_dir);
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let id = path.strip_prefix(&root).unwrap_or(&path).with_extension("");
                let id = id.to_string_lossy().replace('\\', "/");
                found.push((id, path));
            }
        }
    }
    found.sort();
    found
}

/// The level name from the command line (`--level x` or `--level=x`), else
/// from the environment value. The command line wins.
pub fn level_name(args: &[String], env: Option<&str>) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == LEVEL_ARG {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix("--level=") {
            return Some(value.to_string());
        }
    }
    env.filter(|v| !v.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_paint_and_parse_back() {
        let tiles = vec![((0, 0), WallKind::Tough), ((3, 2), WallKind::Mat), ((1, 2), WallKind::Pore)];
        let rows = paint_tiles(&tiles, 4, 3);
        assert_eq!(rows, vec![".o.~", "....", "#..."]);
        let mut back = parse_tiles(&rows, 4, 3).unwrap();
        back.sort_by_key(|(t, _)| (t.1, t.0));
        let mut want = tiles.clone();
        want.sort_by_key(|(t, _)| (t.1, t.0));
        assert_eq!(back, want);
        assert!(parse_tiles(&["...".into()], 4, 1).is_err());
        assert!(parse_tiles(&["..?.".into()], 4, 1).is_err());
    }

    #[test]
    fn every_base_level_survives_a_trip_through_a_file() {
        for stage in crate::stage::STAGES {
            let def = stage_base(stage);
            let file = LevelFile::from_def(&def, stage);
            let text = file.to_json().unwrap();
            let back = LevelFile::from_json(&text).unwrap().to_def().unwrap();
            assert_eq!(back, def, "{stage:?}");
        }
    }

    #[test]
    fn a_short_file_takes_the_stage_defaults() {
        let text = r#"{
            "name": "tiny", "stage": "Vents", "size": [16, 12],
            "currents": { "Minerals": { "drift": [2.0, 0.0] } },
            "chamber": { "x": [10, 12], "y": [5, 7], "pores": [11], "lit": true },
            "mobs": { "lurker": { "label": "Lurker", "archetype": "Predator", "proposed": "waits" } },
            "spawns": [ { "mob": "lurker", "at": [100, 0] } ],
            "gate": { "at": [96, 0] }, "start": [-100, 0]
        }"#;
        let def = LevelFile::from_json(text).unwrap().to_def().unwrap();
        let base = LevelDef::vents_base();
        assert_eq!(def.gate_gene, base.gate_gene);
        assert_eq!(def.uv_fraction, base.uv_fraction);
        let minerals = def.params(Channel::Minerals);
        assert_eq!(minerals.drift, Vec2::new(2.0, 0.0));
        assert_eq!(minerals.decay, base.params(Channel::Minerals).decay);
        assert_eq!(def.npcs[0].archetype, Archetype::Predator);
    }

    #[test]
    fn bad_references_are_errors() {
        let mut file = LevelFile::from_def(&LevelDef::vents_base(), Stage::Vents);
        file.spawns.push(SpawnFile { mob: "nobody".into(), at: [0.0, 0.0] });
        assert!(file.to_def().unwrap_err().contains("nobody"));
        let mut file = LevelFile::from_def(&LevelDef::vents_base(), Stage::Vents);
        file.gate.sprite = Some("unicorn".into());
        assert!(file.to_def().is_err());
    }

    #[test]
    fn the_level_name_comes_from_the_command_line_first() {
        let args = vec!["--level".to_string(), "vents/a".to_string()];
        assert_eq!(level_name(&args, Some("vents/b")).as_deref(), Some("vents/a"));
        assert_eq!(level_name(&[], Some("vents/b")).as_deref(), Some("vents/b"));
        assert_eq!(level_name(&["--level=x".into()], None).as_deref(), Some("x"));
        assert_eq!(level_name(&[], Some("")), None);
    }
}
