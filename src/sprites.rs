//! # Sprite sheets and animation
//!
//! Loads the sprite sheets that `art/gen_sprites.py` writes to
//! `assets/sprites/anim/`, and plays them. The sheet list, frame sizes, anchors
//! and frame rates all come from `assets/sprites/anim/manifest.json`, so a new
//! sheet from the art generator needs no code change. See `docs/art-direction.md`.
//!
//! * `Sheets` (resource): every sheet of the manifest, loaded at startup. The
//!   loading screen waits for it (see `Sheets::progress`).
//! * `Animator` (component): the sprite name, the clip to play (`idle`, `move`,
//!   `use`...) and the row (the direction, for the 8-direction sheets). The
//!   `animate` system sets the atlas frame of the `Sprite` from it.
//!
//! The sheets bake the 8 directions, because turning a pixel sprite at run time
//! would turn its light and break its pixel grid. So a sprite is never rotated:
//! the game picks a row with `direction_row`.

use bevy::prelude::*;
use bevy::sprite::Anchor;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// The manifest, compiled in so the code and the art cannot drift apart.
const MANIFEST: &str = include_str!("../assets/sprites/anim/manifest.json");
/// Folder of the sheets, inside `assets/`.
const SHEET_DIR: &str = "sprites/anim";

/// Screen pixels for each sprite pixel. Whole numbers keep the pixel grid even.
pub const PIXEL_SCALE: f32 = 3.0;

/// Clip names, as the manifest writes them.
pub mod clip {
    pub const IDLE: &str = "idle";
    pub const MOVE: &str = "move";
    pub const USE: &str = "use";
    pub const HOLD: &str = "hold";
    pub const SPAWN: &str = "spawn";
    pub const DEATH: &str = "death";
    pub const ENGULF: &str = "engulf";
    pub const DIGEST_END: &str = "digest_end";
    pub const VICTORY: &str = "victory";
}

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, load_sheets)
        .add_systems(Update, animate.in_set(SpriteSystems::Animate));
}

/// Systems that change which frame a sprite shows. Run the systems that pick a
/// clip or a row before this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpriteSystems {
    Animate,
}

// --------------------------------------------------------------------------
// Manifest
// --------------------------------------------------------------------------

/// One sheet (one clip of one sprite).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ClipSpec {
    pub file: String,
    pub columns: u32,
    pub rows: u32,
    pub fps: f32,
    #[serde(rename = "loop")]
    pub looped: bool,
    /// Present on sheets whose rows are the 8 directions.
    #[serde(default)]
    pub directions: Option<Vec<String>>,
    /// Frame size of this sheet, when it differs from the sprite's (the
    /// engulf pairs have a size for each clip).
    #[serde(default)]
    pub frame_size: Option<[u32; 2]>,
    /// Anchor of this sheet, when it differs from the sprite's.
    #[serde(default)]
    pub anchor_px: Option<[f32; 2]>,
}

impl ClipSpec {
    /// True if each row is a direction (see `direction_row`).
    pub fn is_directional(&self) -> bool {
        self.directions.is_some()
    }

    /// The frame size of this sheet.
    pub fn frame_size_or(&self, default: UVec2) -> UVec2 {
        self.frame_size.map_or(default, |[x, y]| UVec2::new(x, y))
    }
}

/// A sprite of the manifest: its frame, anchor and clips.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteInfo {
    pub frame_size: UVec2,
    /// The pixel of a frame that sits on the entity position (y down).
    pub anchor_px: Vec2,
    /// Body radius in sprite pixels. Only cells have one.
    pub radius: Option<f32>,
    pub clips: HashMap<String, ClipSpec>,
}

impl SpriteInfo {
    /// The `Anchor` that puts `anchor_px` on the transform. The anchor is
    /// rounded down to a whole pixel (the cell frames are centred on a pixel
    /// centre, 32.5), so a sprite never sits on a half pixel.
    pub fn anchor(&self) -> Anchor {
        anchor_for(self.anchor_px, self.frame_size)
    }

    /// The `Anchor` of one sheet of the sprite (its own anchor and size, if it
    /// has them).
    pub fn clip_anchor(&self, spec: &ClipSpec) -> Anchor {
        let anchor = spec.anchor_px.map_or(self.anchor_px, |[x, y]| Vec2::new(x, y));
        anchor_for(anchor, spec.frame_size_or(self.frame_size))
    }
}

fn anchor_for(anchor_px: Vec2, frame_size: UVec2) -> Anchor {
    let px = anchor_px.floor();
    let size = frame_size.as_vec2();
    Anchor(Vec2::new(px.x / size.x - 0.5, 0.5 - px.y / size.y))
}

/// Reads the manifest. An entry without one `frame_size` for the whole sprite
/// (the engulf pairs have a size for each clip) takes the size and anchor of its
/// first clip as the sprite's, and every clip keeps its own.
pub fn parse_manifest(json: &str) -> Result<HashMap<String, SpriteInfo>, serde_json::Error> {
    let entries: HashMap<String, Map<String, Value>> = serde_json::from_str(json)?;
    let mut out = HashMap::new();
    for (name, entry) in entries {
        let mut clips: HashMap<String, ClipSpec> = HashMap::new();
        for (key, value) in &entry {
            if value.get("file").is_some() {
                clips.insert(key.clone(), serde_json::from_value(value.clone())?);
            }
        }
        let own = entry
            .get("frame_size")
            .and_then(vec2)
            .zip(entry.get("anchor_px").and_then(vec2));
        let from_clip = || {
            // Sorted, so that the choice does not depend on hash order.
            let mut names: Vec<_> = clips.keys().collect();
            names.sort();
            names.into_iter().find_map(|name| {
                let c = &clips[name];
                Some((
                    Vec2::new(c.frame_size?[0] as f32, c.frame_size?[1] as f32),
                    Vec2::from(c.anchor_px?),
                ))
            })
        };
        let Some((frame_size, anchor_px)) = own.or_else(from_clip) else {
            continue;
        };
        out.insert(
            name,
            SpriteInfo {
                frame_size: frame_size.as_uvec2(),
                anchor_px,
                radius: entry.get("radius").and_then(Value::as_f64).map(|r| r as f32),
                clips,
            },
        );
    }
    Ok(out)
}

fn vec2(value: &Value) -> Option<Vec2> {
    let [x, y] = value.as_array()?.as_slice() else {
        return None;
    };
    Some(Vec2::new(x.as_f64()? as f32, y.as_f64()? as f32))
}

// --------------------------------------------------------------------------
// Pure helpers: directions and frames
// --------------------------------------------------------------------------

/// Number of directions in a directional sheet.
pub const DIRECTIONS: usize = 8;

/// The row of a directional sheet for a world direction (+y up). The rows are E,
/// SE, S, SW, W, NW, N, NE: screen angles in steps of 45 degrees, +y down. A
/// zero direction gives row 0.
pub fn direction_row(dir: Vec2) -> usize {
    if dir == Vec2::ZERO {
        return 0;
    }
    let screen_angle = (-dir.y).atan2(dir.x); // flip y: the rows count clockwise on screen
    let step = (screen_angle / std::f32::consts::FRAC_PI_4).round() as i32;
    step.rem_euclid(DIRECTIONS as i32) as usize
}

/// The world direction (+y up, unit length) that a row points to.
pub fn row_direction(row: usize) -> Vec2 {
    let angle = row as f32 * std::f32::consts::FRAC_PI_4;
    Vec2::new(angle.cos(), -angle.sin())
}

/// The row that points the other way.
pub fn opposite_row(row: usize) -> usize {
    (row + DIRECTIONS / 2) % DIRECTIONS
}

/// The frame to show `elapsed` seconds into a clip of `frames` frames at `fps`.
/// Returns the frame and whether a one-shot clip has ended (it then stays on
/// its last frame). A looped clip never ends.
pub fn frame_at(elapsed: f32, fps: f32, frames: usize, looped: bool) -> (usize, bool) {
    let step = (elapsed.max(0.0) * fps).floor() as usize;
    if looped {
        (step % frames, false)
    } else if step >= frames {
        (frames - 1, true)
    } else {
        (step, false)
    }
}

// --------------------------------------------------------------------------
// Loading
// --------------------------------------------------------------------------

/// A loaded sheet.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub image: Handle<Image>,
    pub layout: Handle<TextureAtlasLayout>,
    pub spec: ClipSpec,
}

/// A sprite of the manifest, with its sheets.
#[derive(Debug, Clone)]
pub struct SpriteSheets {
    pub info: SpriteInfo,
    pub sheets: HashMap<String, Sheet>,
}

/// Every sheet of the manifest.
#[derive(Resource, Debug, Default)]
pub struct Sheets {
    sprites: HashMap<String, SpriteSheets>,
}

/// How far the sheets have loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadProgress {
    pub pending: usize,
    pub failed: usize,
}

impl Sheets {
    pub fn sprite(&self, name: &str) -> Option<&SpriteSheets> {
        self.sprites.get(name)
    }

    pub fn sheet(&self, name: &str, clip: &str) -> Option<&Sheet> {
        self.sprites.get(name)?.sheets.get(clip)
    }

    /// Sheets still loading, and sheets that failed to load.
    pub fn progress(&self, server: &AssetServer) -> LoadProgress {
        let mut progress = LoadProgress {
            pending: 0,
            failed: 0,
        };
        for sheet in self.sprites.values().flat_map(|s| s.sheets.values()) {
            match server.load_state(&sheet.image) {
                bevy::asset::LoadState::Loaded => {}
                bevy::asset::LoadState::Failed(_) => progress.failed += 1,
                _ => progress.pending += 1,
            }
        }
        progress
    }

    /// The parts of a sprite for `name`, showing clip `clip`: the `Sprite`, its
    /// `Anchor` and the `Animator` that plays it. `None` if the manifest has no
    /// such sheet.
    pub fn bundle(&self, name: &'static str, clip: &'static str) -> Option<impl Bundle + use<>> {
        self.bundle_row(name, clip, 0)
    }

    /// Like `bundle`, for a directional sheet that starts on `row`.
    pub fn bundle_row(
        &self,
        name: &'static str,
        clip: &'static str,
        row: usize,
    ) -> Option<impl Bundle + use<>> {
        let sprite = self.sprites.get(name)?;
        let sheet = sprite.sheets.get(clip)?;
        Some((
            Sprite::from_atlas_image(
                sheet.image.clone(),
                TextureAtlas {
                    layout: sheet.layout.clone(),
                    index: 0,
                },
            ),
            sprite.info.clip_anchor(&sheet.spec),
            Animator {
                row,
                ..Animator::new(name, clip)
            },
        ))
    }
}

fn load_sheets(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let manifest = parse_manifest(MANIFEST).expect("assets/sprites/anim/manifest.json is valid");
    let mut sheets = Sheets::default();
    for (name, info) in manifest {
        let loaded = info
            .clips
            .iter()
            .map(|(clip, spec)| {
                let layout = TextureAtlasLayout::from_grid(
                    spec.frame_size_or(info.frame_size),
                    spec.columns,
                    spec.rows,
                    None,
                    None,
                );
                let sheet = Sheet {
                    image: server.load(format!("{SHEET_DIR}/{}", spec.file)),
                    layout: layouts.add(layout),
                    spec: spec.clone(),
                };
                (clip.clone(), sheet)
            })
            .collect();
        sheets.sprites.insert(
            name,
            SpriteSheets {
                info,
                sheets: loaded,
            },
        );
    }
    commands.insert_resource(sheets);
}

// --------------------------------------------------------------------------
// Playing
// --------------------------------------------------------------------------

/// Plays a clip of a sprite. Set `row` for the 8-direction sheets. Change the
/// clip with `play`, so the frame carries over.
#[derive(Component, Debug, Clone)]
pub struct Animator {
    pub name: &'static str,
    pub clip: &'static str,
    /// The row of a directional sheet (see `direction_row`).
    pub row: usize,
    elapsed: f32,
    finished: bool,
}

impl Animator {
    pub fn new(name: &'static str, clip: &'static str) -> Self {
        Self {
            name,
            clip,
            row: 0,
            elapsed: 0.0,
            finished: false,
        }
    }

    /// True when a one-shot clip has played to its last frame.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Switch to `clip`. Two looped clips with the same number of frames keep
    /// their frame (so a push does not jump when idle turns to move). Other
    /// switches start the clip from its first frame. The same clip is a no-op.
    pub fn play(&mut self, clip: &'static str, sheets: &Sheets) {
        if clip == self.clip {
            return;
        }
        let from = sheets.sheet(self.name, self.clip).map(|s| &s.spec);
        let to = sheets.sheet(self.name, clip).map(|s| &s.spec);
        self.elapsed = match (from, to) {
            (Some(from), Some(to))
                if from.looped && to.looped && from.columns == to.columns && to.fps > 0.0 =>
            {
                let (frame, _) = frame_at(self.elapsed, from.fps, from.columns as usize, true);
                (frame as f32 + 0.5) / to.fps // the middle of the frame: safe from rounding
            }
            _ => 0.0,
        };
        self.clip = clip;
        self.finished = false;
    }
}

/// Shows the frame that each `Animator` asks for.
fn animate(
    time: Res<Time>,
    sheets: Option<Res<Sheets>>,
    mut sprites: Query<(&mut Animator, &mut Sprite)>,
) {
    let Some(sheets) = sheets else {
        return;
    };
    for (mut animator, mut sprite) in &mut sprites {
        let Some(sheet) = sheets.sheet(animator.name, animator.clip) else {
            continue;
        };
        let spec = &sheet.spec;
        animator.elapsed += time.delta_secs();
        let (frame, finished) =
            frame_at(animator.elapsed, spec.fps, spec.columns as usize, spec.looped);
        animator.finished = finished;
        let row = if spec.is_directional() {
            animator.row.min(spec.rows as usize - 1)
        } else {
            0
        };
        let index = row * spec.columns as usize + frame;
        if sprite.image != sheet.image {
            sprite.image = sheet.image.clone();
        }
        let atlas = TextureAtlas {
            layout: sheet.layout.clone(),
            index,
        };
        if sprite.texture_atlas.as_ref() != Some(&atlas) {
            sprite.texture_atlas = Some(atlas);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rows_follow_the_screen_angle_with_y_up() {
        // Rows: E, SE, S, SW, W, NW, N, NE. World +y is up, so N is +y.
        assert_eq!(direction_row(Vec2::X), 0);
        assert_eq!(direction_row(Vec2::new(1.0, -1.0)), 1); // down and right: SE
        assert_eq!(direction_row(Vec2::NEG_Y), 2);
        assert_eq!(direction_row(Vec2::NEG_X), 4);
        assert_eq!(direction_row(Vec2::Y), 6);
        assert_eq!(direction_row(Vec2::new(1.0, 1.0)), 7); // up and right: NE
        assert_eq!(direction_row(Vec2::ZERO), 0);
    }

    #[test]
    fn row_direction_is_the_inverse_of_direction_row() {
        for row in 0..DIRECTIONS {
            assert_eq!(direction_row(row_direction(row)), row);
            assert_eq!(
                direction_row(-row_direction(row)),
                opposite_row(row),
                "row {row}"
            );
        }
    }

    #[test]
    fn frames_loop_or_stop_on_the_last_frame() {
        assert_eq!(frame_at(0.0, 10.0, 8, true), (0, false));
        assert_eq!(frame_at(0.35, 10.0, 8, true), (3, false));
        assert_eq!(frame_at(0.85, 10.0, 8, true), (0, false), "wraps");
        assert_eq!(frame_at(0.75, 10.0, 8, false), (7, false));
        assert_eq!(frame_at(5.0, 10.0, 8, false), (7, true), "one-shot ends");
    }

    #[test]
    fn manifest_has_the_player_sheets_and_pixel_anchors() {
        let manifest = parse_manifest(MANIFEST).unwrap();
        let player = &manifest["player_cell"];
        assert_eq!(player.frame_size, UVec2::new(64, 64));
        assert_eq!(player.radius, Some(20.0));
        let idle = &player.clips[clip::IDLE];
        assert_eq!((idle.columns, idle.rows, idle.looped), (8, 1, true));
        let walk = &player.clips[clip::MOVE];
        assert_eq!((walk.columns, walk.rows), (8, 8));
        assert!(walk.is_directional() && !idle.is_directional());
        assert!(!player.clips[clip::SPAWN].looped);
        // The 32.5 anchor rounds down to the pixel 32, the middle of the frame.
        assert_eq!(player.anchor(), Anchor(Vec2::ZERO));
        let flagellum = &manifest["overlay_flagellum"];
        assert_eq!(flagellum.anchor(), Anchor(Vec2::ZERO));
        assert!(flagellum.clips.contains_key(clip::USE));
    }

    #[test]
    fn the_engulf_pairs_and_the_victory_clip_are_in_the_manifest() {
        let manifest = parse_manifest(MANIFEST).unwrap();
        for pair in ["engulf_player_prey", "engulf_player_symbiont"] {
            let info = &manifest[pair];
            let engulf = &info.clips[clip::ENGULF];
            assert_eq!(engulf.frame_size_or(info.frame_size), UVec2::new(112, 112));
            assert!(engulf.is_directional() && !engulf.looped);
            assert!(info.clips.contains_key(clip::DIGEST_END));
        }
        let victory = &manifest["gate_cell_victory"].clips[clip::VICTORY];
        assert_eq!((victory.columns, victory.rows), (16, 1));
        assert!(manifest["prey_bacillus"].clips.contains_key(clip::DEATH));
    }

    /// The gate cell of every stage has an idle sheet, and the NPC sprites of the
    /// shallow-water level are in the manifest.
    #[test]
    fn the_gate_sprite_of_each_level_is_in_the_manifest() {
        let manifest = parse_manifest(MANIFEST).unwrap();
        for stage in crate::stage::STAGES {
            let Some(def) = stage.level_def(1) else { continue };
            let sprite = &manifest[def.gate_sprite];
            assert!(sprite.clips.contains_key(clip::IDLE), "{}", def.gate_sprite);
            assert_eq!(sprite.frame_size, UVec2::new(72, 72), "the gate size of the art doc");
            for npc in &def.npcs {
                assert!(manifest.contains_key(crate::npc::spec(npc.archetype).sprite));
            }
        }
    }

    /// Every sheet in the manifest exists and its PNG has the size that the
    /// manifest says. This catches art and code that drift apart.
    #[test]
    fn manifest_sheets_exist_with_the_right_size() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(SHEET_DIR);
        for (name, info) in parse_manifest(MANIFEST).unwrap() {
            for (clip, spec) in &info.clips {
                let bytes = std::fs::read(dir.join(&spec.file))
                    .unwrap_or_else(|e| panic!("{name}/{clip}: {}: {e}", spec.file));
                let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
                let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
                assert_eq!(
                    (width, height),
                    {
                        let frame = spec.frame_size_or(info.frame_size);
                        (frame.x * spec.columns, frame.y * spec.rows)
                    },
                    "{name}/{clip}: {}",
                    spec.file
                );
            }
        }
    }
}
