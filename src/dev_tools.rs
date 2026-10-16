//! Development tools for the game. This plugin is only enabled in dev builds.

use crate::actions::ActionBar;
use crate::blocks::genome::Genome;
use crate::codex::save_dir;
use crate::engulf::Invulnerable;
use crate::metabolism::CurrencyPools;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::Proteome;
use crate::run::{Carry, RunState};
use crate::scenario::{self, DUMP_FILE, Scenario};
use bevy::{
    input::common_conditions::input_just_pressed, prelude::*, ui_render::GlobalUiDebugOptions,
};

pub(crate) fn plugin(app: &mut App) {
    // Toggle the debug overlay for UI.
    // The UI debug overlay comes with the `bevy_ui_debug` feature.
    app.add_systems(
        Update,
        (
            toggle_debug_ui.run_if(input_just_pressed(TOGGLE_KEY)),
            dump_scenario.run_if(input_just_pressed(DUMP_KEY)),
        ),
    );
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;
/// Writes the player's cell as a scenario file (see `scenario`).
const DUMP_KEY: KeyCode = KeyCode::F9;

/// Fetches the state of the player's cell and writes it as a scenario to the
/// save directory. The scenario is built by `Scenario::from_state`.
fn dump_scenario(
    run: Res<RunState>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<(
        &Genome,
        &Proteome,
        &ActionBar,
        &CurrencyPools,
        &Transform,
        Has<Invulnerable>,
    )>,
) {
    let Some(dir) = save_dir() else {
        warn!("scenario dump: no save directory");
        return;
    };
    let Ok(controlled) = players.single() else {
        warn!("scenario dump: no player cell (press the key in a run)");
        return;
    };
    let Ok((genome, proteome, bar, pools, transform, invulnerable)) = cells.get(controlled.0) else {
        return;
    };
    let carry = Carry {
        genome: genome.clone(),
        proteome: proteome.clone(),
        action_bar: bar.clone(),
    };
    let scenario = Scenario::from_state(
        run.stage(),
        run.seed,
        &carry,
        pools,
        transform.translation.truncate(),
        invulnerable,
    );
    let path = dir.join(DUMP_FILE);
    match scenario::save(&scenario, &path) {
        Ok(()) => info!("scenario dump: wrote {}", path.display()),
        Err(err) => warn!("scenario dump: cannot write {}: {err}", path.display()),
    }
}

fn toggle_debug_ui(mut options: ResMut<GlobalUiDebugOptions>) {
    options.toggle();
}
