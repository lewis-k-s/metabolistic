//! # The pilus in the world
//!
//! The graphical and physical part of the pilus (see `pilus` for the rules):
//!
//! - **Joint**: while a cell has a `Tether`, a `DistanceJoint` links the owner
//!   to the target. Its maximum length is the tether length, so a shrinking
//!   tether pulls both bodies together, each by its mass. A wall tile has no body,
//!   so a static anchor body sits at the tip. The joint is removed when the
//!   tether ends or changes target. The joint is a little soft
//!   (`JOINT_COMPLIANCE`): a hard XPBD limit that shrinks each frame turns
//!   each step into a velocity kick, and the bodies fly together much faster
//!   than the reel. Avian needs its `xpbd_joints` feature for any joint.
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

/// Compliance of the pilus joint (inverse stiffness, pixels per newton). With
/// 0.001 a reeled prey runs at most about 8 px ahead of the fibre and the fibre
/// stretches about 1 px against a strong pull (see the test). **Playtest.**
pub const JOINT_COMPLIANCE: f32 = 0.001;

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
            DistanceJoint::new(owner, other)
                .with_limits(0.0, tether.length)
                .with_compliance(JOINT_COMPLIANCE),
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
            max_length: 160.0,
            wanted: 50.0,
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

    // --- Physics: the joint pulls ---

    use std::time::Duration;

    /// A player body (radius 20) at the origin and a prey body (radius 11) at
    /// x = 120 that pushes east with `flee`. The joint limit shrinks at the reel
    /// speed for 3 s. Returns the most that the prey ran ahead of the limit, the
    /// most that the fibre stretched, and the last distance.
    fn reel_bodies(flee: f32) -> (f32, f32, f32) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::diagnostic::DiagnosticsPlugin,
            PhysicsPlugins::default().with_length_unit(20.0),
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 64.0)));
        let body = |radius: f32, mass: f32, drag: f32, x: f32, force: f32| {
            (
                RigidBody::Dynamic,
                Collider::circle(radius),
                Mass(mass),
                LinearDamping(drag),
                LockedAxes::ROTATION_LOCKED,
                ConstantForce(Vec2::new(force, 0.0)),
                Transform::from_xyz(x, 0.0, 0.0),
            )
        };
        let a = app.world_mut().spawn(body(20.0, 1.0, 3.0, 0.0, 0.0)).id();
        let b = app.world_mut().spawn(body(11.0, 0.32, 1.7, 120.0, flee)).id();
        let joint = app
            .world_mut()
            .spawn(DistanceJoint::new(a, b).with_limits(0.0, 120.0).with_compliance(JOINT_COMPLIANCE))
            .id();
        app.finish();
        app.cleanup();
        let (mut ahead, mut stretch, mut distance) = (0.0_f32, 0.0_f32, 0.0);
        for i in 0..192 {
            let max = (120.0 - i as f32 * crate::pilus::PILUS_RETRACT_SPEED / 64.0).max(35.0);
            app.world_mut().get_mut::<DistanceJoint>(joint).unwrap().limits.max = max;
            app.update();
            let at = |e| app.world().get::<Transform>(e).unwrap().translation.truncate();
            distance = at(a).distance(at(b));
            if max > 40.0 {
                ahead = ahead.max(max - distance);
            }
            stretch = stretch.max(distance - max);
        }
        (ahead, stretch, distance)
    }

    #[test]
    fn the_joint_reels_the_prey_in_smoothly_and_holds_a_fleeing_prey() {
        let (ahead, stretch, distance) = reel_bodies(0.0);
        assert!(distance < 36.0, "the prey came in: {distance}");
        assert!(ahead < 12.0, "the prey did not fly ahead of the fibre: {ahead}");
        assert!(stretch < 2.0, "{stretch}");
        let (ahead, stretch, distance) = reel_bodies(600.0);
        assert!(distance < 37.0, "a fleeing prey came in too: {distance}");
        assert!(ahead < 2.0 && stretch < 3.0, "{ahead} {stretch}");
    }
}
