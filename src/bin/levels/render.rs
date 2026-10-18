//! PNG and text views of a level. Agents read these; people use the viewer.

use std::fmt::Write as _;
use std::path::Path;

use bevy::prelude::Vec2;
use metabolistic3d::environment::Channel;
use metabolistic3d::envgrid::CELL_SIZE;
use metabolistic3d::exchange::HEAT_DAMAGE_THRESHOLD;
use metabolistic3d::npc::Archetype;
use metabolistic3d::walls::WallKind;

use crate::Loaded;

/// The level at which a channel shows at full strength in the views. Heat is
/// twice the damage threshold.
pub fn reference(channel: Channel) -> f32 {
    match channel {
        Channel::Heat => HEAT_DAMAGE_THRESHOLD * 2.0,
        Channel::Minerals => 20.0,
        Channel::Light => 10.0,
        Channel::O2 => 5.0,
        Channel::Water => 1.0,
    }
}

/// The colour of a wall kind, 0..1 RGB.
pub fn wall_colour(kind: WallKind) -> [f32; 3] {
    match kind {
        WallKind::Tough => [0.42, 0.38, 0.36],
        WallKind::Wall => [0.62, 0.32, 0.45],
        WallKind::Pore => [0.95, 0.78, 0.45],
        WallKind::Mat => [0.25, 0.48, 0.24],
        WallKind::Waxy => [0.78, 0.74, 0.42],
    }
}

/// The colour of a creature dot.
fn mob_colour(archetype: Archetype, key: bool) -> [f32; 3] {
    if key {
        return [1.0, 0.9, 0.2];
    }
    match archetype {
        Archetype::Prey => [0.45, 0.9, 0.45],
        Archetype::Predator => [1.0, 0.3, 0.3],
        Archetype::Symbiont => [0.4, 0.7, 1.0],
        Archetype::Gate => [0.95, 0.4, 0.95],
    }
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<[f32; 3]>,
}

impl Canvas {
    fn new(width: usize, height: usize, fill: [f32; 3]) -> Self {
        Self { width, height, pixels: vec![fill; width * height] }
    }

    fn blend(&mut self, x: i64, y: i64, colour: [f32; 3], alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let p = &mut self.pixels[y as usize * self.width + x as usize];
        for i in 0..3 {
            p[i] = p[i] * (1.0 - alpha) + colour[i] * alpha;
        }
    }

    fn add(&mut self, x: usize, y: usize, colour: [f32; 3], amount: f32) {
        let p = &mut self.pixels[y * self.width + x];
        for i in 0..3 {
            p[i] += colour[i] * amount;
        }
    }

    fn disc(&mut self, centre: (f32, f32), radius: f32, colour: [f32; 3], alpha: f32) {
        let r = radius.ceil() as i64 + 1;
        let (cx, cy) = centre;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx as i64 + dx, cy as i64 + dy);
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                if d <= radius {
                    self.blend(x, y, colour, alpha);
                }
            }
        }
    }

    fn ring(&mut self, centre: (f32, f32), radius: f32, colour: [f32; 3], alpha: f32, dashed: bool) {
        let steps = (radius * 8.0).max(16.0) as usize;
        for i in 0..steps {
            if dashed && (i / 4) % 2 == 1 {
                continue;
            }
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            self.blend((centre.0 + a.cos() * radius) as i64, (centre.1 + a.sin() * radius) as i64, colour, alpha);
        }
    }

    fn line(&mut self, from: (f32, f32), to: (f32, f32), colour: [f32; 3], alpha: f32) {
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            self.blend(
                (from.0 + (to.0 - from.0) * t) as i64,
                (from.1 + (to.1 - from.1) * t) as i64,
                colour,
                alpha,
            );
        }
    }

    fn rgb_bytes(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
    }
}

/// Writes a PNG of the level: channels, walls, vents, shelters, creatures,
/// start and gate. `scale` is pixels per grid cell.
pub fn write_png(level: &Loaded, path: &Path, scale: usize) -> Result<(), String> {
    let (def, grid) = (&level.def, &level.grid);
    let s = scale as f32;
    let (w, h) = (def.width * scale, def.height * scale);
    let mut canvas = Canvas::new(w, h, def.field.background());
    let origin = grid.origin();
    let to_px = |p: Vec2| ((p.x - origin.x) / CELL_SIZE * s, h as f32 - (p.y - origin.y) / CELL_SIZE * s);

    let uv = def.uv_fraction > 0.5;
    let layers = [
        (Channel::Heat, [1.0, 0.36, 0.12]),
        (Channel::Minerals, [0.2, 0.85, 0.65]),
        (Channel::Light, if uv { [0.7, 0.45, 1.0] } else { [1.0, 0.94, 0.55] }),
        (Channel::O2, [0.55, 0.75, 1.0]),
        (Channel::Water, [0.25, 0.45, 0.9]),
    ];
    for py in 0..h {
        for px in 0..w {
            let world = Vec2::new(origin.x + (px as f32 + 0.5) / s * CELL_SIZE, origin.y + (h - py) as f32 / s * CELL_SIZE);
            for (channel, colour) in layers {
                let v = grid.sample(channel, world);
                if v > 0.01 {
                    canvas.add(px, py, colour, (v / reference(channel)).clamp(0.0, 1.0).sqrt() * 0.75);
                }
            }
        }
    }
    // The heat-damage line: cell edges where heat crosses the threshold.
    let hot = |x: usize, y: usize| grid.level(Channel::Heat, x, y) >= HEAT_DAMAGE_THRESHOLD;
    for y in 0..def.height {
        for x in 0..def.width {
            if !hot(x, y) {
                continue;
            }
            let (x0, y0) = (x * scale, (def.height - 1 - y) * scale);
            for i in 0..scale {
                if x == 0 || !hot(x - 1, y) {
                    canvas.blend(x0 as i64, (y0 + i) as i64, [1.0, 0.2, 0.1], 0.9);
                }
                if x + 1 == def.width || !hot(x + 1, y) {
                    canvas.blend((x0 + scale - 1) as i64, (y0 + i) as i64, [1.0, 0.2, 0.1], 0.9);
                }
                if y + 1 == def.height || !hot(x, y + 1) {
                    canvas.blend((x0 + i) as i64, y0 as i64, [1.0, 0.2, 0.1], 0.9);
                }
                if y == 0 || !hot(x, y - 1) {
                    canvas.blend((x0 + i) as i64, (y0 + scale - 1) as i64, [1.0, 0.2, 0.1], 0.9);
                }
            }
        }
    }
    for ((x, y), kind) in def.wall_tiles() {
        let alpha = if kind == WallKind::Mat { 0.6 } else { 1.0 };
        for dy in 0..scale {
            for dx in 0..scale {
                let edge = dx == 0 || dy == 0;
                let colour = wall_colour(kind).map(|c| if edge { c * 0.7 } else { c });
                canvas.blend((x * scale + dx) as i64, ((def.height - 1 - y) * scale + dy) as i64, colour, alpha);
            }
        }
    }
    let per_px = s / CELL_SIZE;
    for (centre, radius) in &def.food_zones {
        canvas.ring(to_px(*centre), radius * per_px, [0.9, 0.7, 0.4], 0.5, true);
    }
    for (centre, radius) in &def.metal_zones {
        canvas.ring(to_px(*centre), radius * per_px, [0.75, 0.75, 0.85], 0.5, true);
    }
    for vent in &def.vents {
        canvas.disc(to_px(vent.position), vent.radius * per_px, [1.0, 0.85, 0.6], 0.8);
        canvas.ring(to_px(vent.position), vent.radius * per_px + 1.0, [0.1, 0.05, 0.0], 0.9, false);
    }
    for shelter in &def.shelters {
        canvas.ring(to_px(shelter.position), shelter.radius * per_px, [0.3, 1.0, 0.95], 1.0, false);
        canvas.ring(to_px(shelter.position), shelter.radius * per_px - 1.0, [0.3, 1.0, 0.95], 0.6, false);
    }
    let key = def.key_gene.map(|k| k.gene);
    for npc in &def.npcs {
        let dot = (scale as f32 * 0.45).max(2.5);
        let is_key = npc.archetype == Archetype::Prey && npc.signature.is_some() && npc.signature == key;
        canvas.disc(to_px(npc.position), dot + 1.0, [0.0, 0.0, 0.0], 0.8);
        canvas.disc(to_px(npc.position), dot, mob_colour(npc.archetype, is_key), 1.0);
    }
    for virus in &def.viruses {
        let (x, y) = to_px(*virus);
        let r = (scale as f32 * 0.5).max(3.0);
        canvas.line((x - r, y - r), (x + r, y + r), [0.85, 0.4, 1.0], 1.0);
        canvas.line((x - r, y + r), (x + r, y - r), [0.85, 0.4, 1.0], 1.0);
    }
    let gate = to_px(def.gate_position);
    canvas.disc(gate, s * 1.1, [0.0, 0.0, 0.0], 0.8);
    canvas.disc(gate, s, mob_colour(Archetype::Gate, false), 1.0);
    let start = to_px(def.start);
    canvas.ring(start, s * 0.9, [1.0, 1.0, 1.0], 1.0, false);
    canvas.ring(start, s * 0.9 + 1.0, [1.0, 1.0, 1.0], 1.0, false);
    canvas.disc(start, 2.0, [1.0, 1.0, 1.0], 1.0);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&canvas.rgb_bytes()).map_err(|e| e.to_string())
}

/// The map as text (one character a grid cell), or one channel as digits
/// 0-9 (`9` is the reference level or more). Row 0 is the top.
pub fn ascii(level: &Loaded, channel: Option<Channel>) -> String {
    let (def, grid) = (&level.def, &level.grid);
    let mut rows = vec![vec![' '; def.width]; def.height];
    for (y, row) in rows.iter_mut().enumerate() {
        let gy = def.height - 1 - y;
        for (x, c) in row.iter_mut().enumerate() {
            *c = match channel {
                Some(channel) => {
                    let v = grid.level(channel, x, gy) / reference(channel);
                    if v < 0.02 { '.' } else { char::from_digit(((v.sqrt() * 9.0).round() as u32).min(9), 10).unwrap() }
                }
                None => {
                    let heat = grid.level(Channel::Heat, x, gy);
                    let light = grid.level(Channel::Light, x, gy);
                    let minerals = grid.level(Channel::Minerals, x, gy);
                    if heat >= HEAT_DAMAGE_THRESHOLD {
                        '^'
                    } else if light >= 1.0 {
                        '|'
                    } else if minerals >= 3.0 {
                        ':'
                    } else {
                        '.'
                    }
                }
            };
        }
    }
    let mut put = |p: Vec2, c: char| {
        if let Some((x, y)) = metabolistic3d::walls::tile_at(grid.origin(), p)
            && x < def.width
            && y < def.height
        {
            rows[def.height - 1 - y][x] = c;
        }
    };
    if channel.is_none() {
        for ((x, y), kind) in def.wall_tiles() {
            put(grid.cell_center(x, y), metabolistic3d::level_file::tile_char(kind));
        }
        for shelter in &def.shelters {
            put(shelter.position, 'S');
        }
        for vent in &def.vents {
            put(vent.position, 'V');
        }
        let key = def.key_gene.map(|k| k.gene);
        for npc in &def.npcs {
            put(
                npc.position,
                match npc.archetype {
                    Archetype::Prey if npc.signature.is_some() && npc.signature == key => 'k',
                    Archetype::Prey => 'p',
                    Archetype::Predator => 'P',
                    Archetype::Symbiont => 's',
                    Archetype::Gate => 'G',
                },
            );
        }
        for virus in &def.viruses {
            put(*virus, 'v');
        }
        put(def.gate_position, 'G');
        put(def.start, '@');
    }
    let mut out = String::new();
    let label = channel.map(|c| format!("{c:?} (0-9, 9 = {} or more)", reference(c))).unwrap_or_else(|| {
        "map: ^ heat damage, | light, : minerals, # rock, W wall, o pore, ~ mat, V vent, S shelter, @ start, G gate, p prey, k key prey, P predator, s symbiont, v virus".into()
    });
    let _ = writeln!(out, "{} ({}), {} x {} cells, origin {:?}", level.id, level.def.name, def.width, def.height, grid.origin());
    let _ = writeln!(out, "{label}");
    for row in rows {
        out.extend(row);
        out.push('\n');
    }
    if !level.errors.is_empty() {
        let _ = writeln!(out, "broken rules: {}", level.errors.join(", "));
    }
    out
}

/// The channels, the tile and the nearest things at a world point.
pub fn probe(level: &Loaded, point: Vec2) -> String {
    let (def, grid) = (&level.def, &level.grid);
    let mut out = String::new();
    let tile = metabolistic3d::walls::tile_at(grid.origin(), point);
    let _ = writeln!(out, "{} at {point:?}, tile {tile:?}", level.id);
    for channel in Channel::ALL {
        let _ = writeln!(out, "  {:9} {:8.2}", format!("{channel:?}"), grid.sample(channel, point));
    }
    let mut things: Vec<(f32, String)> = Vec::new();
    for v in &def.vents {
        things.push((v.position.distance(point), format!("vent at {:?}", v.position)));
    }
    for s in &def.shelters {
        things.push((s.position.distance(point), format!("shelter at {:?}", s.position)));
    }
    for (spawn, npc) in level.file.spawns.iter().zip(&def.npcs) {
        things.push((npc.position.distance(point), format!("{} at {:?}", spawn.mob, npc.position)));
    }
    things.push((def.gate_position.distance(point), "gate".into()));
    things.push((def.start.distance(point), "start".into()));
    things.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, what) in things.iter().take(6) {
        let _ = writeln!(out, "  {d:7.0} px  {what}");
    }
    out
}
