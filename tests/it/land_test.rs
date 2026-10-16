//! # Milestone M8: land (headless)
//!
//! Respiration, oxygen uptake from the air, hydration and desiccation, the
//! cuticle, drag on dry ground, the waxy wall, the land level, the key-gene top-up
//! and the whole stage chain Vents -> Shallow water -> Land -> run complete.
//!
//! Water is set on `LocalEnvironment` (no grid), except where a test says it uses
//! a grid.

use std::collections::BTreeSet;
use std::time::Duration;

use approx::assert_abs_diff_eq;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use metabolistic3d::blocks::cuticle;
use metabolistic3d::blocks::genome::{BlockKind, GeneState, Genome};
use metabolistic3d::blocks::{fermentation, respiration};
use metabolistic3d::cell::{Cell, DamageSource, DamageTaken, DeathCause};
use metabolistic3d::codex::{Codex, CodexStore, codex_path};
use metabolistic3d::desiccation::{self, CuticleShield, Hydration};
use metabolistic3d::envgrid::{EnvGrid, FieldParams};
use metabolistic3d::environment::{Channel, LocalEnvironment};
use metabolistic3d::gate::GateProgress;
use metabolistic3d::genome_damage::{GeneRng, UvFraction};
use metabolistic3d::layout::{generate_land_counted, validate};
use metabolistic3d::level::{CurrentLevel, FieldKind, LevelDef};
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::{BlockInput, CellTick, CurrencyPools, FlowmapTimingConfig, solve_tick};
use metabolistic3d::molecules::Currency;
use metabolistic3d::movement::{BASE_THRUST, body_drag, body_mass, drag_in_field};
use metabolistic3d::cell::CellSize;
use metabolistic3d::npc::{Archetype, KeyGeneRule, NpcSlots, SpawnSlot, spawn_npc, spawn_slot};
use metabolistic3d::oxygen::o2_exchange_system;
use metabolistic3d::proteins::{ProteinTarget, Proteome};
use metabolistic3d::run::{RunState, RunSummary, death_message, starter_carry};
use metabolistic3d::shared::{spawn_cell_from_carry, spawn_starter_cell};
use metabolistic3d::stage::Stage;
use metabolistic3d::walls::{WallKind, WallMap, solid_for};
use metabolistic3d::{GameState, MetabolisticApp};

use crate::common::*;

// --- Helpers ---

/// A headless app on the land level (the level says that cells dry out), with
/// no UV and a fixed gene seed.
fn land_app() -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(UvFraction(0.0));
    app.insert_resource(GeneRng::seeded(3));
    app.insert_resource(CurrentLevel(LevelDef::land_base()));
    app
}

/// The starter cell with `kinds` expressed (one enzyme each unless set), and the
/// given water level under it.
fn land_cell(app: &mut App, kinds: &[BlockKind], water: f32) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    express_blocks(app, cell, kinds);
    app.world_mut().get_mut::<LocalEnvironment>(cell).unwrap().set(Channel::Water, water);
    cell
}

fn hydration(app: &App, cell: Entity) -> f32 {
    app.world().get::<Hydration>(cell).unwrap().0
}

fn membrane(app: &App, cell: Entity) -> f32 {
    app.world().get::<Membrane>(cell).unwrap().integrity
}

fn run_seconds(app: &mut App, seconds: f32) {
    let ticks = (seconds * 5.0).round() as usize;
    for _ in 0..ticks {
        sim_step(app);
    }
}

// --- Respiration ---

#[test]
fn respiration_keeps_its_stoichiometry() {
    let profile = respiration::flux_profile();
    let mut pools = CurrencyPools::default();
    for currency in [Currency::CarbonSkeletons, Currency::O2, Currency::ReducingPower] {
        pools.set(currency, 100.0);
    }
    let kinetics = respiration::kinetics();
    let input = BlockInput { k_half: kinetics.k_half, ..BlockInput::new(&profile, 2.0) };
    let result = solve_tick(&pools, &[input], 1.0);
    let turnover = result.turnover[0];
    assert!(turnover > 0.0);
    let delta = |c: Currency| result.deltas.get(&c).copied().unwrap_or(0.0);
    assert_abs_diff_eq!(delta(Currency::ATP), turnover * respiration::ATP_PER_TURNOVER, epsilon = 1e-4);
    assert_abs_diff_eq!(delta(Currency::CarbonSkeletons), -turnover * respiration::CARBON_PER_TURNOVER, epsilon = 1e-4);
    assert_abs_diff_eq!(delta(Currency::O2), -turnover * respiration::O2_PER_TURNOVER, epsilon = 1e-4);
    assert_abs_diff_eq!(delta(Currency::ReducingPower), -turnover * respiration::REDUCING_POWER_PER_TURNOVER, epsilon = 1e-4);
    assert_abs_diff_eq!(delta(Currency::ROS), turnover * respiration::ROS_PER_TURNOVER, epsilon = 1e-4);
    assert!(delta(Currency::ROS) > 0.0, "it makes a little ROS");
}

#[test]
fn respiration_gives_about_three_times_the_atp_per_carbon_of_fermentation() {
    let fermentation_atp = fermentation::flux_profile().get_production(Currency::ATP)
        / fermentation::flux_profile().get_demand(Currency::Pyruvate);
    let ratio = respiration::atp_per_carbon() / fermentation_atp;
    assert!((2.5..=3.5).contains(&ratio), "{ratio}");
}

#[test]
fn respiration_needs_o2_and_carbon() {
    let profile = respiration::flux_profile();
    let kinetics = respiration::kinetics();
    let turnover = |o2: f32, carbon: f32| {
        let mut pools = CurrencyPools::default();
        pools.set(Currency::O2, o2);
        pools.set(Currency::CarbonSkeletons, carbon);
        pools.set(Currency::ReducingPower, 50.0);
        let input = BlockInput { k_half: kinetics.k_half, ..BlockInput::new(&profile, 1.0) };
        solve_tick(&pools, &[input], 1.0).turnover[0]
    };
    assert_eq!(turnover(0.0, 50.0), 0.0, "no O2, no respiration");
    assert_eq!(turnover(50.0, 0.0), 0.0, "no carbon, no respiration");
    assert!(turnover(2.0, 50.0) < turnover(8.0, 50.0), "more O2, more turnover");
    assert!(turnover(8.0, 50.0) < turnover(40.0, 50.0));
    // The pools never go negative, even with a tiny O2 stock.
    let mut pools = CurrencyPools::default();
    pools.set(Currency::O2, 0.01);
    pools.set(Currency::CarbonSkeletons, 50.0);
    pools.set(Currency::ReducingPower, 50.0);
    let input = BlockInput { k_half: kinetics.k_half, ..BlockInput::new(&profile, 10.0) };
    let result = solve_tick(&pools, &[input], 1.0);
    assert!(result.deltas[&Currency::O2] >= -0.01 - 1e-6);
}

/// A grid with every cell at `level` of O2, still (no decay, no diffusion), so
/// that only the cell changes it. Level 0 is a vacuum.
fn air(level: f32) -> EnvGrid {
    let mut grid = EnvGrid::centered(8, 8);
    for channel in Channel::ALL {
        grid.set_params(channel, FieldParams::STILL);
    }
    for x in 0..8 {
        for y in 0..8 {
            grid.set_level(Channel::O2, x, y, level);
        }
    }
    grid
}

/// The air of the land level: O2 everywhere, refilled by the sky as the cell
/// uses it.
fn land_air() -> EnvGrid {
    LevelDef::land_base().grid()
}

#[test]
fn o2_uptake_from_the_air_conserves_grid_and_cell_mass() {
    let mut app = land_app();
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::O2, 0.0)]);
    app.insert_resource(air(4.0));
    app.world_mut().entity_mut(cell).insert(Transform::from_translation(Vec3::new(-10.0, -10.0, 0.0)));
    let total = |app: &App| pool(app, cell, Currency::O2) + app.world().resource::<EnvGrid>().total(Channel::O2);
    let before = total(&app);
    let mut last = 0.0;
    for _ in 0..60 {
        app.world_mut().run_system_once(o2_exchange_system).unwrap();
        assert_abs_diff_eq!(total(&app), before, epsilon = 1e-2);
        let now = pool(&app, cell, Currency::O2);
        assert!(now >= last - 1e-6, "the cell takes O2 in while the air has more");
        last = now;
    }
    let level = app.world().resource::<EnvGrid>().sample(Channel::O2, Vec2::new(-10.0, -10.0));
    assert!(last > 2.0 && last <= 4.0 + 1e-3, "the cell fills up to the air level: {last} of {level}");
}

/// A cell that respires in air: carbon and reducing power in stock, O2 from the
/// grid.
fn respiring_cell(app: &mut App, extra: &[BlockKind]) -> Entity {
    let mut kinds = vec![BlockKind::Respiration];
    kinds.extend_from_slice(extra);
    let cell = spawn_test_cell(
        app,
        &kinds,
        &[
            (Currency::ATP, 20.0),
            (Currency::CarbonSkeletons, 80.0),
            (Currency::ReducingPower, 80.0),
            (Currency::O2, 0.0),
        ],
    );
    app.world_mut().entity_mut(cell).insert(Transform::from_translation(Vec3::new(-10.0, -10.0, 0.0)));
    cell
}

#[test]
fn a_cell_in_air_makes_atp_by_respiration_and_without_air_it_does_not() {
    let net_atp = |level: f32| {
        let mut app = land_app();
        let cell = respiring_cell(&mut app, &[]);
        app.insert_resource(if level > 0.0 { land_air() } else { air(0.0) });
        run_seconds(&mut app, 10.0);
        pool(&app, cell, Currency::ATP)
    };
    let in_air = net_atp(4.0);
    let in_vacuum = net_atp(0.0);
    assert!(in_air > 20.0, "respiration pays the upkeep and more: {in_air}");
    assert!(in_vacuum < 20.0, "without O2 only the upkeep is paid: {in_vacuum}");
}

#[test]
fn respiration_makes_ros_so_catalase_still_matters() {
    let ros = |extra: &[BlockKind]| {
        let mut app = land_app();
        let cell = respiring_cell(&mut app, extra);
        app.insert_resource(land_air());
        run_seconds(&mut app, 40.0);
        pool(&app, cell, Currency::ROS)
    };
    let bare = ros(&[]);
    let guarded = ros(&[BlockKind::Catalase]);
    assert!(bare > 0.3, "respiration and the O2 pool make ROS: {bare}");
    assert!(guarded < 0.7 * bare, "catalase lowers it: {guarded} against {bare}");
}

// --- Hydration and desiccation ---

#[test]
fn a_cell_dries_on_dry_ground_and_the_membrane_pays() {
    let mut app = land_app();
    let cell = land_cell(&mut app, &[], 0.0);
    run_seconds(&mut app, 60.0);
    assert!(hydration(&app, cell) < 0.1, "{}", hydration(&app, cell));
    assert!(membrane(&app, cell) < 100.0 || app.world().get::<DamageTaken>(cell).unwrap().desiccation > 0.0);
    let taken = *app.world().get::<DamageTaken>(cell).unwrap();
    assert!(taken.desiccation > 5.0, "{taken:?}");
    assert_eq!(taken.main_source(), Some(DamageSource::Desiccation));
    let message = death_message(Some(DeathCause::MembraneBroken), taken.main_source());
    assert!(message.contains("desiccation"), "{message}");
}

#[test]
fn a_wet_cell_never_dries_and_a_dry_cell_recovers_in_water() {
    let mut app = land_app();
    let cell = land_cell(&mut app, &[], 10.0);
    app.world_mut().get_mut::<Hydration>(cell).unwrap().0 = 0.15;
    run_seconds(&mut app, 20.0);
    assert!(hydration(&app, cell) > 0.95, "{}", hydration(&app, cell));
    let before = membrane(&app, cell);
    run_seconds(&mut app, 60.0);
    assert!(hydration(&app, cell) > 0.95, "stays wet");
    assert!(membrane(&app, cell) >= before - 1e-3);
    // Water stops the loss: with the water gone it falls again.
    app.world_mut().get_mut::<LocalEnvironment>(cell).unwrap().set(Channel::Water, 0.0);
    run_seconds(&mut app, 10.0);
    assert!(hydration(&app, cell) < 0.8);
}

#[test]
fn the_desiccation_rate_does_not_depend_on_the_tick_length() {
    let run = |ticks_per_second: f32| {
        let mut app = land_app();
        app.insert_resource(FlowmapTimingConfig { updates_per_second: ticks_per_second, manual: true, ..default() });
        let cell = land_cell(&mut app, &[], 0.4);
        app.world_mut().entity_mut(cell).insert(CellTick::every(1));
        for _ in 0..(12.0 * ticks_per_second) as usize {
            sim_step(&mut app);
        }
        hydration(&app, cell)
    };
    let slow = run(2.0);
    let fast = run(10.0);
    assert_abs_diff_eq!(slow, fast, epsilon = 2e-3);
    assert!(slow < 1.0 && slow > 0.4, "it dries a little in a moist spot: {slow}");
}

#[test]
fn only_land_dries_cells_and_creatures_do_not_dry() {
    // No land level: the cell stays full, even with no water.
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(UvFraction(0.0));
    let cell = land_cell(&mut app, &[], 0.0);
    run_seconds(&mut app, 60.0);
    assert_eq!(hydration(&app, cell), 1.0);
    // Shallow water: the same.
    app.insert_resource(CurrentLevel(LevelDef::shallow_base()));
    run_seconds(&mut app, 30.0);
    assert_eq!(hydration(&app, cell), 1.0);
    // Land, but a creature: it does not dry (land-adapted, for now).
    let mut app = land_app();
    let npc = {
        let mut commands = app.world_mut().commands();
        spawn_npc(&mut commands, Archetype::Prey, Vec2::ZERO, None, 0)
    };
    app.world_mut().flush();
    run_seconds(&mut app, 60.0);
    assert_eq!(hydration(&app, npc), 1.0);
}

// --- The cuticle ---

#[test]
fn the_cuticle_lowers_water_loss_costs_lipids_and_never_stops_it() {
    let mut app = land_app();
    let plain = land_cell(&mut app, &[], 0.0);
    let waxy = land_cell(&mut app, &[BlockKind::Cuticle], 0.0);
    set_enzymes(&mut app, waxy, BlockKind::Cuticle, 2);
    run_seconds(&mut app, 8.0);
    let shield = app.world().get::<CuticleShield>(waxy).unwrap().0;
    assert!(shield > 0.2 && shield < cuticle::MAX_PROTECTION, "{shield}");
    assert_eq!(app.world().get::<CuticleShield>(plain).unwrap().0, 0.0);
    assert!(hydration(&app, waxy) > hydration(&app, plain) + 0.05, "{} against {}", hydration(&app, waxy), hydration(&app, plain));
    assert!(pool(&app, waxy, Currency::Lipids) < pool(&app, plain, Currency::Lipids) - 0.3, "it costs lipids");
    // It does not stop the loss: the pure rule dries a cell with the best shield too.
    assert!(desiccation::water_loss_rate(cuticle::MAX_PROTECTION) > 0.0);
    assert!(desiccation::hydration_after(1.0, 0.0, 10.0, 5000.0) < 0.05);
    // Without lipids the block stops, and so does the shield.
    set_pools(&mut app, waxy, &[(Currency::Lipids, 0.0)]);
    app.world_mut().get_mut::<Genome>(waxy).unwrap().table.insert(BlockKind::LipidMetabolism, GeneState::Silent);
    sim_step(&mut app);
    assert!(app.world().get::<CuticleShield>(waxy).unwrap().0 < shield);
}

#[test]
fn a_mutated_cuticle_gene_half_works_and_a_silent_one_does_nothing() {
    let shield = |state: GeneState| {
        let mut app = land_app();
        let cell = land_cell(&mut app, &[BlockKind::Cuticle], 0.0);
        set_enzymes(&mut app, cell, BlockKind::Cuticle, 3);
        app.world_mut().get_mut::<Genome>(cell).unwrap().table.insert(BlockKind::Cuticle, state);
        run_seconds(&mut app, 3.0);
        app.world().get::<CuticleShield>(cell).unwrap().0
    };
    let active = shield(GeneState::Expressed);
    let mutated = shield(GeneState::Mutated);
    assert!(active > mutated && mutated > 0.0);
    assert_eq!(shield(GeneState::Silent), 0.0);
}

#[test]
fn the_cuticle_is_a_cheap_gene_with_a_block() {
    assert!(metabolistic3d::blocks::block_spec(BlockKind::Cuticle).is_some());
    assert_eq!(metabolistic3d::prepare::gene_cost(BlockKind::Cuticle), 1);
    assert_eq!(metabolistic3d::prepare::gene_cost(BlockKind::Respiration), 1);
    assert_eq!(metabolistic3d::prepare::gene_cost(BlockKind::Multicellularity), 0);
    assert!(cuticle::protection(0.0) == 0.0);
    assert!(cuticle::protection(1000.0) <= cuticle::MAX_PROTECTION);
}

// --- Drag ---

#[test]
fn dry_ground_is_much_heavier_than_water_and_other_fields_are_unchanged() {
    assert_eq!(FieldKind::Vents.drag_multiplier(0.0), 1.0);
    assert_eq!(FieldKind::Shallow.drag_multiplier(0.0), 1.0);
    let dry = FieldKind::Land.drag_multiplier(0.0);
    let wet = FieldKind::Land.drag_multiplier(10.0);
    assert!(dry >= 2.0 && wet < 1.4 && dry > 1.8 * wet, "{dry} against {wet}");
    // The field of the level at two positions.
    let def = LevelDef::land_base();
    let grid = def.grid();
    let size = CellSize::default();
    let speed = |at: Vec2, shield: f32| {
        let multiplier = def.field.drag_multiplier(grid.sample(Channel::Water, at));
        BASE_THRUST / (body_mass(&size) * drag_in_field(body_drag(&size), multiplier, shield))
    };
    let in_puddle = speed(metabolistic3d::land::CUTICLE_PREY, 0.0);
    let on_plain = speed(Vec2::new(-330.0, -180.0), 0.0);
    assert!(on_plain < 0.5 * in_puddle, "{on_plain} against {in_puddle}");
    // A cuticle adds a little drag only.
    let with_cuticle = speed(metabolistic3d::land::CUTICLE_PREY, 0.9);
    assert!(with_cuticle < in_puddle && with_cuticle > 0.75 * in_puddle);
}

// --- The waxy wall ---

#[test]
fn a_waxy_wall_is_solid_for_cells_and_stops_water_but_not_o2() {
    let bare = Proteome::default();
    assert!(solid_for(WallKind::Waxy, &bare, false));
    assert!(!solid_for(WallKind::Waxy, &bare, true), "an adhesin crawl passes it like any wall");
    assert!(!WallKind::Waxy.blocks_grid());
    // A 12 x 3 grid, water and O2 on the left, a waxy column in the middle.
    let mut grid = EnvGrid::centered(12, 3);
    for channel in Channel::ALL {
        grid.set_params(channel, FieldParams { diffusion: 1.0, ..FieldParams::STILL });
    }
    for y in 0..3 {
        grid.set_channel_leak(Channel::Water, 6, y, metabolistic3d::walls::WAXY_WATER_FACTOR);
        for x in 0..6 {
            grid.set_level(Channel::Water, x, y, 10.0);
            grid.set_level(Channel::O2, x, y, 10.0);
        }
    }
    let water = grid.total(Channel::Water);
    let o2 = grid.total(Channel::O2);
    for _ in 0..120 {
        grid.step(1.0);
    }
    let right = |grid: &EnvGrid, channel: Channel| (7..12).map(|x| grid.level(channel, x, 1)).sum::<f32>();
    assert!(right(&grid, Channel::Water) < 1e-4, "water stays on its side");
    assert!(right(&grid, Channel::O2) > 3.0, "O2 goes through");
    assert_abs_diff_eq!(grid.total(Channel::Water), water, epsilon = 1e-2);
    assert_abs_diff_eq!(grid.total(Channel::O2), o2, epsilon = 1e-2);
}

// --- The land level ---

#[test]
fn land_levels_are_valid_over_two_hundred_seeds() {
    let mut retries = 0;
    for seed in 0..200u64 {
        let (def, attempts) = generate_land_counted(seed);
        assert_eq!(validate(&def), Ok(()), "seed {seed}");
        assert_eq!(def.field, FieldKind::Land);
        assert_eq!(def.gate_gene, BlockKind::Multicellularity);
        assert!(def.uv_fraction > 0.4 && def.air_o2 > 0.0 && !def.wet.is_empty());
        retries += u32::from(attempts > 1);
    }
    assert!(retries <= 10, "{retries} retries");
}

#[test]
fn the_land_level_uses_every_wall_kind_and_its_gate_is_in_a_wet_waxy_room() {
    let def = LevelDef::land_base();
    let kinds: BTreeSet<_> = def.wall_tiles().iter().map(|(_, k)| format!("{k:?}")).collect();
    for kind in ["Wall", "Pore", "Mat", "Tough", "Waxy"] {
        assert!(kinds.contains(kind), "{kind} is missing: {kinds:?}");
    }
    assert_eq!(def.chamber.wall, WallKind::Waxy);
    let grid = def.grid();
    assert!(grid.sample(Channel::Water, def.gate_position) > 3.0, "the gate room is wet");
    assert!(grid.sample(Channel::O2, def.gate_position) > 2.0, "and has air");
    assert!(grid.sample(Channel::O2, def.start) > 3.0);
    assert!(grid.sample(Channel::Water, def.start) < 3.0, "the start is not in the water");
    // Light from above, the same everywhere; UV is half.
    assert_abs_diff_eq!(grid.sample(Channel::Light, def.start), grid.sample(Channel::Light, Vec2::new(500.0, -400.0)), epsilon = 0.3);
    assert_eq!(def.uv_fraction, 0.5);
    // Little carbon on dry ground: every food zone is in or next to a wet zone.
    for (p, _) in &def.food_zones {
        let near = def.wet.iter().map(|w| w.distance(*p)).fold(f32::INFINITY, f32::min);
        assert!(near < 60.0, "{p} is {near} from water");
    }
    // The map and the grid agree on the walls.
    let map: WallMap = def.wall_map();
    for ((x, y), kind) in map.tiles() {
        assert_eq!(grid.is_wall(x, y), kind.blocks_grid());
    }
}

// --- The key gene ---

fn cuticle_slots() -> NpcSlots {
    NpcSlots(vec![
        (Archetype::Prey, Vec2::new(900.0, 0.0), Some(BlockKind::Cuticle)),
        (Archetype::Prey, Vec2::new(900.0, 100.0), Some(BlockKind::Cuticle)),
        (Archetype::Prey, Vec2::new(900.0, 200.0), Some(BlockKind::Cuticle)),
        (Archetype::Prey, Vec2::new(-900.0, 0.0), Some(BlockKind::Catalase)),
        (Archetype::Predator, Vec2::new(-900.0, 100.0), None),
    ])
}

fn timed_app() -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
    app.insert_resource(UvFraction(0.0));
    app.update();
    app
}

fn count_carriers(app: &mut App, gene: BlockKind) -> usize {
    let mut q = app.world_mut().query::<(&SpawnSlot, &Genome)>();
    q.iter(app.world()).filter(|(_, g)| g.get_gene_state(&gene).is_some()).count()
}

fn kill_all_slots(app: &mut App) {
    let doomed: Vec<Entity> = app.world_mut().query::<(Entity, &SpawnSlot)>().iter(app.world()).map(|(e, _)| e).collect();
    for e in doomed {
        app.world_mut().despawn(e);
    }
}

#[test]
fn cuticle_prey_are_topped_up_until_the_player_has_a_cuticle() {
    let mut app = timed_app();
    let player = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    // The level sets the rule (the world does it from the `LevelDef`).
    let key = LevelDef::land_base().key_gene.unwrap();
    assert_eq!(key.gene, BlockKind::Cuticle);
    app.insert_resource(KeyGeneRule(Some((key.gene, key.count))));
    app.insert_resource(cuticle_slots());
    {
        let slots = app.world().resource::<NpcSlots>().clone();
        let mut commands = app.world_mut().commands();
        for i in 0..3 {
            spawn_slot(&mut commands, &slots, i);
        }
    }
    app.world_mut().flush();
    assert_eq!(count_carriers(&mut app, BlockKind::Cuticle), 3);
    kill_all_slots(&mut app);
    for _ in 0..200 {
        app.update();
    }
    assert_eq!(count_carriers(&mut app, BlockKind::Cuticle), 3, "topped up to 3");
    assert_eq!(count_carriers(&mut app, BlockKind::Catalase), 0, "the catalase prey is no carrier here");
    // With the gene, no top-up.
    {
        let mut genome = app.world_mut().get_mut::<Genome>(player).unwrap();
        genome.add_gene(BlockKind::Cuticle);
        genome.express_gene(BlockKind::Cuticle);
    }
    kill_all_slots(&mut app);
    for _ in 0..40 {
        app.update();
    }
    assert_eq!(count_carriers(&mut app, BlockKind::Cuticle), 0, "no top-up once the player has a cuticle");
}

#[test]
fn a_cuticle_prey_carries_the_cuticle_block_and_engulfing_it_gives_the_gene() {
    let genome = metabolistic3d::npc::npc_genome(Archetype::Prey, Some(BlockKind::Cuticle));
    assert!(genome.get_gene_state(&BlockKind::Cuticle).is_some());
    let level = LevelDef::land_base();
    let carriers: Vec<_> = level.npcs.iter().filter(|n| n.signature == Some(BlockKind::Cuticle)).collect();
    assert_eq!(carriers.len(), level.key_gene.unwrap().count);
    for n in carriers {
        assert_eq!(n.archetype, Archetype::Prey);
        assert!(n.position.distance(level.key_gene.unwrap().centre) < metabolistic3d::layout::KEY_ZONE_RADIUS);
    }
}

// --- The stage chain ---

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("metabolistic3d-land-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn go(app: &mut App, state: GameState) {
    app.world_mut().resource_mut::<NextState<GameState>>().set(state);
    frames(app, 2);
}

fn state(app: &App) -> GameState {
    app.world().resource::<State<GameState>>().get().clone()
}

#[test]
fn the_run_goes_from_the_vents_through_shallow_water_to_land_and_then_completes() {
    let dir = TempDir::new("chain");
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
    app.insert_resource(CodexStore { path: Some(codex_path(&dir.0)) });
    app.update();
    *app.world_mut().resource_mut::<RunState>() = RunState::begin(11, BTreeSet::new());

    let expected = [
        (Stage::Vents, BlockKind::LightCapture),
        (Stage::ShallowWater, BlockKind::Respiration),
        (Stage::Land, BlockKind::Multicellularity),
    ];
    for (index, (stage, gene)) in expected.into_iter().enumerate() {
        go(&mut app, GameState::Run);
        let run = app.world().resource::<RunState>().clone();
        assert_eq!(run.stage(), stage);
        let def = run.level_def().expect("every stage has a level");
        assert_eq!(def.gate_gene, gene);
        app.insert_resource(CurrentLevel(def));
        // The player, from the carry, and the gate cell in contact (forced win).
        let carry = run.cell_carry();
        {
            let mut commands = app.world_mut().commands();
            spawn_cell_from_carry(&mut commands, Vec2::ZERO, carry);
        }
        app.world_mut().flush();
        let gate = {
            let mut commands = app.world_mut().commands();
            spawn_npc(&mut commands, Archetype::Gate, Vec2::new(47.0, 0.0), None, 0)
        };
        app.world_mut().flush();
        app.world_mut().entity_mut(gate).insert(GateProgress::default());
        // 3 s of contact + the victory clip. Stop when the stage ends: in the
        // game the world is gone then, but this test keeps its own entities.
        for _ in 0..60 {
            app.update();
            if state(&app) != GameState::Run {
                break;
            }
        }
        // The gate cell still stands in this test, so it can win once more in the
        // frame of the state change (in the game it is despawned with the world).
        app.world_mut().remove_resource::<metabolistic3d::run::PendingVictory>();
        if index < 2 {
            assert_eq!(state(&app), GameState::StageComplete, "stage {index}");
            let run = app.world().resource::<RunState>();
            let carry = run.cell_carry();
            assert_eq!(carry.genome.get_gene_state(&gene), Some(&GeneState::Expressed), "stage {index} {gene:?}");
            if gene == BlockKind::Respiration {
                assert_eq!(carry.proteome.count(ProteinTarget::Enzyme(BlockKind::Respiration)), 1, "the free enzyme");
            }
            // Clear the world of this stage: the next one starts clean.
            let doomed: Vec<Entity> = app
                .world_mut()
                .query_filtered::<Entity, Or<(With<Cell>, With<metabolistic3d::player::Player>)>>()
                .iter(app.world())
                .collect();
            for e in doomed {
                app.world_mut().despawn(e);
            }
        } else {
            assert_eq!(state(&app), GameState::RunOver);
            let summary = *app.world().resource::<RunSummary>();
            assert!(summary.run_complete, "winning land completes the run");
            assert_eq!(summary.victory, Some(BlockKind::Multicellularity));
            let message = metabolistic3d::run::run_over_message(&summary);
            assert!(message.contains("Multicellularity") && !message.contains("More stages"), "{message}");
        }
    }
    // The genes of the three gates are in the codex.
    let run = app.world().resource::<RunState>();
    assert!(!run.active, "the run is finished");
    let codex = app.world().resource::<Codex>();
    for gene in [BlockKind::LightCapture, BlockKind::Respiration, BlockKind::Multicellularity] {
        assert!(codex.genes.contains(&gene), "{gene:?}");
    }
}

#[test]
fn the_land_gate_reward_has_no_block_so_it_costs_nothing_and_adds_no_enzyme() {
    let mut carry = starter_carry(&BTreeSet::new());
    metabolistic3d::run::grant_gate_enzyme(&mut carry, BlockKind::Multicellularity);
    assert_eq!(carry.proteome.count(ProteinTarget::Enzyme(BlockKind::Multicellularity)), 0);
}

/// Prints the tuning numbers for the roadmap. Run with
/// `cargo th --test land_test print_land_tuning -- --ignored --nocapture`.
#[test]
#[ignore]
fn print_land_tuning() {
    for enzymes in [1u32, 2, 3] {
        for catalase in [0u32, 1] {
            let mut app = land_app();
            let extra: Vec<BlockKind> = if catalase > 0 { vec![BlockKind::Catalase] } else { vec![] };
            let cell = respiring_cell(&mut app, &extra);
            set_enzymes(&mut app, cell, BlockKind::Respiration, enzymes);
            app.insert_resource(land_air());
            run_seconds(&mut app, 30.0);
            let atp0 = pool(&app, cell, Currency::ATP);
            run_seconds(&mut app, 20.0);
            let atp1 = pool(&app, cell, Currency::ATP);
            eprintln!(
                "respiration x{enzymes} catalase {catalase}: ATP/s {:.2}  O2 {:.2}  ROS {:.2}  carbon {:.1}",
                (atp1 - atp0) / 20.0,
                pool(&app, cell, Currency::O2),
                pool(&app, cell, Currency::ROS),
                pool(&app, cell, Currency::CarbonSkeletons),
            );
        }
    }
    for enzymes in [0u32, 1, 2, 4] {
        let mut app = land_app();
        let kinds: Vec<BlockKind> = if enzymes > 0 { vec![BlockKind::Cuticle] } else { vec![] };
        let cell = land_cell(&mut app, &kinds, 0.0);
        if enzymes > 0 {
            set_enzymes(&mut app, cell, BlockKind::Cuticle, enzymes);
        }
        let mut seconds = 0.0;
        while hydration(&app, cell) > desiccation::DAMAGE_THRESHOLD && seconds < 300.0 {
            sim_step(&mut app);
            seconds += 0.2;
        }
        eprintln!(
            "cuticle x{enzymes}: shield {:.2}  {seconds:.0} s on dry ground to the damage threshold",
            app.world().get::<CuticleShield>(cell).unwrap().0
        );
    }
    let size = CellSize::default();
    for (name, multiplier) in [("water", 1.0), ("dry", FieldKind::Land.drag_multiplier(0.0))] {
        let speed = BASE_THRUST / (body_mass(&size) * drag_in_field(body_drag(&size), multiplier, 0.0));
        eprintln!("crawl speed in {name}: {speed:.0} px/s (a flagellum doubles the thrust)");
    }
}
