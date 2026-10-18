//! # Secretions: area clouds (headless)
//!
//! A switched-on secretion block puts its product into the grid and pays for
//! it; microcin hurts prey only; a siderophore turns rust into Metals with
//! nothing lost; an AI-2 quorum makes a clump. See `secretion`.

use approx::assert_abs_diff_eq;
use bevy::prelude::*;
use metabolistic3d::blocks::genome::BlockKind;
use metabolistic3d::cell::DamageTaken;
use metabolistic3d::envgrid::{EnvGrid, FieldParams};
use metabolistic3d::environment::Channel;
use metabolistic3d::membrane::Membrane;
use metabolistic3d::molecules::Currency;
use metabolistic3d::movement::world_radius;
use metabolistic3d::npc::{Archetype, spawn_npc};
use metabolistic3d::particles::{RUST_AMOUNT, RustPickup, spawn_rust};
use metabolistic3d::secretion::autoinducer::{Clump, clump_radius};
use metabolistic3d::secretion::{
    Secreting, Secretion, SecretionBar, ToggleSecretion, cloud_params, microcin, siderophore,
};
use metabolistic3d::MetabolisticApp;

use crate::common::*;

/// A grid where every secreted channel moves as in a still field. Channels in
/// `still` neither spread nor decay, so their totals are easy to book.
fn grid(still: &[Channel]) -> EnvGrid {
    let mut grid = EnvGrid::centered(16, 16);
    for channel in Channel::SECRETED {
        let params = if still.contains(&channel) {
            FieldParams::STILL
        } else {
            cloud_params(channel, Vec2::ZERO)
        };
        grid.set_params(channel, params);
    }
    grid
}

fn app_with(grid: EnvGrid) -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(grid);
    app.update();
    app
}

fn toggle(app: &mut App, cell: Entity, key: usize) {
    app.world_mut().write_message(ToggleSecretion { cell, key });
    app.update();
}

fn total(app: &App, channel: Channel) -> f32 {
    app.world().resource::<EnvGrid>().total(channel)
}

fn npc(app: &mut App, archetype: Archetype, position: Vec2, gene: Option<BlockKind>) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_npc(&mut commands, archetype, position, gene, 0)
    };
    app.world_mut().flush();
    cell
}

#[test]
fn a_switched_on_secretion_fills_the_grid_and_pays_for_it() {
    let mut app = app_with(grid(&[Channel::Microcin]));
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::Microcin],
        &[(Currency::ATP, 100.0), (Currency::AminoAcids, 50.0)],
    );
    assert_eq!(
        app.world().get::<SecretionBar>(cell).unwrap().keys[0],
        Some(Secretion::Microcin),
        "the bar fills itself"
    );
    for _ in 0..5 {
        sim_step(&mut app);
    }
    assert_eq!(total(&app, Channel::Microcin), 0.0, "off: nothing goes out");
    assert_eq!(pool(&app, cell, Currency::AminoAcids), 50.0);

    toggle(&mut app, cell, 0);
    assert!(app.world().get::<Secreting>(cell).unwrap().is_on(Secretion::Microcin));
    for _ in 0..10 {
        sim_step(&mut app);
    }
    let used = 50.0 - pool(&app, cell, Currency::AminoAcids);
    assert!(used > 0.0, "the block ran");
    // Every turnover uses 0.4 AminoAcids and puts out YIELD microcin.
    let aa_per_turnover = Secretion::Microcin.costs()[1].1;
    assert_abs_diff_eq!(
        total(&app, Channel::Microcin),
        used / aa_per_turnover * microcin::YIELD,
        epsilon = 1e-2
    );

    toggle(&mut app, cell, 0);
    let after = pool(&app, cell, Currency::AminoAcids);
    for _ in 0..5 {
        sim_step(&mut app);
    }
    assert_eq!(pool(&app, cell, Currency::AminoAcids), after, "off again: no cost");
}

#[test]
fn microcin_hurts_prey_but_not_the_immune_or_the_predator() {
    let mut app = app_with(grid(&[Channel::Microcin]));
    let prey_at = Vec2::new(-150.0, 0.0);
    let immune_at = Vec2::new(0.0, 150.0);
    let predator_at = Vec2::new(150.0, 0.0);
    let prey = npc(&mut app, Archetype::Prey, prey_at, None);
    let immune = npc(&mut app, Archetype::Prey, immune_at, Some(BlockKind::Microcin));
    let predator = npc(&mut app, Archetype::Predator, predator_at, None);
    {
        let mut grid = app.world_mut().resource_mut::<EnvGrid>();
        for at in [prey_at, immune_at, predator_at] {
            grid.deposit(Channel::Microcin, at, 1.0);
        }
    }
    let before = total(&app, Channel::Microcin);
    for _ in 0..8 {
        sim_step(&mut app);
    }
    let membrane = |app: &App, e: Entity| app.world().get::<Membrane>(e).unwrap().clone();
    let prey_damage = app.world().get::<DamageTaken>(prey).unwrap().microcin;
    assert!(prey_damage > 0.0, "the prey was hit");
    assert!(membrane(&app, prey).integrity < membrane(&app, prey).max);
    assert_eq!(app.world().get::<DamageTaken>(immune).unwrap().microcin, 0.0);
    assert_eq!(app.world().get::<DamageTaken>(predator).unwrap().microcin, 0.0);
    // The cloud lost exactly what the prey took.
    assert_abs_diff_eq!(
        before - total(&app, Channel::Microcin),
        prey_damage / microcin::DAMAGE_PER_UNIT,
        epsilon = 1e-3
    );
}

#[test]
fn a_siderophore_turns_rust_into_metals_and_loses_nothing() {
    let mut app = app_with(grid(&[Channel::FeSiderophore]));
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::Siderophore],
        &[(Currency::ATP, 200.0), (Currency::CarbonSkeletons, 100.0), (Currency::Metals, 0.0)],
    );
    let rust = {
        let mut commands = app.world_mut().commands();
        spawn_rust(&mut commands, Vec2::new(10.0, 0.0))
    };
    app.world_mut().flush();
    toggle(&mut app, cell, 0);
    for _ in 0..100 {
        sim_step(&mut app);
    }
    let metals = pool(&app, cell, Currency::Metals);
    assert!(metals > 0.5, "the cell took up iron: {metals}");
    let left = app.world().get::<RustPickup>(rust).map_or(0.0, |r| r.remaining);
    assert_abs_diff_eq!(
        left + total(&app, Channel::FeSiderophore) + metals,
        RUST_AMOUNT,
        epsilon = 1e-3
    );
    // No siderophore receptor, no iron: the complex is a public good only for
    // cells with FepA.
    assert!(!siderophore::has_receptor(&Default::default()));
}

#[test]
fn symbionts_in_an_ai2_quorum_make_a_clump() {
    let mut app = app_with(grid(&[Channel::Autoinducer]));
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::ATP, 50.0)]);
    let player_radius = world_radius(&Default::default());
    let symbionts: Vec<Entity> = [1.0, -1.0]
        .into_iter()
        .map(|side| npc(&mut app, Archetype::Symbiont, Vec2::new(side * (player_radius + 11.0), 0.0), None))
        .collect();
    let symbiont_radius = world_radius(app.world().get(symbionts[0]).unwrap());
    sim_step(&mut app);
    assert_eq!(app.world().get::<Clump>(cell).unwrap().0, 0.0, "no call, no clump");

    {
        let mut grid = app.world_mut().resource_mut::<EnvGrid>();
        for y in 0..16 {
            for x in 0..16 {
                grid.set_level(Channel::Autoinducer, x, y, 2.0);
            }
        }
    }
    sim_step(&mut app);
    let clump = app.world().get::<Clump>(cell).unwrap().0;
    assert_abs_diff_eq!(
        clump,
        clump_radius(player_radius, &[symbiont_radius, symbiont_radius]) - player_radius,
        epsilon = 1e-4
    );
}
