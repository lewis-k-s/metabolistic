//! # The pilus in the world
//!
//! The graphical and physical part of the pilus (see `pilus` for the rules):
//!
//! - **Joint**: while a cell has a `Tether`, a `DistanceJoint` links the owner
//!   to the target. Its maximum length is the tether length, so a shrinking
//!   tether pulls both bodies together, each by its mass. A wall tile has no body,
//!   so a static anchor body sits at the tip. The joint is removed when the
//!   tether ends or changes target.
//! - **Line**: a sprite (`pilus_line.png`, a tiled strand) from the mount point
//!   on the owner to the anchor. It is the one sprite here that turns and
//!   stretches, because it is a line and not a baked body.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::cell::{Cell, CellSize};
use crate::movement::world_radius;
use crate::pilus::{PILUS_MOUNT_FRACTION, PilusJointsActive, PilusTether, Tether};
use crate::InRun;

/// Z of the pilus line: above the cells, below the overlays.
const LINE_Z: f32 = 0.3;
/// Thickness of the line in pixels.
const LINE_THICKNESS: f32 = 3.0;

/// A joint made for a tether.
#[derive(Component, Debug, Clone, Copy)]
pub struct PilusJoint {
    pub owner: Entity,
    pub target: Option<Entity>,
    /// The static body at the tip, for a wall.
    pub anchor: Option<Entity>,
}

/// The line sprite of a tether.
#[derive(Component, Debug, Clone, Copy)]
pub struct PilusLine {
    pub owner: Entity,
}

pub fn plugin(app: &mut App) {
    app.insert_resource(PilusJointsActive)
        .add_systems(
            FixedUpdate,
            sync_pilus_joints
                .after(crate::pilus::pilus_reel_system)
                .run_if(in_state(InRun)),
        )
        .add_systems(Update, draw_pilus_lines.run_if(in_state(InRun)));
}

/// Does a joint still fit the tether of its owner?
pub fn joint_fits(joint: &PilusJoint, tether: Option<&Tether>) -> bool {
    tether.is_some_and(|t| t.target == joint.target)
}

/// Makes, updates and removes the joints.
fn sync_pilus_joints(
    owners: Query<(Entity, &PilusTether), With<Cell>>,
    mut joints: Query<(Entity, &PilusJoint, &mut DistanceJoint)>,
    mut commands: Commands,
) {
    let mut has_joint: Vec<Entity> = Vec::new();
    for (entity, joint, mut distance) in &mut joints {
        let tether = owners.get(joint.owner).ok().and_then(|(_, t)| t.0);
        if joint_fits(joint, tether.as_ref()) {
            let length = tether.map_or(0.0, |t| t.length);
            if distance.limits.max != length {
                distance.limits.max = length;
            }
            has_joint.push(joint.owner);
        } else {
            if let Some(anchor) = joint.anchor {
                commands.entity(anchor).despawn();
            }
            commands.entity(entity).despawn();
        }
    }
    for (owner, tether) in &owners {
        let Some(tether) = tether.0 else {
            continue;
        };
        if has_joint.contains(&owner) {
            continue;
        }
        let (other, anchor) = match tether.target {
            Some(target) => (target, None),
            None => {
                let anchor = commands
                    .spawn((
                        Name::new("Pilus Anchor"),
                        RigidBody::Static,
                        Transform::from_translation(tether.anchor.extend(0.0)),
                        DespawnOnExit(InRun),
                    ))
                    .id();
                (anchor, Some(anchor))
            }
        };
        commands.spawn((
            Name::new("Pilus Joint"),
            PilusJoint {
                owner,
                target: tether.target,
                anchor,
            },
            DistanceJoint::new(owner, other).with_limits(0.0, tether.length),
            DespawnOnExit(InRun),
        ));
    }
}

/// The two ends of the line: the mount point on the owner and the point on the
/// target rim (or the anchor of a wall). `None` if the ends coincide.
pub fn line_ends(
    owner: Vec2,
    owner_radius: f32,
    anchor: Vec2,
    target_radius: Option<f32>,
) -> Option<(Vec2, Vec2)> {
    let direction = (anchor - owner).try_normalize()?;
    let start = owner + direction * owner_radius * PILUS_MOUNT_FRACTION;
    let end = anchor - direction * target_radius.unwrap_or(0.0);
    (start.distance(end) > 0.5).then_some((start, end))
}

/// Draws one line for each tether.
fn draw_pilus_lines(
    asset_server: Res<AssetServer>,
    owners: Query<(Entity, &PilusTether, &Transform, &CellSize), With<Cell>>,
    sizes: Query<&CellSize, With<Cell>>,
    mut lines: Query<(Entity, &PilusLine, &mut Transform, &mut Sprite, &mut Visibility), Without<Cell>>,
    mut commands: Commands,
) {
    let mut drawn: Vec<Entity> = Vec::new();
    for (entity, line, mut transform, mut sprite, mut visibility) in &mut lines {
        let ends = owners.get(line.owner).ok().and_then(|(_, tether, owner, size)| {
            let tether = tether.0?;
            let target_radius = tether.target.and_then(|t| sizes.get(t).ok()).map(world_radius);
            line_ends(
                owner.translation.truncate(),
                world_radius(size),
                tether.anchor,
                target_radius,
            )
        });
        let Some((start, end)) = ends else {
            commands.entity(entity).despawn();
            continue;
        };
        drawn.push(line.owner);
        let middle = (start + end) * 0.5;
        let delta = end - start;
        transform.translation = middle.extend(LINE_Z);
        transform.rotation = Quat::from_rotation_z(delta.to_angle());
        sprite.custom_size = Some(Vec2::new(delta.length(), LINE_THICKNESS));
        *visibility = Visibility::Inherited;
    }
    for (owner, tether, ..) in &owners {
        if tether.0.is_none() || drawn.contains(&owner) {
            continue;
        }
        commands.spawn((
            Name::new("Pilus Line"),
            PilusLine { owner },
            Sprite {
                image: asset_server.load("sprites/pilus_line.png"),
                custom_size: Some(Vec2::new(1.0, LINE_THICKNESS)),
                image_mode: SpriteImageMode::Tiled {
                    tile_x: true,
                    tile_y: false,
                    stretch_value: 1.0,
                },
                ..default()
            },
            Transform::from_translation(Vec3::new(0.0, 0.0, LINE_Z)),
            DespawnOnExit(InRun),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pilus::Pull;

    fn tether(target: Option<Entity>) -> Tether {
        Tether {
            target,
            anchor: Vec2::ZERO,
            length: 50.0,
            min_length: 30.0,
            pull: Pull::TargetToOwner,
        }
    }

    #[test]
    fn a_joint_fits_only_its_own_target() {
        let a = Entity::from_raw_u32(1).unwrap();
        let b = Entity::from_raw_u32(2).unwrap();
        let joint = PilusJoint { owner: a, target: Some(b), anchor: None };
        assert!(joint_fits(&joint, Some(&tether(Some(b)))));
        assert!(!joint_fits(&joint, Some(&tether(None))));
        assert!(!joint_fits(&joint, None));
    }

    #[test]
    fn the_line_runs_from_the_mount_to_the_target_rim() {
        let (start, end) = line_ends(Vec2::ZERO, 20.0, Vec2::new(60.0, 0.0), Some(10.0)).unwrap();
        assert!((start.x - 19.0).abs() < 1e-4);
        assert!((end.x - 50.0).abs() < 1e-4);
        // A wall tip has no radius.
        let (_, end) = line_ends(Vec2::ZERO, 20.0, Vec2::new(60.0, 0.0), None).unwrap();
        assert_eq!(end.x, 60.0);
        // A body at the anchor has no line.
        assert_eq!(line_ends(Vec2::ZERO, 20.0, Vec2::ZERO, None), None);
    }
}
