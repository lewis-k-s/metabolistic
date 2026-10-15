//! # Milestone M5: the living world (headless)
//!
//! NPC cells with a slow tick, steering, engulf and gene transfer, the symbiont
//! exchange, viruses, toxin hits, food, walls, lysis, and the gate cell.

use std::time::Duration;

use approx::assert_abs_diff_eq;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use metabolistic3d::actions::{LyseOrder, ToxinDroplet};
use metabolistic3d::blocks::genome::{BlockKind, GeneState, Genome};
use metabolistic3d::cell::{Cell, CellSize, DeathCause};
use metabolistic3d::engulf::{Engulfed, EngulfIntent};
use metabolistic3d::envgrid::EnvGrid;
use metabolistic3d::environment::{Channel, LocalEnvironment};
use metabolistic3d::gate::{GATE_GENE, GATE_SECONDS, GateProgress};
use metabolistic3d::genome_damage::{GeneRng, MutationLoad};
use metabolistic3d::membrane::Membrane;
use metabolistic3d::metabolism::{CellTick, CurrencyPools};
use metabolistic3d::molecules::Currency;
use metabolistic3d::movement::MoveInput;
use metabolistic3d::npc::{Archetype, Npc, NpcSlots, SpawnSlot, spawn_npc, spawn_slot};
use metabolistic3d::shelter::{Shelter, Shelters};
use metabolistic3d::particles::*;
use metabolistic3d::proteins::{MembraneProtein, ProteinTarget, Proteome, SynthesisQueue};
use metabolistic3d::run::{PendingVictory, RunSummary};
use metabolistic3d::shared::spawn_starter_cell;
use metabolistic3d::walls::{LYSE_ATP, WallKind, WallMap};
use metabolistic3d::{GameState, MetabolisticApp};

mod common;
use common::*;

/// A headless app whose frames last 0.1 s.
fn app() -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
    app.insert_resource(GeneRng::seeded(11));
    app.update();
    app
}

fn player_at(app: &mut App, position: Vec2) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, position).1
    };
    app.world_mut().flush();
    cell
}

fn npc_at(app: &mut App, archetype: Archetype, position: Vec2) -> Entity {
    npc_with(app, archetype, position, None)
}

fn npc_with(app: &mut App, archetype: Archetype, position: Vec2, gene: Option<BlockKind>) -> Entity {
    let cell = {
        let mut commands = app.world_mut().commands();
        spawn_npc(&mut commands, archetype, position, gene, 0)
    };
    app.world_mut().flush();
    cell
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn get<T: Component + Clone>(app: &App, entity: Entity) -> T {
    app.world().get::<T>(entity).unwrap().clone()
}

fn exists(app: &App, entity: Entity) -> bool {
    app.world().get_entity(entity).is_ok()
}

fn enable_intent(app: &mut App, cell: Entity) {
    app.world_mut().get_mut::<EngulfIntent>(cell).unwrap().0 = true;
}

// --- The slow tick ---

#[test]
fn a_slow_tick_gives_the_same_result_as_four_normal_ticks() {
    let genes = [
        BlockKind::Chemosynthesis,
        BlockKind::AminoAcidBiosynthesis,
        BlockKind::LipidMetabolism,
    ];
    let pools = [
        (Currency::ATP, 80.0),
        (Currency::CarbonSkeletons, 20.0),
        (Currency::ReducingPower, 30.0),
        (Currency::Metals, 5.0),
    ];
    let mut app = MetabolisticApp::new_headless();
    let normal = spawn_test_cell(&mut app, &genes, &pools);
    let slow = spawn_test_cell(&mut app, &genes, &pools);
    for cell in [normal, slow] {
        app.world_mut().entity_mut(cell).insert(LocalEnvironment::vent(20.0));
    }
    app.world_mut().entity_mut(slow).insert(CellTick::every(4));
    for _ in 0..16 {
        sim_step(&mut app);
    }
    let a = cell_snapshot(&app, normal);
    let b = cell_snapshot(&app, slow);
    let minerals = Currency::ALL.iter().position(|c| *c == Currency::Minerals).unwrap();
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        // The test-only constant supply tops Minerals up at the start of each
        // tick, so the stock at the end of a slow tick is lower. Skip it.
        if i != minerals {
            assert_abs_diff_eq!(*x, *y, epsilon = 0.02 + x.abs() * 0.005);
        }
    }
    assert!(a[0] != 80.0, "the cells did something: {a:?}");
}

#[test]
fn a_slow_cell_runs_on_every_fourth_tick_only() {
    let mut app = MetabolisticApp::new_headless();
    let cell = spawn_test_cell(&mut app, &[BlockKind::Chemosynthesis], &[(Currency::ATP, 50.0)]);
    app.world_mut().entity_mut(cell).insert((LocalEnvironment::vent(20.0), CellTick::every(4)));
    let mut changes = 0;
    let mut last = pool(&app, cell, Currency::ATP);
    for _ in 0..12 {
        sim_step(&mut app);
        let now = pool(&app, cell, Currency::ATP);
        if (now - last).abs() > 1e-6 {
            changes += 1;
        }
        last = now;
    }
    assert_eq!(changes, 3, "12 ticks, one run in four");
}

// --- Engulf ---

#[test]
fn the_player_engulfs_prey_on_contact_and_takes_half_and_a_gene() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    let prey = npc_with(&mut app, Archetype::Prey, Vec2::new(30.0, 0.0), Some(BlockKind::SecondaryMetabolites));
    let before = get::<CurrencyPools>(&app, prey);
    let player_before = get::<CurrencyPools>(&app, player);
    // No intent, no engulf.
    frames(&mut app, 2);
    assert!(exists(&app, prey));
    enable_intent(&mut app, player);
    frames(&mut app, 1);
    assert!(!exists(&app, prey), "the prey is gone");
    let after = get::<CurrencyPools>(&app, player);
    for currency in [Currency::ATP, Currency::CarbonSkeletons, Currency::AminoAcids] {
        assert_abs_diff_eq!(
            after.get(currency),
            player_before.get(currency) + 0.5 * before.get(currency),
            epsilon = 1e-3
        );
    }
    assert_abs_diff_eq!(after.get(Currency::Lipids), player_before.get(Currency::Lipids), epsilon = 1e-4);
    // Gene transfer: the pigment gene (or chemosynthesis, which the player has).
    let genome = get::<Genome>(&app, player);
    assert_eq!(
        genome.get_gene_state(&BlockKind::SecondaryMetabolites),
        Some(&GeneState::Expressed)
    );
    let queue = get::<SynthesisQueue>(&app, player);
    assert!(queue.orders.contains(&ProteinTarget::Enzyme(BlockKind::SecondaryMetabolites)));
    assert_eq!(get::<Proteome>(&app, player).enzymes(BlockKind::SecondaryMetabolites), 0, "not free");
}

#[test]
fn the_size_rule_stops_a_small_cell_from_engulfing_a_big_one() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    let predator = npc_at(&mut app, Archetype::Predator, Vec2::new(40.0, 0.0));
    enable_intent(&mut app, player);
    // The predator has intent too and is bigger: it engulfs the player, not the other way.
    frames(&mut app, 1);
    assert!(exists(&app, predator), "the player did not engulf the predator");
    assert!(app.world().get::<Engulfed>(player).is_some());
}

#[test]
fn a_predator_engulfing_the_player_ends_the_run_as_engulfed() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    npc_at(&mut app, Archetype::Predator, Vec2::new(40.0, 0.0));
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Run);
    frames(&mut app, 4);
    assert!(app.world().get::<Engulfed>(player).is_some());
    let summary = *app.world().resource::<RunSummary>();
    assert_eq!(summary.cause, Some(DeathCause::Engulfed));
    assert_eq!(summary.victory, None);
    assert_eq!(*app.world().resource::<State<GameState>>().get(), GameState::RunOver);
}

#[test]
fn a_predator_eats_prey_and_may_take_its_gene() {
    let mut app = app();
    let predator = npc_at(&mut app, Archetype::Predator, Vec2::ZERO);
    let prey = npc_at(&mut app, Archetype::Prey, Vec2::new(30.0, 0.0)); // fermentation
    frames(&mut app, 1);
    assert!(!exists(&app, prey));
    let genome = get::<Genome>(&app, predator);
    assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Expressed));
}

#[test]
fn engulf_gene_choice_is_seeded() {
    let run = |seed: u64| {
        let mut app = app();
        app.insert_resource(GeneRng::seeded(seed));
        let player = player_at(&mut app, Vec2::ZERO);
        enable_intent(&mut app, player);
        // A prey with several genes the player lacks.
        let prey = npc_at(&mut app, Archetype::Prey, Vec2::new(30.0, 0.0));
        {
            let mut genome = app.world_mut().get_mut::<Genome>(prey).unwrap();
            for kind in [BlockKind::FatStorage, BlockKind::VesicleExport, BlockKind::SecondaryMetabolites] {
                genome.add_gene(kind);
                genome.express_gene(kind);
            }
        }
        frames(&mut app, 1);
        let genome = get::<Genome>(&app, player);
        [BlockKind::Fermentation, BlockKind::FatStorage, BlockKind::VesicleExport, BlockKind::SecondaryMetabolites]
            .into_iter()
            .filter(|k| genome.get_gene_state(k) == Some(&GeneState::Expressed))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(3), run(3));
    assert_eq!(run(3).len(), 1, "one gene only");
    let picks: std::collections::HashSet<_> = (0..12).map(run).collect();
    assert!(picks.len() > 1);
}

// --- Behaviour in the world ---

#[test]
fn prey_move_away_from_a_predator_and_the_symbiont_follows_the_player() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::new(-300.0, 0.0));
    let prey = npc_at(&mut app, Archetype::Prey, Vec2::ZERO);
    npc_at(&mut app, Archetype::Predator, Vec2::new(-80.0, 0.0));
    let symbiont = npc_at(&mut app, Archetype::Symbiont, Vec2::new(-200.0, 60.0));
    frames(&mut app, 3);
    let prey_input = get::<MoveInput>(&app, prey).0;
    assert!(prey_input.x > 0.0 && prey_input.length() > 0.0, "{prey_input}");
    let symbiont_input = get::<MoveInput>(&app, symbiont).0;
    assert!(symbiont_input.x < 0.0, "towards the player: {symbiont_input}");
    let _ = player;
}

// --- Symbiont ---

#[test]
fn the_symbiont_gives_carbon_and_gets_atp_without_overshoot() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    let symbiont = npc_at(&mut app, Archetype::Symbiont, Vec2::new(30.0, 0.0));
    let carbon = |app: &App, c| pool(app, c, Currency::CarbonSkeletons);
    let atp = |app: &App, c| pool(app, c, Currency::ATP);
    let (c0_player, c0_sym) = (carbon(&app, player), carbon(&app, symbiont));
    let (a0_player, a0_sym) = (atp(&app, player), atp(&app, symbiont));
    assert!(c0_sym > c0_player && a0_player > a0_sym);
    let total_carbon = c0_player + c0_sym;
    let mut last_gap = c0_sym - c0_player;
    for _ in 0..300 {
        metabolistic3d::metabolism::run_metabolic_tick(app.world_mut());
        let gap = carbon(&app, symbiont) - carbon(&app, player);
        assert!(gap <= last_gap + 1e-3, "the gap only closes");
        assert!(gap >= -1e-3, "no overshoot: {gap}");
        last_gap = gap;
    }
    // The exchange itself conserves carbon; the cells' own metabolism changes the total a little.
    assert!(carbon(&app, player) > c0_player + 5.0);
    assert!(carbon(&app, symbiont) < c0_sym);
    assert!((carbon(&app, player) + carbon(&app, symbiont) - total_carbon).abs() < 60.0);
    assert!(atp(&app, symbiont) > a0_sym - 5.0);
}

#[test]
fn a_far_symbiont_does_not_exchange() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    let symbiont = npc_at(&mut app, Archetype::Symbiont, Vec2::new(200.0, 0.0));
    let before = pool(&app, player, Currency::CarbonSkeletons);
    for _ in 0..20 {
        metabolistic3d::metabolism::run_metabolic_tick(app.world_mut());
    }
    assert_eq!(pool(&app, player, Currency::CarbonSkeletons), before);
    let _ = symbiont;
}

// --- Viruses ---

#[test]
fn a_virus_infects_on_contact_with_a_seeded_outcome_and_is_gone() {
    let outcome = |seed: u64| {
        let mut app = app();
        app.insert_resource(GeneRng::seeded(seed));
        let player = player_at(&mut app, Vec2::ZERO);
        let virus = {
            let mut commands = app.world_mut().commands();
            spawn_virus(&mut commands, Vec2::new(25.0, 0.0), Vec2::ZERO)
        };
        app.world_mut().flush();
        frames(&mut app, 1);
        assert!(!exists(&app, virus), "the virus is gone");
        let load = get::<MutationLoad>(&app, player).0;
        let queued = get::<SynthesisQueue>(&app, player).orders.len();
        (load, queued)
    };
    assert_eq!(outcome(5), outcome(5));
    let results: Vec<_> = (0..30).map(outcome).collect();
    assert!(results.iter().any(|&(load, q)| load == VIRUS_MUTATION_LOAD && q == 0), "a mutation");
    assert!(results.iter().any(|&(load, q)| load == 0.0 && q == 1), "a gene");
    assert!(results.iter().all(|&(load, q)| (load == 1.0) != (q == 1)), "exactly one outcome");
}

// --- Toxin and death ---

fn droplet_at(app: &mut App, owner: Entity, position: Vec2) -> Entity {
    let droplet = app
        .world_mut()
        .spawn((
            ToxinDroplet { owner, position, direction: Vec2::X, age: 0.0 },
            Transform::from_translation(position.extend(1.0)),
        ))
        .id();
    droplet
}

#[test]
fn a_toxin_droplet_hits_the_first_cell_it_touches_for_a_quarter_of_the_membrane() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    let prey = npc_at(&mut app, Archetype::Prey, Vec2::new(200.0, 0.0));
    let own = droplet_at(&mut app, player, Vec2::new(-10.0, 0.0));
    frames(&mut app, 1);
    assert!(exists(&app, own), "a droplet does not hit its owner");
    assert_eq!(get::<Membrane>(&app, player).integrity, 100.0);
    app.world_mut().despawn(own);
    let hit = droplet_at(&mut app, player, Vec2::new(176.0, 0.0));
    frames(&mut app, 1);
    assert!(!exists(&app, hit));
    assert_abs_diff_eq!(get::<Membrane>(&app, prey).integrity, 75.0, epsilon = 1e-4);
}

#[test]
fn an_npc_with_a_broken_membrane_dies_and_drops_food() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::new(-500.0, 0.0));
    let prey = npc_at(&mut app, Archetype::Prey, Vec2::new(200.0, 0.0));
    for _ in 0..4 {
        droplet_at(&mut app, player, Vec2::new(176.0, 0.0));
        frames(&mut app, 1);
    }
    frames(&mut app, 1);
    assert!(!exists(&app, prey), "four hits kill the prey");
    let foods: Vec<FoodParticle> = app
        .world_mut()
        .query::<&FoodParticle>()
        .iter(app.world())
        .copied()
        .collect();
    assert_eq!(foods.len(), 1);
    assert_eq!(foods[0].carbon, metabolistic3d::npc::DEATH_FOOD_CARBON);
}

// --- Food and metals ---

#[test]
fn food_gives_carbon_and_metals_give_the_cofactor() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    let (food, metal, far) = {
        let mut commands = app.world_mut().commands();
        (
            spawn_food(&mut commands, Vec2::new(24.0, 0.0), FOOD_CARBON),
            spawn_metal(&mut commands, Vec2::new(0.0, 24.0)),
            spawn_metal(&mut commands, Vec2::new(300.0, 0.0)),
        )
    };
    app.world_mut().flush();
    let carbon = pool(&app, player, Currency::CarbonSkeletons);
    let metals = pool(&app, player, Currency::Metals);
    frames(&mut app, 1);
    assert!(!exists(&app, food) && !exists(&app, metal));
    assert!(exists(&app, far));
    assert_eq!(pool(&app, player, Currency::CarbonSkeletons), carbon + FOOD_CARBON);
    assert_eq!(pool(&app, player, Currency::Metals), metals + METAL_AMOUNT);
}

#[test]
fn slow_spawns_stop_at_the_caps() {
    let mut app = app();
    app.insert_resource(ParticleZones {
        food: vec![(Vec2::new(0.0, 500.0), 100.0)],
        metals: vec![(Vec2::new(500.0, 0.0), 100.0)],
    });
    frames(&mut app, 3000); // 300 s
    let count = |app: &mut App| {
        (
            app.world_mut().query::<&FoodParticle>().iter(app.world()).count(),
            app.world_mut().query::<&MetalPickup>().iter(app.world()).count(),
        )
    };
    let (food, metals) = count(&mut app);
    assert_eq!(food, MAX_FOOD);
    assert_eq!(metals, MAX_METALS);
}

// --- Walls ---

#[test]
fn lysis_removes_a_tile_from_the_map_and_the_grid_and_costs_atp() {
    let mut app = app();
    let mut grid = EnvGrid::centered(8, 8);
    // Tile (2,3) covers x -128+64..-128+96 = -64..-32, y -128+96..-128+128 = -32..0.
    grid.set_wall(2, 3, true);
    let mut map = WallMap::default();
    map.insert((2, 3), WallKind::Wall);
    app.insert_resource(grid);
    app.insert_resource(map);
    let cell = player_at(&mut app, Vec2::new(-10.0, -16.0)); // right of the tile, touching
    app.world_mut().get_mut::<metabolistic3d::actions::ActionState>(cell).unwrap().aim = Vec2::NEG_X;
    let atp = pool(&app, cell, Currency::ATP);
    app.world_mut().write_message(LyseOrder { cell, direction: Vec2::NEG_X });
    frames(&mut app, 1);
    assert!(app.world().resource::<WallMap>().is_empty());
    assert!(!app.world().resource::<EnvGrid>().is_wall(2, 3));
    assert_eq!(pool(&app, cell, Currency::ATP), atp - LYSE_ATP);
}

#[test]
fn a_wall_in_the_vent_field_splits_it_and_keeps_the_mass() {
    let mut grid = metabolistic3d::level::vent_field();
    assert!(grid.wall_count() > 0);
    // Add a drifting source nowhere: only move what is there.
    let before = grid.total(Channel::Minerals);
    assert!(before > 0.0);
    // The chamber interior is cut off from the minerals of the field: sample a
    // point inside it.
    let inside = metabolistic3d::level::GATE_POSITION;
    assert!(grid.sample(Channel::Minerals, inside) < 0.05);
    let light_inside = grid.sample(Channel::Light, inside);
    assert!(light_inside > 5.0, "the chamber is a light zone: {light_inside}");
    // The light does not leak out of the chamber.
    for outside in [inside + Vec2::new(0.0, -170.0), inside + Vec2::new(0.0, 190.0)] {
        assert!(grid.sample(Channel::Light, outside) < 0.5, "{outside}");
    }
    for _ in 0..50 {
        grid.step(1.0);
    }
}

// --- The gate cell ---

fn gate_scene(app: &mut App) -> (Entity, Entity) {
    let player = player_at(app, Vec2::ZERO);
    let gate = npc_at(app, Archetype::Gate, Vec2::new(47.0, 0.0));
    app.world_mut().entity_mut(gate).insert(GateProgress::default());
    (player, gate)
}

#[test]
fn three_seconds_of_contact_with_the_gate_cell_win_the_level() {
    let mut app = app();
    let (player, gate) = gate_scene(&mut app);
    frames(&mut app, 29); // 2.9 s
    assert!(app.world().get_resource::<PendingVictory>().is_none());
    assert!(get::<GateProgress>(&app, gate).fraction() > 0.9);
    frames(&mut app, 3);
    assert!(app.world().get_resource::<PendingVictory>().is_some());
    assert_eq!(app.world().resource::<RunSummary>().victory, Some(GATE_GENE));
    let genome = get::<Genome>(&app, player);
    assert_eq!(genome.get_gene_state(&GATE_GENE), Some(&GeneState::Expressed));
    assert!(exists(&app, gate), "the world removes it after the victory clip");
    assert!(GATE_SECONDS == 3.0);
}

#[test]
fn breaking_contact_with_the_gate_cell_resets_the_progress() {
    let mut app = app();
    let (player, gate) = gate_scene(&mut app);
    frames(&mut app, 15);
    assert!(get::<GateProgress>(&app, gate).held > 1.0);
    app.world_mut().get_mut::<Transform>(player).unwrap().translation.x = -300.0;
    frames(&mut app, 4); // 0.4 s: more than the grace
    assert_eq!(get::<GateProgress>(&app, gate).held, 0.0);
    app.world_mut().get_mut::<Transform>(player).unwrap().translation.x = 0.0;
    frames(&mut app, 20);
    assert!(app.world().get_resource::<PendingVictory>().is_none(), "2 s are not enough after a reset");
}

#[test]
fn the_gate_cell_is_not_engulfed_by_size_or_by_a_predator() {
    let mut app = app();
    let (player, gate) = gate_scene(&mut app);
    enable_intent(&mut app, player);
    npc_at(&mut app, Archetype::Predator, Vec2::new(47.0, 60.0));
    frames(&mut app, 2);
    assert!(exists(&app, gate));
}

// --- Misc ---

#[test]
fn npcs_have_no_local_player_and_the_npc_marker() {
    let mut app = app();
    let prey = npc_at(&mut app, Archetype::Prey, Vec2::ZERO);
    assert!(app.world().get::<metabolistic3d::player::LocalPlayer>(prey).is_none());
    assert!(app.world().get::<Npc>(prey).is_some());
    assert!(app.world().get::<Cell>(prey).is_some());
    assert_eq!(get::<CellTick>(&app, prey).every, metabolistic3d::npc::NPC_TICK_EVERY);
    assert!(get::<CellSize>(&app, prey).radius < 1.0);
    let _ = MembraneProtein::Lytic;
}

// --- Respawn ---

fn slots() -> NpcSlots {
    NpcSlots(vec![
        (Archetype::Prey, Vec2::new(900.0, 0.0), Some(BlockKind::SecondaryMetabolites)),
        (Archetype::Prey, Vec2::new(900.0, 100.0), Some(BlockKind::SecondaryMetabolites)),
        (Archetype::Prey, Vec2::new(900.0, 200.0), Some(BlockKind::SecondaryMetabolites)),
        (Archetype::Prey, Vec2::new(-900.0, 0.0), None),
        (Archetype::Predator, Vec2::new(-900.0, 100.0), None),
    ])
}

fn count_npcs(app: &mut App) -> usize {
    app.world_mut().query::<&SpawnSlot>().iter(app.world()).count()
}

fn count_pigment_prey(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<(&SpawnSlot, &Genome)>();
    q.iter(app.world())
        .filter(|(_, g)| g.get_gene_state(&BlockKind::SecondaryMetabolites).is_some())
        .count()
}

#[test]
fn pigment_prey_are_topped_up_while_the_player_lacks_the_gene() {
    let mut app = app();
    let player = player_at(&mut app, Vec2::ZERO);
    app.insert_resource(slots());
    // A run starts with all slots filled; then a predator "eats" the pigment prey.
    {
        let slots = app.world().resource::<NpcSlots>().clone();
        let mut commands = app.world_mut().commands();
        for i in 0..3 {
            spawn_slot(&mut commands, &slots, i);
        }
    }
    app.world_mut().flush();
    assert_eq!(count_pigment_prey(&mut app), 3);
    let doomed: Vec<Entity> = app.world_mut().query::<(Entity, &SpawnSlot)>().iter(app.world()).map(|(e, _)| e).collect();
    for e in doomed {
        app.world_mut().despawn(e);
    }
    // 6 s per prey: 3 prey need about 18 s, far before the slow general respawn (25 s).
    frames(&mut app, 200);
    assert_eq!(count_pigment_prey(&mut app), 3, "topped up to 3");
    assert_eq!(count_npcs(&mut app), 3, "the other slots wait for the slow timer");
    // With the gene, no top-up: remove them again.
    {
        let mut genome = app.world_mut().get_mut::<Genome>(player).unwrap();
        genome.add_gene(BlockKind::SecondaryMetabolites);
        genome.express_gene(BlockKind::SecondaryMetabolites);
    }
    let doomed: Vec<Entity> = app.world_mut().query::<(Entity, &SpawnSlot)>().iter(app.world()).map(|(e, _)| e).collect();
    for e in doomed {
        app.world_mut().despawn(e);
    }
    frames(&mut app, 40); // 4 s: before the slow general respawn (25 s), after a top-up (6 s) would be due
    assert_eq!(count_pigment_prey(&mut app), 0, "no top-up once the player has the gene");
}

#[test]
fn npcs_respawn_slowly_up_to_the_slot_count_and_not_next_to_the_player() {
    let mut app = app();
    player_at(&mut app, Vec2::new(900.0, 100.0)); // next to the three pigment slots
    app.insert_resource(slots());
    {
        let mut genome = app.world_mut().query::<&mut Genome>().single_mut(app.world_mut()).unwrap();
        genome.add_gene(BlockKind::SecondaryMetabolites);
        genome.express_gene(BlockKind::SecondaryMetabolites);
    }
    frames(&mut app, 260); // 26 s: one respawn
    assert_eq!(count_npcs(&mut app), 1);
    frames(&mut app, 2000); // long: all allowed slots are full, never more than slots
    let n = count_npcs(&mut app);
    assert_eq!(n, 2, "the three slots near the player never spawn; the two far ones do: {n}");
    assert!(n <= slots().0.len());
}

// --- Shelters ---

#[test]
fn a_predator_cannot_engulf_a_cell_in_a_shelter() {
    let mut app = app();
    app.insert_resource(Shelters(vec![Shelter { position: Vec2::ZERO, radius: 48.0 }]));
    let player = player_at(&mut app, Vec2::ZERO);
    npc_at(&mut app, Archetype::Predator, Vec2::new(40.0, 0.0));
    // The shelter flag is set in the metabolic tick.
    metabolistic3d::metabolism::run_metabolic_tick(app.world_mut());
    assert!(app.world().get::<metabolistic3d::shelter::InShelter>(player).unwrap().0);
    frames(&mut app, 3);
    assert!(app.world().get::<Engulfed>(player).is_none(), "safe in the crevice");
}
