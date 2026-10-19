//! # Engulf and gene transfer
//!
//! A cell engulfs another cell when
//!
//! 1. it **wants** to (`EngulfIntent`): a predator always does, the player holds
//!    the engulf key;
//! 2. the two bodies **touch** (`in_contact`);
//! 3. the **size rule** holds: the engulfer's radius is at least
//!    `ENGULF_SIZE_RATIO` x the target's radius (`can_engulf`).
//!
//! The engulfer then gets `ENGULF_SHARE` of the target's ATP, carbon skeletons
//! and amino acids, and **one gene** (`pick_gene`): a gene that the target
//! expresses and that the engulfer does not have, or has silent or mutated. The
//! gene is added as expressed. The enzyme for it is not free: one enzyme goes
//! in the synthesis queue (`take_gene`). The target is gone. If the target is
//! the player's cell, the run ends with the cause "Engulfed" (see `run`).
//!
//! The gate cell has no size check and is not engulfed by this system (see
//! `gate`). A cell with `Invulnerable` is not engulfed (the smoke test).
//!
//! Gene transfer from a virus uses `take_gene` too (see `particles`).

use bevy::prelude::*;
use rand::prelude::*;

use crate::blocks::block_spec;
use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, CellSize};
use crate::genome_damage::GeneRng;
use crate::metabolism::{CurrencyPools, FlowLedger, FlowSource};
use crate::molecules::Currency;
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{ProteinTarget, Proteome, SynthesisQueue};
use crate::shelter::InShelter;

/// The engulfer's radius must be at least this many times the target's.
pub const ENGULF_SIZE_RATIO: f32 = 1.2;
/// Share of the target's transferred pools that the engulfer gets.
pub const ENGULF_SHARE: f32 = 0.5;
/// Gap (pixels) at which two bodies count as touching. Physics keeps touching
/// bodies at a gap of about 0.
pub const CONTACT_GAP: f32 = 3.0;
/// The pools that an engulfer takes a share of.
pub const TRANSFERRED: [Currency; 3] = [
    Currency::ATP,
    Currency::CarbonSkeletons,
    Currency::AminoAcids,
];

/// Whether a cell tries to engulf what it touches. Predators keep it on; the
/// player's engulf key sets it.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngulfIntent(pub bool);

/// A cell that cannot die or be engulfed. For the smoke test only.
#[derive(Component, Debug, Clone, Copy)]
pub struct Invulnerable;

/// Put on the player's cell when a cell engulfs it. The run ends.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Engulfed;

/// The gate cell: not engulfed by size and contact (see `gate`).
#[derive(Component, Debug, Clone, Copy)]
pub struct GateCell;

// --- Pure rules ---

/// The size rule.
pub fn can_engulf(engulfer_radius: f32, target_radius: f32) -> bool {
    engulfer_radius >= ENGULF_SIZE_RATIO * target_radius
}

/// Do two round bodies touch?
pub fn in_contact(a: Vec2, a_radius: f32, b: Vec2, b_radius: f32) -> bool {
    a.distance(b) <= a_radius + b_radius + CONTACT_GAP
}

/// What the engulfer gets from the target's pools.
pub fn engulf_yield(target: &CurrencyPools) -> [(Currency, f32); 3] {
    TRANSFERRED.map(|c| (c, target.get(c) * ENGULF_SHARE))
}

/// Genes of the target that the engulfer could take: expressed in the target,
/// and absent, silent or mutated in the engulfer. Sorted.
pub fn candidate_genes(target: &Genome, engulfer: &Genome) -> Vec<BlockKind> {
    let mut genes: Vec<BlockKind> = target
        .table
        .iter()
        .filter(|(_, state)| matches!(state, GeneState::Expressed | GeneState::Enhanced))
        .filter(|(kind, _)| {
            matches!(
                engulfer.get_gene_state(kind),
                None | Some(GeneState::Silent | GeneState::Mutated)
            )
        })
        .map(|(kind, _)| *kind)
        .collect();
    genes.sort();
    genes
}

/// Picks one candidate gene with the seeded `rng`. `None` if there is none.
/// With no candidate, the function takes no number from `rng`.
pub fn pick_gene(target: &Genome, engulfer: &Genome, rng: &mut impl Rng) -> Option<BlockKind> {
    let genes = candidate_genes(target, engulfer);
    (!genes.is_empty()).then(|| genes[rng.random_range(0..genes.len())])
}

/// Gives a gene: it is added expressed (a silent or mutated gene becomes
/// expressed). If the block exists and the cell has no enzyme for it, and none
/// in the queue, one enzyme goes in the synthesis queue: it must be built.
pub fn take_gene(
    genome: &mut Genome,
    proteome: &Proteome,
    queue: &mut SynthesisQueue,
    kind: BlockKind,
) {
    genome.table.insert(kind, GeneState::Expressed);
    let target = ProteinTarget::Enzyme(kind);
    if block_spec(kind).is_some()
        && proteome.count(target) == 0
        && !queue.orders.contains(&target)
    {
        queue.orders.push_back(target);
    }
}

// --- System ---

/// A cell engulfed another cell.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct EngulfEvent {
    pub engulfer: Entity,
    pub target: Entity,
    /// The gene that the engulfer took, if there was one.
    pub gene: Option<BlockKind>,
    /// What the target was, if it was an NPC (the player's cell: `None`).
    pub target_archetype: Option<crate::npc::Archetype>,
    /// Direction from the engulfer to the target (unit vector).
    pub direction: Vec2,
    /// Position of the engulfer.
    pub position: Vec2,
}

/// A cell as `plan_engulfs` sees it.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub entity: Entity,
    pub position: Vec2,
    pub radius: f32,
    /// Wants to engulf.
    pub intent: bool,
    /// Can be engulfed at all (not the gate cell, not invulnerable).
    pub edible: bool,
    /// Inside a shelter: hostile cells (NPCs) cannot engulf it.
    pub sheltered: bool,
    /// The local player's cell.
    pub player: bool,
    /// What a clump of symbionts adds to the radius as a target (pixels, see
    /// `secretion::autoinducer::Clump`).
    pub clump: f32,
}

impl Body {
    pub fn new(entity: Entity, position: Vec2, radius: f32, intent: bool, edible: bool) -> Self {
        Self {
            entity,
            position,
            radius,
            intent,
            edible,
            sheltered: false,
            player: false,
            clump: 0.0,
        }
    }
}

/// Pairs `(engulfer, target)` from a snapshot of the cells. Each cell is in at
/// most one pair as the target, and a target does not engulf. The engulfer
/// takes the nearest target. A sheltered target is safe from every cell except
/// the player's.
pub fn plan_engulfs(bodies: &[Body]) -> Vec<(Entity, Entity)> {
    let mut sorted = bodies.to_vec();
    sorted.sort_by_key(|b| b.entity);
    let mut eaten: Vec<Entity> = Vec::new();
    let mut pairs = Vec::new();
    for me in &sorted {
        if !me.intent || eaten.contains(&me.entity) {
            continue;
        }
        let nearest = sorted
            .iter()
            .filter(|o| {
                o.entity != me.entity
                    && o.edible
                    && (!o.sheltered || me.player)
                    && !eaten.contains(&o.entity)
                    && can_engulf(me.radius, o.radius + o.clump)
                    && in_contact(me.position, me.radius, o.position, o.radius)
            })
            .min_by(|a, b| {
                a.position
                    .distance_squared(me.position)
                    .total_cmp(&b.position.distance_squared(me.position))
                    .then(a.entity.cmp(&b.entity))
            });
        if let Some(target) = nearest {
            eaten.push(target.entity);
            pairs.push((me.entity, target.entity));
        }
    }
    pairs
}

#[allow(clippy::type_complexity)]
pub fn engulf_system(
    mut rng: ResMut<GeneRng>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    snapshot: Query<
        (
            Entity,
            &Transform,
            &CellSize,
            &EngulfIntent,
            Has<GateCell>,
            Has<Invulnerable>,
            Has<Engulfed>,
            &InShelter,
            Has<crate::stun::Stunned>,
            Option<&crate::secretion::autoinducer::Clump>,
        ),
        With<Cell>,
    >,
    mut cells: Query<
        (
            &Transform,
            &mut CurrencyPools,
            &mut Genome,
            &mut SynthesisQueue,
            &Proteome,
            &mut FlowLedger,
        ),
        With<Cell>,
    >,
    npcs: Query<&crate::npc::Npc>,
    mut events: MessageWriter<EngulfEvent>,
    mut commands: Commands,
) {
    let player_cells: Vec<Entity> = players.iter().map(|c| c.0).collect();
    let bodies: Vec<Body> = snapshot
        .iter()
        .map(|(entity, transform, size, intent, gate, invulnerable, engulfed, shelter, stunned, clump)| Body {
            sheltered: shelter.0,
            player: player_cells.contains(&entity),
            clump: clump.map_or(0.0, |c| c.0),
            ..Body::new(
                entity,
                transform.translation.truncate(),
                world_radius(size),
                // A stunned cell cannot act, so it cannot engulf.
                intent.0 && !stunned,
                !gate && !invulnerable && !engulfed,
            )
        })
        .collect();
    if !bodies.iter().any(|b| b.intent) {
        return;
    }
    for (engulfer, target) in plan_engulfs(&bodies) {
        let Ok((target_transform, target_pools, target_genome, ..)) = cells.get(target) else {
            continue;
        };
        let target_position = target_transform.translation.truncate();
        let gains = engulf_yield(target_pools);
        let target_genome = target_genome.clone();
        let Ok((transform, mut pools, mut genome, mut queue, proteome, mut ledger)) =
            cells.get_mut(engulfer)
        else {
            continue;
        };
        for (currency, amount) in gains {
            ledger.apply(&mut pools, FlowSource::Engulf, currency, amount);
        }
        let gene = pick_gene(&target_genome, &genome, &mut rng.0);
        if let Some(kind) = gene {
            take_gene(&mut genome, proteome, &mut queue, kind);
        }
        let position = transform.translation.truncate();
        events.write(EngulfEvent {
            engulfer,
            target,
            gene,
            target_archetype: npcs.get(target).ok().map(|n| n.archetype),
            direction: (target_position - position).normalize_or(Vec2::X),
            position,
        });
        if player_cells.contains(&target) {
            commands.entity(target).insert(Engulfed);
        } else {
            commands.entity(target).despawn();
        }
    }
}

pub struct EngulfPlugin;

impl Plugin for EngulfPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EngulfEvent>()
            .add_systems(Update, engulf_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genome_of(genes: &[(BlockKind, GeneState)]) -> Genome {
        let mut genome = Genome::default();
        for (kind, state) in genes {
            genome.table.insert(*kind, state.clone());
        }
        genome
    }

    fn e(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    #[test]
    fn the_size_rule_needs_1_2_times_the_radius() {
        assert!(can_engulf(24.0, 20.0));
        assert!(!can_engulf(23.9, 20.0));
        assert!(can_engulf(20.0, 11.25), "the player can eat prey");
        assert!(!can_engulf(20.0, 25.0), "and cannot eat the gate cell by size");
        assert!(!can_engulf(20.0, 20.0));
    }

    #[test]
    fn contact_is_touching_bodies() {
        assert!(in_contact(Vec2::ZERO, 20.0, Vec2::new(31.0, 0.0), 11.0));
        assert!(in_contact(Vec2::ZERO, 20.0, Vec2::new(33.0, 0.0), 11.0), "within the gap");
        assert!(!in_contact(Vec2::ZERO, 20.0, Vec2::new(40.0, 0.0), 11.0));
    }

    #[test]
    fn the_engulfer_gets_half_of_three_pools() {
        let mut pools = CurrencyPools::default();
        pools.set(Currency::ATP, 80.0);
        pools.set(Currency::CarbonSkeletons, 30.0);
        pools.set(Currency::AminoAcids, 10.0);
        pools.set(Currency::Lipids, 99.0);
        let gains = engulf_yield(&pools);
        assert_eq!(gains[0], (Currency::ATP, 40.0));
        assert_eq!(gains[1], (Currency::CarbonSkeletons, 15.0));
        assert_eq!(gains[2], (Currency::AminoAcids, 5.0));
        assert!(gains.iter().all(|(c, _)| *c != Currency::Lipids));
    }

    #[test]
    fn only_missing_silent_or_mutated_genes_can_be_taken() {
        let target = genome_of(&[
            (BlockKind::Chemosynthesis, GeneState::Expressed),
            (BlockKind::Fermentation, GeneState::Expressed),
            (BlockKind::FatStorage, GeneState::Expressed),
            (BlockKind::SecondaryMetabolites, GeneState::Silent),
            (BlockKind::VesicleExport, GeneState::Mutated),
            (BlockKind::LipidMetabolism, GeneState::Enhanced),
        ]);
        let engulfer = genome_of(&[
            (BlockKind::Chemosynthesis, GeneState::Expressed), // has it
            (BlockKind::Fermentation, GeneState::Silent),      // silent: can take
            (BlockKind::LipidMetabolism, GeneState::Enhanced), // has it (enhanced)
        ]);
        assert_eq!(
            candidate_genes(&target, &engulfer),
            vec![BlockKind::Fermentation, BlockKind::FatStorage],
            "not silent or mutated genes of the target, not genes already expressed"
        );
        let mutated_in_engulfer = genome_of(&[(BlockKind::FatStorage, GeneState::Mutated)]);
        assert!(candidate_genes(&target, &mutated_in_engulfer).contains(&BlockKind::FatStorage));
    }

    #[test]
    fn the_same_seed_picks_the_same_gene_and_nothing_to_take_is_none() {
        let target = genome_of(&[
            (BlockKind::Fermentation, GeneState::Expressed),
            (BlockKind::FatStorage, GeneState::Expressed),
            (BlockKind::VesicleExport, GeneState::Expressed),
        ]);
        let engulfer = Genome::default();
        let pick = |seed| pick_gene(&target, &engulfer, &mut StdRng::seed_from_u64(seed));
        assert_eq!(pick(5), pick(5));
        let picks: std::collections::HashSet<_> = (0..40).map(pick).collect();
        assert!(picks.len() > 1, "the seed matters: {picks:?}");
        assert!(picks.iter().all(Option::is_some));
        assert_eq!(pick_gene(&Genome::default(), &engulfer, &mut StdRng::seed_from_u64(1)), None);
    }

    #[test]
    fn a_taken_gene_is_expressed_and_its_enzyme_is_queued_once() {
        let mut genome = genome_of(&[(BlockKind::Fermentation, GeneState::Mutated)]);
        let proteome = Proteome::default();
        let mut queue = SynthesisQueue::default();
        take_gene(&mut genome, &proteome, &mut queue, BlockKind::Fermentation);
        assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Expressed));
        assert_eq!(queue.orders.len(), 1, "not free: one enzyme to build");
        take_gene(&mut genome, &proteome, &mut queue, BlockKind::Fermentation);
        assert_eq!(queue.orders.len(), 1, "no second order");
        // A cell that has an enzyme already queues nothing.
        let proteome = Proteome::with_enzymes(&[(BlockKind::FatStorage, 1)]);
        take_gene(&mut genome, &proteome, &mut queue, BlockKind::FatStorage);
        assert_eq!(queue.orders.len(), 1);
        // A gene with no block (the land gate cell's) has nothing to build.
        take_gene(&mut genome, &proteome, &mut queue, BlockKind::Multicellularity);
        assert_eq!(genome.get_gene_state(&BlockKind::Multicellularity), Some(&GeneState::Expressed));
        assert_eq!(queue.orders.len(), 1);
    }

    #[test]
    fn plans_follow_intent_size_and_contact() {
        let big = Body::new(e(1), Vec2::ZERO, 25.0, true, true);
        let small = Body::new(e(2), Vec2::new(30.0, 0.0), 11.0, false, true);
        let far = Body::new(e(3), Vec2::new(500.0, 0.0), 11.0, false, true);
        let peer = Body::new(e(4), Vec2::new(0.0, 40.0), 25.0, false, true);
        assert_eq!(plan_engulfs(&[big, small, far, peer]), vec![(e(1), e(2))]);
        // No intent, no engulf.
        let passive = Body::new(e(1), Vec2::ZERO, 25.0, false, true);
        assert!(plan_engulfs(&[passive, small]).is_empty());
        // A target that cannot be engulfed (the gate cell) is left alone.
        let gate = Body::new(e(5), Vec2::new(30.0, 0.0), 11.0, false, false);
        assert!(plan_engulfs(&[big, gate]).is_empty());
    }

    #[test]
    fn a_cell_is_engulfed_once() {
        // Both big cells touch the prey: only one of them gets it.
        let a = Body::new(e(1), Vec2::ZERO, 25.0, true, true);
        let b = Body::new(e(2), Vec2::new(60.0, 0.0), 25.0, true, true);
        let prey = Body::new(e(3), Vec2::new(30.0, 0.0), 11.0, false, true);
        let pairs = plan_engulfs(&[a, b, prey]);
        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!(pairs[0].1, e(3));
        // The same input gives the same pair.
        assert_eq!(plan_engulfs(&[prey, b, a]), pairs);
    }

    #[test]
    fn a_sheltered_target_is_safe_from_npcs_but_not_from_the_player() {
        let predator = Body::new(e(1), Vec2::ZERO, 25.0, true, true);
        let mut victim = Body::new(e(2), Vec2::new(30.0, 0.0), 11.0, false, true);
        assert_eq!(plan_engulfs(&[predator, victim]).len(), 1);
        victim.sheltered = true;
        assert!(plan_engulfs(&[predator, victim]).is_empty(), "safe in a shelter");
        let player = Body { player: true, ..predator };
        assert_eq!(plan_engulfs(&[player, victim]).len(), 1, "the player may still engulf");
    }

    #[test]
    fn an_engulfer_takes_the_nearest_target() {
        let a = Body::new(e(1), Vec2::ZERO, 25.0, true, true);
        let near = Body::new(e(2), Vec2::new(25.0, 0.0), 11.0, false, true);
        let far = Body::new(e(3), Vec2::new(-34.0, 0.0), 11.0, false, true);
        assert_eq!(plan_engulfs(&[a, far, near]), vec![(e(1), e(2))]);
    }

    #[test]
    fn a_clump_of_symbionts_saves_the_player_from_a_predator() {
        let predator = Body::new(e(1), Vec2::ZERO, 25.0, true, true);
        let mut player = Body::new(e(2), Vec2::new(44.0, 0.0), 20.0, false, true);
        assert_eq!(plan_engulfs(&[predator, player]).len(), 1);
        player.clump = crate::secretion::autoinducer::clump_radius(20.0, &[9.5, 9.5]) - 20.0;
        assert!(plan_engulfs(&[predator, player]).is_empty(), "too big as a clump");
    }
}
