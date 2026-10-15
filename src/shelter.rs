//! # Shelters
//!
//! A shelter is a calm zone: a circle in world space (a vent crevice in the
//! vent level). Inside it:
//!
//! - heat damage and UV damage are 0 (see `exchange::heat_damage_system` and
//!   `genome_damage::uv_damage_system`);
//! - game time stops while the Manage screen is open (`sim_paused`).
//!
//! Outside a shelter, the editor runs in full-speed game time. A shelter is not
//! a save point, and its resources are unlimited for now. **Playtest:** whether
//! to slow time instead of stopping it, and whether shelters have limits.
//!
//! The rule is a pure function, `sim_paused`. `apply_sim_pause_system` pauses
//! `Time<Virtual>` with it. Paused virtual time gives no metabolic ticks, no grid
//! steps and no physics steps, because all of them run on that clock.

use bevy::prelude::*;

use crate::cell::Cell;
use crate::player::{ControlledCell, LocalPlayer};
use crate::GameState;

/// A shelter: a circle in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shelter {
    pub position: Vec2,
    pub radius: f32,
}

/// The shelters of the current level. The world fills it when a run starts.
#[derive(Resource, Default, Debug, Clone)]
pub struct Shelters(pub Vec<Shelter>);

/// True while a cell is inside a shelter. Set in `MetabolicSet::Sense`.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InShelter(pub bool);

/// True if the point is inside one of the shelters.
pub fn point_in_shelter(shelters: &[Shelter], point: Vec2) -> bool {
    shelters
        .iter()
        .any(|s| s.position.distance_squared(point) <= s.radius * s.radius)
}

/// Does game time stop? Only in a shelter, while the editor is open. One
/// function, so that the rule can change to "slow" later.
pub fn sim_paused(in_shelter: bool, editor_open: bool) -> bool {
    in_shelter && editor_open
}

/// Sets `InShelter` of each cell from its position.
pub fn shelter_sense_system(
    shelters: Res<Shelters>,
    mut cells: Query<(&Transform, &mut InShelter), With<Cell>>,
) {
    for (transform, mut inside) in &mut cells {
        let now = point_in_shelter(&shelters.0, transform.translation.truncate());
        if inside.0 != now {
            inside.0 = now;
        }
    }
}

/// Pauses or resumes `Time<Virtual>` by `sim_paused`.
pub fn apply_sim_pause_system(
    state: Res<State<GameState>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&InShelter>,
    mut time: ResMut<Time<Virtual>>,
) {
    let in_shelter = players
        .iter()
        .any(|controlled| cells.get(controlled.0).is_ok_and(|s| s.0));
    let paused = sim_paused(in_shelter, *state.get() == GameState::Manage);
    if paused != time.is_paused() {
        if paused {
            time.pause();
        } else {
            time.unpause();
        }
    }
}

pub struct ShelterPlugin;

impl Plugin for ShelterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shelters>()
            .add_systems(
                crate::metabolism::MetabolicSchedule,
                shelter_sense_system.in_set(crate::metabolism::MetabolicSet::Sense),
            )
            .add_systems(Update, apply_sim_pause_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_stops_only_in_a_shelter_with_the_editor_open() {
        assert!(sim_paused(true, true));
        assert!(!sim_paused(true, false));
        assert!(!sim_paused(false, true), "outside a shelter time runs");
        assert!(!sim_paused(false, false));
    }

    #[test]
    fn a_point_is_inside_a_shelter_up_to_its_radius() {
        let shelters = [Shelter {
            position: Vec2::new(100.0, 0.0),
            radius: 40.0,
        }];
        assert!(point_in_shelter(&shelters, Vec2::new(100.0, 0.0)));
        assert!(point_in_shelter(&shelters, Vec2::new(139.0, 0.0)));
        assert!(!point_in_shelter(&shelters, Vec2::new(141.0, 0.0)));
        assert!(!point_in_shelter(&[], Vec2::ZERO));
    }
}
