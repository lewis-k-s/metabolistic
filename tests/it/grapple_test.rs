//! # The pilus and the injector (headless)
//!
//! A pilus catches prey and reels it in, a bigger cell or a wall pulls the owner,
//! a released pilus lets go, and a stunned NPC does not move and cannot escape
//! the engulf. The headless app has no physics, so the kinematic fallback of
//! `pilus` moves the pulled body (the joints of `world::pilus_view` follow the
//! same rules in the full game).

use std::time::Duration;

use approx::assert_abs_diff_eq;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use metabolistic3d::actions::{ActionBar, ActionKind, ActionState, SlotHeld, UseSlot};
use metabolistic3d::envgrid::EnvGrid;
use metabolistic3d::genome_damage::GeneRng;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::Currency;
use metabolistic3d::movement::MoveInput;
use metabolistic3d::npc::{Archetype, spawn_npc};
use metabolistic3d::pilus::{PILUS_EXTEND_ATP, PilusTether, Pull, reel_cost};
use metabolistic3d::proteins::{MembraneProtein, ProteinTarget, Proteome};
use metabolistic3d::shared::spawn_starter_cell;
use metabolistic3d::stun::{INJECTOR_ATP, INJECTOR_COOLDOWN, INJECTOR_METALS, STUN_SECONDS, Stunned};
use metabolistic3d::walls::{WallKind, WallMap};
use metabolistic3d::{MetabolisticApp, engulf::EngulfIntent};

use crate::common::pool;

const PILUS_SLOT: usize = 2;
const INJECTOR_SLOT: usize = 3;
/// Radius of the player cell and of a prey cell, in pixels.
const PLAYER_RADIUS: f32 = 20.0;
const PREY_RADIUS: f32 = 11.25;

/// A headless app whose frames last 0.1 s.
fn app() -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
    app.insert_resource(GeneRng::seeded(11));
    app.update();
    app
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// A player with a pilus in slot 3 and an injector in slot 4, lots of ATP and
/// metals, aiming east.
fn hunter(app: &mut App, position: Vec2) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, position).1
    };
    app.world_mut().flush();
    let mut e = app.world_mut().entity_mut(cell);
    {
        let mut proteome = e.get_mut::<Proteome>().unwrap();
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Pilus));
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Injector));
    }
    {
        let mut bar = e.get_mut::<ActionBar>().unwrap();
        bar.slots[PILUS_SLOT] = Some(ActionKind::Pilus);
        bar.slots[INJECTOR_SLOT] = Some(ActionKind::Injector);
    }
    {
        let mut pools = e.get_mut::<CurrencyPools>().unwrap();
        pools.set(Currency::ATP, 100.0);
        pools.set(Currency::Metals, 10.0);
    }
    e.get_mut::<ActionState>().unwrap().aim = Vec2::X;
    cell
}

fn npc(app: &mut App, archetype: Archetype, position: Vec2) -> Entity {
    let npc = {
        let mut commands = app.world_mut().commands();
        spawn_npc(&mut commands, archetype, position, None, 0)
    };
    app.world_mut().flush();
    npc
}

fn position(app: &App, e: Entity) -> Vec2 {
    app.world().get::<Transform>(e).unwrap().translation.truncate()
}

fn tether(app: &App, e: Entity) -> PilusTether {
    *app.world().get::<PilusTether>(e).unwrap()
}

fn press(app: &mut App, cell: Entity, slot: usize, hold: bool) {
    let mut held = [false; 4];
    held[slot] = hold;
    app.world_mut().entity_mut(cell).insert(SlotHeld(held));
    app.world_mut().write_message(UseSlot { cell, slot });
}

fn hold(app: &mut App, cell: Entity, slot: usize, on: bool) {
    let mut held = [false; 4];
    held[slot] = on;
    app.world_mut().entity_mut(cell).insert(SlotHeld(held));
}

#[test]
fn a_pilus_catches_prey_reels_it_in_and_the_player_engulfs_it() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    let prey = npc(&mut app, Archetype::Prey, Vec2::new(100.0, 0.0));
    let atp = pool(&app, player, Currency::ATP);

    press(&mut app, player, PILUS_SLOT, true);
    frames(&mut app, 1);
    let t = tether(&app, player).0.expect("the pilus caught the prey");
    assert_eq!(t.target, Some(prey));
    assert_eq!(t.pull, Pull::TargetToOwner, "prey is smaller: it comes to the owner");
    assert_abs_diff_eq!(t.length, 100.0, epsilon = 1e-3);
    // Holding reels it in. The prey is out of engulf range at first.
    frames(&mut app, 4);
    let mid = position(&app, prey).x;
    assert!(mid < 100.0 && mid > 40.0, "reeling: {mid}");
    frames(&mut app, 12);
    let rest = PLAYER_RADIUS + PREY_RADIUS + 1.5;
    assert_abs_diff_eq!(position(&app, prey).x, rest, epsilon = 0.5);
    assert_abs_diff_eq!(position(&app, player).x, 0.0, epsilon = 1e-4);
    // It cost the extension plus PilT for every pixel of fibre.
    let spent = atp - pool(&app, player, Currency::ATP);
    assert_abs_diff_eq!(spent, PILUS_EXTEND_ATP + reel_cost(100.0 - rest), epsilon = 0.05);
    // The prey is now in contact: engulf works.
    app.world_mut().get_mut::<EngulfIntent>(player).unwrap().0 = true;
    frames(&mut app, 2);
    assert!(app.world().get_entity(prey).is_err(), "the prey was engulfed");
    assert_eq!(tether(&app, player).0, None, "the pilus lets go of a gone target");
}

#[test]
fn a_released_pilus_detaches_and_a_miss_costs_only_the_extension() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    let prey = npc(&mut app, Archetype::Prey, Vec2::new(100.0, 0.0));
    press(&mut app, player, PILUS_SLOT, true);
    frames(&mut app, 3);
    assert!(tether(&app, player).0.is_some());
    hold(&mut app, player, PILUS_SLOT, false);
    frames(&mut app, 1);
    assert_eq!(tether(&app, player).0, None);
    let there = position(&app, prey).x;
    frames(&mut app, 5);
    assert_eq!(position(&app, prey).x, there, "a free prey is not pulled");

    // Aim away from the prey: the pilus finds nothing.
    app.world_mut().get_mut::<ActionState>(player).unwrap().aim = Vec2::NEG_X;
    let atp = pool(&app, player, Currency::ATP);
    press(&mut app, player, PILUS_SLOT, true);
    frames(&mut app, 2);
    assert_eq!(tether(&app, player).0, None);
    assert_abs_diff_eq!(pool(&app, player, Currency::ATP), atp - PILUS_EXTEND_ATP, epsilon = 1e-3);
}

#[test]
fn the_pilus_lets_go_when_the_atp_runs_out() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    let prey = npc(&mut app, Archetype::Prey, Vec2::new(130.0, 0.0));
    press(&mut app, player, PILUS_SLOT, true);
    frames(&mut app, 1);
    assert!(tether(&app, player).0.is_some());
    // Just enough for a few pixels of reel.
    app.world_mut().get_mut::<CurrencyPools>(player).unwrap().set(Currency::ATP, 0.05);
    frames(&mut app, 5);
    assert_eq!(tether(&app, player).0, None, "no ATP: the pilus detaches");
    assert!(position(&app, prey).x > 100.0, "it did not get far");
}

#[test]
fn a_bigger_cell_pulls_the_owner_to_it() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    let predator = npc(&mut app, Archetype::Predator, Vec2::new(130.0, 0.0));
    press(&mut app, player, PILUS_SLOT, true);
    frames(&mut app, 1);
    assert_eq!(tether(&app, player).0.unwrap().pull, Pull::OwnerToTarget);
    frames(&mut app, 3);
    assert!(position(&app, player).x > 5.0, "the owner moved to the predator");
    assert_eq!(position(&app, predator).x, 130.0, "the predator stayed");
}

#[test]
fn a_wall_pulls_the_owner_to_it() {
    let mut app = app();
    let mut grid = EnvGrid::centered(8, 8);
    // Tile (5,3) covers x 32..64, y -32..0.
    grid.set_wall(5, 3, true);
    let mut map = WallMap::default();
    map.insert((5, 3), WallKind::Wall);
    app.insert_resource(grid);
    app.insert_resource(map);
    let player = hunter(&mut app, Vec2::new(0.0, -16.0));
    press(&mut app, player, PILUS_SLOT, true);
    frames(&mut app, 1);
    let t = tether(&app, player).0.expect("the pilus stuck to the wall");
    assert_eq!(t.target, None);
    assert_eq!(t.pull, Pull::OwnerToTarget);
    frames(&mut app, 15);
    // The owner rim ends at the anchor: the centre is a radius short of x = 35.
    let x = position(&app, player).x;
    assert!(x > 10.0 && x < 36.0 - PLAYER_RADIUS + 1.0, "x = {x}");
}

#[test]
fn a_stunned_npc_does_not_move_and_cannot_escape() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    // Prey in contact, in the cone along the aim, and another prey far away.
    let prey = npc(&mut app, Archetype::Prey, Vec2::new(PLAYER_RADIUS + PREY_RADIUS + 2.0, 0.0));
    let free = npc(&mut app, Archetype::Prey, Vec2::new(0.0, 400.0));
    frames(&mut app, 3);
    assert_ne!(
        app.world().get::<MoveInput>(free).unwrap().0,
        Vec2::ZERO,
        "an unstunned prey wants to move"
    );
    let atp = pool(&app, player, Currency::ATP);
    let metals = pool(&app, player, Currency::Metals);
    press(&mut app, player, INJECTOR_SLOT, false);
    frames(&mut app, 1);
    assert!(app.world().get::<Stunned>(prey).is_some(), "the target is stunned");
    assert!(app.world().get::<Stunned>(free).is_none(), "only the target in the cone");
    assert_abs_diff_eq!(pool(&app, player, Currency::ATP), atp - INJECTOR_ATP, epsilon = 1e-3);
    assert_abs_diff_eq!(pool(&app, player, Currency::Metals), metals - INJECTOR_METALS, epsilon = 1e-3);
    // The AI is paused for the whole stun.
    let mut seconds = 0.0;
    while app.world().get::<Stunned>(prey).is_some() {
        frames(&mut app, 1);
        seconds += 0.1;
        if app.world().get::<Stunned>(prey).is_some() {
            assert_eq!(app.world().get::<MoveInput>(prey).unwrap().0, Vec2::ZERO);
        }
        assert!(seconds < STUN_SECONDS + 1.0, "the stun ends");
    }
    assert!(seconds >= STUN_SECONDS - 0.3, "the stun lasted {seconds} s");
    // The cooldown holds a second use.
    assert!(app.world().get::<ActionState>(player).unwrap().injector_cooldown > 0.0);
    assert!(INJECTOR_COOLDOWN > STUN_SECONDS - 1.0);
}

#[test]
fn stunned_prey_is_engulfed_even_though_it_would_flee() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    let prey = npc(&mut app, Archetype::Prey, Vec2::new(PLAYER_RADIUS + PREY_RADIUS + 2.0, 0.0));
    press(&mut app, player, INJECTOR_SLOT, false);
    frames(&mut app, 1);
    assert!(app.world().get::<Stunned>(prey).is_some());
    // The steering system may have run before the stun in that frame: one more
    // frame, and the prey wants nothing.
    frames(&mut app, 1);
    assert_eq!(app.world().get::<MoveInput>(prey).unwrap().0, Vec2::ZERO);
    app.world_mut().get_mut::<EngulfIntent>(player).unwrap().0 = true;
    frames(&mut app, 1);
    assert!(app.world().get_entity(prey).is_err());
}

#[test]
fn the_injector_needs_contact_and_the_right_direction() {
    let mut app = app();
    let player = hunter(&mut app, Vec2::ZERO);
    let far = npc(&mut app, Archetype::Prey, Vec2::new(80.0, 0.0));
    let atp = pool(&app, player, Currency::ATP);
    press(&mut app, player, INJECTOR_SLOT, false);
    frames(&mut app, 1);
    assert!(app.world().get::<Stunned>(far).is_none(), "out of reach");
    assert_eq!(pool(&app, player, Currency::ATP), atp, "a use with no target costs nothing");
    // In reach but behind.
    app.world_mut().get_mut::<ActionState>(player).unwrap().aim = Vec2::NEG_X;
    app.world_mut().get_mut::<Transform>(far).unwrap().translation = Vec3::new(35.0, 0.0, 0.0);
    press(&mut app, player, INJECTOR_SLOT, false);
    frames(&mut app, 1);
    assert!(app.world().get::<Stunned>(far).is_none(), "behind the cone");
    // Turn round: now it hits.
    app.world_mut().get_mut::<ActionState>(player).unwrap().aim = Vec2::X;
    press(&mut app, player, INJECTOR_SLOT, false);
    frames(&mut app, 1);
    assert!(app.world().get::<Stunned>(far).is_some());
}
