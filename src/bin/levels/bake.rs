//! The bake: everything the viewer draws, as JSON. The grid channels come from
//! the real `EnvGrid` warm-up, so the viewer shows what the game builds.
//!
//! `viewer_html` puts every bake and the sprites into the viewer template
//! (`tools/level-viewer/viewer.html`), so the result is one file that opens
//! anywhere.

use std::collections::BTreeSet;
use std::path::Path;

use bevy::prelude::Vec2;
use metabolistic3d::environment::Channel;
use metabolistic3d::envgrid::CELL_SIZE;
use metabolistic3d::exchange::HEAT_DAMAGE_THRESHOLD;
use metabolistic3d::level_file::archetype_sprite;
use metabolistic3d::npc::{Archetype, LIGHT_AVOID};
use serde_json::{Value, json};

use crate::Loaded;
use crate::render::reference;

const TEMPLATE: &str = include_str!("../../../tools/level-viewer/viewer.html");
/// The marker in the template that the data replaces.
const DATA_MARKER: &str = "\"__LEVEL_DATA__\"";

fn round(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn pt(p: Vec2) -> Value {
    json!([round(p.x), round(p.y)])
}

fn role(archetype: Archetype) -> &'static str {
    match archetype {
        Archetype::Prey => "prey",
        Archetype::Predator => "hostile",
        Archetype::Symbiont => "ally",
        Archetype::Gate => "gate",
    }
}

/// The bake of one level.
pub fn bake(level: &Loaded) -> Value {
    let (def, grid, file) = (&level.def, &level.grid, &level.file);
    let mut channels = serde_json::Map::new();
    let mut refs = serde_json::Map::new();
    for channel in Channel::ALL {
        if grid.total(channel) < 0.01 {
            continue;
        }
        let mut values = Vec::with_capacity(def.width * def.height);
        for y in 0..def.height {
            for x in 0..def.width {
                values.push(round(grid.level(channel, x, y)));
            }
        }
        channels.insert(format!("{channel:?}"), json!(values));
        refs.insert(format!("{channel:?}"), json!(reference(channel)));
    }
    let currents: serde_json::Map<String, Value> = Channel::ALL
        .iter()
        .map(|c| {
            let p = def.params(*c);
            (
                format!("{c:?}"),
                json!({ "diffusion": p.diffusion, "decay": p.decay, "drift": [p.drift.x, p.drift.y], "open_edges": p.open_drift_edges, "changed": def.field_overrides.iter().any(|(o, _)| o == c) }),
            )
        })
        .collect();
    let key = def.key_gene.map(|k| k.gene);
    let mobs: serde_json::Map<String, Value> = file
        .roster()
        .into_iter()
        .map(|(id, mob, count)| {
            let sprite = mob.sprite.clone().unwrap_or_else(|| archetype_sprite(mob.archetype).to_string());
            (
                id.to_string(),
                json!({
                    "label": mob.label,
                    "archetype": mob.archetype,
                    "role": role(mob.archetype),
                    "signature": mob.signature,
                    "key": mob.archetype == Archetype::Prey && mob.signature.is_some() && mob.signature == key,
                    "sprite": sprite,
                    "proposed": mob.proposed,
                    "notes": mob.notes,
                    "count": count,
                }),
            )
        })
        .collect();
    let probe = |p: Vec2| {
        let v: serde_json::Map<String, Value> =
            Channel::ALL.iter().map(|c| (format!("{c:?}"), json!(round(grid.sample(*c, p))))).collect();
        Value::Object(v)
    };
    let origin = grid.origin();
    let chamber_min = origin + Vec2::new(*def.chamber.x.start() as f32, *def.chamber.y.start() as f32) * CELL_SIZE;
    let chamber_max = origin + Vec2::new((*def.chamber.x.end() + 1) as f32, (*def.chamber.y.end() + 1) as f32) * CELL_SIZE;
    json!({
        "id": level.id,
        "name": def.name,
        "stage": file.stage,
        "summary": file.summary,
        "pillars": file.pillars,
        "proposals": file.proposals,
        "file": file,
        "size": [def.width, def.height],
        "cell": CELL_SIZE,
        "origin": pt(origin),
        "background": def.field.background(),
        "uv_fraction": def.uv_fraction,
        "heat_damage": HEAT_DAMAGE_THRESHOLD,
        "npc_light_avoid": LIGHT_AVOID,
        "channels": channels,
        "refs": refs,
        "currents": currents,
        "walls": def.wall_tiles().iter().map(|((x, y), k)| json!([x, y, k])).collect::<Vec<_>>(),
        "vents": def.vents.iter().map(|v| json!({ "at": pt(v.position), "radius": v.radius, "heat": v.heat, "minerals": v.minerals })).collect::<Vec<_>>(),
        "shafts": def.shafts.iter().map(|s| json!({ "min": pt(s.min), "max": pt(s.max) })).collect::<Vec<_>>(),
        "shelters": def.shelters.iter().map(|s| json!({ "at": pt(s.position), "radius": s.radius })).collect::<Vec<_>>(),
        "chamber": { "min": pt(chamber_min), "max": pt(chamber_max), "lit": def.chamber.lit },
        "gate": { "at": pt(def.gate_position), "gene": def.gate_gene, "sprite": def.gate_sprite },
        "start": pt(def.start),
        "key_gene": def.key_gene.map(|k| json!({ "gene": k.gene, "centre": pt(k.centre), "count": k.count, "dim_edge": k.dim_edge })),
        "mobs": mobs,
        "spawns": file.spawns.iter().map(|s| json!({ "mob": s.mob, "at": s.at })).collect::<Vec<_>>(),
        "viruses": def.viruses.iter().map(|v| pt(*v)).collect::<Vec<_>>(),
        "food_zones": def.food_zones.iter().map(|(c, r)| json!({ "at": pt(*c), "radius": r })).collect::<Vec<_>>(),
        "metal_zones": def.metal_zones.iter().map(|(c, r)| json!({ "at": pt(*c), "radius": r })).collect::<Vec<_>>(),
        "notes": file.notes,
        "errors": level.errors,
        "probes": {
            "start": probe(def.start),
            "gate": probe(def.gate_position),
            "key": def.key_gene.map(|k| probe(k.centre)),
        },
    })
}

/// The viewer page with every bake and the sprites they name.
pub fn viewer_html(bakes: &[Value], assets: &Path) -> String {
    let mut names: BTreeSet<String> = BTreeSet::from(["player_cell".to_string()]);
    for bake in bakes {
        if let Some(mobs) = bake["mobs"].as_object() {
            names.extend(mobs.values().filter_map(|m| m["sprite"].as_str().map(str::to_string)));
        }
        if let Some(s) = bake["gate"]["sprite"].as_str() {
            names.insert(s.to_string());
        }
    }
    let sprites: serde_json::Map<String, Value> = names
        .into_iter()
        .filter_map(|name| {
            let bytes = std::fs::read(assets.join("sprites").join(format!("{name}.png"))).ok()?;
            Some((name, json!(format!("data:image/png;base64,{}", base64(&bytes)))))
        })
        .collect();
    let data = json!({ "levels": bakes, "sprites": sprites });
    // Inside a <script> element, "</" must not appear.
    let text = serde_json::to_string(&data).unwrap_or_default().replace("</", "<\\/");
    TEMPLATE.replacen(DATA_MARKER, &text, 1)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }
}
