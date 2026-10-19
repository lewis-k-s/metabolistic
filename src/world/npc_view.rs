//! # Creatures and light entities in the world
//!
//! The headless modules (`npc`, `particles`, `engulf`, `gate`) hold the logic
//! and spawn entities with a `Transform` only. This module adds what needs
//! graphics and physics:
//!
//! - **NPC bodies**: a circle collider, mass and drag from `CellSize` (the same
//!   as the player's), and a sprite that plays `spawn`, then `idle` or `move`
//!   with the row of the heading. The thrust systems of `player_body` move them.
//!   The gate cell has a static body.
//! - **Sprites of particles**: food, metal pickups, viruses and toxin droplets,
//!   added when the component appears.
//! - **One-shot clips**: the death of an NPC, the engulf of prey or a symbiont by
//!   the player (`engulf`, then `digest_end`, with the player's own sprite hidden
//!   because the frames hold the engulfer), and the victory clip of the gate
//!   cell. The gate cell goes when it is won. The engulf clip follows the
//!   engulfer, because the cell keeps its motion while the clip plays.
//! - **Engulf key**: Space sets `EngulfIntent` of the player's cell.

use bevy::prelude::*;

use crate::actions::{TOXIN_LIFETIME, ToxinDroplet, droplet_alpha};
use crate::cell::{Cell, CellSize};
use crate::engulf::{EngulfEvent, EngulfIntent};
use crate::gate::GateWon;
use crate::npc::{Archetype, Npc, NpcDied, spec};
use crate::particles::{FoodParticle, MetalPickup, RustPickup, Virus};
use crate::player::{ControlledCell, LocalPlayer};
use crate::sprites::{Animator, Sheets, SpriteSystems, clip, direction_row};
use crate::world::player_body::{BlobSprite, attach_body_for};
use crate::{GameState, InRun};

/// Z of the particles: above the grid and walls, below the creatures.
const PARTICLE_Z: f32 = -3.0;
/// Z of the one-shot effects: above the creatures.
const EFFECT_Z: f32 = 1.0;

/// A clip that plays once and then removes its entity.
#[derive(Component)]
pub struct OneShot;

/// The engulf effect: `engulf`, then `digest_end`. It follows the engulfer.
#[derive(Component)]
pub struct EngulfFx {
    pub engulfer: Entity,
}

pub fn plugin(app: &mut App) {
    app.add_observer(food_sprite)
        .add_observer(metal_sprite)
        .add_observer(rust_sprite)
        .add_observer(virus_sprite)
        .add_observer(droplet_sprite)
        .add_systems(
            Update,
            (
                attach_npc_bodies,
                engulf_key.run_if(in_state(GameState::Run)),
                (spawn_death_fx, spawn_engulf_fx, spawn_victory_fx).before(SpriteSystems::Animate),
                (advance_fx, fade_droplets, hide_body_during_engulf).after(SpriteSystems::Animate),
            )
                .run_if(in_state(InRun)),
        )
        .add_systems(
            PostUpdate,
            follow_engulfer
                .before(TransformSystems::Propagate)
                .run_if(in_state(InRun)),
        );
}

// --- Bodies ---

/// Adds the body and the sprite to each new NPC.
///
/// An NPC can be despawned in the frame it appears (engulfed, or dead) before
/// this system's commands run, so the body goes on in a command that checks
/// that the NPC is still there.
fn attach_npc_bodies(
    npcs: Query<(Entity, &Npc, &CellSize, Option<&crate::gate::GateSprite>), Added<Npc>>,
    mut commands: Commands,
) {
    for (entity, npc, size, gate_sprite) in &npcs {
        // The level picks the sprite of its gate cell.
        let sprite = gate_sprite.map_or(spec(npc.archetype).sprite, |g| g.0);
        let gate = npc.archetype == Archetype::Gate;
        let size = *size;
        commands.queue(move |world: &mut World| {
            if world.get_entity(entity).is_err() {
                return;
            }
            let first = if gate { clip::IDLE } else { clip::SPAWN };
            world.resource_scope(|world, sheets: Mut<Sheets>| {
                let mut commands = world.commands();
                attach_body_for(&mut commands, &sheets, entity, &size, sprite, first, gate);
            });
            world.flush();
        });
    }
}

/// Space makes the player's cell try to engulf what it touches.
fn engulf_key(
    keys: Res<ButtonInput<KeyCode>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut intents: Query<&mut EngulfIntent, With<Cell>>,
) {
    let held = keys.pressed(KeyCode::Space);
    for controlled in &players {
        if let Ok(mut intent) = intents.get_mut(controlled.0)
            && intent.0 != held
        {
            intent.0 = held;
        }
    }
}

// --- Particle sprites ---

fn food_sprite(
    added: On<Add, FoodParticle>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    commands.entity(added.entity).insert((
        Sprite::from_image(asset_server.load("sprites/food_carbon.png")),
        Visibility::default(),
        DespawnOnExit(InRun),
    ));
    commands.queue(set_z(added.entity, PARTICLE_Z));
}

fn metal_sprite(
    added: On<Add, MetalPickup>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    commands.entity(added.entity).insert((
        Sprite::from_image(asset_server.load("sprites/pickup_metal_fes.png")),
        Visibility::default(),
        DespawnOnExit(InRun),
    ));
    commands.queue(set_z(added.entity, PARTICLE_Z));
}

/// Rust: the metal sprite in a rust-brown tint until it has its own art.
fn rust_sprite(
    added: On<Add, RustPickup>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let mut sprite = Sprite::from_image(asset_server.load("sprites/pickup_metal_fes.png"));
    sprite.color = RUST_TINT;
    commands.entity(added.entity).insert((sprite, Visibility::default(), DespawnOnExit(InRun)));
    commands.queue(set_z(added.entity, PARTICLE_Z));
}

/// The tint of a rust pickup: the shadow tone of the `pigment` ramp, `#d0541a`.
const RUST_TINT: Color = Color::srgb(0.816, 0.329, 0.102);

fn virus_sprite(added: On<Add, Virus>, sheets: Res<Sheets>, mut commands: Commands) {
    if let Some(sprite) = sheets.bundle("virus_phage", clip::IDLE) {
        commands.entity(added.entity).insert((sprite, Visibility::default(), DespawnOnExit(InRun)));
    }
}

fn droplet_sprite(added: On<Add, ToxinDroplet>, sheets: Res<Sheets>, mut commands: Commands) {
    if let Some(sprite) = sheets.bundle("toxin_droplet", "loop") {
        commands.entity(added.entity).insert((sprite, Visibility::default(), DespawnOnExit(InRun)));
    }
}

/// Sets the z of an entity once it has its `Transform`.
fn set_z(entity: Entity, z: f32) -> impl Command {
    move |world: &mut World| {
        if let Some(mut transform) = world.get_mut::<Transform>(entity) {
            transform.translation.z = z;
        }
    }
}

/// A droplet fades out as it ages.
fn fade_droplets(mut droplets: Query<(&ToxinDroplet, &mut Sprite)>) {
    for (droplet, mut sprite) in &mut droplets {
        let alpha = droplet_alpha(droplet.age.min(TOXIN_LIFETIME));
        sprite.color = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
}

// --- One-shot clips ---

fn spawn_death_fx(
    mut died: MessageReader<NpcDied>,
    sheets: Res<Sheets>,
    shader: Res<crate::world::shader_cells::ShaderCells>,
    canvas: Res<crate::world::shader_cells::Canvas>,
    mut commands: Commands,
) {
    for death in died.read() {
        // A shader body bursts instead (`shader_cells::spawn_lysis`).
        if shader.0 && canvas.is_open() {
            continue;
        }
        let Some(bundle) = sheets.bundle(spec(death.archetype).sprite, clip::DEATH) else {
            continue;
        };
        commands.spawn((
            Name::new("Death"),
            OneShot,
            bundle,
            Transform::from_translation(death.position.round().extend(EFFECT_Z)),
            DespawnOnExit(InRun),
        ));
    }
}

fn spawn_engulf_fx(
    mut events: MessageReader<EngulfEvent>,
    sheets: Res<Sheets>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut commands: Commands,
) {
    for event in events.read() {
        if !players.iter().any(|c| c.0 == event.engulfer) {
            continue;
        }
        let name = match event.target_archetype {
            Some(Archetype::Prey) => "engulf_player_prey",
            Some(Archetype::Symbiont) => "engulf_player_symbiont",
            _ => continue,
        };
        let row = direction_row(event.direction);
        let Some(bundle) = sheets.bundle_row(name, clip::ENGULF, row) else {
            continue;
        };
        commands.spawn((
            Name::new("Engulf"),
            EngulfFx { engulfer: event.engulfer },
            bundle,
            Transform::from_translation(event.position.round().extend(EFFECT_Z)),
            DespawnOnExit(InRun),
        ));
    }
}

fn spawn_victory_fx(
    mut won: MessageReader<GateWon>,
    sheets: Res<Sheets>,
    mut commands: Commands,
) {
    for event in won.read() {
        commands.entity(event.gate).despawn();
        if let Some(bundle) = sheets.bundle("gate_cell_victory", clip::VICTORY) {
            commands.spawn((
                Name::new("Victory"),
                OneShot,
                bundle,
                Transform::from_translation(event.position.round().extend(EFFECT_Z)),
                DespawnOnExit(InRun),
            ));
        }
    }
}

/// Removes finished one-shots, and moves the engulf effect on to `digest_end`.
fn advance_fx(
    sheets: Res<Sheets>,
    mut fx: Query<(Entity, &mut Animator, Has<EngulfFx>, Has<OneShot>)>,
    mut commands: Commands,
) {
    for (entity, mut animator, engulf, one_shot) in &mut fx {
        if !animator.finished() {
            continue;
        }
        if engulf && animator.clip == clip::ENGULF {
            animator.play(clip::DIGEST_END, &sheets);
        } else if engulf || one_shot {
            commands.entity(entity).despawn();
        }
    }
}

/// Keeps the engulf clip on the engulfer, at the whole pixel that the body
/// sprite snaps to. Physics moves the cell after `Update`, so this runs late.
fn follow_engulfer(
    cells: Query<&Transform, (With<Cell>, Without<EngulfFx>)>,
    mut fx: Query<(&EngulfFx, &mut Transform)>,
) {
    for (engulf, mut transform) in &mut fx {
        if let Ok(cell) = cells.get(engulf.engulfer) {
            let wanted = cell.translation.truncate().round().extend(EFFECT_Z);
            if transform.translation != wanted {
                transform.translation = wanted;
            }
        }
    }
}

/// The engulf frames hold the player's cell: hide its own sprite meanwhile.
fn hide_body_during_engulf(
    fx: Query<(), With<EngulfFx>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    children: Query<&Children>,
    mut sprites: Query<&mut Visibility, With<BlobSprite>>,
) {
    let hidden = !fx.is_empty();
    for controlled in &players {
        for child in children.iter_descendants(controlled.0) {
            if let Ok(mut visibility) = sprites.get_mut(child) {
                let wanted = if hidden { Visibility::Hidden } else { Visibility::Inherited };
                if *visibility != wanted {
                    *visibility = wanted;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An NPC that is despawned in the frame it appears must not crash the
    /// attach step (it did, when engulfed by a predator on its first frame).
    #[test]
    fn an_npc_despawned_in_its_first_frame_gets_no_body() {
        let mut app = App::new();
        app.init_resource::<Sheets>().add_systems(
            Update,
            (
                attach_npc_bodies,
                |npcs: Query<Entity, With<Npc>>, mut commands: Commands| {
                    for entity in &npcs {
                        commands.entity(entity).despawn();
                    }
                },
            ),
        );
        let npc = app
            .world_mut()
            .spawn((Npc { archetype: Archetype::Prey }, CellSize::default()))
            .id();
        app.update();
        assert!(app.world().get_entity(npc).is_err());
    }

    #[test]
    fn a_living_npc_gets_a_body() {
        let mut app = App::new();
        app.init_resource::<Sheets>().add_systems(Update, attach_npc_bodies);
        let npc = app
            .world_mut()
            .spawn((Npc { archetype: Archetype::Prey }, CellSize::default()))
            .id();
        app.update();
        assert!(app.world().entity(npc).contains::<avian2d::prelude::RigidBody>());
    }
}
