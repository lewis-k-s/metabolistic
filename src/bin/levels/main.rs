//! # The level tool
//!
//! Views and checks the level files in `assets/levels` (see `level_file`).
//! Run it with the alias: `cargo levels <command>`.
//!
//! | Command | What it does |
//! |---|---|
//! | `list` | Lists the levels with stage, size and creature count. |
//! | `check [ids]` | Parses, builds and validates levels. Exit code 1 if one fails. |
//! | `bake [ids] [--out dir] [--scale n]` | Writes `viewer.html` (every level, open it in a browser), one PNG and one bake JSON per level. Default folder: `target/levels`. |
//! | `ascii <id> [map\|heat\|minerals\|light\|o2\|water]` | Prints the map, or one channel, as text. |
//! | `probe <id> <x> <y>` | Prints the channels, the tile and the nearest things at a world point. |
//! | `export-bases` | Writes the base level of each stage to `assets/levels/<stage>/classic.json`. |
//!
//! An id is `vents/classic`; a stage folder (`vents`) selects all its levels.
//! No id selects every level.

mod bake;
mod render;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bevy::prelude::Vec2;
use metabolistic3d::environment::Channel;
use metabolistic3d::envgrid::EnvGrid;
use metabolistic3d::level::LevelDef;
use metabolistic3d::level_file::{self, LevelFile};
use metabolistic3d::stage::{STAGES, Stage};

/// A level, read, built and checked.
pub struct Loaded {
    pub id: String,
    pub file: LevelFile,
    pub def: LevelDef,
    pub grid: EnvGrid,
    /// Broken layout rules, as text. Empty if the level is valid.
    pub errors: Vec<String>,
}

impl Loaded {
    fn read(id: &str, path: &Path) -> Result<Self, String> {
        let file = level_file::load(path)?;
        let def = file.to_def().map_err(|e| format!("{id}: {e}"))?;
        let grid = def.grid();
        let errors = match metabolistic3d::layout::validate(&def) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(|e| format!("{e:?}")).collect(),
        };
        Ok(Self { id: id.to_string(), file, def, grid, errors })
    }
}

fn stage_folder(stage: Stage) -> &'static str {
    match stage {
        Stage::Vents => "vents",
        Stage::ShallowWater => "shallow",
        Stage::Land => "land",
    }
}

/// The levels that the ids select (all if none).
fn select(assets: &Path, ids: &[String]) -> Result<Vec<(String, PathBuf)>, String> {
    let all = level_file::list(assets);
    if ids.is_empty() {
        return Ok(all);
    }
    let mut picked = Vec::new();
    for id in ids {
        if id.ends_with(".json") {
            picked.push((id.trim_end_matches(".json").to_string(), PathBuf::from(id)));
            continue;
        }
        let matches: Vec<_> = all.iter().filter(|(i, _)| i == id || i.starts_with(&format!("{id}/"))).cloned().collect();
        if matches.is_empty() {
            return Err(format!("no level {id:?}; `cargo levels list` shows them"));
        }
        picked.extend(matches);
    }
    Ok(picked)
}

fn load_all(assets: &Path, ids: &[String]) -> Result<Vec<Loaded>, String> {
    select(assets, ids)?.iter().map(|(id, path)| Loaded::read(id, path)).collect()
}

fn options(args: &[String]) -> (Vec<String>, Vec<(String, String)>) {
    let mut free = Vec::new();
    let mut flags = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(name) = arg.strip_prefix("--") {
            flags.push((name.to_string(), iter.next().cloned().unwrap_or_default()));
        } else {
            free.push(arg.clone());
        }
    }
    (free, flags)
}

fn flag<'a>(flags: &'a [(String, String)], name: &str) -> Option<&'a str> {
    flags.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn run(args: &[String]) -> Result<bool, String> {
    let assets = metabolistic3d::scenario::assets_dir();
    let (command, rest) = args.split_first().map(|(c, r)| (c.as_str(), r)).unwrap_or(("help", &[]));
    let (free, flags) = options(rest);
    match command {
        "list" => {
            for (id, path) in select(&assets, &free)? {
                match level_file::load(&path) {
                    Ok(f) => println!(
                        "{id:28} {:12} {:>3} x {:<3} {:>2} mobs in {} kinds  {}",
                        format!("{:?}", f.stage),
                        f.size[0],
                        f.size[1],
                        f.spawns.len(),
                        f.mobs.len(),
                        f.name
                    ),
                    Err(e) => println!("{id:28} ERROR {e}"),
                }
            }
            Ok(true)
        }
        "check" => {
            let mut ok = true;
            for (id, path) in select(&assets, &free)? {
                match Loaded::read(&id, &path) {
                    Ok(level) if level.errors.is_empty() => println!("ok    {id}"),
                    Ok(level) => {
                        ok = false;
                        println!("FAIL  {id}");
                        for e in &level.errors {
                            println!("        {e}");
                        }
                    }
                    Err(e) => {
                        ok = false;
                        println!("ERROR {id}: {e}");
                    }
                }
            }
            Ok(ok)
        }
        "bake" => {
            let out = flag(&flags, "out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/levels"));
            let scale = flag(&flags, "scale").and_then(|s| s.parse().ok()).unwrap_or(8);
            let levels = load_all(&assets, &free)?;
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let mut bakes = Vec::new();
            for level in &levels {
                let png = out.join(format!("{}.png", level.id));
                render::write_png(level, &png, scale)?;
                let bake = bake::bake(level);
                let json = out.join(format!("{}.bake.json", level.id));
                std::fs::write(&json, serde_json::to_string(&bake).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                let status = if level.errors.is_empty() { "ok".to_string() } else { format!("{} broken rules", level.errors.len()) };
                println!("baked {:28} {}  ({status})", level.id, png.display());
                bakes.push(bake);
            }
            // The fragment is the page body for an artifact host that adds its
            // own document; viewer.html wraps it for a plain browser.
            let fragment = bake::viewer_html(&bakes, &assets);
            let html = out.join("viewer.html");
            let page = format!(
                "<!doctype html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\"></head>\n<body>\n{fragment}\n</body>\n</html>\n"
            );
            std::fs::write(&html, page).map_err(|e| e.to_string())?;
            std::fs::write(out.join("viewer.fragment.html"), &fragment).map_err(|e| e.to_string())?;
            println!("viewer: {}", html.display());
            Ok(levels.iter().all(|l| l.errors.is_empty()))
        }
        "ascii" => {
            let [id, rest @ ..] = free.as_slice() else { return Err("usage: ascii <id> [layer]".into()) };
            let level = load_all(&assets, std::slice::from_ref(id))?.remove(0);
            let layer = rest.first().map(String::as_str).unwrap_or("map");
            let channel = match layer {
                "map" => None,
                name => Some(parse_channel(name)?),
            };
            print!("{}", render::ascii(&level, channel));
            Ok(true)
        }
        "probe" => {
            let [id, x, y] = free.as_slice() else { return Err("usage: probe <id> <x> <y>".into()) };
            let point = Vec2::new(x.parse().map_err(|_| "bad x")?, y.parse().map_err(|_| "bad y")?);
            let level = load_all(&assets, std::slice::from_ref(id))?.remove(0);
            print!("{}", render::probe(&level, point));
            Ok(true)
        }
        "export-bases" => {
            for stage in STAGES {
                let def = level_file::stage_base(stage);
                let mut file = LevelFile::from_def(&def, stage);
                file.name = format!("{} (classic)", def.name);
                let path = level_file::level_dir(&assets).join(stage_folder(stage)).join("classic.json");
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&path, file.to_json().map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
                println!("wrote {}", path.display());
            }
            Ok(true)
        }
        _ => {
            println!("{}", HELP);
            Ok(command == "help")
        }
    }
}

const HELP: &str = "cargo levels <command>
  list [ids]                          levels, stage, size, creatures
  check [ids]                         parse, build and validate
  bake [ids] [--out dir] [--scale n]  viewer.html + PNG + bake JSON (target/levels)
  ascii <id> [map|heat|minerals|light|o2|water]
  probe <id> <x> <y>                  channels and nearby things at a world point
  export-bases                        write each stage's base level to classic.json";

pub fn parse_channel(name: &str) -> Result<Channel, String> {
    Ok(match name.to_lowercase().as_str() {
        "heat" => Channel::Heat,
        "minerals" => Channel::Minerals,
        "light" => Channel::Light,
        "o2" => Channel::O2,
        "water" => Channel::Water,
        other => return Err(format!("unknown layer {other:?}")),
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
