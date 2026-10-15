//! # CPU cost of NPC cells (ignored benchmark)
//!
//! Run with
//!
//! ```text
//! cargo th --release --test npc_benchmark -- --ignored --nocapture
//! ```
//!
//! (A debug build is slower. The dev profile of this repo is optimized, so
//! `cargo th --test npc_benchmark -- --ignored --nocapture` also gives useful
//! numbers.) It spawns 20, 50 and 100 NPC cells in the vent field and prints:
//!
//! - the time of one metabolic tick (`run_metabolic_tick`), and the average
//!   over the 4-tick NPC cycle (NPCs run on every 4th tick);
//! - the time of one frame of behaviour (`app.update()`: steering, engulf,
//!   contacts, particles), with the game at 60 frames a second;
//! - the cost per frame of the game: 5 ticks a second spread over 60 frames,
//!   plus one frame of behaviour.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use metabolistic3d::MetabolisticApp;
use metabolistic3d::level;
use metabolistic3d::metabolism::run_metabolic_tick;
use metabolistic3d::npc::{Archetype, spawn_npc};
use metabolistic3d::shared::spawn_starter_cell;

fn build(npcs: usize, tick_every: Option<u32>) -> App {
    let mut app = MetabolisticApp::new_headless();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_micros(16_667)));
    app.insert_resource(level::vent_field());
    app.insert_resource(level::wall_map());
    {
        let mut commands = app.world_mut().commands();
        spawn_starter_cell(&mut commands, level::START_POSITION);
        let kinds = [Archetype::Prey, Archetype::Prey, Archetype::Symbiont, Archetype::Predator];
        for i in 0..npcs {
            // A spread over the field, away from the walls.
            let x = -900.0 + (i % 20) as f32 * 90.0;
            let y = -600.0 + (i / 20) as f32 * 110.0 + (i % 7) as f32 * 9.0;
            spawn_npc(&mut commands, kinds[i % kinds.len()], Vec2::new(x, y), None, i as u32);
        }
    }
    app.world_mut().flush();
    if let Some(every) = tick_every {
        let mut ticks = app.world_mut().query::<&mut metabolistic3d::metabolism::CellTick>();
        for mut tick in ticks.iter_mut(app.world_mut()) {
            tick.every = every;
        }
    }
    for _ in 0..5 {
        app.update(); // spawn the blocks
    }
    let cells = app.world_mut().query::<&metabolistic3d::cell::Cell>().iter(app.world()).count();
    let blocks = app.world_mut().query::<&metabolistic3d::metabolism::MetabolicNode>().iter(app.world()).count();
    eprintln!("({cells} cells, {blocks} blocks)");
    app
}

fn per_call(mut f: impl FnMut(), calls: u32) -> Duration {
    f(); // warm up
    let start = Instant::now();
    for _ in 0..calls {
        f();
    }
    start.elapsed() / calls
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
#[ignore]
fn npc_cost() {
    eprintln!("\nNPCs | tick (avg of 4) | tick (worst)  | frame (behaviour) | game frame (ms)");
    for npcs in [20, 50, 100] {
        let mut app = build(npcs, None);
        // One NPC cycle is 4 ticks: time 40 cycles, and each tick on its own.
        let mut times = [Duration::ZERO; 4];
        for cycle in 0..40 {
            for slot in 0..4 {
                let start = Instant::now();
                run_metabolic_tick(app.world_mut());
                if cycle >= 8 {
                    times[slot] += start.elapsed();
                }
            }
        }
        let each: Vec<f64> = times.iter().map(|t| ms(*t / 32)).collect();
        let average = each.iter().sum::<f64>() / 4.0;
        let worst = each.iter().cloned().fold(0.0, f64::max);
        let frame = per_call(|| app.update(), 300);
        // The game runs 5 metabolic ticks a second on 60 frames a second.
        let game_frame = ms(frame) + average * 5.0 / 60.0;
        eprintln!(
            "{npcs:>4} | {average:>10.3} ms   | {worst:>9.3} ms | {:>12.3} ms    | {game_frame:.3}",
            ms(frame)
        );
    }
    // For comparison: the same 50 NPCs on every tick.
    let mut app = build(50, Some(1));
    let tick = per_call(|| run_metabolic_tick(app.world_mut()), 100);
    eprintln!("50 NPCs on every tick (no slow tick): {:.3} ms per tick", ms(tick));
}
