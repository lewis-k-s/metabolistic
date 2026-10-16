//! # Movement of a cell
//!
//! How fast a cell can move depends on its body and its membrane proteins:
//!
//! - **Mass** grows with cell area, and **drag** grows with the radius. A bigger
//!   cell is slower with the same thrust.
//! - **Thrust** is a small base thrust (the amoeboid crawl) plus a thrust for
//!   each `Flagellum`.
//! - Moving costs ATP in proportion to the thrust that the cell uses.
//!
//! Everything here is a pure function or a plain component. The physics
//! systems in `world::player_body` (full builds) fetch the data, call these
//! functions, and give the result to avian2d.
//!
//! Units: forces are in world units (pixels) per second squared times mass, so
//! the terminal speed of a body is `thrust / (mass x drag)` pixels per second.

use bevy::prelude::*;

use crate::cell::CellSize;
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};

/// World units (pixels) in one unit of `CellSize::radius`. The player radius
/// is 20 pixels, the radius of the `player_cell` sprite (one world unit is
/// one sprite pixel). **Playtest.**
pub const PIXELS_PER_SIZE_UNIT: f32 = 20.0;
/// Mass for each unit of cell area (`radius²`).
pub const MASS_PER_AREA: f32 = 1.0;
/// Linear damping per second for each unit of radius.
pub const DRAG_PER_RADIUS: f32 = 3.0;
/// Thrust of the amoeboid crawl, with no flagella. **Playtest.**
pub const BASE_THRUST: f32 = 300.0;
/// Thrust that each flagellum adds. **Playtest.**
pub const FLAGELLUM_THRUST: f32 = 300.0;
/// ATP for each unit of thrust used for one second. At the base thrust, a
/// crawling cell uses 0.75 ATP per second. **Playtest.**
pub const ATP_PER_THRUST_SECOND: f32 = 0.0025;

/// The direction that the controller wants to move in: a vector with a length
/// of at most 1 (WASD gives a unit vector or zero). The local player's input
/// system sets it.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct MoveInput(pub Vec2);

/// Thrust that the cell has used since the last metabolic tick, in thrust
/// units times seconds. The movement system adds to it each frame; the
/// metabolic tick pays ATP for it and sets it to 0 (see `exchange`).
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct ThrustEffort(pub f32);

/// Unit direction from the held keys W, A, S, D, or zero. +Y is up on screen.
pub fn wasd_direction(w: bool, a: bool, s: bool, d: bool) -> Vec2 {
    let axis = |neg: bool, pos: bool| pos as i8 as f32 - neg as i8 as f32;
    Vec2::new(axis(a, d), axis(s, w)).normalize_or_zero()
}

/// Radius of the body in world units (pixels).
pub fn world_radius(size: &CellSize) -> f32 {
    size.radius.max(0.0) * PIXELS_PER_SIZE_UNIT
}

/// Mass of the body. It grows with the area.
pub fn body_mass(size: &CellSize) -> f32 {
    (MASS_PER_AREA * size.radius.max(0.0).powi(2)).max(0.01)
}

/// Linear damping of the body, per second. It grows with the radius.
pub fn body_drag(size: &CellSize) -> f32 {
    DRAG_PER_RADIUS * size.radius.max(0.0)
}

/// Extra drag for a full cuticle shield (shield 1). A cuticle adds a little
/// drag: the factor is `1 + CUTICLE_DRAG x shield`. **Playtest.**
pub const CUTICLE_DRAG: f32 = 0.25;

/// Factor on the drag from the cuticle shield (0 to below 1): 1 without a cuticle.
pub fn cuticle_drag_factor(shield: f32) -> f32 {
    1.0 + CUTICLE_DRAG * shield.clamp(0.0, 1.0)
}

/// Drag with the multiplier of the field at the cell's position (`FieldKind::
/// drag_multiplier`: water is 1, dry ground is more) and the cuticle factor.
pub fn drag_in_field(base: f32, field_multiplier: f32, shield: f32) -> f32 {
    base * field_multiplier.max(0.0) * cuticle_drag_factor(shield)
}

/// The most thrust the cell can give: the base thrust plus each flagellum.
pub fn thrust_capacity(proteome: &Proteome) -> f32 {
    let flagella = proteome.count(ProteinTarget::Membrane(MembraneProtein::Flagellum));
    BASE_THRUST + flagella as f32 * FLAGELLUM_THRUST
}

/// The thrust force for an input direction. The input length (at most 1) sets
/// how much of the capacity the cell uses.
pub fn thrust_force(input: Vec2, capacity: f32) -> Vec2 {
    input.clamp_length_max(1.0) * capacity.max(0.0)
}

/// Speed at which drag balances a thrust, in pixels per second.
pub fn terminal_speed(thrust: f32, size: &CellSize) -> f32 {
    let drag = body_drag(size);
    if drag <= 0.0 {
        return f32::INFINITY;
    }
    thrust.max(0.0) / (body_mass(size) * drag)
}

/// ATP for the thrust used. `effort` is in thrust units times seconds.
pub fn move_atp_cost(effort: f32) -> f32 {
    ATP_PER_THRUST_SECOND * effort.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn proteome_with_flagella(n: u32) -> Proteome {
        let mut proteome = Proteome::default();
        for _ in 0..n {
            proteome.add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        }
        proteome
    }

    #[test]
    fn wasd_direction_is_unit_or_zero() {
        assert_eq!(wasd_direction(false, false, false, false), Vec2::ZERO);
        assert_eq!(wasd_direction(true, false, false, false), Vec2::Y);
        assert_eq!(wasd_direction(true, false, true, false), Vec2::ZERO);
        let diagonal = wasd_direction(true, false, false, true);
        assert!((diagonal.length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn player_body_is_20_pixels_across_the_radius() {
        assert_eq!(world_radius(&CellSize { radius: 1.0 }), 20.0);
    }

    #[test]
    fn thrust_is_base_plus_each_flagellum() {
        assert_eq!(thrust_capacity(&proteome_with_flagella(0)), BASE_THRUST);
        assert_eq!(
            thrust_capacity(&proteome_with_flagella(3)),
            BASE_THRUST + 3.0 * FLAGELLUM_THRUST
        );
    }

    #[test]
    fn a_bigger_cell_is_heavier_and_slower_with_the_same_thrust() {
        let small = CellSize { radius: 1.0 };
        let big = CellSize { radius: 2.0 };
        assert!(body_mass(&big) > body_mass(&small));
        assert!(body_drag(&big) > body_drag(&small));
        assert!(terminal_speed(BASE_THRUST, &big) < terminal_speed(BASE_THRUST, &small));
    }

    #[test]
    fn the_field_multiplier_and_the_cuticle_raise_the_drag_and_lower_the_speed() {
        let size = CellSize::default();
        let base = body_drag(&size);
        assert_eq!(drag_in_field(base, 1.0, 0.0), base);
        assert!(drag_in_field(base, 3.0, 0.0) > 2.9 * base);
        let with_cuticle = drag_in_field(base, 1.0, 0.9);
        assert!(with_cuticle > base && with_cuticle < 1.3 * base, "a little drag");
        // The terminal speed is the thrust over mass and drag.
        let speed = |drag: f32| BASE_THRUST / (body_mass(&size) * drag);
        assert!(speed(drag_in_field(base, 3.0, 0.0)) < speed(base) / 2.9);
    }

    #[test]
    fn flagella_raise_the_terminal_speed() {
        let size = CellSize::default();
        let crawl = terminal_speed(thrust_capacity(&proteome_with_flagella(0)), &size);
        let swim = terminal_speed(thrust_capacity(&proteome_with_flagella(1)), &size);
        assert_abs_diff_eq!(crawl, 100.0, epsilon = 1e-3);
        assert!(swim > 1.9 * crawl);
    }

    #[test]
    fn thrust_force_follows_the_input_and_never_exceeds_capacity() {
        assert_eq!(thrust_force(Vec2::ZERO, 300.0), Vec2::ZERO);
        assert_eq!(thrust_force(Vec2::X, 300.0), Vec2::new(300.0, 0.0));
        // A half input uses half of the capacity.
        assert_eq!(thrust_force(Vec2::new(0.0, 0.5), 300.0), Vec2::new(0.0, 150.0));
        // A too long input is cut to length 1.
        assert_abs_diff_eq!(thrust_force(Vec2::new(5.0, 5.0), 300.0).length(), 300.0, epsilon = 1e-3);
        assert_eq!(thrust_force(Vec2::X, -5.0), Vec2::ZERO);
    }

    #[test]
    fn moving_costs_atp_in_proportion_to_the_thrust_used() {
        assert_eq!(move_atp_cost(0.0), 0.0);
        // One second at the base thrust.
        assert_abs_diff_eq!(move_atp_cost(BASE_THRUST), 0.75, epsilon = 1e-6);
        assert_abs_diff_eq!(
            move_atp_cost(2.0 * BASE_THRUST),
            2.0 * move_atp_cost(BASE_THRUST),
            epsilon = 1e-6
        );
        assert_eq!(move_atp_cost(-1.0), 0.0);
    }
}
