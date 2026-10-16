//! # Milestone M2: a cell in a field (headless part)
//!
//! Tests for the environment grid, the exchange between grid and cell, heat
//! damage, and the run states. The graphical part (movement, rendering) is
//! covered by the smoke test.

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use metabolistic3d::cell::{DamageSource, DamageTaken, DeathCause, death_cause, is_alive};
use metabolistic3d::envgrid::{CELL_SIZE, EnvGrid, FieldParams};
use metabolistic3d::environment::{Channel, LocalEnvironment};
use metabolistic3d::exchange::{
    ExchangeStats, HEAT_DAMAGE_THRESHOLD, MINERAL_STOCK_CAP, uptake_capacity,
};
use metabolistic3d::level::{self, START_POSITION, VENTS};
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::CurrencyPools;
use metabolistic3d::molecules::Currency;
use metabolistic3d::movement::{ThrustEffort, move_atp_cost};
use metabolistic3d::player::ControlledCell;
use metabolistic3d::proteins::{MembraneProtein, ProteinTarget, Proteome};
use metabolistic3d::run::RunSummary;
use metabolistic3d::shared::{spawn_starter_cell, starter_proteome};
use metabolistic3d::{GameState, InRun, MetabolisticApp};

use crate::common::*;

/// Ticks per simulated second (default timing).
const TICKS_PER_SEC: usize = 5;

/// A headless app with `grid` and a starter cell at `position`.
fn field_app(grid: EnvGrid, position: Vec2) -> (App, Entity) {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(grid);
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, position).1
    };
    app.world_mut().flush();
    app.update(); // spawn the blocks
    (app, cell)
}

fn alive(app: &App, cell: Entity) -> bool {
    is_alive(
        app.world().get::<CurrencyPools>(cell).unwrap(),
        app.world().get::<Membrane>(cell).unwrap(),
    )
}

fn integrity(app: &App, cell: Entity) -> f32 {
    app.world().get::<Membrane>(cell).unwrap().integrity
}

fn minerals_in_cell(app: &App, cell: Entity) -> f32 {
    app.world()
        .get::<CurrencyPools>(cell)
        .unwrap()
        .get(Currency::Minerals)
}

fn stats(app: &App, cell: Entity) -> ExchangeStats {
    *app.world().get::<ExchangeStats>(cell).unwrap()
}

fn grid_total(app: &App, channel: Channel) -> f32 {
    app.world().resource::<EnvGrid>().total(channel)
}

/// A small closed grid with no sources, no decay and no drift. Diffusion is
/// on, so minerals move between grid cells.
fn closed_grid_with_minerals() -> EnvGrid {
    let mut grid = EnvGrid::centered(16, 16);
    grid.set_params(
        Channel::Minerals,
        FieldParams {
            diffusion: 0.8,
            ..FieldParams::STILL
        },
    );
    for y in 6..10 {
        for x in 6..10 {
            grid.set_level(Channel::Minerals, x, y, 5.0);
        }
    }
    grid
}

// --- Exchange: mass and uptake ---

#[test]
fn mass_is_conserved_between_grid_and_cell() {
    let (mut app, cell) = field_app(closed_grid_with_minerals(), Vec2::ZERO);
    let grid_before = grid_total(&app, Channel::Minerals);
    let stock_before = minerals_in_cell(&app, cell);
    let total_before = grid_before + stock_before;
    for tick in 0..60 * TICKS_PER_SEC {
        sim_step(&mut app);
        let in_grid = grid_total(&app, Channel::Minerals);
        let s = stats(&app, cell);
        let total = in_grid + minerals_in_cell(&app, cell) + s.minerals_used;
        assert!(
            (total - total_before).abs() < 1e-2,
            "tick {tick}: grid {in_grid} + cell {} + used {} != {total_before}",
            minerals_in_cell(&app, cell),
            s.minerals_used
        );
        assert!(app.world().resource::<EnvGrid>().min_level(Channel::Minerals) >= 0.0);
    }
    let s = stats(&app, cell);
    assert!(s.minerals_taken > 0.0, "the cell took minerals in");
    // What the grid lost is what the cell took.
    let lost = grid_before - grid_total(&app, Channel::Minerals);
    assert!((lost - s.minerals_taken).abs() < 1e-2, "lost {lost}, taken {}", s.minerals_taken);
}

#[test]
fn a_short_mineral_stock_limits_chemosynthesis() {
    // An empty grid with heat only: the pool is the only mineral source.
    let mut grid = EnvGrid::centered(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            grid.set_level(Channel::Heat, x, y, 20.0);
        }
    }
    let (mut app, cell) = field_app(grid, Vec2::ZERO);
    set_pools(&mut app, cell, &[(Currency::Minerals, 1.0)]);
    let mut previous = pool(&app, cell, Currency::Minerals);
    for _ in 0..30 * TICKS_PER_SEC {
        sim_step(&mut app);
        let now = pool(&app, cell, Currency::Minerals);
        assert!(now >= 0.0 && now <= previous + 1e-6, "pool {previous} -> {now}");
        previous = now;
    }
    let used = stats(&app, cell).minerals_used;
    assert!(used <= 1.0 + 1e-3, "used {used} of 1.0 minerals");
    assert!(used > 0.5, "chemosynthesis did use the stock: {used}");
}

#[test]
fn heat_is_a_condition_and_is_not_used_up() {
    let mut grid = EnvGrid::centered(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            grid.set_level(Channel::Heat, x, y, 10.0);
        }
    }
    let (mut app, cell) = field_app(grid, Vec2::ZERO);
    for _ in 0..30 * TICKS_PER_SEC {
        sim_step(&mut app);
    }
    let heat = app
        .world()
        .get::<LocalEnvironment>(cell)
        .unwrap()
        .get(Channel::Heat);
    assert!((heat - 10.0).abs() < 1e-3, "cell sees heat {heat}");
    assert!((grid_total(&app, Channel::Heat) - 10.0 * 256.0).abs() < 1e-1);
}

#[test]
fn uptake_is_slow_without_transporters_and_faster_with_them() {
    // The same plentiful grid. The cell has no minerals, and the stock has
    // room. After 5 s, compare the minerals taken in.
    let rich = || {
        let mut grid = EnvGrid::centered(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                grid.set_level(Channel::Minerals, x, y, 100.0);
            }
        }
        grid
    };
    let taken_after = |transporters: u32| {
        let (mut app, cell) = field_app(rich(), Vec2::ZERO);
        let mut proteome = Proteome::default();
        for _ in 0..transporters {
            proteome.add(ProteinTarget::Membrane(MembraneProtein::Transporter));
        }
        app.world_mut().entity_mut(cell).insert(proteome);
        // No chemosynthesis, so nothing uses the stock.
        app.world_mut()
            .get_mut::<CurrencyPools>(cell)
            .unwrap()
            .set(Currency::Minerals, 0.0);
        for _ in 0..5 * TICKS_PER_SEC {
            sim_step(&mut app);
        }
        stats(&app, cell).minerals_taken
    };
    let none = taken_after(0);
    let one = taken_after(1);
    let three = taken_after(3);
    assert!(none > 0.0, "slow, but not zero");
    // The starter genome uses minerals too, but taken in 5 s is capped by the
    // transporter limit, so it must follow `uptake_capacity`.
    assert!(none <= uptake_capacity(0) * 5.0 + 1e-3);
    assert!(one > none && three > one);
}

#[test]
fn the_mineral_stock_stops_at_its_cap() {
    let (mut app, cell) = field_app(closed_grid_with_minerals(), Vec2::ZERO);
    let mut proteome = starter_proteome();
    for _ in 0..4 {
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Transporter));
    }
    app.world_mut().entity_mut(cell).insert(proteome);
    for _ in 0..120 * TICKS_PER_SEC {
        sim_step(&mut app);
        assert!(minerals_in_cell(&app, cell) <= MINERAL_STOCK_CAP + 1e-4);
    }
}

#[test]
fn moving_costs_atp() {
    let (mut app, cell) = field_app(EnvGrid::centered(4, 4), Vec2::ZERO);
    set_pools(&mut app, cell, &[(Currency::ATP, 50.0)]);
    let before = pool(&app, cell, Currency::ATP);
    app.world_mut().get_mut::<ThrustEffort>(cell).unwrap().0 = 300.0;
    // A tick with no blocks of value: compare against an idle tick.
    let (mut idle_app, idle_cell) = field_app(EnvGrid::centered(4, 4), Vec2::ZERO);
    set_pools(&mut idle_app, idle_cell, &[(Currency::ATP, 50.0)]);
    sim_step(&mut app);
    sim_step(&mut idle_app);
    let spent = pool(&idle_app, idle_cell, Currency::ATP) - pool(&app, cell, Currency::ATP);
    assert!((spent - move_atp_cost(300.0)).abs() < 1e-3, "spent {spent}");
    assert!(before > pool(&app, cell, Currency::ATP));
    assert_eq!(app.world().get::<ThrustEffort>(cell).unwrap().0, 0.0, "effort is paid once");
}

// --- Heat damage ---

#[test]
fn heat_above_the_threshold_lowers_integrity() {
    let mut grid = EnvGrid::centered(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            grid.set_level(Channel::Heat, x, y, HEAT_DAMAGE_THRESHOLD + 30.0);
        }
    }
    let (mut app, cell) = field_app(grid, Vec2::ZERO);
    let before = integrity(&app, cell);
    for _ in 0..5 * TICKS_PER_SEC {
        sim_step(&mut app);
    }
    assert!(integrity(&app, cell) < before - 1.0, "integrity {}", integrity(&app, cell));
}

#[test]
fn heat_below_the_threshold_does_no_damage() {
    let mut grid = EnvGrid::centered(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            grid.set_level(Channel::Heat, x, y, HEAT_DAMAGE_THRESHOLD - 1.0);
        }
    }
    let (mut app, cell) = field_app(grid, Vec2::ZERO);
    for _ in 0..30 * TICKS_PER_SEC {
        sim_step(&mut app);
    }
    assert_eq!(integrity(&app, cell), 100.0);
}

// --- The vent field ---

#[test]
fn a_cell_in_a_cold_empty_zone_dies() {
    let far = Vec2::new(-900.0, 600.0);
    let (mut app, cell) = field_app(level::vent_field(), far);
    let mut died_at = None;
    for tick in 0..600 * TICKS_PER_SEC {
        sim_step(&mut app);
        if !alive(&app, cell) {
            died_at = Some(tick);
            break;
        }
    }
    assert!(died_at.is_some(), "a cell with no heat and no minerals must starve");
    assert_eq!(
        death_cause(
            app.world().get::<CurrencyPools>(cell).unwrap(),
            app.world().get::<Membrane>(cell).unwrap()
        ),
        Some(DeathCause::Starved)
    );
}

#[test]
fn a_cell_near_a_plume_lives() {
    let (mut app, cell) = field_app(level::vent_field(), START_POSITION);
    for tick in 0..300 * TICKS_PER_SEC {
        sim_step(&mut app);
        assert!(alive(&app, cell), "the starter cell died at tick {tick}");
    }
    assert!(pool(&app, cell, Currency::ATP) > 10.0, "ATP is steady, not just above 0");
}

#[test]
fn a_cell_in_the_vent_core_burns() {
    let (mut app, cell) = field_app(level::vent_field(), VENTS[0].position);
    let mut died = false;
    for _ in 0..300 * TICKS_PER_SEC {
        sim_step(&mut app);
        if !alive(&app, cell) {
            died = true;
            break;
        }
    }
    assert!(died, "the core must be deadly");
    assert_eq!(
        app.world().get::<DamageTaken>(cell).unwrap().main_source(),
        Some(DamageSource::Heat)
    );
    assert_eq!(
        death_cause(
            app.world().get::<CurrencyPools>(cell).unwrap(),
            app.world().get::<Membrane>(cell).unwrap()
        ),
        Some(DeathCause::MembraneBroken)
    );
}

#[test]
fn the_start_is_near_the_vent_but_not_in_the_core() {
    let grid = level::vent_field();
    let heat = grid.sample(Channel::Heat, START_POSITION);
    assert!(heat > 3.0 && heat < HEAT_DAMAGE_THRESHOLD, "start heat {heat}");
    assert!(grid.sample(Channel::Minerals, START_POSITION) > 1.0);
    assert!(START_POSITION.distance(VENTS[0].position) < 8.0 * CELL_SIZE);
    assert!(grid.sample(Channel::Heat, VENTS[0].position) > HEAT_DAMAGE_THRESHOLD);
}

#[test]
fn far_from_the_vents_the_field_is_cold_and_empty() {
    let grid = level::vent_field();
    for corner in [
        Vec2::new(-900.0, 600.0),
        Vec2::new(-900.0, -600.0),
        Vec2::new(900.0, -600.0),
    ] {
        assert!(grid.sample(Channel::Heat, corner) < 0.1, "{corner}");
        assert!(grid.sample(Channel::Minerals, corner) < 0.1, "{corner}");
    }
}

#[test]
fn the_vent_field_grid_stays_non_negative_while_cells_mine_it() {
    let (mut app, _cell) = field_app(level::vent_field(), START_POSITION);
    for _ in 0..60 * TICKS_PER_SEC {
        sim_step(&mut app);
        let grid = app.world().resource::<EnvGrid>();
        for channel in Channel::ALL {
            assert!(grid.min_level(channel) >= 0.0);
        }
    }
}

// --- Run states ---

/// An app with states only, to test `InRun`.
fn state_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .init_state::<GameState>()
        .add_computed_state::<InRun>();
    app
}

fn set_state(app: &mut App, state: GameState) {
    app.world_mut().resource_mut::<NextState<GameState>>().set(state);
    app.update();
}

fn in_run(app: &App) -> bool {
    app.world().contains_resource::<State<InRun>>()
}

#[derive(Resource, Default)]
struct Entered(u32);

#[test]
fn in_run_covers_run_and_manage_and_starts_once() {
    let mut app = state_app();
    app.init_resource::<Entered>().add_systems(
        OnEnter(InRun),
        |mut entered: ResMut<Entered>| entered.0 += 1,
    );
    app.update();
    assert!(!in_run(&app), "not in a run while loading");
    set_state(&mut app, GameState::Menu);
    assert!(!in_run(&app));
    set_state(&mut app, GameState::Run);
    assert!(in_run(&app));
    set_state(&mut app, GameState::Manage);
    assert!(in_run(&app), "the editor keeps the run");
    set_state(&mut app, GameState::Run);
    assert!(in_run(&app));
    assert_eq!(
        app.world().resource::<Entered>().0,
        1,
        "switching Run and Manage must not restart the run"
    );
    set_state(&mut app, GameState::RunOver);
    assert!(!in_run(&app));
    set_state(&mut app, GameState::Run);
    assert_eq!(app.world().resource::<Entered>().0, 2, "a restart is a new run");
    set_state(&mut app, GameState::Menu);
    assert!(!in_run(&app));
}

#[test]
fn the_world_marked_for_the_run_goes_when_the_run_ends() {
    let mut app = state_app();
    set_state(&mut app, GameState::Run);
    let entity = app.world_mut().spawn(DespawnOnExit(InRun)).id();
    set_state(&mut app, GameState::Manage);
    assert!(app.world().get_entity(entity).is_ok(), "stays in Manage");
    set_state(&mut app, GameState::Run);
    assert!(app.world().get_entity(entity).is_ok(), "stays in Run");
    set_state(&mut app, GameState::RunOver);
    assert!(app.world().get_entity(entity).is_err(), "gone at death");
}

#[test]
fn the_player_dying_ends_the_run() {
    let mut app = MetabolisticApp::new_headless();
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    set_state(&mut app, GameState::Run);
    app.update();
    assert_eq!(*app.world().resource::<State<GameState>>().get(), GameState::Run);

    // The cell has just burned.
    app.world_mut().get_mut::<Membrane>(cell).unwrap().damage(1000.0);
    app.update();
    app.update();
    assert_eq!(
        *app.world().resource::<State<GameState>>().get(),
        GameState::RunOver
    );
    assert_eq!(
        app.world().resource::<RunSummary>().cause,
        Some(DeathCause::MembraneBroken)
    );
    // The player entity still points to the cell: the world removes both later.
    let _ = app.world_mut().query::<&ControlledCell>().iter(app.world()).count();
}

#[test]
fn the_metabolic_tick_runs_in_run_and_manage() {
    // The tick has no state condition. It works whenever a world exists, and
    // the world exists exactly in `InRun`. Here the grid ticks in both screens.
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(closed_grid_with_minerals());
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    for state in [GameState::Run, GameState::Manage] {
        set_state(&mut app, state.clone());
        let before = stats(&app, cell).minerals_taken;
        for _ in 0..10 {
            sim_step(&mut app);
        }
        assert!(stats(&app, cell).minerals_taken > before, "no uptake in {state:?}");
    }
}
