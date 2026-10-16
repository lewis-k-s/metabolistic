//! # The gate cell
//!
//! The gate cell (`gate_cell_phototroph`) ends the vent level. It does not
//! move and it sits in a light zone inside a walled chamber with pores. It is
//! bigger than the player, so the size rule of `engulf` does not apply:
//!
//! - The player engulfs it by staying in contact for `GATE_SECONDS`
//!   (`GateProgress::step`). The HUD shows the progress.
//! - **If the contact breaks** for longer than `GATE_GRACE` seconds, the progress
//!   **resets to 0**. (The short grace stops physics jitter from resetting it.)
//! - When the time is full, the player takes the gate gene (`LevelDef::gate_gene`;
//!   `GATE_GENE` is the default: light capture), the stage is won, and the world plays `gate_cell_victory` for
//!   `VICTORY_DELAY` seconds. Then the run goes on to the next stage, or ends
//!   (see `run`).

use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, CellSize};
use crate::engulf::{GateCell, in_contact};
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::run::{PendingVictory, RunSummary};

/// Seconds of contact that win the level.
pub const GATE_SECONDS: f32 = 3.0;
/// Seconds of lost contact before the progress resets.
pub const GATE_GRACE: f32 = 0.25;
/// The gene that the gate cell of the vents gives: photosynthesis.
pub const GATE_GENE: BlockKind = BlockKind::LightCapture;
/// Seconds between the win and the run-over screen (the victory clip is 1.6 s).
pub const VICTORY_DELAY: f32 = 1.6;

/// The sprite of a gate cell, from its level (`LevelDef::gate_sprite`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSprite(pub &'static str);

/// How long the player has touched the gate cell.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct GateProgress {
    /// Seconds of contact.
    pub held: f32,
    /// Seconds since the contact broke.
    pub lost: f32,
}

impl GateProgress {
    /// The share that is full, 0 to 1.
    pub fn fraction(&self) -> f32 {
        (self.held / GATE_SECONDS).clamp(0.0, 1.0)
    }

    /// One step of `dt` seconds. Returns true when the time is full.
    pub fn step(&mut self, contact: bool, dt: f32) -> bool {
        if contact {
            self.held += dt;
            self.lost = 0.0;
        } else {
            self.lost += dt;
            if self.lost > GATE_GRACE {
                self.held = 0.0;
            }
        }
        self.held >= GATE_SECONDS
    }
}

/// The gate was won. The world plays the victory clip at `position`.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct GateWon {
    pub gate: Entity,
    pub position: Vec2,
    /// The gene that the gate cell gave.
    pub gene: BlockKind,
}

#[allow(clippy::type_complexity)]
pub fn gate_system(
    time: Res<Time>,
    pending: Option<Res<PendingVictory>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut gates: Query<(Entity, &Transform, &CellSize, &mut GateProgress), With<GateCell>>,
    mut bodies: Query<(&Transform, &CellSize, &mut Genome), (With<Cell>, Without<GateCell>)>,
    level: Option<Res<crate::level::CurrentLevel>>,
    mut summary: ResMut<RunSummary>,
    mut won: MessageWriter<GateWon>,
    mut commands: Commands,
) {
    if pending.is_some() {
        return;
    }
    let dt = time.delta_secs();
    // The level says what the gate gives; tests without a level use the default.
    let gate_gene = level.map_or(GATE_GENE, |l| l.0.gate_gene);
    for controlled in &players {
        let Ok((player_transform, player_size, mut genome)) = bodies.get_mut(controlled.0) else {
            continue;
        };
        for (gate, gate_transform, gate_size, mut progress) in &mut gates {
            let contact = in_contact(
                player_transform.translation.truncate(),
                world_radius(player_size),
                gate_transform.translation.truncate(),
                world_radius(gate_size),
            );
            if progress.step(contact, dt) {
                genome.table.insert(gate_gene, GeneState::Expressed);
                summary.victory = Some(gate_gene);
                summary.cause = None;
                commands.insert_resource(PendingVictory(VICTORY_DELAY));
                won.write(GateWon {
                    gate,
                    position: gate_transform.translation.truncate(),
                    gene: gate_gene,
                });
            }
        }
    }
}

pub struct GatePlugin;

impl Plugin for GatePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GateWon>().add_systems(Update, gate_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_seconds_of_contact_win() {
        let mut p = GateProgress::default();
        let mut won = false;
        for _ in 0..29 {
            won |= p.step(true, 0.1);
        }
        assert!(!won && p.fraction() > 0.9 && p.fraction() < 1.0);
        assert!(p.step(true, 0.11), "3.0 s");
    }

    #[test]
    fn a_short_break_is_forgiven_and_a_long_one_resets() {
        let mut p = GateProgress::default();
        for _ in 0..10 {
            p.step(true, 0.1);
        }
        assert!((p.held - 1.0).abs() < 1e-4);
        p.step(false, 0.2); // inside the grace
        assert!((p.held - 1.0).abs() < 1e-4);
        p.step(true, 0.1); // contact is back: the lost time restarts
        assert_eq!(p.lost, 0.0);
        p.step(false, 0.2);
        p.step(false, 0.2); // 0.4 s lost: more than the grace
        assert_eq!(p.held, 0.0, "reset");
        assert_eq!(p.fraction(), 0.0);
    }
}
