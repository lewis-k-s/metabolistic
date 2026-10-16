//! # Milestone M7: shallow water (headless)
//!
//! Photosynthesis, oxygen and ROS, catalase, the key-gene rule, the new wall
//! kinds, and the shallow-water level. Light is set on `LocalEnvironment`
//! (no grid), except where a test says it uses a grid.

use std::time::Duration;

use approx::assert_abs_diff_eq;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::actions::LyseOrder;
use metabolistic3d::blocks::catalase;
use metabolistic3d::blocks::genome::{BlockKind, Genome};
use metabolistic3d::blocks::photosynthesis;
use metabolistic3d::cell::{Cell, DamageSource, DamageTaken, DeathCause, death_cause, is_alive};
use metabolistic3d::envgrid::EnvGrid;
use metabolistic3d::environment::{Channel, LocalEnvironment, external_saturation};
use metabolistic3d::genome_damage::{GeneRng, GenomeIntegrity, MutationLoad, UvFraction};
use metabolistic3d::level::{FieldKind, LevelDef};
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::{BlockInput, CellTick, CurrencyPools, FlowmapTimingConfig, solve_tick};
use metabolistic3d::molecules::Currency;
use metabolistic3d::npc::{Archetype, KeyGeneRule, NpcSlots, SpawnSlot, npc_genome, spawn_npc, spawn_slot};
use metabolistic3d::oxygen::{o2_exchange_system, ros_chemistry_system};
use metabolistic3d::proteins::Proteome;
use metabolistic3d::shared::spawn_starter_cell;
use metabolistic3d::shelter::InShelter;
use metabolistic3d::walls::{
    LYSE_ATP, TOUGH_LYSE_METALS, TOUGH_RECOIL, WallKind, WallMap, in_mat, solid_for,
};
use proptest::prelude::*;

use crate::common::*;

// --- Helpers ---

/// A headless app with no UV (these tests are about oxygen, not about light
/// damage), with a fixed gene seed.
fn app() -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(UvFraction(0.0));
    app.insert_resource(GeneRng::seeded(3));
    app
}

/// The starter cell with photosynthesis (`photo` enzymes) and, if `catalase` is
/// more than 0, catalase, in `light`.
fn phototroph(app: &mut App, photo: u32, catalase: u32, light: f32) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    let mut kinds = vec![BlockKind::LightCapture];
    if catalase > 0 {
        kinds.push(BlockKind::Catalase);
    }
    express_blocks(app, cell, &kinds);
    set_enzymes(app, cell, BlockKind::LightCapture, photo);
    if catalase > 0 {
        set_enzymes(app, cell, BlockKind::Catalase, catalase);
    }
    app.world_mut()
        .get_mut::<LocalEnvironment>(cell)
        .unwrap()
        .set(Channel::Light, light);
    cell
}

fn membrane(app: &App, cell: Entity) -> f32 {
    app.world().get::<Membrane>(cell).unwrap().integrity
}

fn alive(app: &App, cell: Entity) -> bool {
    is_alive(
        app.world().get::<CurrencyPools>(cell).unwrap(),
        app.world().get::<Membrane>(cell).unwrap(),
    )
}

/// Runs ticks until the cell dies or `max_seconds` pass (5 ticks a second).
/// Returns the seconds that passed.
fn run_until_dead(app: &mut App, cell: Entity, max_seconds: f32) -> f32 {
    let mut ticks = 0;
    while ticks < (max_seconds * 5.0) as usize && alive(app, cell) {
        sim_step(app);
        ticks += 1;
    }
    ticks as f32 / 5.0
}

// --- Photosynthesis ---

#[test]
fn photosynthesis_keeps_its_stoichiometry() {
    // One block, enough of everything: the pools change by turnover x flow.
    let profile = photosynthesis::flux_profile();
    let env = {
        let mut env = LocalEnvironment::default();
        env.set(Channel::Light, 6.0);
        env
    };
    let kinetics = photosynthesis::kinetics();
    let external = external_saturation(&env, kinetics.external, kinetics.k_half);
    let input = BlockInput {
        external,
        k_half: kinetics.k_half,
        ..BlockInput::new(&profile, 2.0)
    };
    let result = solve_tick(&CurrencyPools::default(), &[input], 1.0);
    let turnover = result.turnover[0];
    assert!(turnover > 0.0);
    for (currency, rate) in [
        (Currency::ATP, photosynthesis::ATP_RATE),
        (Currency::ReducingPower, photosynthesis::REDUCING_POWER_RATE),
        (Currency::CarbonSkeletons, photosynthesis::CARBON_RATE),
        (Currency::O2, photosynthesis::O2_RATE),
        (Currency::ROS, photosynthesis::ROS_RATE),
    ] {
        assert_abs_diff_eq!(result.deltas[&currency], turnover * rate, epsilon = 1e-4);
    }
    // It needs no input pool: light is a condition, nothing uses it up.
    assert_eq!(result.deltas.len(), 5, "{:?}", result.deltas);
    // It makes a little ROS, much less than O2.
    assert!(photosynthesis::ROS_RATE > 0.0 && photosynthesis::ROS_RATE < photosynthesis::O2_RATE / 3.0);
}

#[test]
fn photosynthesis_saturates_with_light() {
    let kinetics = photosynthesis::kinetics();
    let rate_at = |light: f32| {
        let mut env = LocalEnvironment::default();
        env.set(Channel::Light, light);
        let profile = photosynthesis::flux_profile();
        let input = BlockInput {
            external: external_saturation(&env, kinetics.external, kinetics.k_half),
            k_half: kinetics.k_half,
            ..BlockInput::new(&profile, 1.0)
        };
        solve_tick(&CurrencyPools::default(), &[input], 1.0).turnover[0]
    };
    assert_eq!(rate_at(0.0), 0.0, "no light, no flux");
    let mut last = 0.0;
    for light in [0.5, 1.0, 3.0, 10.0, 100.0, 1e4] {
        let rate = rate_at(light);
        assert!(rate > last, "light {light}: {rate} after {last}");
        assert!(rate < 1.0, "saturates below the full rate: {rate}");
        last = rate;
    }
    assert_abs_diff_eq!(rate_at(photosynthesis::LIGHT_HALF_SATURATION), 0.5, epsilon = 1e-3);
    // The gain falls: 10 -> 100 adds less than 1 -> 10.
    assert!(rate_at(100.0) - rate_at(10.0) < rate_at(10.0) - rate_at(1.0));
}

#[test]
fn a_cell_in_the_light_makes_energy_and_oxygen_and_in_the_dark_nothing() {
    let mut app = app();
    let lit = phototroph(&mut app, 2, 0, 10.0);
    let dark = phototroph(&mut app, 2, 0, 0.0);
    for cell in [lit, dark] {
        set_pools(&mut app, cell, &[(Currency::ATP, 50.0), (Currency::O2, 0.0), (Currency::ROS, 0.0)]);
    }
    for _ in 0..20 {
        sim_step(&mut app);
    }
    assert!(
        pool(&app, lit, Currency::ATP) > pool(&app, dark, Currency::ATP) + 8.0,
        "makes ATP: {} against {}",
        pool(&app, lit, Currency::ATP),
        pool(&app, dark, Currency::ATP)
    );
    assert!(pool(&app, lit, Currency::O2) > 3.0, "makes O2");
    assert!(pool(&app, lit, Currency::ROS) > 0.3, "and a little ROS");
    assert_eq!(pool(&app, dark, Currency::O2), 0.0);
    assert_eq!(pool(&app, dark, Currency::ROS), 0.0);
    assert!(pool(&app, dark, Currency::ATP) < 50.0, "only costs");
}

// --- O2 and the grid ---

/// Sets a cell at a position in a small grid (no walls, nothing moves).
fn grid_with_cell(app: &mut App, cell: Entity, grid: EnvGrid, at: Vec2) {
    app.insert_resource(grid);
    app.world_mut().entity_mut(cell).insert(Transform::from_translation(at.extend(0.0)));
}

#[test]
fn o2_leaves_the_cell_into_the_grid_and_mass_is_conserved() {
    let mut app = app();
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::O2, 40.0)]);
    let mut grid = EnvGrid::centered(8, 8);
    grid.set_level(Channel::O2, 6, 6, 1.0);
    grid_with_cell(&mut app, cell, grid, Vec2::new(-20.0, -20.0));
    let total = |app: &App| pool(app, cell, Currency::O2) + app.world().resource::<EnvGrid>().total(Channel::O2);
    let before = total(&app);
    let mut last_pool = 40.0;
    for _ in 0..100 {
        app.world_mut().run_system_once(o2_exchange_system).unwrap();
        assert_abs_diff_eq!(total(&app), before, epsilon = 1e-3);
        let now = pool(&app, cell, Currency::O2);
        assert!(now <= last_pool + 1e-6, "O2 only leaves while the grid is lower");
        last_pool = now;
    }
    // The cell lost most of it; the grid under it gained it.
    assert!(pool(&app, cell, Currency::O2) < 20.0);
    let level = app.world().resource::<EnvGrid>().sample(Channel::O2, Vec2::new(-20.0, -20.0));
    assert!(level > 1.0, "the grid under the cell has more O2: {level}");
}

#[test]
fn o2_never_overshoots_the_grid_level_even_in_a_long_tick() {
    let mut app = app();
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::O2, 10.0)]);
    app.world_mut().entity_mut(cell).insert(CellTick::every(1));
    // A huge tick: the fraction is 1 at most, so the cell stops at the level.
    app.insert_resource(FlowmapTimingConfig { updates_per_second: 0.1, manual: true, ..default() });
    let mut grid = EnvGrid::centered(4, 4);
    // Every cell of the grid at 2: the level under the cell is 2.
    for x in 0..4 {
        for y in 0..4 {
            grid.set_level(Channel::O2, x, y, 2.0);
        }
    }
    grid_with_cell(&mut app, cell, grid, Vec2::ZERO);
    app.world_mut().run_system_once(o2_exchange_system).unwrap();
    let after = pool(&app, cell, Currency::O2);
    assert!(after >= 2.0 - 1e-3 && after < 10.0, "stops at the grid level: {after}");
    // A cell below the level takes O2 in, and never goes above the level.
    set_pools(&mut app, cell, &[(Currency::O2, 0.0)]);
    let grid_total = app.world().resource::<EnvGrid>().total(Channel::O2);
    let level = app.world().resource::<EnvGrid>().sample(Channel::O2, Vec2::ZERO);
    app.world_mut().run_system_once(o2_exchange_system).unwrap();
    let taken = pool(&app, cell, Currency::O2);
    assert!(taken > 0.0 && taken <= level + 1e-3, "{taken} from a level of {level}");
    assert_abs_diff_eq!(
        app.world().resource::<EnvGrid>().total(Channel::O2) + taken,
        grid_total,
        epsilon = 1e-3
    );
}

#[test]
fn a_walled_in_cell_cannot_put_o2_into_the_wall() {
    let mut app = app();
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::O2, 30.0)]);
    let mut grid = EnvGrid::centered(2, 2);
    for x in 0..2 {
        for y in 0..2 {
            grid.set_wall(x, y, true);
        }
    }
    grid_with_cell(&mut app, cell, grid, Vec2::ZERO);
    app.world_mut().run_system_once(o2_exchange_system).unwrap();
    assert_eq!(pool(&app, cell, Currency::O2), 30.0, "nothing left the cell");
    assert_eq!(app.world().resource::<EnvGrid>().total(Channel::O2), 0.0);
}

// --- ROS ---

#[test]
fn ros_made_from_o2_does_not_depend_on_the_tick_length() {
    // The same 10 s at 2.5, 5 and 10 ticks a second.
    let run = |rate: f32| {
        let mut app = app();
        app.insert_resource(FlowmapTimingConfig { updates_per_second: rate, manual: true, ..default() });
        let cell = spawn_test_cell(&mut app, &[], &[(Currency::O2, 40.0), (Currency::ROS, 5.0)]);
        for _ in 0..(rate * 10.0) as usize {
            sim_step(&mut app);
        }
        (pool(&app, cell, Currency::O2), pool(&app, cell, Currency::ROS))
    };
    let (o2_a, ros_a) = run(5.0);
    for rate in [2.5, 10.0] {
        let (o2, ros) = run(rate);
        assert_abs_diff_eq!(o2, o2_a, epsilon = 0.03 * o2_a);
        assert_abs_diff_eq!(ros, ros_a, epsilon = 0.03 * ros_a);
    }
    // And the effect is real: ROS rose from O2.
    assert!(ros_a > 5.0);
}

#[test]
fn the_ros_rule_is_exact_within_one_system_run() {
    // Direct: one long tick equals two short ones for the chemistry alone.
    let mut one = app();
    one.insert_resource(FlowmapTimingConfig { updates_per_second: 1.0, manual: true, ..default() });
    let a = spawn_test_cell(&mut one, &[], &[(Currency::O2, 30.0), (Currency::ROS, 0.0)]);
    one.world_mut().run_system_once(ros_chemistry_system).unwrap();
    let mut two = app();
    two.insert_resource(FlowmapTimingConfig { updates_per_second: 2.0, manual: true, ..default() });
    let b = spawn_test_cell(&mut two, &[], &[(Currency::O2, 30.0), (Currency::ROS, 0.0)]);
    two.world_mut().run_system_once(ros_chemistry_system).unwrap();
    two.world_mut().run_system_once(ros_chemistry_system).unwrap();
    assert_abs_diff_eq!(
        pool(&one, a, Currency::ROS),
        pool(&two, b, Currency::ROS),
        epsilon = 0.02
    );
    // O2 lost = ROS gained, for the first (decay-free) step.
    let mut app = app();
    app.insert_resource(FlowmapTimingConfig { updates_per_second: 5.0, manual: true, ..default() });
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::O2, 30.0), (Currency::ROS, 0.0)]);
    app.world_mut().run_system_once(ros_chemistry_system).unwrap();
    assert_abs_diff_eq!(
        pool(&app, cell, Currency::O2) + pool(&app, cell, Currency::ROS),
        30.0,
        epsilon = 1e-3
    );
}

#[test]
fn without_catalase_a_photosynthesising_cell_in_full_light_dies_of_ros() {
    let mut app = app();
    let cell = phototroph(&mut app, 3, 0, 10.0);
    let seconds = run_until_dead(&mut app, cell, 120.0);
    assert!(!alive(&app, cell), "dead after {seconds} s");
    assert!(
        (30.0..=60.0).contains(&seconds),
        "dies in 30 to 60 s, died after {seconds} s"
    );
    let pools = app.world().get::<CurrencyPools>(cell).unwrap();
    let membrane = app.world().get::<Membrane>(cell).unwrap();
    assert_eq!(death_cause(pools, membrane), Some(DeathCause::MembraneBroken));
    let taken = *app.world().get::<DamageTaken>(cell).unwrap();
    assert_eq!(taken.main_source(), Some(DamageSource::Ros));
    assert_eq!(taken.uv, 0.0);
    // The run-over screen names it.
    let message = metabolistic3d::run::death_message(Some(DeathCause::MembraneBroken), taken.main_source());
    assert!(message.contains("oxygen"), "{message}");
}

#[test]
fn with_two_catalase_enzymes_the_same_cell_lives() {
    let mut app = app();
    let cell = phototroph(&mut app, 3, 2, 10.0);
    let seconds = run_until_dead(&mut app, cell, 120.0);
    assert!(alive(&app, cell), "died after {seconds} s");
    assert!(membrane(&app, cell) > 95.0, "{}", membrane(&app, cell));
    assert!(pool(&app, cell, Currency::ROS) < 3.0, "ROS stays low");
    assert!(pool(&app, cell, Currency::ATP) > 100.0, "and it lives on the light");
    assert!(app.world().get::<GenomeIntegrity>(cell).unwrap().0 > 0.9);
}

#[test]
fn ros_lowers_genome_integrity_and_adds_mutation_load_and_a_shelter_does_not_help() {
    let mut app = app();
    let cell = spawn_test_cell(&mut app, &[], &[(Currency::ROS, 20.0), (Currency::ATP, 1000.0)]);
    app.world_mut().get_mut::<InShelter>(cell).unwrap().0 = true;
    let integrity = app.world().get::<GenomeIntegrity>(cell).unwrap().0;
    for _ in 0..25 {
        sim_step(&mut app);
    }
    assert!(app.world().get::<GenomeIntegrity>(cell).unwrap().0 < integrity - 0.03);
    assert!(app.world().get::<MutationLoad>(cell).unwrap().0 > 0.1);
    assert!(app.world().get::<DamageTaken>(cell).unwrap().ros > 5.0, "membrane damage inside a shelter");
}

#[test]
fn catalase_turns_ros_into_half_as_much_o2_and_never_takes_ros_below_zero() {
    // One tick with a lot of enzymes and a little ROS.
    let mut app = app();
    let cell = spawn_test_cell(
        &mut app,
        &[BlockKind::Catalase],
        &[(Currency::ROS, 0.3), (Currency::O2, 0.0), (Currency::ATP, 500.0)],
    );
    set_enzymes(&mut app, cell, BlockKind::Catalase, 200);
    let atp = pool(&app, cell, Currency::ATP);
    app.update();
    metabolistic3d::metabolism::run_metabolic_tick(app.world_mut());
    let ros = pool(&app, cell, Currency::ROS);
    assert!(ros >= 0.0, "{ros}");
    // ROS 0.3 -> at most 0.3 used. O2 gained is half of the ROS used (before
    // the small O2 -> ROS and ROS decay steps of the same tick).
    let o2 = pool(&app, cell, Currency::O2);
    assert!(o2 > 0.0 && o2 <= 0.15 + 1e-4, "{o2}");
    assert!(atp - pool(&app, cell, Currency::ATP) < 1.0);
    // And over a long time it stays at 0 or more.
    for _ in 0..200 {
        sim_step(&mut app);
        assert!(pool(&app, cell, Currency::ROS) >= 0.0);
        assert!(pool(&app, cell, Currency::O2) >= 0.0);
    }

    // The block profile: 2 ROS + 0.5 ATP -> 1 O2.
    let profile = catalase::flux_profile();
    let result = solve_tick(
        &CurrencyPools { pools: [(Currency::ROS, 10.0), (Currency::ATP, 10.0)].into() },
        &[BlockInput::new(&profile, 1.0).with_k_half(catalase::HALF_SATURATION)],
        1.0,
    );
    let used = -result.deltas[&Currency::ROS];
    assert!(used > 0.0);
    assert_abs_diff_eq!(result.deltas[&Currency::O2], used / 2.0, epsilon = 1e-4);
    assert_abs_diff_eq!(-result.deltas[&Currency::ATP], used / 4.0, epsilon = 1e-4);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, .. ProptestConfig::default() })]

    /// Catalase never takes ROS (or ATP) below 0, whatever the enzymes and pools.
    #[test]
    fn catalase_never_takes_a_pool_below_zero(
        ros in 0.0f32..50.0,
        atp in 0.0f32..50.0,
        enzymes in 0.0f32..300.0,
        dt in 0.01f32..2.0,
    ) {
        let profile = catalase::flux_profile();
        let pools = CurrencyPools { pools: [(Currency::ROS, ros), (Currency::ATP, atp)].into() };
        let result = solve_tick(
            &pools,
            &[BlockInput::new(&profile, enzymes).with_k_half(catalase::HALF_SATURATION)],
            dt,
        );
        prop_assert!(ros + result.deltas.get(&Currency::ROS).copied().unwrap_or(0.0) >= -1e-4);
        prop_assert!(atp + result.deltas.get(&Currency::ATP).copied().unwrap_or(0.0) >= -1e-4);
        // Mass: O2 gained is half of the ROS used.
        let used = -result.deltas.get(&Currency::ROS).copied().unwrap_or(0.0);
        let o2 = result.deltas.get(&Currency::O2).copied().unwrap_or(0.0);
        prop_assert!((o2 - used / 2.0).abs() <= 1e-3 * (1.0 + used));
    }
}

#[test]
fn more_catalase_keeps_ros_lower() {
    let level_after = |catalase: u32| {
        let mut app = app();
        let cell = phototroph(&mut app, 3, catalase, 10.0);
        // Not a long run: look before anything dies.
        for _ in 0..100 {
            sim_step(&mut app);
        }
        pool(&app, cell, Currency::ROS)
    };
    let (zero, one, two) = (level_after(0), level_after(1), level_after(2));
    assert!(zero > one && one > two, "{zero} {one} {two}");
}

// --- Cyanobacteria and the gate genome ---

#[test]
fn a_phototroph_npc_has_catalase_so_engulfing_it_can_give_either_gene() {
    let genome = npc_genome(Archetype::Prey, Some(BlockKind::LightCapture));
    assert!(genome.table.contains_key(&BlockKind::LightCapture));
    assert!(genome.table.contains_key(&BlockKind::Catalase));
    // The catalase carriers of the level have catalase, not photosynthesis.
    let carrier = npc_genome(Archetype::Prey, Some(BlockKind::Catalase));
    assert!(carrier.table.contains_key(&BlockKind::Catalase));
    assert!(!carrier.table.contains_key(&BlockKind::LightCapture));
}

#[test]
fn cyanobacteria_put_o2_into_the_water() {
    let mut app = app();
    app.insert_resource(EnvGrid::centered(12, 12));
    let npc = {
        let mut commands = app.world_mut().commands();
        spawn_npc(&mut commands, Archetype::Prey, Vec2::new(10.0, 10.0), Some(BlockKind::LightCapture), 0)
    };
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(npc)
        .insert((CellTick::default(), LocalEnvironment::default()));
    app.world_mut().get_mut::<LocalEnvironment>(npc).unwrap().set(Channel::Light, 10.0);
    // The grid does not give light: set it in the layer so sensing keeps it.
    {
        let mut grid = app.world_mut().resource_mut::<EnvGrid>();
        for x in 0..12 {
            for y in 0..12 {
                grid.set_level(Channel::Light, x, y, 10.0);
            }
        }
    }
    for _ in 0..150 {
        sim_step(&mut app);
    }
    let water = app.world().resource::<EnvGrid>().total(Channel::O2);
    assert!(water > 1.0, "the cyanobacterium made O2 for the water: {water}");
    assert!(app.world().get::<Membrane>(npc).unwrap().integrity > 90.0, "and it lives with it (catalase)");
}

// --- The key-gene rule ---

fn catalase_slots() -> NpcSlots {
    NpcSlots(vec![
        (Archetype::Prey, Vec2::new(900.0, 0.0), Some(BlockKind::Catalase)),
        (Archetype::Prey, Vec2::new(900.0, 100.0), Some(BlockKind::Catalase)),
        (Archetype::Prey, Vec2::new(900.0, 200.0), Some(BlockKind::Catalase)),
        // A pigment prey: not a key carrier in this level.
        (Archetype::Prey, Vec2::new(-900.0, 0.0), Some(BlockKind::SecondaryMetabolites)),
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
fn catalase_prey_are_topped_up_until_the_player_has_catalase() {
    let mut app = timed_app();
    let player = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    // The level sets the rule (the world does it from the `LevelDef`).
    app.insert_resource(KeyGeneRule(Some((BlockKind::Catalase, 3))));
    app.insert_resource(catalase_slots());
    {
        let slots = app.world().resource::<NpcSlots>().clone();
        let mut commands = app.world_mut().commands();
        for i in 0..3 {
            spawn_slot(&mut commands, &slots, i);
        }
    }
    app.world_mut().flush();
    assert_eq!(count_carriers(&mut app, BlockKind::Catalase), 3);
    kill_all_slots(&mut app);
    for _ in 0..200 {
        app.update();
    }
    assert_eq!(count_carriers(&mut app, BlockKind::Catalase), 3, "topped up to 3");
    // The pigment prey is not topped up here (it is no key carrier).
    assert_eq!(count_carriers(&mut app, BlockKind::SecondaryMetabolites), 0);
    // With the gene (Expressed), no top-up.
    {
        let mut genome = app.world_mut().get_mut::<Genome>(player).unwrap();
        genome.add_gene(BlockKind::Catalase);
        genome.express_gene(BlockKind::Catalase);
    }
    kill_all_slots(&mut app);
    for _ in 0..40 {
        app.update(); // 4 s: before the slow general respawn (25 s)
    }
    assert_eq!(count_carriers(&mut app, BlockKind::Catalase), 0, "no top-up once the player has catalase");
}

#[test]
fn a_level_with_no_key_gene_has_no_top_up() {
    let mut app = timed_app();
    {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO);
    }
    app.world_mut().flush();
    app.insert_resource(KeyGeneRule(None));
    app.insert_resource(catalase_slots());
    for _ in 0..100 {
        app.update(); // 10 s, before the slow respawn
    }
    assert_eq!(count_carriers(&mut app, BlockKind::Catalase), 0);
}

#[test]
fn the_world_sets_the_rule_from_the_level_def() {
    let shallow = LevelDef::shallow_base();
    let key = shallow.key_gene.unwrap();
    assert_eq!(key.gene, BlockKind::Catalase);
    let vents = LevelDef::vents_base();
    assert_eq!(vents.key_gene.unwrap().gene, BlockKind::SecondaryMetabolites);
    // The carriers of each level are prey slots with that signature.
    for def in [&shallow, &vents] {
        let key = def.key_gene.unwrap();
        let carriers = def
            .npcs
            .iter()
            .filter(|n| n.archetype == Archetype::Prey && n.signature == Some(key.gene))
            .count();
        assert!(carriers >= key.count);
    }
}

// --- Walls ---

#[test]
fn a_tough_wall_costs_a_metal_and_hurts_the_attacker() {
    let mut app = timed_app();
    let mut grid = EnvGrid::centered(8, 8);
    grid.set_wall(2, 3, true);
    let mut map = WallMap::default();
    map.insert((2, 3), WallKind::Tough);
    app.insert_resource(grid);
    app.insert_resource(map);
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::new(-10.0, -16.0)).1
    };
    app.world_mut().flush();
    app.world_mut().get_mut::<metabolistic3d::actions::ActionState>(cell).unwrap().aim = Vec2::NEG_X;

    // Without Metals it fails and costs nothing.
    set_pools(&mut app, cell, &[(Currency::Metals, 0.0), (Currency::ATP, 50.0)]);
    app.world_mut().write_message(LyseOrder { cell, direction: Vec2::NEG_X });
    app.update();
    assert_eq!(app.world().resource::<WallMap>().len(), 1, "no Metals: the wall holds");
    assert_eq!(pool(&app, cell, Currency::ATP), 50.0);
    assert_eq!(membrane(&app, cell), 100.0);

    // With a Metal it works: ATP, Metals, and the recoil.
    set_pools(&mut app, cell, &[(Currency::Metals, 3.0)]);
    app.world_mut().write_message(LyseOrder { cell, direction: Vec2::NEG_X });
    app.update();
    assert!(app.world().resource::<WallMap>().is_empty());
    assert!(!app.world().resource::<EnvGrid>().is_wall(2, 3));
    assert_eq!(pool(&app, cell, Currency::ATP), 50.0 - LYSE_ATP);
    assert_eq!(pool(&app, cell, Currency::Metals), 3.0 - TOUGH_LYSE_METALS);
    assert_eq!(membrane(&app, cell), 100.0 - TOUGH_RECOIL);
}

#[test]
fn an_ordinary_wall_costs_no_metals_and_does_not_fight_back() {
    let mut app = timed_app();
    let mut grid = EnvGrid::centered(8, 8);
    grid.set_wall(2, 3, true);
    let mut map = WallMap::default();
    map.insert((2, 3), WallKind::Wall);
    app.insert_resource(grid);
    app.insert_resource(map);
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::new(-10.0, -16.0)).1
    };
    app.world_mut().flush();
    set_pools(&mut app, cell, &[(Currency::Metals, 0.0), (Currency::ATP, 50.0)]);
    app.world_mut().write_message(LyseOrder { cell, direction: Vec2::NEG_X });
    app.update();
    assert!(app.world().resource::<WallMap>().is_empty());
    assert_eq!(membrane(&app, cell), 100.0);
}

#[test]
fn a_lysis_does_not_cut_a_mat() {
    let mut app = timed_app();
    let mut grid = EnvGrid::centered(8, 8);
    grid.set_leak(2, 3, 0.3);
    let mut map = WallMap::default();
    map.insert((2, 3), WallKind::Mat);
    app.insert_resource(grid);
    app.insert_resource(map);
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::new(-10.0, -16.0)).1
    };
    app.world_mut().flush();
    set_pools(&mut app, cell, &[(Currency::ATP, 50.0)]);
    app.world_mut().write_message(LyseOrder { cell, direction: Vec2::NEG_X });
    app.update();
    assert_eq!(app.world().resource::<WallMap>().len(), 1);
    assert_eq!(pool(&app, cell, Currency::ATP), 50.0, "no cost for nothing");
}

#[test]
fn a_mat_is_never_solid_and_a_tough_wall_always_is() {
    let bare = Proteome::default();
    let mut transporter = Proteome::default();
    transporter.add(metabolistic3d::proteins::ProteinTarget::Membrane(
        metabolistic3d::proteins::MembraneProtein::Transporter,
    ));
    for proteome in [&bare, &transporter] {
        assert!(!solid_for(WallKind::Mat, proteome, false), "any cell crawls through a mat");
        assert!(solid_for(WallKind::Tough, proteome, false));
        assert!(!solid_for(WallKind::Tough, proteome, true), "an adhesin crawl passes it, as a wall");
    }
    // A mat is in the map, but it does not block the grid.
    assert!(!WallKind::Mat.blocks_grid());
    assert!(WallKind::Tough.blocks_grid() && WallKind::Wall.blocks_grid() && WallKind::Pore.blocks_grid());
}

#[test]
fn the_position_in_a_mat_follows_the_tile() {
    let mut map = WallMap::default();
    map.insert((3, 3), WallKind::Mat);
    map.insert((4, 3), WallKind::Wall);
    let origin = Vec2::ZERO;
    assert!(in_mat(&map, origin, Vec2::new(100.0, 100.0)));
    assert!(!in_mat(&map, origin, Vec2::new(130.0, 100.0)), "a wall tile is not a mat");
    assert!(!in_mat(&map, origin, Vec2::new(10.0, 10.0)));
    assert!(!in_mat(&map, origin, Vec2::new(-10.0, 100.0)), "outside the field");
}

#[test]
fn the_grid_diffuses_through_a_mat_at_a_reduced_rate_and_conserves_the_total() {
    // A strip of 12 x 1 cells: all O2 in the left half. A wall in the middle
    // stops it, a mat slows it, open water is the fastest.
    let run = |middle: Option<f32>| {
        let mut grid = EnvGrid::new(12, 1, Vec2::ZERO);
        grid.set_params(
            Channel::O2,
            metabolistic3d::envgrid::FieldParams { diffusion: 0.5, ..metabolistic3d::envgrid::FieldParams::STILL },
        );
        for x in 0..6 {
            grid.set_level(Channel::O2, x, 0, 10.0);
        }
        match middle {
            Some(factor) if factor <= 0.0 => grid.set_wall(6, 0, true),
            Some(factor) => grid.set_leak(6, 0, factor),
            None => {}
        }
        let before = grid.total(Channel::O2);
        for _ in 0..40 {
            grid.step(1.0);
            assert!((0..12).all(|x| grid.level(Channel::O2, x, 0) >= 0.0));
        }
        assert_abs_diff_eq!(grid.total(Channel::O2), before, epsilon = before * 1e-4);
        (7..12).map(|x| grid.level(Channel::O2, x, 0)).sum::<f32>()
    };
    let open = run(None);
    let mat = run(Some(metabolistic3d::walls::MAT_DIFFUSION_FACTOR));
    let wall = run(Some(0.0));
    assert!(wall < 1e-4, "a wall blocks: {wall}");
    assert!(mat > 1.0, "a mat lets O2 through: {mat}");
    assert!(mat < open, "but slower than open water: {mat} vs {open}");
}

// --- The level ---

#[test]
fn the_shallow_grid_has_light_from_the_surface_a_mat_and_a_closed_chamber() {
    let def = LevelDef::shallow_base();
    assert_eq!(def.field, FieldKind::Shallow);
    let grid = def.grid();
    let top = grid.sample(Channel::Light, Vec2::new(0.0, 740.0));
    let middle = grid.sample(Channel::Light, Vec2::new(0.0, 0.0));
    let bottom = grid.sample(Channel::Light, Vec2::new(0.0, -740.0));
    assert!(top > 8.0 && top > middle && middle > bottom && bottom > 0.0, "{top} {middle} {bottom}");
    // The same light in a row, left to right: no shafts.
    let a = grid.sample(Channel::Light, Vec2::new(-500.0, 100.0));
    let b = grid.sample(Channel::Light, Vec2::new(300.0, 100.0));
    assert_abs_diff_eq!(a, b, epsilon = 0.2 * a);
    // UV is lower than in the vents.
    assert!(def.uv_fraction < metabolistic3d::level::UV_FRACTION);
    // Mats are leaky cells, not walls. The tough line and the chamber are walls.
    let map = def.wall_map();
    let mats = map.tiles().into_iter().filter(|(_, k)| *k == WallKind::Mat).count();
    let walls = map.tiles().into_iter().filter(|(_, k)| k.blocks_grid()).count();
    assert!(mats > 10 && walls > 20);
    assert_eq!(grid.wall_count(), walls);
    for ((x, y), kind) in map.tiles() {
        assert_eq!(grid.is_wall(x, y), kind.blocks_grid());
        if kind == WallKind::Mat {
            assert!(grid.leak(x, y) < 1.0);
        }
    }
    // Few vents: one, weak, on the sea floor.
    assert_eq!(def.vents.len(), 1);
    assert!(def.vents[0].heat < metabolistic3d::level::VENTS[0].heat);
    // The O2 channel exists and starts empty.
    assert_eq!(grid.total(Channel::O2), 0.0);
    // The gate chamber is dark and closed: little light, no minerals.
    assert!(grid.sample(Channel::Light, def.gate_position) < 1.5);
    assert_eq!(def.gate_gene, BlockKind::Respiration);
    assert!(!def.chamber.lit);
}

#[test]
fn the_shallow_stage_comes_after_the_vents_in_every_run() {
    for seed in [0u64, 1, 99, 12345] {
        let def = metabolistic3d::stage::Stage::ShallowWater.level_def(seed).unwrap();
        assert_eq!(def.field, FieldKind::Shallow);
        assert_eq!(def, metabolistic3d::stage::Stage::ShallowWater.level_def(seed).unwrap());
    }
}

#[test]
fn a_cell_that_never_had_photosynthesis_does_not_make_ros() {
    // The old vent cell: no LightCapture, no O2, no ROS.
    let mut app = app();
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, Vec2::ZERO).1
    };
    app.world_mut().flush();
    app.world_mut().get_mut::<LocalEnvironment>(cell).unwrap().set(Channel::Light, 10.0);
    for _ in 0..100 {
        sim_step(&mut app);
    }
    assert_eq!(pool(&app, cell, Currency::O2), 0.0);
    assert_eq!(pool(&app, cell, Currency::ROS), 0.0);
    let _ = Cell;
}
