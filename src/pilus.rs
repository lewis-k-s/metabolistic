//! # Type IV pilus: extend, grab, retract
//!
//! The `Pilus` membrane protein is modelled on the **type IV pilus** of
//! *Myxococcus* and of predators such as *Bdellovibrio*. A long, thin fibre of
//! pilin grows out of the membrane (the ATPase PilB adds subunits), sticks to a
//! surface or to another cell with its tip, and then shrinks again (the ATPase
//! PilT removes subunits). The shrinking fibre pulls with a force of many
//! piconewtons. Cells use it to crawl (twitching motility) and to pull prey
//! close.
//!
//! Rules:
//!
//! - **Press** the slot: the pilus shoots along the aim up to `PILUS_RANGE`
//!   (from the mount point on the rim). It costs `PILUS_EXTEND_ATP`. The nearest
//!   cell or wall tile on the line is the hit (`cast_pilus`). A hit makes a
//!   `Tether`; a miss ends the pilus at once.
//! - **Hold** the slot: the pilus stays attached. The fibre never has slack:
//!   it is never longer than the real distance, and it holds the target at the
//!   wanted length (`Tether::wanted`, at first the length of the hit).
//! - **Reel** (`ReelPilus`, the mouse wheel on the Run screen): each notch moves
//!   the wanted length by `REEL_PIXELS_PER_NOTCH`, in or out, between the
//!   minimum and `max_length` (`reel_wanted`). The fibre retracts to it at
//!   `PILUS_RETRACT_SPEED` and pays out at `PILUS_PAYOUT_SPEED` (`reel_step`).
//!   PilT pays `PILUS_RETRACT_ATP_PER_PX` for each pixel that retracts
//!   (`reel_cost`); paying out is free. The step is capped at the gap, so it
//!   does not depend on the step time.
//! - **Release** the slot, or run out of ATP, or lose the target: the pilus
//!   detaches.
//! - Who moves (`pull_mode`): a target smaller than the owner is pulled to the
//!   owner, so that the owner can engulf it. A target as big or bigger, and a
//!   wall, pulls the owner to it (twitching motility).
//!
//! In the full game, `world::pilus_view` makes an avian2d `DistanceJoint` whose
//! maximum length is the tether length, so both bodies feel the pull by their
//! mass. Without physics (headless tests) `pilus_kinematic_system` moves only
//! the pulled body. The rules in this module are the same in both cases.
//!
//! The rules are pure functions. The systems fetch data and call them.

use bevy::prelude::*;

use crate::actions::{ActionBar, ActionKind, ActionState, SlotHeld, UseSlot};
use crate::cell::{Cell, CellSize};
use crate::engulf::CONTACT_GAP;
use crate::envgrid::EnvGrid;
use crate::metabolism::{CurrencyPools, FlowLedger, FlowSource};
use crate::molecules::Currency;
use crate::movement::world_radius;
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};
use crate::stun::{Candidate, Stunned};
use crate::walls::WallMap;

// --- Tuning. All of these need **playtest**. ---

/// Length of the pilus in pixels, from the mount point on the rim.
pub const PILUS_RANGE: f32 = 140.0;
/// ATP to shoot the pilus (PilB adds subunits).
pub const PILUS_EXTEND_ATP: f32 = 1.0;
/// Speed of the retraction in pixels per second.
pub const PILUS_RETRACT_SPEED: f32 = 70.0;
/// ATP per second of retraction (PilT). At 70 px/s, reeling in a full range
/// (about 2 s) costs about 4 ATP, more than a toxin shot.
pub const PILUS_RETRACT_ATP_PER_SEC: f32 = 2.0;
/// ATP for each pixel of fibre that retracts.
pub const PILUS_RETRACT_ATP_PER_PX: f32 = PILUS_RETRACT_ATP_PER_SEC / PILUS_RETRACT_SPEED;
/// Speed at which the fibre pays out, in pixels per second.
pub const PILUS_PAYOUT_SPEED: f32 = 120.0;
/// Change of the wanted length for one notch of the mouse wheel, in pixels.
pub const REEL_PIXELS_PER_NOTCH: f32 = 20.0;
/// Gap in pixels between the rims at which the reel stops.
pub const PILUS_STOP_GAP: f32 = CONTACT_GAP * 0.5;
/// Distance in pixels between the samples of the wall search along the line.
pub const WALL_SEARCH_STEP: f32 = 4.0;
/// Fraction of the radius where the pilus mounts on the rim.
pub const PILUS_MOUNT_FRACTION: f32 = 0.95;
/// Seconds that the overlay plays its `use` clip (the extension).
pub const PILUS_FLASH_SECONDS: f32 = 0.5;

/// Who moves when the pilus retracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pull {
    /// The target is smaller: it comes to the owner.
    TargetToOwner,
    /// The target is as big or bigger, or it is a wall: the owner goes to it.
    OwnerToTarget,
}

/// The pilus of a cell, attached to something.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tether {
    /// The cell at the tip, or `None` for a wall tile.
    pub target: Option<Entity>,
    /// The world position of the tip. It follows the target centre.
    pub anchor: Vec2,
    /// Distance between the owner centre and the anchor that the fibre allows.
    pub length: f32,
    /// The shortest length: the rims touch.
    pub min_length: f32,
    /// The longest length that the fibre can pay out to.
    pub max_length: f32,
    /// The length that the player reels to (see `reel_wanted`).
    pub wanted: f32,
    pub pull: Pull,
}

/// An order to reel a pilus in (negative `pixels`) or out (positive). The Run
/// screen sends it from the mouse wheel.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ReelPilus {
    pub cell: Entity,
    pub pixels: f32,
}

/// The tether of a cell. `None` while the pilus is in.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct PilusTether(pub Option<Tether>);

/// Marks an app that has physics: the joints do the pulling (see
/// `world::pilus_view`) and `pilus_kinematic_system` does not run.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PilusJointsActive;

/// What a pilus hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PilusHit {
    /// The cell hit, or `None` for a wall.
    pub target: Option<Entity>,
    /// Distance from the mount point along the line.
    pub distance: f32,
}

// --- Pure rules ---

/// The mount point of the pilus: on the rim along the aim.
pub fn pilus_mount(position: Vec2, radius: f32, aim: Vec2) -> Vec2 {
    position + aim.normalize_or(Vec2::X) * radius * PILUS_MOUNT_FRACTION
}

/// Distance along a ray (unit `direction`) to the first point inside a circle,
/// within `range`. A ray that starts inside the circle hits at 0.
pub fn ray_circle_hit(
    origin: Vec2,
    direction: Vec2,
    range: f32,
    centre: Vec2,
    radius: f32,
) -> Option<f32> {
    let to_centre = centre - origin;
    let along = direction.dot(to_centre);
    let closest_sq = to_centre.length_squared() - along * along;
    let radius_sq = radius * radius;
    if closest_sq > radius_sq {
        return None;
    }
    let half_chord = (radius_sq - closest_sq).sqrt();
    let entry = along - half_chord;
    let exit = along + half_chord;
    if exit < 0.0 || entry > range {
        return None;
    }
    Some(entry.max(0.0))
}

/// Distance along a ray to the first sample inside a wall, within `range`.
pub fn ray_wall_hit(
    origin: Vec2,
    direction: Vec2,
    range: f32,
    step: f32,
    is_wall: impl Fn(Vec2) -> bool,
) -> Option<f32> {
    let step = step.max(0.5);
    let mut t = step;
    while t <= range + step * 0.5 {
        let t_clamped = t.min(range);
        if is_wall(origin + direction * t_clamped) {
            return Some(t_clamped);
        }
        t += step;
    }
    None
}

/// The nearest hit of a pilus shot from `origin` along `aim`: the nearest cell
/// of `cells`, or the wall at `wall_distance` if that is nearer.
pub fn cast_pilus(
    origin: Vec2,
    aim: Vec2,
    cells: &[Candidate],
    wall_distance: Option<f32>,
) -> Option<PilusHit> {
    let direction = aim.normalize_or(Vec2::X);
    let cell_hit = cells
        .iter()
        .filter_map(|c| {
            ray_circle_hit(origin, direction, PILUS_RANGE, c.position, c.radius)
                .map(|distance| PilusHit { target: Some(c.entity), distance })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance).then(a.target.cmp(&b.target)));
    match (cell_hit, wall_distance) {
        (Some(cell), Some(wall)) if wall < cell.distance => {
            Some(PilusHit { target: None, distance: wall })
        }
        (Some(cell), _) => Some(cell),
        (None, Some(wall)) => Some(PilusHit { target: None, distance: wall }),
        (None, None) => None,
    }
}

/// Who moves: a target smaller than the owner comes to it. A wall, an equal cell
/// and a bigger cell pull the owner.
pub fn pull_mode(owner_radius: f32, target_radius: Option<f32>) -> Pull {
    match target_radius {
        Some(r) if r < owner_radius => Pull::TargetToOwner,
        _ => Pull::OwnerToTarget,
    }
}

/// The shortest tether length (centre to anchor). For a cell, the rims keep a
/// small gap. For a wall, the owner rim touches the anchor.
pub fn min_length(owner_radius: f32, target_radius: Option<f32>) -> f32 {
    match target_radius {
        Some(r) => owner_radius + r + PILUS_STOP_GAP,
        None => owner_radius + 1.0,
    }
}

/// The tether length after `dt` seconds of retraction. It never goes below
/// `min_length` (the step is capped at the gap), so the result does not depend on
/// how `dt` is split. A length already below the minimum stays.
pub fn retract_step(length: f32, min_length: f32, rate: f32, dt: f32) -> f32 {
    let gap = (length - min_length).max(0.0);
    let step = (rate * dt).max(0.0).min(gap);
    length - step
}

/// The fibre length after `dt` seconds of reeling to `wanted`: it retracts at
/// `PILUS_RETRACT_SPEED` and pays out at `PILUS_PAYOUT_SPEED`. The step is capped
/// at the gap, so the result does not depend on how `dt` is split.
pub fn reel_step(length: f32, wanted: f32, dt: f32) -> f32 {
    if wanted < length {
        retract_step(length, wanted, PILUS_RETRACT_SPEED, dt)
    } else {
        let step = (PILUS_PAYOUT_SPEED * dt).max(0.0).min(wanted - length);
        length + step
    }
}

/// The wanted length after a reel order of `pixels` (negative is in). A reel in
/// starts from the shorter of the wanted and the real fibre length, a reel out
/// from the longer, so that a stale wanted length never eats a notch. The result
/// stays between the minimum and the maximum length.
pub fn reel_wanted(tether: &Tether, pixels: f32) -> f32 {
    let base = if pixels < 0.0 {
        tether.wanted.min(tether.length)
    } else {
        tether.wanted.max(tether.length)
    };
    (base + pixels).clamp(tether.min_length, tether.max_length.max(tether.min_length))
}

/// ATP for retracting `pixels` of fibre.
pub fn reel_cost(pixels: f32) -> f32 {
    PILUS_RETRACT_ATP_PER_PX * pixels.max(0.0)
}

/// The position of a pulled body so that it is at most `length` from the fixed
/// point (the kinematic version of the joint). A body that is nearer stays.
pub fn pull_position(fixed: Vec2, pulled: Vec2, length: f32) -> Vec2 {
    let offset = pulled - fixed;
    let distance = offset.length();
    if distance <= length || distance < 1e-4 {
        pulled
    } else {
        fixed + offset / distance * length
    }
}

/// The tether that a hit makes, from the owner's position and radius and the
/// mount point. `target_radius` and `target_position` are `Some` for a cell.
pub fn make_tether(
    owner: Vec2,
    owner_radius: f32,
    mount: Vec2,
    aim: Vec2,
    hit: PilusHit,
    target: Option<(Vec2, f32)>,
) -> Tether {
    let target_radius = target.map(|(_, r)| r);
    let anchor = match target {
        Some((position, _)) => position,
        None => mount + aim.normalize_or(Vec2::X) * hit.distance,
    };
    let min = min_length(owner_radius, target_radius);
    let length = owner.distance(anchor).max(min);
    let reach = owner_radius * PILUS_MOUNT_FRACTION + PILUS_RANGE + target_radius.unwrap_or(0.0);
    Tether {
        target: hit.target,
        anchor,
        length,
        min_length: min,
        max_length: reach.max(length),
        wanted: length,
        pull: pull_mode(owner_radius, target_radius),
    }
}

// --- Systems ---

/// Shoots the pilus for each `UseSlot` of a pilus slot.
#[allow(clippy::type_complexity)]
pub fn pilus_launch_system(
    mut orders: MessageReader<UseSlot>,
    grid: Option<Res<EnvGrid>>,
    walls: Option<Res<WallMap>>,
    mut cells: Query<
        (
            &ActionBar,
            &Proteome,
            &mut ActionState,
            &mut PilusTether,
            &mut CurrencyPools,
            &mut FlowLedger,
            &Transform,
            &CellSize,
            Has<Stunned>,
        ),
        With<Cell>,
    >,
    others: Query<(Entity, &Transform, &CellSize), With<Cell>>,
) {
    for order in orders.read() {
        let Ok((bar, proteome, mut state, mut tether, mut pools, mut ledger, transform, size, stunned)) =
            cells.get_mut(order.cell)
        else {
            continue;
        };
        if bar.slots.get(order.slot).copied().flatten() != Some(ActionKind::Pilus)
            || proteome.count(ProteinTarget::Membrane(MembraneProtein::Pilus)) == 0
            || tether.0.is_some()
            || stunned
            || pools.get(Currency::ATP) < PILUS_EXTEND_ATP
        {
            continue;
        }
        ledger.apply(&mut pools, FlowSource::Action, Currency::ATP, -PILUS_EXTEND_ATP);
        state.pilus_flash = PILUS_FLASH_SECONDS;
        let position = transform.translation.truncate();
        let radius = world_radius(size);
        let aim = state.aim;
        let mount = pilus_mount(position, radius, aim);
        let candidates: Vec<(Candidate, Vec2)> = others
            .iter()
            .filter(|(entity, ..)| *entity != order.cell)
            .map(|(entity, t, s)| {
                let position = t.translation.truncate();
                (Candidate { entity, position, radius: world_radius(s) }, position)
            })
            .collect();
        let candidate_list: Vec<Candidate> = candidates.iter().map(|(c, _)| *c).collect();
        let wall_distance = match (grid.as_deref(), walls.as_deref()) {
            (Some(grid), Some(walls)) => ray_wall_hit(
                mount,
                aim.normalize_or(Vec2::X),
                PILUS_RANGE,
                WALL_SEARCH_STEP,
                |p| grid.cell_at(p).is_some_and(|tile| walls.get(tile).is_some()),
            ),
            _ => None,
        };
        let Some(hit) = cast_pilus(mount, aim, &candidate_list, wall_distance) else {
            continue;
        };
        let target = hit
            .target
            .and_then(|e| candidate_list.iter().find(|c| c.entity == e))
            .map(|c| (c.position, c.radius));
        tether.0 = Some(make_tether(position, radius, mount, aim, hit, target));
    }
}

/// Applies the reel orders to the wanted length of each tether.
pub fn pilus_reel_input_system(mut orders: MessageReader<ReelPilus>, mut tethers: Query<&mut PilusTether>) {
    for order in orders.read() {
        let Ok(mut tether) = tethers.get_mut(order.cell) else {
            continue;
        };
        if let Some(current) = tether.0.as_mut() {
            current.wanted = reel_wanted(current, order.pixels);
        }
    }
}

/// Reels a held pilus to its wanted length and lets go when the slot is
/// released, the ATP is out, or the target is gone.
#[allow(clippy::type_complexity)]
pub fn pilus_reel_system(
    time: Res<Time>,
    mut owners: Query<
        (
            &ActionBar,
            &SlotHeld,
            &mut ActionState,
            &mut PilusTether,
            &mut CurrencyPools,
            &mut FlowLedger,
            &Transform,
        ),
        With<Cell>,
    >,
    targets: Query<&Transform, With<Cell>>,
) {
    let dt = time.delta_secs();
    for (bar, held, mut state, mut tether, mut pools, mut ledger, transform) in &mut owners {
        let Some(mut current) = tether.0 else {
            if state.pilus_taut || state.pilus_reeling {
                state.pilus_taut = false;
                state.pilus_reeling = false;
            }
            continue;
        };
        let holding = bar
            .slots
            .iter()
            .zip(held.0)
            .any(|(kind, held)| held && *kind == Some(ActionKind::Pilus));
        if let Some(target) = current.target {
            match targets.get(target) {
                Ok(t) => current.anchor = t.translation.truncate(),
                Err(_) => {
                    tether.0 = None;
                    state.let_go();
                    continue;
                }
            }
        }
        if !holding {
            tether.0 = None;
            state.let_go();
            continue;
        }
        // The fibre never has slack: it follows the real distance.
        let actual = transform.translation.truncate().distance(current.anchor);
        let length = current.length.min(actual).max(current.min_length);
        let next = reel_step(length, current.wanted, dt);
        let cost = reel_cost(length - next);
        if pools.get(Currency::ATP) < cost {
            tether.0 = None;
            state.let_go();
            continue;
        }
        if cost > 0.0 {
            ledger.apply(&mut pools, FlowSource::Action, Currency::ATP, -cost);
        }
        current.length = next;
        tether.0 = Some(current);
        state.pilus_taut = true;
        state.pilus_reeling = next < length;
    }
}

/// Moves the pulled body when the app has no physics. The owner stays when it
/// pulls a smaller cell; the target stays when it pulls the owner.
pub fn pilus_kinematic_system(
    owners: Query<(Entity, &PilusTether)>,
    mut bodies: Query<&mut Transform, With<Cell>>,
) {
    for (owner, tether) in &owners {
        let Some(tether) = tether.0 else {
            continue;
        };
        let Ok(owner_position) = bodies.get(owner).map(|t| t.translation.truncate()) else {
            continue;
        };
        let (pulled, fixed) = match tether.pull {
            Pull::TargetToOwner => match tether.target {
                Some(target) => (target, owner_position),
                None => continue,
            },
            Pull::OwnerToTarget => (owner, tether.anchor),
        };
        if let Ok(mut transform) = bodies.get_mut(pulled) {
            let next = pull_position(fixed, transform.translation.truncate(), tether.length);
            transform.translation.x = next.x;
            transform.translation.y = next.y;
        }
    }
}

pub struct PilusPlugin;

impl Plugin for PilusPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ReelPilus>()
            .add_systems(Update, (pilus_launch_system, pilus_reel_input_system).chain())
            .add_systems(
                FixedUpdate,
                (
                    pilus_reel_system,
                    pilus_kinematic_system.run_if(not(resource_exists::<PilusJointsActive>)),
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn e(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    fn cand(n: u32, x: f32, y: f32, r: f32) -> Candidate {
        Candidate { entity: e(n), position: Vec2::new(x, y), radius: r }
    }

    #[test]
    fn a_ray_hits_a_circle_in_range_only() {
        let hit = ray_circle_hit(Vec2::ZERO, Vec2::X, 100.0, Vec2::new(50.0, 0.0), 10.0);
        assert_abs_diff_eq!(hit.unwrap(), 40.0, epsilon = 1e-4);
        // Out of range.
        assert_eq!(ray_circle_hit(Vec2::ZERO, Vec2::X, 30.0, Vec2::new(50.0, 0.0), 10.0), None);
        // Off the line.
        assert_eq!(ray_circle_hit(Vec2::ZERO, Vec2::X, 100.0, Vec2::new(50.0, 15.0), 10.0), None);
        // Behind.
        assert_eq!(ray_circle_hit(Vec2::ZERO, Vec2::X, 100.0, Vec2::new(-50.0, 0.0), 10.0), None);
        // Starts inside: hits at 0.
        assert_eq!(ray_circle_hit(Vec2::ZERO, Vec2::X, 100.0, Vec2::new(3.0, 0.0), 10.0), Some(0.0));
    }

    #[test]
    fn a_grazing_ray_hits_the_edge() {
        let hit = ray_circle_hit(Vec2::ZERO, Vec2::X, 100.0, Vec2::new(50.0, 9.0), 10.0);
        assert!(hit.is_some());
    }

    #[test]
    fn the_wall_search_finds_the_first_wall_sample() {
        let hit = ray_wall_hit(Vec2::ZERO, Vec2::X, 100.0, 4.0, |p| p.x >= 30.0);
        assert_eq!(hit, Some(32.0));
        assert_eq!(ray_wall_hit(Vec2::ZERO, Vec2::X, 20.0, 4.0, |p| p.x >= 30.0), None);
    }

    #[test]
    fn the_nearest_thing_on_the_line_is_the_hit() {
        let cells = [cand(1, 80.0, 0.0, 10.0), cand(2, 40.0, 0.0, 10.0)];
        let hit = cast_pilus(Vec2::ZERO, Vec2::X, &cells, None).unwrap();
        assert_eq!(hit.target, Some(e(2)));
        // A wall in front of both.
        let hit = cast_pilus(Vec2::ZERO, Vec2::X, &cells, Some(20.0)).unwrap();
        assert_eq!(hit.target, None);
        assert_eq!(hit.distance, 20.0);
        // A wall behind the cell.
        let hit = cast_pilus(Vec2::ZERO, Vec2::X, &cells, Some(120.0)).unwrap();
        assert_eq!(hit.target, Some(e(2)));
        assert_eq!(cast_pilus(Vec2::ZERO, Vec2::Y, &cells, None), None);
    }

    #[test]
    fn the_range_limits_the_shot() {
        let far = [cand(1, PILUS_RANGE + 30.0, 0.0, 10.0)];
        assert_eq!(cast_pilus(Vec2::ZERO, Vec2::X, &far, None), None);
        let edge = [cand(1, PILUS_RANGE + 5.0, 0.0, 10.0)];
        assert!(cast_pilus(Vec2::ZERO, Vec2::X, &edge, None).is_some());
    }

    #[test]
    fn a_smaller_cell_comes_to_you_and_a_bigger_cell_or_wall_pulls_you() {
        assert_eq!(pull_mode(20.0, Some(11.0)), Pull::TargetToOwner);
        assert_eq!(pull_mode(20.0, Some(20.0)), Pull::OwnerToTarget);
        assert_eq!(pull_mode(20.0, Some(25.0)), Pull::OwnerToTarget);
        assert_eq!(pull_mode(20.0, None), Pull::OwnerToTarget);
    }

    #[test]
    fn the_retract_step_stops_at_the_minimum_for_any_step_time() {
        let min = 40.0;
        for dt in [0.001, 0.016, 0.5, 3.0, 100.0] {
            let mut length = 120.0;
            let mut steps = 0;
            while length > min && steps < 100_000 {
                let next = retract_step(length, min, PILUS_RETRACT_SPEED, dt);
                assert!(next >= min - 1e-4, "dt {dt}: overshoot to {next}");
                assert!(next <= length);
                length = next;
                steps += 1;
            }
            assert_abs_diff_eq!(length, min, epsilon = 1e-3);
        }
        // Already below the minimum: stays.
        assert_eq!(retract_step(30.0, 40.0, 70.0, 1.0), 30.0);
        // A negative step does nothing.
        assert_eq!(retract_step(100.0, 40.0, 70.0, -1.0), 100.0);
    }

    #[test]
    fn the_total_reel_cost_does_not_depend_on_the_step() {
        let total = |dt: f32| {
            let mut length = 140.0;
            let mut cost = 0.0;
            loop {
                let next = retract_step(length, 40.0, PILUS_RETRACT_SPEED, dt);
                if next == length {
                    return cost;
                }
                cost += reel_cost(length - next);
                length = next;
            }
        };
        assert_abs_diff_eq!(total(0.01), total(1.0), epsilon = 1e-3);
        assert_abs_diff_eq!(total(0.01), reel_cost(100.0), epsilon = 1e-3);
        assert_eq!(reel_cost(-5.0), 0.0);
    }

    #[test]
    fn the_reel_moves_to_the_wanted_length_for_any_step_time() {
        for dt in [0.001, 0.016, 0.5, 3.0] {
            let mut length = 100.0;
            for _ in 0..10_000 {
                length = reel_step(length, 60.0, dt);
                assert!(length >= 60.0 - 1e-4, "dt {dt}: overshoot to {length}");
            }
            assert_abs_diff_eq!(length, 60.0, epsilon = 1e-3);
            for _ in 0..10_000 {
                length = reel_step(length, 90.0, dt);
                assert!(length <= 90.0 + 1e-4, "dt {dt}: overshoot to {length}");
            }
            assert_abs_diff_eq!(length, 90.0, epsilon = 1e-3);
        }
        assert_eq!(reel_step(50.0, 50.0, 1.0), 50.0);
    }

    #[test]
    fn a_reel_order_moves_the_wanted_length_inside_the_limits() {
        let t = Tether {
            target: None,
            anchor: Vec2::ZERO,
            length: 80.0,
            min_length: 30.0,
            max_length: 160.0,
            wanted: 100.0,
            pull: Pull::TargetToOwner,
        };
        // In: from the shorter of wanted and length, so the notch is not lost.
        assert_eq!(reel_wanted(&t, -20.0), 60.0);
        // Out: from the longer.
        assert_eq!(reel_wanted(&t, 20.0), 120.0);
        assert_eq!(reel_wanted(&t, -500.0), 30.0);
        assert_eq!(reel_wanted(&t, 500.0), 160.0);
    }

    #[test]
    fn the_kinematic_pull_keeps_the_body_inside_the_length() {
        let p = pull_position(Vec2::ZERO, Vec2::new(100.0, 0.0), 60.0);
        assert_abs_diff_eq!(p.x, 60.0, epsilon = 1e-4);
        // Nearer bodies stay.
        assert_eq!(pull_position(Vec2::ZERO, Vec2::new(30.0, 0.0), 60.0), Vec2::new(30.0, 0.0));
    }

    #[test]
    fn a_hit_makes_a_tether_with_the_right_lengths() {
        let hit = PilusHit { target: Some(e(1)), distance: 30.0 };
        let t = make_tether(
            Vec2::ZERO,
            20.0,
            Vec2::new(19.0, 0.0),
            Vec2::X,
            hit,
            Some((Vec2::new(60.0, 0.0), 11.25)),
        );
        assert_eq!(t.pull, Pull::TargetToOwner);
        assert_abs_diff_eq!(t.length, 60.0);
        assert_eq!(t.wanted, t.length, "the hit holds its length until the player reels");
        assert_abs_diff_eq!(t.max_length, 19.0 + PILUS_RANGE + 11.25);
        assert_abs_diff_eq!(t.min_length, 20.0 + 11.25 + PILUS_STOP_GAP);
        let wall = make_tether(
            Vec2::ZERO,
            20.0,
            Vec2::new(19.0, 0.0),
            Vec2::X,
            PilusHit { target: None, distance: 40.0 },
            None,
        );
        assert_eq!(wall.pull, Pull::OwnerToTarget);
        assert_abs_diff_eq!(wall.anchor.x, 59.0);
        assert_abs_diff_eq!(wall.length, 59.0);
    }

    #[test]
    fn reeling_costs_real_atp() {
        // A full-range reel costs more ATP than a toxin shot.
        assert!(reel_cost(PILUS_RANGE) + PILUS_EXTEND_ATP > crate::actions::TOXIN_ATP);
    }
}
