//! # Contractile injection and stun
//!
//! The `Injector` is a membrane protein modelled on the **type VI secretion
//! system** (T6SS), a contact-dependent spear. A sheath contracts and pushes a
//! tube with toxic effectors through the membrane of a cell that touches the
//! injector. Many bacteria use it against rivals and prey. Here, the effector
//! paralyses the target for a short time.
//!
//! Rules:
//!
//! - The target must be in contact, in a cone round the aim (`in_cone`). The
//!   injector does not reach far (`INJECTOR_REACH_GAP`).
//! - A use costs `INJECTOR_ATP` and `INJECTOR_METALS` (the spear is built with
//!   a metal cofactor) and starts a cooldown. The use is paid only when a target
//!   is in the cone, so a miss in the open water costs nothing.
//! - The target gets `Stunned` for `STUN_SECONDS`. A stunned cell has no thrust,
//!   its NPC AI is paused, it cannot engulf, and its sprite has a tint. A new
//!   stun does not shorten the old one (`stun_after`).
//!
//! Stunned prey cannot flee, so the hunter can engulf it (see `engulf`).
//!
//! The rules are pure functions. `injector_system` fetches the data and calls
//! them.

use bevy::prelude::*;

use crate::actions::{ActionBar, ActionKind, ActionState, UseSlot};
use crate::cell::{Cell, CellSize};
use crate::metabolism::{CurrencyPools, FlowLedger, FlowSource};
use crate::molecules::Currency;
use crate::movement::world_radius;
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};

// --- Tuning. All of these need **playtest**. ---

/// Gap in pixels between the rims at which the spear still reaches.
pub const INJECTOR_REACH_GAP: f32 = 10.0;
/// Half angle of the cone round the aim, in degrees.
pub const INJECTOR_CONE_DEGREES: f32 = 35.0;
/// ATP for one use.
pub const INJECTOR_ATP: f32 = 4.0;
/// Metals (a cofactor) for one use.
pub const INJECTOR_METALS: f32 = 1.0;
/// Seconds between two uses.
pub const INJECTOR_COOLDOWN: f32 = 6.0;
/// Seconds that the overlay plays its `use` clip.
pub const INJECTOR_FLASH_SECONDS: f32 = 0.6;
/// Seconds of stun.
pub const STUN_SECONDS: f32 = 4.0;

/// A cell that cannot move or act for a short time.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Stunned {
    /// Seconds left.
    pub left: f32,
}

// --- Pure rules ---

/// Seconds of stun after a new stun of `duration`. A stun never shortens the
/// old one.
pub fn stun_after(current: Option<f32>, duration: f32) -> f32 {
    current.unwrap_or(0.0).max(duration)
}

/// Seconds of stun left after `dt`. `None` when the stun is over. The result does
/// not depend on how `dt` is split.
pub fn stun_tick(left: f32, dt: f32) -> Option<f32> {
    let left = left - dt.max(0.0);
    (left > 0.0).then_some(left)
}

/// The thrust of a body: zero while stunned.
pub fn stunned_thrust(thrust: Vec2, stunned: bool) -> Vec2 {
    if stunned { Vec2::ZERO } else { thrust }
}

/// Does a round target at `target` (radius `target_radius`) lie in the cone of
/// the injector? The injector sits on a cell at `origin` with radius
/// `owner_radius` and points along `aim` (a unit vector). The rim gap must be at
/// most `INJECTOR_REACH_GAP`. The direction to the target centre must be inside
/// the half angle, plus the angle that the target itself covers (so a big target
/// that touches the cone edge is hit).
pub fn in_cone(
    origin: Vec2,
    owner_radius: f32,
    aim: Vec2,
    target: Vec2,
    target_radius: f32,
) -> bool {
    let offset = target - origin;
    let distance = offset.length();
    if distance - owner_radius - target_radius > INJECTOR_REACH_GAP {
        return false;
    }
    if distance <= owner_radius {
        // The centres are almost on top of each other: any direction hits.
        return true;
    }
    let aim = aim.normalize_or(Vec2::X);
    let angle = aim.angle_to(offset / distance).abs();
    let covered = (target_radius / distance).clamp(0.0, 1.0).asin();
    angle <= INJECTOR_CONE_DEGREES.to_radians() + covered
}

/// A candidate target of the injector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub entity: Entity,
    pub position: Vec2,
    pub radius: f32,
}

/// The nearest candidate in the cone, if any.
pub fn pick_stun_target(
    origin: Vec2,
    owner_radius: f32,
    aim: Vec2,
    candidates: &[Candidate],
) -> Option<Entity> {
    candidates
        .iter()
        .filter(|c| in_cone(origin, owner_radius, aim, c.position, c.radius))
        .min_by(|a, b| {
            a.position
                .distance_squared(origin)
                .total_cmp(&b.position.distance_squared(origin))
                .then(a.entity.cmp(&b.entity))
        })
        .map(|c| c.entity)
}

/// Why the injector did not fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectRefused {
    Cooldown,
    NotEnoughAtp,
    NotEnoughMetals,
}

/// Can the injector fire with these pools and this cooldown?
pub fn check_injection(atp: f32, metals: f32, cooldown: f32) -> Result<(), InjectRefused> {
    if cooldown > 0.0 {
        Err(InjectRefused::Cooldown)
    } else if atp < INJECTOR_ATP {
        Err(InjectRefused::NotEnoughAtp)
    } else if metals < INJECTOR_METALS {
        Err(InjectRefused::NotEnoughMetals)
    } else {
        Ok(())
    }
}

/// The strength of the stun tint, 0 to 1: full while the stun lasts, and fading
/// in its last half second. The view turns it into a colour.
pub fn stun_tint(left: f32) -> f32 {
    (left / 0.5).clamp(0.0, 1.0)
}

// --- Systems ---

/// Fires the injector for each `UseSlot` of an injector slot.
#[allow(clippy::type_complexity)]
pub fn injector_system(
    mut orders: MessageReader<UseSlot>,
    mut cells: Query<
        (
            &ActionBar,
            &Proteome,
            &mut ActionState,
            &mut CurrencyPools,
            &mut FlowLedger,
            &Transform,
            &CellSize,
            Has<Stunned>,
        ),
        With<Cell>,
    >,
    others: Query<(Entity, &Transform, &CellSize), With<Cell>>,
    stunned: Query<&Stunned>,
    mut commands: Commands,
) {
    for order in orders.read() {
        let Ok((bar, proteome, mut state, mut pools, mut ledger, transform, size, owner_stunned)) =
            cells.get_mut(order.cell)
        else {
            continue;
        };
        if bar.slots.get(order.slot).copied().flatten() != Some(ActionKind::Injector)
            || proteome.count(ProteinTarget::Membrane(MembraneProtein::Injector)) == 0
            || owner_stunned
        {
            continue;
        }
        if let Err(why) = check_injection(
            pools.get(Currency::ATP),
            pools.get(Currency::Metals),
            state.injector_cooldown,
        ) {
            debug!("Injector refused: {why:?}");
            continue;
        }
        let origin = transform.translation.truncate();
        let radius = world_radius(size);
        let candidates: Vec<Candidate> = others
            .iter()
            .filter(|(entity, ..)| *entity != order.cell)
            .map(|(entity, t, s)| Candidate {
                entity,
                position: t.translation.truncate(),
                radius: world_radius(s),
            })
            .collect();
        let Some(target) = pick_stun_target(origin, radius, state.aim, &candidates) else {
            continue;
        };
        ledger.apply(&mut pools, FlowSource::Action, Currency::ATP, -INJECTOR_ATP);
        ledger.apply(&mut pools, FlowSource::Action, Currency::Metals, -INJECTOR_METALS);
        state.injector_cooldown = INJECTOR_COOLDOWN;
        state.injector_flash = INJECTOR_FLASH_SECONDS;
        let left = stun_after(stunned.get(target).ok().map(|s| s.left), STUN_SECONDS);
        commands.entity(target).insert(Stunned { left });
    }
}

/// Counts the stun down and removes it at the end.
pub fn stun_timer_system(
    time: Res<Time>,
    mut cells: Query<(Entity, &mut Stunned)>,
    mut commands: Commands,
) {
    for (entity, mut stun) in &mut cells {
        match stun_tick(stun.left, time.delta_secs()) {
            Some(left) => stun.left = left,
            None => {
                commands.entity(entity).remove::<Stunned>();
            }
        }
    }
}

pub struct StunPlugin;

impl Plugin for StunPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (injector_system, stun_timer_system).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    #[test]
    fn the_cone_needs_contact_and_the_right_direction() {
        let o = Vec2::ZERO;
        // Rims 5 px apart, straight ahead.
        assert!(in_cone(o, 20.0, Vec2::X, Vec2::new(35.0, 0.0), 10.0));
        // Rims 20 px apart: too far.
        assert!(!in_cone(o, 20.0, Vec2::X, Vec2::new(50.0, 0.0), 10.0));
        // Close but behind.
        assert!(!in_cone(o, 20.0, Vec2::X, Vec2::new(-35.0, 0.0), 10.0));
        // Close, 90 degrees off the aim.
        assert!(!in_cone(o, 20.0, Vec2::X, Vec2::new(0.0, 35.0), 10.0));
    }

    #[test]
    fn a_big_target_at_the_cone_edge_is_hit() {
        // Both centres are 45 degrees off the aim: outside the 35 degree cone.
        // A small target (radius 3, covers 7.6 degrees) is missed.
        assert!(!in_cone(Vec2::ZERO, 10.0, Vec2::X, Vec2::new(16.0, 16.0), 3.0));
        // A big one (radius 20, covers 30 degrees) is hit.
        assert!(in_cone(Vec2::ZERO, 10.0, Vec2::X, Vec2::new(28.0, 28.0), 20.0));
    }

    #[test]
    fn overlapping_bodies_are_always_in_the_cone() {
        assert!(in_cone(Vec2::ZERO, 20.0, Vec2::X, Vec2::new(0.0, 5.0), 10.0));
    }

    #[test]
    fn the_nearest_target_in_the_cone_is_picked() {
        let near = Candidate { entity: e(1), position: Vec2::new(36.0, 3.0), radius: 10.0 };
        let far = Candidate { entity: e(2), position: Vec2::new(38.0, 0.0), radius: 10.0 };
        let behind = Candidate { entity: e(3), position: Vec2::new(-30.0, 0.0), radius: 10.0 };
        assert_eq!(pick_stun_target(Vec2::ZERO, 20.0, Vec2::X, &[far, behind, near]), Some(e(1)));
        assert_eq!(pick_stun_target(Vec2::ZERO, 20.0, Vec2::X, &[behind]), None);
    }

    #[test]
    fn the_stun_timer_does_not_depend_on_the_step() {
        for dt in [0.01, 0.1, 1.0, 10.0] {
            let mut left = STUN_SECONDS;
            let mut t = 0.0;
            while let Some(next) = stun_tick(left, dt) {
                left = next;
                t += dt;
            }
            assert!(t + dt >= STUN_SECONDS - 1e-3 && t < STUN_SECONDS + 1e-3, "dt {dt}: {t}");
        }
    }

    #[test]
    fn a_new_stun_never_shortens_the_old_one() {
        assert_eq!(stun_after(None, 4.0), 4.0);
        assert_eq!(stun_after(Some(1.0), 4.0), 4.0);
        assert_eq!(stun_after(Some(9.0), 4.0), 9.0);
    }

    #[test]
    fn a_stunned_body_has_no_thrust() {
        assert_eq!(stunned_thrust(Vec2::new(5.0, 1.0), true), Vec2::ZERO);
        assert_eq!(stunned_thrust(Vec2::new(5.0, 1.0), false), Vec2::new(5.0, 1.0));
    }

    #[test]
    fn the_injector_checks_cooldown_atp_and_metals() {
        assert_eq!(check_injection(10.0, 5.0, 0.0), Ok(()));
        assert_eq!(check_injection(10.0, 5.0, 1.0), Err(InjectRefused::Cooldown));
        assert_eq!(
            check_injection(INJECTOR_ATP - 0.1, 5.0, 0.0),
            Err(InjectRefused::NotEnoughAtp)
        );
        assert_eq!(
            check_injection(10.0, INJECTOR_METALS - 0.1, 0.0),
            Err(InjectRefused::NotEnoughMetals)
        );
    }
}
