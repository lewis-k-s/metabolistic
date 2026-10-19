//! # Look gallery (dev)
//!
//! `METABOLISTIC_LOOK_GALLERY=1` with a scenario launch spawns one cell of each
//! `cell_look` round the player on the first Run screen, so a screenshot shows
//! every body side by side. It is the review tool for the shader bodies (plan:
//! `docs/npc-shader-cells-plan.md`). Use it with the offscreen frames:
//!
//! ```bash
//! METABOLISTIC_SCENARIO=vent_motile METABOLISTIC_LOOK_GALLERY=1 METABOLISTIC_OFFSCREEN_SECS=10 cargo run
//! ```
//!
//! One prey has a damaged membrane (30 %), and one prey dies after 4 s, so
//! the frames show the thin rim and the lysis (a shot 0.3 s after the death
//! goes to `target/gallery/lysis.png`). The predator does not engulf.

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};

use crate::GameState;
use crate::blocks::genome::BlockKind;
use crate::cell_look::{BodyLook, CellLook};
use crate::engulf::EngulfIntent;
use crate::membrane::Membrane;
use crate::npc::{Archetype, spawn_npc};
use crate::player::{ControlledCell, LocalPlayer};

/// Seconds after which the doomed prey dies.
const DEATH_AT: f32 = 4.0;
/// Distance of the ring of cells from the player, in pixels.
const RING: f32 = 95.0;

pub fn requested() -> bool {
    std::env::var_os("METABOLISTIC_LOOK_GALLERY").is_some_and(|v| !v.is_empty() && v != "0")
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (spawn_gallery, doom).run_if(in_state(GameState::Run)));
}


/// The prey that dies at `DEATH_AT`.
#[derive(Component)]
struct Doomed;

/// The cells of the gallery: archetype, signature and an optional look.
fn entries() -> Vec<(Archetype, Option<BlockKind>, Option<CellLook>)> {
    vec![
        (Archetype::Prey, None, None),
        (Archetype::Predator, None, None),
        (Archetype::Symbiont, None, None),
        (Archetype::Gate, Some(BlockKind::LightCapture), None),
        (Archetype::Gate, None, Some(CellLook::gate_proteobacterium())),
        (Archetype::Prey, Some(BlockKind::Slime), None),
        (Archetype::Prey, Some(BlockKind::Microcin), None),
    ]
}

fn spawn_gallery(
    mut done: Local<bool>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&Transform>,
    mut commands: Commands,
) {
    if *done {
        return;
    }
    let Some(centre) = players.iter().find_map(|c| cells.get(c.0).ok()).map(|t| t.translation.truncate()) else {
        return;
    };
    *done = true;
    let entries = entries();
    let n = entries.len() + 1;
    for (i, (archetype, signature, look)) in entries.into_iter().enumerate() {
        let angle = i as f32 / n as f32 * std::f32::consts::TAU;
        let at = centre + RING * Vec2::new(angle.cos(), angle.sin() * 0.9);
        let entity = spawn_npc(&mut commands, archetype, at, signature, 100 + i as u32);
        commands.entity(entity).insert(EngulfIntent(false));
        if let Some(look) = look {
            commands.entity(entity).insert(BodyLook(look));
        }
    }
    // A damaged prey.
    let angle = (n - 1) as f32 / n as f32 * std::f32::consts::TAU;
    let at = centre + RING * Vec2::new(angle.cos(), angle.sin() * 0.9);
    let entity = spawn_npc(&mut commands, Archetype::Prey, at, None, 120);
    commands.entity(entity).insert(Membrane { integrity: 30.0, max: 100.0 });
    info!("look gallery: spawned {} cells round {centre}", n);
}

/// Seconds after the death at which the gallery saves a shot of the lysis
/// (`target/gallery/lysis.png`, or `METABOLISTIC_GALLERY_DIR`).
const LYSIS_SHOT_AFTER: f32 = 0.3;

/// Seconds before its death at which the doomed prey appears next to the
/// player (so it has no time to swim out of view).
const DOOMED_LEAD: f32 = 0.5;

#[allow(clippy::too_many_arguments)]
fn doom(
    time: Res<Time>,
    mut start: Local<Option<f32>>,
    mut step: Local<u8>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&Transform>,
    mut doomed: Query<&mut Membrane, With<Doomed>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let since = now - *start.get_or_insert(now);
    if *step == 0 && since >= DEATH_AT - DOOMED_LEAD {
        let Some(centre) = players.iter().find_map(|c| cells.get(c.0).ok()).map(|t| t.translation.truncate()) else {
            return;
        };
        *step = 1;
        let entity = spawn_npc(&mut commands, Archetype::Prey, centre + Vec2::new(45.0, 0.0), None, 121);
        commands.entity(entity).insert(Doomed);
    } else if *step == 1 && since >= DEATH_AT {
        *step = 2;
        for mut membrane in &mut doomed {
            membrane.integrity = 0.0;
        }
    } else if *step == 2 && since >= DEATH_AT + LYSIS_SHOT_AFTER {
        *step = 3;
        let dir = std::env::var("METABOLISTIC_GALLERY_DIR").unwrap_or_else(|_| "target/gallery".into());
        let _ = std::fs::create_dir_all(&dir);
        let path = std::path::PathBuf::from(dir).join("lysis.png");
        info!("look gallery: lysis shot to {}", path.display());
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}
