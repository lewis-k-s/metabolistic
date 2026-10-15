//! # Level layout from a seed
//!
//! The vent level has fixed parts (a core vent, two light shafts, a gate
//! chamber, two wall lines) and this module places them. `generate_vents`
//! moves the second vent, the shafts, the shelters and the start inside small
//! boxes, picks the pores along each wall line, and scatters the creatures,
//! viruses and particle zones. The same seed gives the same layout.
//!
//! `validate` holds the hard rules, as a pure function of a `LevelDef`:
//!
//! - the start is not in heat or in light, and is clear of walls;
//! - a shelter is within `SHELTER_REACH` of the start;
//! - the pigment prey wait in the dim edge of a light shaft;
//! - the gate chamber has a pore, and the gate cell is inside it;
//! - no shelter, vent, creature, virus or zone centre overlaps a wall, a vent
//!   or the chamber, and all are inside the field;
//! - no predator starts close to the player.
//!
//! A layout that fails a rule is thrown away and a new one is made from the
//! same seed (attempt number + 1), up to `MAX_ATTEMPTS`. The base layout always
//! passes, so `generate_vents` never fails. A test checks that a retry is rare.

use bevy::prelude::*;
use rand::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::level::{self, LevelDef};
use crate::npc::Archetype;
use crate::walls::touching_wall;

/// Layouts to try before the base layout is used.
pub const MAX_ATTEMPTS: u32 = 8;
/// A shelter must be at most this far from the start.
pub const SHELTER_REACH: f32 = 250.0;
/// The start must be this far from the edge of a vent's emitter.
pub const START_VENT_CLEARANCE: f32 = 120.0;
/// The start must be this far from a light shaft.
pub const START_SHAFT_CLEARANCE: f32 = 48.0;
/// A creature or virus must be this far from a wall tile.
pub const WALL_CLEARANCE: f32 = 20.0;
/// A food or metal zone centre must be this far from a wall tile (the zone is
/// a circle, so its edge can still reach a wall).
pub const ZONE_CLEARANCE: f32 = 4.0;
/// A creature or zone centre must be this far from a vent centre.
pub const VENT_CLEARANCE: f32 = 60.0;
/// Predators start at least this far from the player.
pub const PREDATOR_DISTANCE: f32 = 400.0;
/// The pigment prey are this far below the lower end of a shaft: at least
/// `min`, at most `max` (world units).
pub const DIM_EDGE: (f32, f32) = (30.0, 130.0);

/// One broken rule.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutError {
    StartInHeat,
    StartInLight,
    StartAtWall,
    NoShelterNearStart,
    ShelterBlocked(usize),
    PigmentPreyNotInDimEdge,
    ChamberHasNoPore,
    GateOutsideChamber,
    /// A point of the layout is in a wall, a vent, the chamber or outside the field.
    Overlap(&'static str, Vec2),
    PredatorNearStart,
}

/// True if the point is inside the rectangle grown by `margin`.
fn in_rect(point: Vec2, min: Vec2, max: Vec2, margin: f32) -> bool {
    point.cmpge(min - Vec2::splat(margin)).all() && point.cmple(max + Vec2::splat(margin)).all()
}

/// The hard rules of a level. `Ok` if all hold, else every broken rule.
pub fn validate(def: &LevelDef) -> Result<(), Vec<LayoutError>> {
    let mut errors = Vec::new();
    let origin = -Vec2::new(def.width as f32, def.height as f32) * crate::envgrid::CELL_SIZE * 0.5;
    let half = -origin;
    let walls = def.wall_map();
    let chamber_min = origin + Vec2::new(*def.chamber.x.start() as f32, *def.chamber.y.start() as f32) * crate::envgrid::CELL_SIZE;
    let chamber_max = origin + Vec2::new((*def.chamber.x.end() + 1) as f32, (*def.chamber.y.end() + 1) as f32) * crate::envgrid::CELL_SIZE;

    // The start.
    if def.vents.iter().any(|v| def.start.distance(v.position) < v.radius + START_VENT_CLEARANCE) {
        errors.push(LayoutError::StartInHeat);
    }
    if def.shafts.iter().any(|s| in_rect(def.start, s.min, s.max, START_SHAFT_CLEARANCE))
        || in_rect(def.start, chamber_min, chamber_max, START_SHAFT_CLEARANCE)
    {
        errors.push(LayoutError::StartInLight);
    }
    if touching_wall(&walls, origin, def.start, WALL_CLEARANCE) {
        errors.push(LayoutError::StartAtWall);
    }

    // Shelters.
    if !def.shelters.iter().any(|s| s.position.distance(def.start) <= SHELTER_REACH) {
        errors.push(LayoutError::NoShelterNearStart);
    }
    for (index, shelter) in def.shelters.iter().enumerate() {
        let inside = shelter.position.abs().cmple(half - Vec2::splat(shelter.radius)).all();
        let on_vent = def.vents.iter().any(|v| shelter.position.distance(v.position) <= shelter.radius + 40.0);
        let on_wall = touching_wall(&walls, origin, shelter.position, shelter.radius);
        if !inside || on_vent || on_wall {
            errors.push(LayoutError::ShelterBlocked(index));
        }
    }

    // The pigment prey.
    let pigment: Vec<_> = def
        .npcs
        .iter()
        .filter(|n| n.signature == Some(BlockKind::SecondaryMetabolites))
        .collect();
    let in_dim_edge = |p: Vec2| {
        def.shafts.iter().any(|s| {
            let below = s.min.y - p.y;
            p.x >= s.min.x - 80.0 && p.x <= s.max.x + 80.0 && below >= DIM_EDGE.0 && below <= DIM_EDGE.1
        })
    };
    if pigment.len() < crate::npc::PIGMENT_PREY_TARGET || !pigment.iter().all(|n| in_dim_edge(n.position)) {
        errors.push(LayoutError::PigmentPreyNotInDimEdge);
    }

    // The gate chamber.
    if !def.chamber.pores.iter().any(|x| def.chamber.x.contains(x)) {
        errors.push(LayoutError::ChamberHasNoPore);
    }
    if !in_rect(def.gate_position, chamber_min, chamber_max, 0.0) {
        errors.push(LayoutError::GateOutsideChamber);
    }

    // Overlaps.
    let free = |what: &'static str, p: Vec2, clearance: f32, errors: &mut Vec<LayoutError>| {
        let bad = !p.abs().cmplt(half).all()
            || def.vents.iter().any(|v| p.distance(v.position) <= VENT_CLEARANCE)
            || touching_wall(&walls, origin, p, clearance)
            || in_rect(p, chamber_min, chamber_max, 0.0);
        if bad {
            errors.push(LayoutError::Overlap(what, p));
        }
    };
    for vent in &def.vents {
        if !vent.position.abs().cmplt(half).all() || touching_wall(&walls, origin, vent.position, vent.radius) {
            errors.push(LayoutError::Overlap("vent", vent.position));
        }
    }
    for npc in &def.npcs {
        free("creature", npc.position, WALL_CLEARANCE, &mut errors);
        if npc.archetype == Archetype::Predator && npc.position.distance(def.start) < PREDATOR_DISTANCE {
            errors.push(LayoutError::PredatorNearStart);
        }
    }
    for p in &def.viruses {
        free("virus", *p, WALL_CLEARANCE, &mut errors);
    }
    for (p, _) in &def.food_zones {
        free("food zone", *p, ZONE_CLEARANCE, &mut errors);
    }
    for (p, _) in &def.metal_zones {
        free("metal zone", *p, ZONE_CLEARANCE, &mut errors);
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// Picks `count` distinct values of `range`, at least `gap` apart, in order.
fn pick_pores(rng: &mut StdRng, range: std::ops::RangeInclusive<usize>, count: usize, gap: usize) -> Vec<usize> {
    let mut picked: Vec<usize> = Vec::new();
    let mut tries = 0;
    while picked.len() < count && tries < 100 {
        tries += 1;
        let value = rng.random_range(range.clone());
        if picked.iter().all(|p| p.abs_diff(value) >= gap) {
            picked.push(value);
        }
    }
    picked.sort_unstable();
    picked
}

/// Moves a point out of the clearance round the vents (radially).
fn push_from_vents(point: Vec2, vents: &[level::Vent]) -> Vec2 {
    let mut point = point;
    for vent in vents {
        let away = point - vent.position;
        if away.length() < VENT_CLEARANCE + 15.0 {
            point = vent.position + away.normalize_or(Vec2::X) * (VENT_CLEARANCE + 15.0);
        }
    }
    point
}

fn jitter(rng: &mut StdRng, amount: f32) -> Vec2 {
    Vec2::new(rng.random_range(-amount..=amount), rng.random_range(-amount..=amount))
}

/// One layout, not checked.
fn propose(seed: u64, attempt: u32) -> LevelDef {
    let mixed = seed ^ u64::from(attempt).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut rng = StdRng::seed_from_u64(mixed);
    let rng = &mut rng;
    let mut def = LevelDef::vents_base();
    def.seed = seed;

    // The second vent moves; what belongs to it moves with it.
    let vent_shift = Vec2::new(rng.random_range(-30.0..=30.0), rng.random_range(-50.0..=50.0));
    def.vents[1].position += vent_shift;
    def.start += jitter(rng, 12.0);

    // Shafts: along x, and a little along y for shaft A.
    let a_shift = Vec2::new(rng.random_range(-25.0..=25.0), rng.random_range(-30.0..=30.0));
    def.shafts[0].min += a_shift;
    def.shafts[0].max += a_shift;
    let b_shift = Vec2::new(rng.random_range(-30.0..=30.0), 0.0);
    def.shafts[1].min += b_shift;
    def.shafts[1].max += b_shift;

    // Shelters: one near the start, one near the second vent.
    def.shelters[0].position += jitter(rng, 22.0);
    def.shelters[1].position = def.vents[1].position + (level::SHELTERS[1].position - level::VENTS[1].position) + jitter(rng, 12.0);

    // Pores.
    def.lines[0].pores = pick_pores(rng, 31..=46, 2, 3);
    def.lines[1].pores = pick_pores(rng, 31..=47, 2, 3);
    def.chamber.pores = pick_pores(rng, def.chamber.x.clone(), 2, 2);

    // The pigment prey follow shaft A.
    let shaft_a = def.shafts[0];
    let centre = Vec2::new((shaft_a.min.x + shaft_a.max.x) * 0.5, shaft_a.min.y - 38.0) + jitter(rng, 10.0);
    def.pigment_prey = centre;
    for npc in &mut def.npcs {
        if npc.signature == Some(BlockKind::SecondaryMetabolites) {
            npc.position = centre + (npc.position - level::PIGMENT_PREY_POSITION) + jitter(rng, 8.0);
        } else {
            npc.position = push_from_vents(npc.position + jitter(rng, 35.0), &def.vents);
        }
    }
    for p in &mut def.viruses {
        *p += jitter(rng, 60.0);
    }
    for (index, (p, _)) in def.food_zones.iter_mut().enumerate() {
        // Along x more than along y: the zone south of the core lies close to a wall.
        *p += Vec2::new(rng.random_range(-18.0..=18.0), rng.random_range(-8.0..=8.0));
        if index == 1 {
            *p += vent_shift;
        }
    }
    for (index, (p, _)) in def.metal_zones.iter_mut().enumerate() {
        *p += jitter(rng, 15.0);
        if index >= 2 {
            *p += vent_shift;
        }
    }
    def
}

/// The vent level for a seed, and the number of attempts that it took (1 when
/// the first layout was good).
pub fn generate_vents_counted(seed: u64) -> (LevelDef, u32) {
    for attempt in 0..MAX_ATTEMPTS {
        let def = propose(seed, attempt);
        if validate(&def).is_ok() {
            return (def, attempt + 1);
        }
    }
    let mut base = LevelDef::vents_base();
    base.seed = seed;
    (base, MAX_ATTEMPTS + 1)
}

/// The vent level for a seed. The same seed gives the same level.
pub fn generate_vents(seed: u64) -> LevelDef {
    generate_vents_counted(seed).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_layout_is_valid() {
        assert_eq!(validate(&LevelDef::vents_base()), Ok(()));
    }

    #[test]
    fn two_hundred_seeds_give_valid_layouts_and_a_retry_is_rare() {
        let mut retries = 0;
        let mut fallbacks = 0;
        for seed in 0..200u64 {
            let (def, attempts) = generate_vents_counted(seed);
            assert_eq!(validate(&def), Ok(()), "seed {seed}");
            assert_eq!(def.seed, seed);
            retries += u32::from(attempts > 1);
            fallbacks += u32::from(attempts > MAX_ATTEMPTS);
        }
        assert_eq!(fallbacks, 0, "the base layout is only a safety net");
        assert!(retries <= 10, "{retries} of 200 seeds needed a retry");
    }

    /// Counts the broken rules over many proposals. Run with
    /// `cargo th layout::tests::print_failures -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_failures() {
        let mut counts = std::collections::BTreeMap::new();
        for seed in 0..200u64 {
            if let Err(errors) = validate(&propose(seed, 0)) {
                for e in errors {
                    let key = match e {
                        LayoutError::Overlap(what, p) => format!("Overlap({what} {})", (p / 50.0).round() * 50.0),
                        other => format!("{other:?}"),
                    };
                    *counts.entry(key).or_insert(0) += 1;
                }
            }
        }
        eprintln!("{counts:#?}");
    }

    #[test]
    fn the_same_seed_gives_the_same_layout_and_other_seeds_differ() {
        assert_eq!(generate_vents(7), generate_vents(7));
        assert_ne!(generate_vents(7), generate_vents(8));
        assert_ne!(generate_vents(7).wall_tiles(), generate_vents(8).wall_tiles());
    }

    #[test]
    fn the_validator_catches_each_rule() {
        let base = LevelDef::vents_base();
        let mut def = base.clone();
        def.start = def.vents[0].position + Vec2::new(60.0, 0.0);
        assert!(validate(&def).unwrap_err().contains(&LayoutError::StartInHeat));

        let mut def = base.clone();
        def.start = (def.shafts[0].min + def.shafts[0].max) * 0.5;
        assert!(validate(&def).unwrap_err().contains(&LayoutError::StartInLight));

        let mut def = base.clone();
        def.shelters[0].position = Vec2::new(-700.0, 500.0);
        assert!(validate(&def).unwrap_err().contains(&LayoutError::NoShelterNearStart));

        let mut def = base.clone();
        def.npcs[0].position = Vec2::new(320.0, 600.0);
        assert!(validate(&def).unwrap_err().contains(&LayoutError::PigmentPreyNotInDimEdge));

        let mut def = base.clone();
        def.chamber.pores.clear();
        assert!(validate(&def).unwrap_err().contains(&LayoutError::ChamberHasNoPore));

        // A creature on a wall tile.
        let mut def = base.clone();
        let ((x, y), _) = def.wall_tiles()[0];
        let grid = level::vent_field();
        def.npcs[3].position = grid.cell_center(x, y);
        assert!(validate(&def).unwrap_err().iter().any(|e| matches!(e, LayoutError::Overlap(..))));

        let mut def = base.clone();
        def.npcs[8].position = def.start + Vec2::new(100.0, 0.0);
        assert!(validate(&def).unwrap_err().contains(&LayoutError::PredatorNearStart));
    }

    #[test]
    fn generated_levels_build_a_grid_with_the_right_walls() {
        let def = generate_vents(3);
        let grid = def.cold_grid();
        assert_eq!(grid.wall_count(), def.wall_map().len());
        assert!(def.wall_tiles().iter().filter(|(_, k)| *k == crate::walls::WallKind::Pore).count() >= 5);
    }
}
