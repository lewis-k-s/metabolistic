//! # Interior pieces in the world
//!
//! Spawns the pieces that `interior::interior_plan` picks as sprites inside a
//! cell: each piece is a child of the `BodyDeform` pivot, so it stretches with
//! the body. The pivot turns to the motion, so a piece turns back by the same
//! angle (like the body sprite), and its offset is turned back too: the piece
//! then goes where the stretched body goes, and is still when the cell is still.
//!
//! - Key `V` on the Run screen cycles the `CellDisplay` mode.
//! - The player's cell and the NPCs with a real genome use the same system.
//!   `interior::limits` keeps NPCs to a few pieces.

use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, Genome};
use crate::actions::Throttle;
use crate::cell::{Cell, CellBlocks, CellSize};
use crate::interior::{
    CellDisplay, CellRole, Look, PIECES, Piece, interior_plan, limits, piece_def, slot_offset,
};
use crate::movement::world_radius;
use crate::npc::{Archetype, Npc};
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::Proteome;
use crate::sprites::{Sheets, SpriteSystems, clip};
use crate::world::npc_view::EngulfFx;
use crate::world::player_body::{BodyDeform, deform_transforms};
use crate::{GameState, InRun};

/// Z of a piece in the pivot: above the body sprite, under the overlays (0.5).
const PIECE_Z: f32 = 0.1;
/// The clip of a mutated gene.
const GLITCH: &str = "glitch";

pub fn plugin(app: &mut App) {
    app.init_resource::<CellDisplay>()
        .add_systems(
            Update,
            (
                cycle_display.run_if(in_state(GameState::Run)),
                tick_notice,
                sync_interior.before(SpriteSystems::Animate),
            )
                .chain()
                .run_if(in_state(InRun)),
        )
        .add_systems(
            PostUpdate,
            place_pieces
                .before(TransformSystems::Propagate)
                .run_if(in_state(InRun)),
        );
}

/// A piece sprite of a cell. `offset` is its place relative to the body centre.
#[derive(Component, Debug, Clone, Copy)]
pub struct InteriorPiece {
    pub kind: BlockKind,
    pub offset: Vec2,
}

/// The plan that a cell's piece children were built from.
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
struct InteriorPlan(Vec<Piece>);

fn cycle_display(keys: Res<ButtonInput<KeyCode>>, mut display: ResMut<CellDisplay>) {
    if keys.just_pressed(KeyCode::KeyV) {
        display.cycle();
    }
}

fn tick_notice(time: Res<Time>, mut display: ResMut<CellDisplay>) {
    if display.notice > 0.0 {
        display.notice = (display.notice - time.delta_secs()).max(0.0);
    }
}

/// The clip of a look.
fn look_clip(look: Look) -> &'static str {
    match look {
        Look::Normal => clip::IDLE,
        Look::Glitch => GLITCH,
    }
}

/// Builds the piece children of each cell when its plan changes.
#[allow(clippy::too_many_arguments)] // A Bevy system: each argument is a param.
fn sync_interior(
    sheets: Res<Sheets>,
    display: Res<CellDisplay>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    engulfing: Query<(), With<EngulfFx>>,
    cells: Query<
        (&Genome, &Proteome, &CellBlocks, &CellSize, Option<&Npc>, Option<&InteriorPlan>),
        With<Cell>,
    >,
    throttles: Query<&Throttle>,
    pivots: Query<(Entity, &ChildOf, Option<&Children>), With<BodyDeform>>,
    pieces: Query<(), With<InteriorPiece>>,
    mut visibilities: Query<&mut Visibility, With<InteriorPiece>>,
    mut commands: Commands,
) {
    let player = players.iter().next().map(|c| c.0);
    let hidden = !engulfing.is_empty();
    for (pivot, parent, children) in &pivots {
        let cell = parent.parent();
        let Ok((genome, proteome, blocks, size, npc, current)) = cells.get(cell) else {
            continue;
        };
        let role = if Some(cell) == player {
            CellRole::Player
        } else if npc.is_some_and(|n| n.archetype != Archetype::Gate) {
            CellRole::Npc
        } else {
            CellRole::Plain
        };
        let radius = world_radius(size);
        let previous = current.map(|c| c.0.as_slice()).unwrap_or(&[]);
        let activity = |kind| {
            blocks
                .get(kind)
                .and_then(|e| throttles.get(e).ok())
                .map_or(1.0, |t| t.0)
        };
        let plan = interior_plan(genome, proteome, activity, previous, limits(display.mode, role, radius));
        if current.is_none_or(|c| c.0 != plan) {
            for child in children.into_iter().flat_map(|c| c.iter()) {
                if pieces.contains(child) {
                    commands.entity(child).despawn();
                }
            }
            for piece in &plan {
                let Some(def) = piece_def(piece.kind) else {
                    continue;
                };
                let Some(sprite) = sheets.bundle(def.sprite, look_clip(piece.look)) else {
                    continue;
                };
                let offset = piece.slot.map_or(Vec2::ZERO, |slot| slot_offset(slot, radius));
                commands.entity(pivot).with_child((
                    Name::new(def.sprite),
                    InteriorPiece { kind: piece.kind, offset },
                    sprite,
                    Transform::from_translation(offset.extend(PIECE_Z)),
                ));
            }
            commands.entity(cell).insert(InteriorPlan(plan));
        }
        if role == CellRole::Player {
            let wanted = if hidden { Visibility::Hidden } else { Visibility::Inherited };
            for child in children.into_iter().flat_map(|c| c.iter()) {
                if let Ok(mut visibility) = visibilities.get_mut(child)
                    && *visibility != wanted
                {
                    *visibility = wanted;
                }
            }
        }
    }
}

/// The local transform of a piece under a deformed pivot. The piece turns back
/// by the pivot angle, and so does its offset: the pivot then turns and
/// stretches the offset like the body (see `player_body::deform_transforms`).
pub fn piece_transform(offset: Vec2, deform: BodyDeform) -> Transform {
    let (_, rotation) = deform_transforms(Vec2::ZERO, deform);
    Transform {
        translation: (rotation * offset.extend(0.0)).with_z(PIECE_Z),
        rotation,
        ..default()
    }
}

/// Places each piece under its pivot, after the stretch is known.
fn place_pieces(
    pivots: Query<&BodyDeform>,
    mut pieces: Query<(&ChildOf, &InteriorPiece, &mut Transform)>,
) {
    for (parent, piece, mut transform) in &mut pieces {
        let Ok(deform) = pivots.get(parent.parent()) else {
            continue;
        };
        let wanted = piece_transform(piece.offset, *deform);
        if *transform != wanted {
            *transform = wanted;
        }
    }
}

/// Every sprite name that a piece may use. The loading screen and the tests use
/// it to check that the manifest has the art.
pub fn piece_sprites() -> impl Iterator<Item = &'static str> {
    PIECES.iter().map(|p| p.sprite)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_piece_stays_on_its_pixel() {
        let t = piece_transform(Vec2::new(-9.0, 9.0), BodyDeform::default());
        assert_eq!(t.translation, Vec3::new(-9.0, 9.0, PIECE_Z));
        assert_eq!(t.rotation, Quat::IDENTITY);
    }

    #[test]
    fn a_piece_moves_with_the_stretched_body() {
        // Stretch along +X by 20%: a piece at x = 10 ends at x = 12 in the world,
        // a piece at y = 10 ends at y = 10 / 1.2.
        let deform = BodyDeform { stretch: 0.2, angle: 0.0 };
        let (pivot, _) = deform_transforms(Vec2::ZERO, deform);
        let world = |offset: Vec2| {
            let local = piece_transform(offset, deform);
            pivot.to_matrix().transform_point3(local.translation)
        };
        assert!((world(Vec2::new(10.0, 0.0)).x - 12.0).abs() < 1e-4);
        assert!((world(Vec2::new(0.0, 10.0)).y - 10.0 / 1.2).abs() < 1e-4);
        // Along +Y (the pivot turned): the same, on the other axis.
        let deform = BodyDeform { stretch: 0.2, angle: std::f32::consts::FRAC_PI_2 };
        let (pivot, _) = deform_transforms(Vec2::ZERO, deform);
        let local = piece_transform(Vec2::new(0.0, 10.0), deform);
        let at = pivot.to_matrix().transform_point3(local.translation);
        assert!((at.y - 12.0).abs() < 1e-4, "{at}");
    }

    #[test]
    fn a_mutated_look_plays_the_glitch_clip() {
        assert_eq!(look_clip(Look::Normal), clip::IDLE);
        assert_eq!(look_clip(Look::Glitch), GLITCH);
    }
}
