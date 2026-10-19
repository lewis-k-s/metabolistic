//! # Non-player cells
//!
//! An NPC is a `Cell` with the same model as the player: a genome, pools, a
//! proteome, blocks and a membrane. It has no `LocalPlayer`. It differs in two
//! ways:
//!
//! - It runs a **slow tick**: `CellTick::every(NPC_TICK_EVERY)`. The cell takes
//!   part in every 4th metabolic tick and simulates 4 x the tick length. The
//!   solver does not depend on the tick length, so the result is the same as
//!   four normal ticks (a test checks this).
//! - Its movement comes from a **steering function**, a pure function of what
//!   the NPC senses (`steer`). There is no pathfinding. The result is written to
//!   `MoveInput`, and the physics systems of `world` move the body like the
//!   player's.
//!
//! ## Archetypes
//!
//! | Archetype | Size | Behaviour |
//! |---|---|---|
//! | Prey (`prey_bacillus`) | small | Wanders along mineral gradients, avoids the heat of the vent core, flees a cell that could engulf it. |
//! | Predator (`predator_cell`) | bigger than the player | Chases the nearest cell it could engulf inside its sense radius, else wanders. Engulfs on contact (see `engulf`). |
//! | Symbiont (`symbiont_cell`) | small | Stays near the player. In contact, carbon flows to the player and ATP flows back (`balance_flow`). The player can still engulf it. |
//! | Gate (`gate_cell_phototroph`) | big | Does not move (see `gate`). |
//!
//! Each archetype has a small genome with one signature gene. Engulfing a cell
//! can give that gene to the engulfer (gene transfer).
//!
//! ## Secreted clouds
//!
//! NPCs also sense the clouds that the player secretes (`CloudSenses`, see
//! `secretion`):
//!
//! - Prey swim away from a microcin cloud (`secretion::microcin::AVOID`).
//! - A symbiont in an AI-2 quorum closes in to contact, and its exchange runs
//!   faster. With no player, it swims up the AI-2 call.
//! - A predator with no prey in sight swims up the AI-2 call (it eavesdrops).
//!
//! An NPC dies when its membrane reaches 0 or its ATP runs out. It drops food.

use bevy::prelude::*;
use rand::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, CellSize, death_cause};
use crate::cell_look::{BodyLook, CellLook, look_for};
use crate::engulf::{EngulfIntent, GateCell, can_engulf};
use crate::envgrid::{CELL_SIZE, EnvGrid};
use crate::environment::Channel;
use crate::membrane::Membrane;
use crate::metabolism::{CellTick, CurrencyPools, FlowmapTimingConfig, MetabolicSet};
use crate::molecules::Currency;
use crate::movement::{MoveInput, terminal_speed, thrust_capacity, world_radius};
use crate::particles::spawn_food;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};
use crate::shelter::{Shelter, Shelters, point_in_shelter};
use crate::secretion::autoinducer;
use crate::stun::Stunned;

// --- Tuning. All of these need **playtest**. ---

/// An NPC takes part in every this many metabolic ticks.
pub const NPC_TICK_EVERY: u32 = 4;
/// Seconds between two decisions of an NPC (the steering runs at about 7 Hz).
pub const THINK_SECONDS: f32 = 0.15;
/// Heat level above which prey and predators move away from the heat. The
/// membrane damage starts at 40.
pub const HEAT_AVOID: f32 = 25.0;
/// Light level above which NPCs move away from the light (UV hurts them too).
pub const LIGHT_AVOID: f32 = 0.4;
/// Rate at which an NPC turns when it wanders, in radians per second.
pub const WANDER_TURN_RATE: f32 = 1.5;
/// Gap (pixels) at which a symbiont stops moving towards the player.
pub const FOLLOW_GAP: f32 = 5.0;
/// Distance over which a symbiont slows down, as it comes close.
pub const FOLLOW_EASE: f32 = 40.0;
/// Gap (pixels) at which the symbiont exchange runs.
pub const SYMBIONT_CONTACT_GAP: f32 = 10.0;
/// Rate of the symbiont exchange, per second, for each pool. The difference
/// between the two cells falls by `exp(-2 x rate x t)`.
pub const SYMBIONT_RATE: f32 = 0.1;
/// Carbon skeletons that a dead NPC drops as food.
pub const DEATH_FOOD_CARBON: f32 = 8.0;

/// Extra distance (pixels) from a shelter at which a predator turns away.
pub const SHELTER_KEEP_OUT: f32 = 30.0;
/// Seconds between two respawns of NPCs (one NPC at a time).
pub const RESPAWN_SECONDS: f32 = 25.0;
/// Seconds between two respawns of key-gene prey while the player lacks the gene.
pub const KEY_RESPAWN_SECONDS: f32 = 6.0;
/// Pigment prey kept alive while the player has no pigment gene (the vents).
pub const PIGMENT_PREY_TARGET: usize = 3;
/// An NPC does not respawn closer than this to the player (pixels).
pub const RESPAWN_CLEAR_DISTANCE: f32 = 260.0;

/// The kinds of NPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Archetype {
    Prey,
    Predator,
    Symbiont,
    /// The gate cell of the level.
    Gate,
}

/// Marks a cell as an NPC and gives its archetype.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Npc {
    pub archetype: Archetype,
}

/// The data of an archetype.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcSpec {
    /// Radius in size units (see `movement::PIXELS_PER_SIZE_UNIT`).
    pub size: f32,
    /// Radius in pixels in which the NPC senses other cells.
    pub sense_radius: f32,
    /// Wanted speed in pixels per second.
    pub speed: f32,
    /// The sprite name.
    pub sprite: &'static str,
    /// The gene that the archetype always has, besides chemosynthesis.
    pub signature: Option<BlockKind>,
}

/// The data of an archetype. Prey is smaller than the player (radius 20), the
/// predator is bigger: its radius of 25 is 1.25 x the player's, so the size
/// rule (1.2) lets it engulf the player, and the player (20) cannot engulf it.
pub fn spec(archetype: Archetype) -> NpcSpec {
    match archetype {
        Archetype::Prey => NpcSpec {
            size: 11.25 / 20.0,
            sense_radius: 160.0,
            speed: 85.0,
            sprite: "prey_bacillus",
            signature: Some(BlockKind::Fermentation),
        },
        Archetype::Predator => NpcSpec {
            size: 25.0 / 20.0,
            sense_radius: 230.0,
            speed: 85.0,
            sprite: "predator_cell",
            signature: Some(BlockKind::VesicleExport),
        },
        Archetype::Symbiont => NpcSpec {
            size: 9.5 / 20.0,
            sense_radius: 400.0,
            speed: 95.0,
            sprite: "symbiont_cell",
            signature: Some(BlockKind::FatStorage),
        },
        Archetype::Gate => NpcSpec {
            size: 25.0 / 20.0,
            sense_radius: 0.0,
            speed: 0.0,
            sprite: "gate_cell_phototroph",
            signature: Some(BlockKind::LightCapture),
        },
    }
}

/// The small genome of an archetype: chemosynthesis, and the signature gene
/// (or `signature` instead of the default one). A phototroph (light capture)
/// also has catalase, like a cyanobacterium: it lives with its own oxygen. So
/// engulfing one can give either gene.
pub fn npc_genome(archetype: Archetype, signature: Option<BlockKind>) -> Genome {
    let mut genome = Genome::default();
    let mut kinds = vec![BlockKind::Chemosynthesis];
    let signature = signature.or(spec(archetype).signature);
    kinds.extend(signature);
    if signature == Some(BlockKind::LightCapture) {
        kinds.push(BlockKind::Catalase);
    }
    for kind in kinds {
        genome.add_gene(kind);
        genome.express_gene(kind);
    }
    genome
}

/// The proteome of an archetype: one enzyme for each block it has, and a
/// flagellum for the predator (it is big, so it needs the thrust).
pub fn npc_proteome(genome: &Genome, archetype: Archetype) -> Proteome {
    let mut proteome = Proteome::default();
    let mut kinds: Vec<_> = genome.table.keys().copied().collect();
    kinds.sort();
    for kind in kinds {
        if crate::blocks::block_spec(kind).is_some() {
            proteome.add(ProteinTarget::Enzyme(kind));
        }
    }
    if archetype == Archetype::Predator {
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
    }
    proteome
}

/// Start pools of an NPC. The symbiont starts rich in carbon and poor in ATP,
/// so that the exchange with the player has a direction.
pub fn npc_pools(archetype: Archetype) -> CurrencyPools {
    let mut pools = CurrencyPools::with_defaults();
    if archetype == Archetype::Symbiont {
        pools.set(Currency::CarbonSkeletons, 60.0);
        pools.set(Currency::ATP, 40.0);
    }
    pools
}

/// Spawns an NPC. `index` staggers the slow ticks, so that the NPCs do not all
/// run on the same tick. `signature` replaces the archetype's signature gene.
/// The world (`world::npc_view`) adds the body and the sprite.
pub fn spawn_npc(
    commands: &mut Commands,
    archetype: Archetype,
    position: Vec2,
    signature: Option<BlockKind>,
    index: u32,
) -> Entity {
    let spec = spec(archetype);
    let genome = npc_genome(archetype, signature);
    let proteome = npc_proteome(&genome, archetype);
    let mut entity = commands.spawn((
        Cell,
        Npc { archetype },
        genome,
        proteome,
        npc_pools(archetype),
        CellSize { radius: spec.size },
        CellTick::every(NPC_TICK_EVERY).with_phase(index % NPC_TICK_EVERY),
        Transform::from_translation(position.extend(0.0)),
        NpcBrain::new(index),
        EngulfIntent(archetype == Archetype::Predator),
        BodyLook(look_for(archetype, signature)),
        Name::new(format!("{archetype:?}")),
    ));
    if archetype == Archetype::Gate {
        entity.insert(GateCell);
    }
    entity.id()
}

// --- Steering (pure) ---

/// Another cell, as an NPC senses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbour {
    pub position: Vec2,
    pub radius: f32,
}

/// The secreted clouds at an NPC: levels, and gradients (any length).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CloudSenses {
    pub microcin: f32,
    pub microcin_gradient: Vec2,
    pub autoinducer: f32,
    pub autoinducer_gradient: Vec2,
}

/// What an NPC senses.
#[derive(Debug, Clone, Copy)]
pub struct Senses<'a> {
    pub position: Vec2,
    pub radius: f32,
    pub sense_radius: f32,
    /// Other cells (not the NPC itself). Cells beyond the sense radius are
    /// ignored.
    pub neighbours: &'a [Neighbour],
    /// The player's cell, for the symbiont.
    pub player: Option<Neighbour>,
    /// Direction of rising minerals (any length).
    pub mineral_gradient: Vec2,
    pub heat: f32,
    /// Direction of rising heat (any length).
    pub heat_gradient: Vec2,
    pub light: f32,
    /// Direction of rising light (any length).
    pub light_gradient: Vec2,
    /// A unit vector: the current wander heading.
    pub wander: Vec2,
    /// The shelters of the level. Hostile cells do not enter them.
    pub shelters: &'a [Shelter],
    /// The clouds that cells secrete.
    pub clouds: CloudSenses,
}

fn within(senses: &Senses, other: &Neighbour) -> bool {
    senses.position.distance(other.position) <= senses.sense_radius
}

/// Away from heat and light, if the NPC is in a hot or bright place.
fn heat_escape(senses: &Senses) -> Option<Vec2> {
    let mut away = Vec2::ZERO;
    if senses.heat > HEAT_AVOID {
        away -= senses.heat_gradient.normalize_or_zero();
    }
    if senses.light > LIGHT_AVOID {
        away -= senses.light_gradient.normalize_or_zero();
    }
    let hazard = senses.heat > HEAT_AVOID || senses.light > LIGHT_AVOID;
    hazard.then(|| away.normalize_or(-senses.wander))
}

/// Away from a microcin cloud, if the level is above `microcin::AVOID`.
fn microcin_escape(senses: &Senses) -> Option<Vec2> {
    (senses.clouds.microcin > crate::secretion::microcin::AVOID)
        .then(|| (-senses.clouds.microcin_gradient).normalize_or(-senses.wander))
}

/// The wish of an NPC with nothing else to do: along the mineral gradient, with
/// a share of the random walk. Slow.
fn forage(senses: &Senses) -> Vec2 {
    let gradient = senses.mineral_gradient.normalize_or_zero();
    (gradient * 0.7 + senses.wander * 0.5).normalize_or_zero() * 0.6
}

/// The direction (length 0 to 1) that an NPC wants to move in. The length is the
/// share of its wanted speed.
pub fn steer(archetype: Archetype, senses: &Senses) -> Vec2 {
    match archetype {
        Archetype::Gate => Vec2::ZERO,
        Archetype::Prey => {
            // Flee every cell that could engulf this one, weighted by nearness.
            let mut away = Vec2::ZERO;
            for other in senses.neighbours.iter().filter(|o| within(senses, o)) {
                if can_engulf(other.radius, senses.radius) {
                    let offset = senses.position - other.position;
                    let distance = offset.length().max(1.0);
                    away += offset / distance * (senses.sense_radius / distance);
                }
            }
            if away != Vec2::ZERO {
                return away.normalize();
            }
            heat_escape(senses)
                .or_else(|| microcin_escape(senses))
                .unwrap_or_else(|| forage(senses))
        }
        Archetype::Predator => {
            // Turn away from a shelter: hostile cells do not enter.
            if let Some(shelter) = senses.shelters.iter().find(|s| {
                s.position.distance(senses.position) < s.radius + senses.radius + SHELTER_KEEP_OUT
            }) {
                return (senses.position - shelter.position).normalize_or(-senses.wander);
            }
            // A cell inside a shelter is not prey.
            let prey = senses
                .neighbours
                .iter()
                .filter(|o| {
                    within(senses, o)
                        && can_engulf(senses.radius, o.radius)
                        && !point_in_shelter(senses.shelters, o.position)
                })
                .min_by(|a, b| {
                    let da = a.position.distance_squared(senses.position);
                    let db = b.position.distance_squared(senses.position);
                    da.total_cmp(&db)
                });
            if let Some(prey) = prey {
                return (prey.position - senses.position).normalize_or_zero();
            }
            heat_escape(senses)
                .or_else(|| {
                    autoinducer::eavesdrop(senses.clouds.autoinducer, senses.clouds.autoinducer_gradient)
                })
                .unwrap_or_else(|| forage(senses))
        }
        Archetype::Symbiont => {
            if let Some(player) = senses.player {
                let gap = senses.position.distance(player.position) - senses.radius - player.radius;
                // In a quorum the symbiont closes in to contact (a clump).
                let follow_gap = autoinducer::follow_gap(senses.clouds.autoinducer, FOLLOW_GAP);
                let strength = ((gap - follow_gap) / FOLLOW_EASE).clamp(0.0, 1.0);
                let toward = (player.position - senses.position).normalize_or_zero();
                if strength > 0.0 {
                    return toward * strength;
                }
                return Vec2::ZERO;
            }
            heat_escape(senses)
                .or_else(|| {
                    autoinducer::recruit(senses.clouds.autoinducer, senses.clouds.autoinducer_gradient)
                })
                .unwrap_or_else(|| forage(senses) * 0.5)
        }
    }
}

/// The part of the full thrust that gives a wanted speed. The terminal speed of
/// a small cell is high, so the input is a fraction.
pub fn speed_fraction(wanted_speed: f32, proteome: &Proteome, size: &CellSize) -> f32 {
    let top = terminal_speed(thrust_capacity(proteome), size);
    if top <= 0.0 || !top.is_finite() {
        return 0.0;
    }
    (wanted_speed / top).clamp(0.0, 1.0)
}

/// Carbon or ATP that moves from the cell with more to the cell with less in
/// `dt` seconds. Both cells move towards the middle, each by `rate`: the
/// difference falls by `exp(-2 x rate x dt)` and never changes sign, so there
/// is no overshoot, and two steps of `dt` equal one step of `2 dt`.
pub fn balance_flow(from: f32, to: f32, rate: f32, dt: f32) -> f32 {
    let difference = from - to;
    if difference <= 0.0 || rate <= 0.0 || dt <= 0.0 {
        return 0.0;
    }
    difference * 0.5 * (1.0 - (-2.0 * rate * dt).exp())
}

// --- Components and systems ---

/// The decision state of an NPC.
#[derive(Component, Debug, Clone, Copy)]
pub struct NpcBrain {
    /// The wander heading, in radians.
    pub heading: f32,
    /// Seconds until the next decision.
    pub think_in: f32,
}

impl NpcBrain {
    pub fn new(index: u32) -> Self {
        Self {
            heading: index as f32 * 2.399, // the golden angle: a different start for each
            think_in: (index % 8) as f32 * THINK_SECONDS / 8.0,
        }
    }
}

/// Random numbers for the wander of the NPCs. Seeded.
#[derive(Resource)]
pub struct NpcRng(pub StdRng);

impl NpcRng {
    pub fn seeded(seed: u64) -> Self {
        Self(StdRng::seed_from_u64(seed))
    }

    pub fn from_entropy() -> Self {
        Self::seeded(rand::rng().random())
    }
}

impl Default for NpcRng {
    fn default() -> Self {
        Self::seeded(0x4E50_43)
    }
}

/// The gradient of a channel at a position, from the grid (any length).
pub fn gradient(grid: &EnvGrid, channel: Channel, position: Vec2) -> Vec2 {
    let h = CELL_SIZE;
    Vec2::new(
        grid.sample(channel, position + Vec2::X * h) - grid.sample(channel, position - Vec2::X * h),
        grid.sample(channel, position + Vec2::Y * h) - grid.sample(channel, position - Vec2::Y * h),
    )
}

/// The secreted clouds at a position. A dormant channel gives 0 without
/// sampling.
pub fn cloud_senses(grid: &EnvGrid, position: Vec2) -> CloudSenses {
    let mut clouds = CloudSenses::default();
    if !grid.is_dormant(Channel::Microcin) {
        clouds.microcin = grid.sample(Channel::Microcin, position);
        clouds.microcin_gradient = gradient(grid, Channel::Microcin, position);
    }
    if !grid.is_dormant(Channel::Autoinducer) {
        clouds.autoinducer = grid.sample(Channel::Autoinducer, position);
        clouds.autoinducer_gradient = gradient(grid, Channel::Autoinducer, position);
    }
    clouds
}

/// Decides the move of each NPC a few times a second.
#[allow(clippy::type_complexity)]
pub fn npc_steering_system(
    time: Res<Time>,
    grid: Option<Res<EnvGrid>>,
    shelters: Res<Shelters>,
    level: Option<Res<crate::level::CurrentLevel>>,
    mut rng: ResMut<NpcRng>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    bodies: Query<(Entity, &Transform, &CellSize), With<Cell>>,
    mut npcs: Query<
        (
            Entity,
            &Npc,
            &Transform,
            &CellSize,
            &Proteome,
            &mut MoveInput,
            &mut NpcBrain,
            Has<Stunned>,
        ),
        With<Cell>,
    >,
) {
    let dt = time.delta_secs();
    // In the light of shallow water, creatures do not run from light.
    let like_light = level.is_some_and(|l| l.0.npcs_like_light);
    let player = players
        .iter()
        .next()
        .and_then(|c| bodies.get(c.0).ok())
        .map(|(entity, transform, size)| {
            (
                entity,
                Neighbour {
                    position: transform.translation.truncate(),
                    radius: world_radius(size),
                },
            )
        });
    let mut neighbours: Vec<Neighbour> = Vec::new();
    for (entity, npc, transform, size, proteome, mut input, mut brain, stunned) in &mut npcs {
        // A stunned NPC does not think or push. Its AI is paused.
        if stunned {
            if input.0 != Vec2::ZERO {
                input.0 = Vec2::ZERO;
            }
            continue;
        }
        brain.think_in -= dt;
        if brain.think_in > 0.0 {
            continue;
        }
        brain.think_in = THINK_SECONDS;
        if npc.archetype == Archetype::Gate {
            continue;
        }
        brain.heading += rng.0.random_range(-1.0..1.0) * WANDER_TURN_RATE * THINK_SECONDS * 2.0;
        let spec = spec(npc.archetype);
        let position = transform.translation.truncate();
        neighbours.clear();
        neighbours.extend(
            bodies
                .iter()
                .filter(|(other, other_transform, _)| {
                    *other != entity
                        && other_transform.translation.truncate().distance(position)
                            <= spec.sense_radius
                })
                .map(|(_, other_transform, other_size)| Neighbour {
                    position: other_transform.translation.truncate(),
                    radius: world_radius(other_size),
                }),
        );
        let (mineral_gradient, heat, heat_gradient, light, light_gradient) = match grid.as_deref() {
            Some(grid) => (
                gradient(grid, Channel::Minerals, position),
                grid.sample(Channel::Heat, position),
                gradient(grid, Channel::Heat, position),
                grid.sample(Channel::Light, position),
                gradient(grid, Channel::Light, position),
            ),
            None => (Vec2::ZERO, 0.0, Vec2::ZERO, 0.0, Vec2::ZERO),
        };
        let (light, light_gradient) = if like_light { (0.0, Vec2::ZERO) } else { (light, light_gradient) };
        let clouds = grid.as_deref().map_or_else(CloudSenses::default, |grid| cloud_senses(grid, position));
        let senses = Senses {
            position,
            radius: world_radius(size),
            sense_radius: spec.sense_radius,
            neighbours: &neighbours,
            player: player.filter(|(p, _)| *p != entity).map(|(_, n)| n),
            mineral_gradient,
            heat,
            heat_gradient,
            light,
            light_gradient,
            wander: Vec2::new(brain.heading.cos(), brain.heading.sin()),
            shelters: &shelters.0,
            clouds,
        };
        let wish = steer(npc.archetype, &senses);
        let wanted = wish * speed_fraction(spec.speed, proteome, size);
        if input.0 != wanted {
            input.0 = wanted;
        }
    }
}

/// A symbiont in contact with the player gives carbon and gets ATP.
pub fn symbiont_exchange_system(
    timing: Res<FlowmapTimingConfig>,
    grid: Option<Res<EnvGrid>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    symbionts: Query<(Entity, &Npc, &Transform, &CellSize)>,
    bodies: Query<(&Transform, &CellSize), With<Cell>>,
    mut pools: Query<&mut CurrencyPools, With<Cell>>,
) {
    let dt = timing.target_delta();
    for controlled in &players {
        let player = controlled.0;
        let Ok((player_transform, player_size)) = bodies.get(player) else {
            continue;
        };
        for (entity, npc, transform, size) in &symbionts {
            if npc.archetype != Archetype::Symbiont {
                continue;
            }
            let gap = transform
                .translation
                .truncate()
                .distance(player_transform.translation.truncate())
                - world_radius(size)
                - world_radius(player_size);
            if gap > SYMBIONT_CONTACT_GAP {
                continue;
            }
            let (Ok(symbiont_pools), Ok(player_pools)) = (pools.get(entity), pools.get(player))
            else {
                continue;
            };
            // A symbiont in an AI-2 quorum trades faster.
            let call = grid.as_deref().map_or(0.0, |g| g.sample(Channel::Autoinducer, transform.translation.truncate()));
            let rate = autoinducer::exchange_rate(call);
            let carbon = balance_flow(
                symbiont_pools.get(Currency::CarbonSkeletons),
                player_pools.get(Currency::CarbonSkeletons),
                rate,
                dt,
            );
            let atp = balance_flow(
                player_pools.get(Currency::ATP),
                symbiont_pools.get(Currency::ATP),
                rate,
                dt,
            );
            if let Ok(mut s) = pools.get_mut(entity) {
                s.modify(Currency::CarbonSkeletons, -carbon);
                s.modify(Currency::ATP, atp);
            }
            if let Ok(mut p) = pools.get_mut(player) {
                p.modify(Currency::CarbonSkeletons, carbon);
                p.modify(Currency::ATP, -atp);
            }
        }
    }
}

/// An NPC died. The world plays its death clip.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct NpcDied {
    pub archetype: Archetype,
    pub position: Vec2,
    /// The body, for the lysis of a shader body.
    pub look: CellLook,
    pub size: CellSize,
}

/// An NPC with a broken membrane (or no ATP) dies and drops food.
pub fn npc_death_system(
    npcs: Query<(Entity, &Npc, &Transform, &CurrencyPools, &Membrane, &CellTick, &CellSize, Option<&BodyLook>), With<Cell>>,
    mut died: MessageWriter<NpcDied>,
    mut commands: Commands,
) {
    for (entity, npc, transform, pools, membrane, tick, size, look) in &npcs {
        // ATP changes only when the cell ticks: check it then.
        if npc.archetype == Archetype::Gate {
            continue;
        }
        let broken = membrane.integrity <= 0.0;
        if !broken && (!tick.due || death_cause(pools, membrane).is_none()) {
            continue;
        }
        let position = transform.translation.truncate();
        spawn_food(&mut commands, position, DEATH_FOOD_CARBON);
        died.write(NpcDied {
            archetype: npc.archetype,
            position,
            look: look.map_or_else(|| look_for(npc.archetype, None), |l| l.0),
            size: *size,
        });
        commands.entity(entity).despawn();
    }
}

// --- Respawn ---

/// The creatures of the level, by slot: `(archetype, position, signature)`.
/// The world fills it when a run starts.
#[derive(Resource, Default, Debug, Clone)]
pub struct NpcSlots(pub Vec<(Archetype, Vec2, Option<BlockKind>)>);

/// The slot of the level that an NPC came from.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnSlot(pub usize);

/// Timers of the respawns.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct RespawnTimers {
    pub general: f32,
    pub key: f32,
}

/// The key-gene rule of the level: while the player lacks `gene`, the world
/// keeps `count` prey that carry it. `None` switches the rule off. The default
/// is the pigment rule of the vents. The world sets it from the `LevelDef`.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct KeyGeneRule(pub Option<(BlockKind, usize)>);

impl Default for KeyGeneRule {
    fn default() -> Self {
        Self(Some((BlockKind::SecondaryMetabolites, PIGMENT_PREY_TARGET)))
    }
}

/// Spawns the NPC of a slot.
pub fn spawn_slot(commands: &mut Commands, slots: &NpcSlots, slot: usize) -> Entity {
    let (archetype, position, signature) = slots.0[slot];
    let entity = spawn_npc(commands, archetype, position, signature, slot as u32);
    commands.entity(entity).insert(SpawnSlot(slot));
    entity
}

/// Empty slots to fill, lowest first, at most `count`. `alive[i]` says if slot
/// `i` has an NPC; `allowed[i]` says if it may spawn now (for example, the
/// player is not too close). With `only_key`, only key-gene slots count.
pub fn slots_to_fill(
    alive: &[bool],
    allowed: &[bool],
    key: &[bool],
    count: usize,
    only_key: bool,
) -> Vec<usize> {
    (0..alive.len())
        .filter(|&i| !alive[i] && allowed[i] && (!only_key || key[i]))
        .take(count)
        .collect()
}

/// How many key-gene prey must be added: while the player lacks the gene, the
/// world keeps `target` of them. Never negative.
pub fn key_prey_needed(player_has_gene: bool, alive: usize, target: usize) -> usize {
    if player_has_gene { 0 } else { target.saturating_sub(alive) }
}

/// Fills empty slots slowly (one NPC every `RESPAWN_SECONDS`, never above the
/// number of slots) and keeps the key-gene prey up while the player lacks the
/// gene of the level (one every `KEY_RESPAWN_SECONDS`): pigment prey in the
/// vents, catalase prey in shallow water.
#[allow(clippy::type_complexity)]
pub fn npc_respawn_system(
    time: Res<Time>,
    slots: Res<NpcSlots>,
    rule: Res<KeyGeneRule>,
    mut timers: ResMut<RespawnTimers>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    player_cells: Query<(&Transform, &Genome), With<Cell>>,
    npcs: Query<&SpawnSlot>,
    mut commands: Commands,
) {
    if slots.0.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let key_gene = rule.0.map(|(gene, _)| gene);
    let target = rule.0.map_or(0, |(_, count)| count);
    let Some((player_position, has_key)) = players
        .iter()
        .find_map(|c| player_cells.get(c.0).ok())
        .map(|(t, g)| {
            (
                t.translation.truncate(),
                key_gene.is_none_or(|gene| {
                    matches!(
                        g.get_gene_state(&gene),
                        Some(GeneState::Expressed | GeneState::Enhanced)
                    )
                }),
            )
        })
    else {
        return;
    };
    let mut alive = vec![false; slots.0.len()];
    for slot in &npcs {
        if let Some(a) = alive.get_mut(slot.0) {
            *a = true;
        }
    }
    let allowed: Vec<bool> = slots
        .0
        .iter()
        .map(|(_, position, _)| position.distance(player_position) > RESPAWN_CLEAR_DISTANCE)
        .collect();
    let key: Vec<bool> = slots
        .0
        .iter()
        .map(|(a, _, s)| *a == Archetype::Prey && key_gene.is_some() && *s == key_gene)
        .collect();

    let missing = alive.iter().filter(|a| !**a).count();
    let (left, due) = crate::particles::timer_step(
        timers.general,
        dt,
        RESPAWN_SECONDS,
        slots.0.len() - missing,
        slots.0.len(),
    );
    timers.general = left;
    let mut fill = slots_to_fill(&alive, &allowed, &key, due, false);

    let alive_key = (0..alive.len()).filter(|&i| alive[i] && key[i]).count();
    let needed = key_prey_needed(has_key, alive_key, target);
    let (left, due) = crate::particles::timer_step(
        timers.key,
        dt,
        KEY_RESPAWN_SECONDS,
        target - needed.min(target),
        target,
    );
    timers.key = left;
    let mut taken = alive.clone();
    for &i in &fill {
        taken[i] = true;
    }
    fill.extend(slots_to_fill(&taken, &allowed, &key, due.min(needed), true));
    for slot in fill {
        spawn_slot(&mut commands, &slots, slot);
    }
}

pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NpcRng>()
            .init_resource::<NpcSlots>()
            .init_resource::<KeyGeneRule>()
            .init_resource::<RespawnTimers>()
            .add_message::<NpcDied>()
            .add_systems(
                Update,
                (npc_steering_system, npc_death_system, npc_respawn_system),
            )
            .add_systems(
                crate::metabolism::MetabolicSchedule,
                symbiont_exchange_system.in_set(MetabolicSet::Exchange),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engulf::CONTACT_GAP;

    fn senses(neighbours: &[Neighbour]) -> Senses<'_> {
        Senses {
            position: Vec2::ZERO,
            radius: 11.25,
            sense_radius: 160.0,
            neighbours,
            player: None,
            mineral_gradient: Vec2::ZERO,
            heat: 0.0,
            heat_gradient: Vec2::ZERO,
            light: 0.0,
            light_gradient: Vec2::ZERO,
            wander: Vec2::X,
            shelters: &[],
            clouds: CloudSenses::default(),
        }
    }

    fn neighbour(x: f32, y: f32, radius: f32) -> Neighbour {
        Neighbour {
            position: Vec2::new(x, y),
            radius,
        }
    }

    #[test]
    fn archetype_sizes_follow_the_engulf_rule() {
        let player = 20.0;
        let radius = |a| spec(a).size * crate::movement::PIXELS_PER_SIZE_UNIT;
        assert!(can_engulf(player, radius(Archetype::Prey)));
        assert!(can_engulf(player, radius(Archetype::Symbiont)));
        assert!(!can_engulf(player, radius(Archetype::Predator)), "the player cannot eat a predator");
        assert!(can_engulf(radius(Archetype::Predator), player), "a predator can eat the player");
        assert!(can_engulf(radius(Archetype::Predator), radius(Archetype::Prey)));
        assert!(!can_engulf(radius(Archetype::Predator), radius(Archetype::Gate)));
    }

    #[test]
    fn prey_flee_a_bigger_cell_inside_the_sense_radius() {
        let predator = neighbour(60.0, 0.0, 25.0);
        let wish = steer(Archetype::Prey, &senses(&[predator]));
        assert!(wish.x < -0.99 && wish.y.abs() < 1e-3, "away from the predator: {wish}");
        // Beyond the sense radius, it does not flee (it forages along the heading).
        let far = neighbour(300.0, 0.0, 25.0);
        let wish = steer(Archetype::Prey, &senses(&[far]));
        assert!(wish.x > 0.0, "{wish}");
        // A smaller cell is no threat.
        let small = neighbour(40.0, 0.0, 5.0);
        assert!(steer(Archetype::Prey, &senses(&[small])).x > 0.0);
        // Two threats: away from the sum, the near one counts more.
        let near = neighbour(50.0, 0.0, 25.0);
        let farther = neighbour(0.0, 120.0, 25.0);
        let wish = steer(Archetype::Prey, &senses(&[near, farther]));
        assert!(wish.x < 0.0 && wish.y < 0.0);
        assert!(wish.x.abs() > wish.y.abs());
    }

    #[test]
    fn prey_follow_minerals_and_leave_the_heat() {
        let mut s = senses(&[]);
        s.mineral_gradient = Vec2::new(0.0, 3.0);
        let wish = steer(Archetype::Prey, &s);
        assert!(wish.y > 0.3, "up the gradient: {wish}");
        assert!(wish.length() <= 0.6 + 1e-4, "foraging is slow");
        s.heat = 30.0;
        s.heat_gradient = Vec2::new(5.0, 0.0);
        let wish = steer(Archetype::Prey, &s);
        assert!(wish.x < -0.99, "down the heat gradient: {wish}");
        s.heat = 0.0;
        s.light = 3.0;
        s.light_gradient = Vec2::new(0.0, 2.0);
        let wish = steer(Archetype::Prey, &s);
        assert!(wish.y < -0.99, "out of the light: {wish}");
    }

    #[test]
    fn predators_chase_the_nearest_smaller_cell() {
        let mut s = senses(&[]);
        s.radius = 25.0;
        s.sense_radius = 230.0;
        let player = neighbour(100.0, 0.0, 20.0);
        let prey = neighbour(0.0, -60.0, 11.25);
        let peer = neighbour(-30.0, 0.0, 25.0);
        let all = [player, prey, peer];
        s.neighbours = &all;
        let wish = steer(Archetype::Predator, &s);
        assert!(wish.y < -0.99, "the nearest smaller cell is the prey: {wish}");
        // Nothing smaller inside the radius: it wanders.
        let far = [neighbour(400.0, 0.0, 11.0), peer];
        s.neighbours = &far;
        s.wander = Vec2::Y;
        let wish = steer(Archetype::Predator, &s);
        assert!(wish.y > 0.0 && wish.x.abs() < 1e-3, "{wish}");
    }

    #[test]
    fn the_symbiont_stays_near_the_player() {
        // A tiny integrator: the symbiont moves at its speed towards its wish.
        let player = neighbour(100.0, 50.0, 20.0);
        let mut position = Vec2::new(-200.0, 150.0);
        let wanted_speed = spec(Archetype::Symbiont).speed;
        let radius = 9.5;
        let mut closest = f32::MAX;
        for step in 0..1200 {
            let mut s = senses(&[]);
            s.position = position;
            s.radius = radius;
            s.player = Some(player);
            let wish = steer(Archetype::Symbiont, &s);
            position += wish * wanted_speed * 0.05;
            let gap = position.distance(player.position) - radius - player.radius;
            closest = closest.min(gap);
            if step > 600 {
                assert!(gap < SYMBIONT_CONTACT_GAP + 2.0 && gap > -2.0, "step {step}: gap {gap}");
            }
        }
        assert!(closest > -2.0, "it does not push into the player: {closest}");
        // Far from the player it moves at full speed.
        let mut s = senses(&[]);
        s.radius = radius;
        s.player = Some(player);
        s.position = Vec2::new(-400.0, 50.0);
        assert!(steer(Archetype::Symbiont, &s).length() > 0.99);
    }

    #[test]
    fn predators_turn_away_from_shelters_and_ignore_prey_inside_them() {
        let shelter = [Shelter { position: Vec2::new(100.0, 0.0), radius: 48.0 }];
        let mut s = senses(&[]);
        s.radius = 25.0;
        s.sense_radius = 230.0;
        s.shelters = &shelter;
        // Near the shelter: away from it.
        s.position = Vec2::new(100.0 - 48.0 - 25.0 - 10.0, 0.0);
        let wish = steer(Archetype::Predator, &s);
        assert!(wish.x < -0.99, "{wish}");
        // Prey inside the shelter is not chased; prey outside is.
        s.position = Vec2::new(-100.0, 0.0);
        let inside = [neighbour(100.0, 10.0, 20.0)];
        s.neighbours = &inside;
        s.wander = Vec2::Y;
        let wish = steer(Archetype::Predator, &s);
        assert!(wish.x.abs() < 1e-3 && wish.y > 0.0, "wanders instead: {wish}");
        let outside = [neighbour(-100.0, -100.0, 20.0)];
        s.neighbours = &outside;
        assert!(steer(Archetype::Predator, &s).y < -0.99);
    }

    #[test]
    fn empty_slots_fill_lowest_first_within_the_count_and_the_allowed_set() {
        let alive = [true, false, false, false, true];
        let allowed = [true, true, false, true, true];
        let pigment = [false, false, true, true, false];
        assert_eq!(slots_to_fill(&alive, &allowed, &pigment, 5, false), vec![1, 3]);
        assert_eq!(slots_to_fill(&alive, &allowed, &pigment, 1, false), vec![1]);
        assert_eq!(slots_to_fill(&alive, &allowed, &pigment, 5, true), vec![3], "slot 2 is not allowed");
        assert!(slots_to_fill(&alive, &allowed, &pigment, 0, false).is_empty());
    }

    #[test]
    fn key_prey_are_topped_up_only_while_the_player_lacks_the_gene() {
        assert_eq!(key_prey_needed(false, 0, PIGMENT_PREY_TARGET), PIGMENT_PREY_TARGET);
        assert_eq!(key_prey_needed(false, 2, 3), 1);
        assert_eq!(key_prey_needed(false, 3, 3), 0);
        assert_eq!(key_prey_needed(false, 5, 3), 0);
        assert_eq!(key_prey_needed(true, 0, 3), 0);
        assert_eq!(key_prey_needed(false, 0, 0), 0, "a level with no rule");
    }

    #[test]
    fn the_gate_cell_does_not_move() {
        let mut s = senses(&[]);
        s.mineral_gradient = Vec2::X;
        assert_eq!(steer(Archetype::Gate, &s), Vec2::ZERO);
    }

    #[test]
    fn a_wanted_speed_gives_a_fraction_of_the_thrust() {
        let prey = spec(Archetype::Prey);
        let size = CellSize { radius: prey.size };
        let proteome = Proteome::default();
        let f = speed_fraction(prey.speed, &proteome, &size);
        assert!(f > 0.0 && f < 1.0);
        assert!((terminal_speed(thrust_capacity(&proteome), &size) * f - prey.speed).abs() < 1e-2);
        // The predator has a flagellum and can reach its speed.
        let genome = npc_genome(Archetype::Predator, None);
        let p = npc_proteome(&genome, Archetype::Predator);
        let size = CellSize { radius: spec(Archetype::Predator).size };
        let f = speed_fraction(spec(Archetype::Predator).speed, &p, &size);
        assert!(f > 0.5 && f < 1.0, "{f}");
    }

    #[test]
    fn the_balance_flow_never_overshoots_and_ignores_the_tick_length() {
        // One long step equals many short ones.
        let (mut a, mut b) = (80.0_f32, 20.0_f32);
        for _ in 0..20 {
            let x = balance_flow(a, b, SYMBIONT_RATE, 0.5);
            a -= x;
            b += x;
        }
        let x = balance_flow(80.0, 20.0, SYMBIONT_RATE, 10.0);
        assert!((a - (80.0 - x)).abs() < 1e-3, "{a} vs {}", 80.0 - x);
        assert!((b - (20.0 + x)).abs() < 1e-3);
        // The levels meet in the middle and the flow stops.
        assert!(a >= 50.0 - 1e-3 && b <= 50.0 + 1e-3);
        let late = balance_flow(1e6, 0.0, SYMBIONT_RATE, 1e6);
        assert!(late <= 5e5 + 1.0, "at most half the difference: {late}");
        assert_eq!(balance_flow(10.0, 30.0, SYMBIONT_RATE, 1.0), 0.0, "never backwards");
        assert_eq!(balance_flow(10.0, 10.0, SYMBIONT_RATE, 1.0), 0.0);
    }

    #[test]
    fn each_archetype_has_a_small_genome_with_a_signature_gene() {
        for archetype in [Archetype::Prey, Archetype::Predator, Archetype::Symbiont, Archetype::Gate] {
            let genome = npc_genome(archetype, None);
            assert!(genome.table.len() <= 3);
            assert!(genome.table.contains_key(&BlockKind::Chemosynthesis));
            assert!(genome.table.contains_key(&spec(archetype).signature.unwrap()));
        }
        let pigment = npc_genome(Archetype::Prey, Some(BlockKind::SecondaryMetabolites));
        assert!(pigment.table.contains_key(&BlockKind::SecondaryMetabolites));
        assert!(!pigment.table.contains_key(&BlockKind::Fermentation));
    }

    #[test]
    fn contact_distance_matches_the_engulf_contact_gap() {
        assert!(SYMBIONT_CONTACT_GAP > CONTACT_GAP, "exchange starts before engulf range");
    }

    #[test]
    fn prey_swim_out_of_a_microcin_cloud() {
        let mut s = senses(&[]);
        s.clouds.microcin = crate::secretion::microcin::AVOID * 4.0;
        s.clouds.microcin_gradient = Vec2::new(2.0, 0.0); // the cloud is to the east
        let wish = steer(Archetype::Prey, &s);
        assert!(wish.x < -0.9, "west, away from the cloud: {wish}");
        // A weak cloud is ignored.
        s.clouds.microcin = crate::secretion::microcin::AVOID * 0.5;
        assert!(steer(Archetype::Prey, &s).x >= 0.0);
        // A predator does not care: it has no receptor.
        s.clouds.microcin = 100.0;
        assert!(steer(Archetype::Predator, &s).x >= 0.0);
    }

    #[test]
    fn a_predator_with_no_prey_follows_the_ai2_call() {
        let mut s = senses(&[]);
        s.radius = 25.0;
        s.clouds.autoinducer = autoinducer::EAVESDROP * 3.0;
        s.clouds.autoinducer_gradient = Vec2::new(0.0, -1.0);
        assert_eq!(steer(Archetype::Predator, &s), Vec2::NEG_Y);
        // Prey in sight comes first.
        let prey = [neighbour(50.0, 0.0, 11.25)];
        let mut s2 = senses(&prey);
        s2.radius = 25.0;
        s2.clouds = s.clouds;
        assert_eq!(steer(Archetype::Predator, &s2), Vec2::X);
    }

    #[test]
    fn a_symbiont_in_quorum_closes_in_to_contact() {
        // The symbiont is 3 px from the player's rim: inside the normal gap.
        let player = neighbour(9.5 + 20.0 + 3.0, 0.0, 20.0);
        let mut s = senses(&[]);
        s.radius = 9.5;
        s.player = Some(player);
        assert_eq!(steer(Archetype::Symbiont, &s), Vec2::ZERO, "near enough");
        s.clouds.autoinducer = autoinducer::QUORUM;
        let wish = steer(Archetype::Symbiont, &s);
        assert!(wish.x > 0.0, "in quorum it closes in: {wish}");
        // With no player it swims up the call.
        s.player = None;
        s.clouds.autoinducer_gradient = Vec2::Y;
        assert_eq!(steer(Archetype::Symbiont, &s), Vec2::Y);
    }
}
