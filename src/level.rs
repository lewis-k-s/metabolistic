//! # The vent field
//!
//! The one hand-built level of M2. All sizes are in world units (pixels). The
//! grid is centred on the world origin.
//!
//! - **Vent core**: the main vent at the origin. Its heat is high enough to
//!   burn the membrane within about 2.5 grid cells, and it emits minerals.
//! - **Plumes**: minerals drift east and north, and decay with distance. Heat
//!   spreads in a wider, weaker halo, because chemosynthesis needs both.
//! - **A second, weaker vent** in the north-east.
//! - **Cold zones**: far from the vents there is no heat and no minerals.
//!
//! `vent_field` builds the grid and runs it to a steady state, so the player
//! starts in a field that already has plumes.

use bevy::prelude::*;

use crate::envgrid::{EnvGrid, FieldParams};
use crate::environment::Channel;

/// Grid size in grid cells. At 32 px a cell, the field is 2048 x 1536 px.
pub const FIELD_WIDTH: usize = 64;
pub const FIELD_HEIGHT: usize = 48;

/// Simulated seconds that `vent_field` runs before the game starts.
const WARM_UP_SECONDS: f32 = 150.0;

/// A vent: a source of heat and minerals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vent {
    pub position: Vec2,
    /// Radius of the emitter in world units.
    pub radius: f32,
    /// Heat emission per second.
    pub heat: f32,
    /// Mineral emission per second.
    pub minerals: f32,
}

/// The vents of the field. The first one is the core.
pub const VENTS: [Vent; 2] = [
    Vent {
        position: Vec2::new(0.0, 0.0),
        radius: 50.0,
        heat: 500.0,
        minerals: 70.0,
    },
    Vent {
        position: Vec2::new(400.0, 280.0),
        radius: 40.0,
        heat: 160.0,
        minerals: 25.0,
    },
];

/// Where the player starts: in the warm mineral plume, near the vent but not
/// in the core.
pub const START_POSITION: Vec2 = Vec2::new(190.0, 70.0);

/// How the channels move in the vent field.
pub fn field_params(channel: Channel) -> FieldParams {
    match channel {
        Channel::Heat => FieldParams {
            diffusion: 1.0,
            decay: 0.2,
            drift: Vec2::new(0.3, 0.12),
            open_drift_edges: true,
        },
        Channel::Minerals => FieldParams {
            diffusion: 0.4,
            decay: 0.07,
            drift: Vec2::new(0.7, 0.25),
            open_drift_edges: true,
        },
    }
}

/// The vent field before the warm-up: emitters set, grid empty.
pub fn vent_field_cold() -> EnvGrid {
    let mut grid = EnvGrid::centered(FIELD_WIDTH, FIELD_HEIGHT);
    for channel in Channel::ALL {
        grid.set_params(channel, field_params(channel));
    }
    for vent in VENTS {
        grid.add_emitter(Channel::Heat, vent.position, vent.radius, vent.heat);
        grid.add_emitter(Channel::Minerals, vent.position, vent.radius, vent.minerals);
    }
    grid
}

/// The vent field, run to a steady state.
pub fn vent_field() -> EnvGrid {
    let mut grid = vent_field_cold();
    for _ in 0..WARM_UP_SECONDS as usize {
        grid.step(1.0);
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints the field as text. Run with `cargo th print_field -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_field() {
        let grid = vent_field();
        for channel in Channel::ALL {
            eprintln!("{channel:?}  total {:.1}", grid.total(channel));
            for y in (0..FIELD_HEIGHT).rev() {
                let row: String = (0..FIELD_WIDTH)
                    .map(|x| {
                        let v = grid.level(channel, x, y);
                        match v {
                            v if v >= 40.0 => '#',
                            v if v >= 10.0 => '+',
                            v if v >= 3.0 => ':',
                            v if v >= 0.5 => '.',
                            _ => ' ',
                        }
                    })
                    .collect();
                eprintln!("{row}");
            }
        }
        let at = |p: Vec2| (grid.sample(Channel::Heat, p), grid.sample(Channel::Minerals, p));
        eprintln!("start {:?}", at(START_POSITION));
        eprintln!("core {:?}", at(Vec2::ZERO));
        eprintln!("far {:?}", at(Vec2::new(-900.0, 600.0)));
        eprintln!("vent2 {:?}", at(VENTS[1].position));
        let max_x = (0..FIELD_WIDTH).map(|x| grid.level(Channel::Minerals, x, 20)).fold(0.0, f32::max);
        eprintln!("edge column minerals {:?}", (0..FIELD_HEIGHT).map(|y| grid.level(Channel::Minerals, FIELD_WIDTH - 1, y)).fold(0.0f32, f32::max));
        let _ = max_x;
    }
}
