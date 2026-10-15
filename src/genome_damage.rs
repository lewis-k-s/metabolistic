//! # Genome damage
//!
//! UV light hurts a cell in two ways. Pigments screen both.
//!
//! - **Membrane**: UV lowers membrane integrity at once (lipid peroxidation).
//!   The damage goes to `DamageTaken` as `DamageSource::Uv`.
//! - **Genome**: UV lowers `GenomeIntegrity` (a value in [0, 1]) and adds to
//!   the `MutationLoad`. Each time the load passes 1, one expressed gene
//!   becomes `Mutated`, so its block runs at half rate. The load is an
//!   accumulator, not a chance per tick, so the tick length does not change the
//!   number of mutations. A seeded RNG (`GeneRng`) picks the gene.
//!
//! Genome integrity has two more effects:
//!
//! - **Translation efficiency** (`translation_efficiency`) multiplies the
//!   protein synthesis rate. It falls with integrity. Below
//!   `FRAGMENT_THRESHOLD` the genome is fragmented: efficiency is 0, so the
//!   cell makes no new proteins.
//! - **Repair** (`repair_step`) costs ATP and Nucleotides. It raises integrity
//!   and turns `Mutated` genes back to `Expressed`. It stops at the gap and
//!   when a pool runs out.
//!
//! Pigments (`Currency::Pigments`) are the shield: `shield = P / (K + P)`. The
//! shield never reaches 1. Pigments decay slowly with an exact decay.
//!
//! UV is the `Channel::Light` level at the cell times `UvFraction`, a per-level
//! constant. In the vents all light is hazard, so the fraction is 1.
//!
//! The pure functions here do the work. The systems fetch data and call them.

use bevy::prelude::*;
use rand::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, DamageSource, DamageTaken};
use crate::environment::{Channel, LocalEnvironment};
use crate::membrane::Membrane;
use crate::metabolism::{CurrencyPools, FlowmapTimingConfig, MetabolicSet, decay_fraction};
use crate::molecules::Currency;

// --- Tuning. All of these need **playtest**. ---

/// Share of the light level that counts as UV in the vent level.
pub const VENT_UV_FRACTION: f32 = 1.0;
/// Membrane integrity points lost per second for each UV unit.
pub const UV_MEMBRANE_PER_UV_SEC: f32 = 0.8;
/// Genome integrity lost per second for each UV unit.
pub const UV_GENOME_PER_UV_SEC: f32 = 0.008;
/// Mutation load gained per second for each UV unit.
pub const UV_MUTATION_PER_UV_SEC: f32 = 0.04;

/// Below this genome integrity the genome is fragmented: no new proteins.
pub const FRAGMENT_THRESHOLD: f32 = 0.25;
/// Translation efficiency just above the fragment threshold.
pub const MIN_TRANSLATION_EFFICIENCY: f32 = 0.2;

/// Pigment level at which the shield is 0.5.
pub const SHIELD_HALF_SATURATION: f32 = 10.0;
/// The shield is never more than this, so float rounding cannot make it 1.
pub const MAX_SHIELD: f32 = 0.999;
/// Pigment decay per second. A half-life of about 70 s.
pub const PIGMENT_DECAY_PER_SEC: f32 = 0.01;

/// Repair work per second. Work 1.0 raises integrity from 0 to 1.
pub const REPAIR_WORK_PER_SEC: f32 = 0.02;
/// ATP for one unit of repair work.
pub const REPAIR_ATP_PER_WORK: f32 = 60.0;
/// Nucleotides for one unit of repair work.
pub const REPAIR_NUCLEOTIDES_PER_WORK: f32 = 30.0;
/// Repair work that turns one mutated gene back to expressed.
pub const GENE_REPAIR_WORK: f32 = 0.1;
/// Float tolerance, in work units, when progress is compared with a gene's cost.
const WORK_EPSILON: f32 = 1e-6;

/// Seed of `GeneRng` unless the game sets another one.
pub const DEFAULT_GENE_SEED: u64 = 0x5EED_0F_6E_4E;

// --- Components and resources ---

/// How intact the cell's genome is: 1 is whole, 0 is destroyed.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GenomeIntegrity(pub f32);

impl Default for GenomeIntegrity {
    fn default() -> Self {
        Self(1.0)
    }
}

/// UV damage that waits to become mutations. Each full unit mutates one gene.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct MutationLoad(pub f32);

/// Repair work done on the next mutated gene, in work units.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct RepairProgress(pub f32);

/// The share of the light level that is UV, set by the level.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct UvFraction(pub f32);

impl Default for UvFraction {
    fn default() -> Self {
        Self(VENT_UV_FRACTION)
    }
}

/// The random numbers that pick the mutated gene. Seeded, so tests are
/// deterministic.
#[derive(Resource)]
pub struct GeneRng(pub StdRng);

impl GeneRng {
    pub fn seeded(seed: u64) -> Self {
        Self(StdRng::seed_from_u64(seed))
    }

    /// A seed from the system, for the game.
    pub fn from_entropy() -> Self {
        Self::seeded(rand::rng().random())
    }
}

impl Default for GeneRng {
    fn default() -> Self {
        Self::seeded(DEFAULT_GENE_SEED)
    }
}

// --- Pure functions ---

/// UV at a place from the light level there.
pub fn uv_level(light: f32, fraction: f32) -> f32 {
    light.max(0.0) * fraction.max(0.0)
}

/// UV shield from the pigment level: `P / (K + P)`. It is 0 without pigments
/// and never reaches 1.
pub fn shield(pigments: f32) -> f32 {
    if pigments <= 0.0 {
        return 0.0;
    }
    (pigments / (SHIELD_HALF_SATURATION + pigments)).min(MAX_SHIELD)
}

/// Pigments left after `dt` seconds of decay.
pub fn decayed_pigments(pigments: f32, dt: f32) -> f32 {
    pigments.max(0.0) * (1.0 - decay_fraction(PIGMENT_DECAY_PER_SEC, dt))
}

/// The UV that gets through the shield.
pub fn effective_uv(uv: f32, shield: f32) -> f32 {
    uv.max(0.0) * (1.0 - shield.clamp(0.0, 1.0))
}

/// Membrane integrity points lost in `dt` seconds.
pub fn uv_membrane_damage(uv: f32, shield: f32, dt: f32) -> f32 {
    effective_uv(uv, shield) * UV_MEMBRANE_PER_UV_SEC * dt.max(0.0)
}

/// Genome integrity lost in `dt` seconds.
pub fn uv_genome_damage(uv: f32, shield: f32, dt: f32) -> f32 {
    effective_uv(uv, shield) * UV_GENOME_PER_UV_SEC * dt.max(0.0)
}

/// Mutation load gained in `dt` seconds.
pub fn uv_mutation_load(uv: f32, shield: f32, dt: f32) -> f32 {
    effective_uv(uv, shield) * UV_MUTATION_PER_UV_SEC * dt.max(0.0)
}

/// Factor on the protein synthesis rate. It grows with integrity. It is 0
/// below `FRAGMENT_THRESHOLD` (a fragmented genome) and 1 at full integrity.
pub fn translation_efficiency(integrity: f32) -> f32 {
    if integrity < FRAGMENT_THRESHOLD {
        return 0.0;
    }
    let t = ((integrity - FRAGMENT_THRESHOLD) / (1.0 - FRAGMENT_THRESHOLD)).clamp(0.0, 1.0);
    MIN_TRANSLATION_EFFICIENCY + (1.0 - MIN_TRANSLATION_EFFICIENCY) * t
}

/// True if the genome is too damaged to make new proteins.
pub fn is_fragmented(integrity: f32) -> bool {
    integrity < FRAGMENT_THRESHOLD
}

/// Uses up the whole units of mutation `load`: each unit mutates one of the
/// expressed genes, picked with `rng`. Returns the load that is left and the
/// genes that mutated, in order. With no expressed gene left, the whole units
/// are dropped and only the fraction below 1 stays, so a gene that repair
/// restores later is not hit again at once by a backlog.
pub fn apply_mutation_load(
    genome: &mut Genome,
    load: f32,
    rng: &mut impl Rng,
) -> (f32, Vec<BlockKind>) {
    let mut load = load.max(0.0);
    let mut mutated = Vec::new();
    while load >= 1.0 {
        let mut candidates = genome.get_expressed_genes();
        if candidates.is_empty() {
            return (load.fract(), mutated);
        }
        candidates.sort();
        let kind = candidates[rng.random_range(0..candidates.len())];
        genome.mutate_gene(kind);
        mutated.push(kind);
        load -= 1.0;
    }
    (load, mutated)
}

/// What one tick of repair does.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RepairStep {
    /// Integrity gained.
    pub integrity: f32,
    /// New repair progress on the next mutated gene.
    pub progress: f32,
    /// Mutated genes that return to expressed.
    pub genes: u32,
    pub atp: f32,
    pub nucleotides: f32,
}

/// Repair in a tick of `dt` seconds. The work is `REPAIR_WORK_PER_SEC x dt`. It
/// is split between the integrity and the mutated genes (all of it goes to
/// one of them if the other needs nothing). It never goes past the gap, and
/// it is limited by the ATP and the Nucleotides in the pools.
pub fn repair_step(
    integrity: f32,
    progress: f32,
    mutated: u32,
    atp: f32,
    nucleotides: f32,
    dt: f32,
) -> RepairStep {
    let integrity_gap = (1.0 - integrity).max(0.0);
    let gene_gap = (mutated as f32 * GENE_REPAIR_WORK - progress).max(0.0);
    if (integrity_gap <= 0.0 && gene_gap <= 0.0) || dt <= 0.0 {
        return RepairStep { progress, ..default() };
    }
    let affordable = (atp.max(0.0) / REPAIR_ATP_PER_WORK)
        .min(nucleotides.max(0.0) / REPAIR_NUCLEOTIDES_PER_WORK);
    let budget = (REPAIR_WORK_PER_SEC * dt).min(affordable);

    let (integrity_share, gene_share) = match (integrity_gap > 0.0, gene_gap > 0.0) {
        (true, true) => (budget * 0.5, budget * 0.5),
        (true, false) => (budget, 0.0),
        _ => (0.0, budget),
    };
    // Work that one side cannot use goes to the other side.
    let mut integrity_work = integrity_share.min(integrity_gap);
    let mut gene_work = gene_share.min(gene_gap);
    gene_work = (gene_work + (budget - integrity_work - gene_work)).min(gene_gap);
    integrity_work = (integrity_work + (budget - integrity_work - gene_work)).min(integrity_gap);

    // The remainder is signed and carried over, so a gene always costs
    // `GENE_REPAIR_WORK` in total, up to `WORK_EPSILON`.
    let new_progress = progress + gene_work;
    let genes = (((new_progress + WORK_EPSILON) / GENE_REPAIR_WORK).floor().max(0.0) as u32)
        .min(mutated);
    let new_progress = new_progress - genes as f32 * GENE_REPAIR_WORK;
    let work = integrity_work + gene_work;
    RepairStep {
        integrity: integrity_work,
        progress: new_progress,
        genes,
        atp: work * REPAIR_ATP_PER_WORK,
        nucleotides: work * REPAIR_NUCLEOTIDES_PER_WORK,
    }
}

// --- Systems ---

/// UV lowers the membrane and the genome, and adds to the mutation load.
#[allow(clippy::type_complexity)]
pub fn uv_damage_system(
    timing: Res<FlowmapTimingConfig>,
    fraction: Res<UvFraction>,
    mut cells: Query<
        (
            &LocalEnvironment,
            &CurrencyPools,
            &mut Membrane,
            &mut DamageTaken,
            &mut GenomeIntegrity,
            &mut MutationLoad,
        ),
        With<Cell>,
    >,
) {
    let dt = timing.target_delta();
    for (env, pools, mut membrane, mut taken, mut genome_integrity, mut load) in &mut cells {
        let uv = uv_level(env.get(Channel::Light), fraction.0);
        if uv <= 0.0 {
            continue;
        }
        let shield = shield(pools.get(Currency::Pigments));
        let membrane_damage = uv_membrane_damage(uv, shield, dt);
        if membrane_damage > 0.0 {
            membrane.damage(membrane_damage);
            taken.add(DamageSource::Uv, membrane_damage);
        }
        genome_integrity.0 = (genome_integrity.0 - uv_genome_damage(uv, shield, dt)).max(0.0);
        load.0 += uv_mutation_load(uv, shield, dt);
    }
}

/// Turns full units of mutation load into mutated genes.
pub fn mutation_system(
    mut rng: ResMut<GeneRng>,
    mut cells: Query<(&mut Genome, &mut MutationLoad), With<Cell>>,
) {
    for (mut genome, mut load) in &mut cells {
        if load.0 < 1.0 {
            continue;
        }
        let (left, mutated) = apply_mutation_load(&mut genome, load.0, &mut rng.0);
        load.0 = left;
        for kind in mutated {
            debug!("UV mutated gene {kind:?}");
        }
    }
}

/// Repairs the genome with ATP and Nucleotides.
pub fn genome_repair_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<
        (
            &mut Genome,
            &mut GenomeIntegrity,
            &mut RepairProgress,
            &mut CurrencyPools,
        ),
        With<Cell>,
    >,
) {
    let dt = timing.target_delta();
    for (mut genome, mut integrity, mut progress, mut pools) in &mut cells {
        // Most ticks there is nothing to repair: no allocation then.
        let mutated = genome
            .table
            .values()
            .filter(|state| **state == GeneState::Mutated)
            .count() as u32;
        if mutated == 0 && integrity.0 >= 1.0 {
            continue;
        }
        let step = repair_step(
            integrity.0,
            progress.0,
            mutated,
            pools.get(Currency::ATP),
            pools.get(Currency::Nucleotides),
            dt,
        );
        if progress.0 != step.progress {
            progress.0 = step.progress;
        }
        if step.integrity > 0.0 {
            integrity.0 = (integrity.0 + step.integrity).min(1.0);
        }
        if step.atp > 0.0 {
            pools.modify(Currency::ATP, -step.atp);
            pools.modify(Currency::Nucleotides, -step.nucleotides);
        }
        if step.genes > 0 {
            let mut kinds: Vec<BlockKind> = genome
                .table
                .iter()
                .filter(|(_, state)| **state == GeneState::Mutated)
                .map(|(kind, _)| *kind)
                .collect();
            kinds.sort();
            for &kind in kinds.iter().take(step.genes as usize) {
                genome.restore_gene(kind);
            }
        }
    }
}

/// Pigments decay slowly.
pub fn pigment_decay_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<&mut CurrencyPools, With<Cell>>,
) {
    let dt = timing.target_delta();
    for mut pools in &mut cells {
        let pigments = pools.get(Currency::Pigments);
        if pigments > 0.0 {
            pools.set(Currency::Pigments, decayed_pigments(pigments, dt));
        }
    }
}

pub struct GenomeDamagePlugin;

impl Plugin for GenomeDamagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UvFraction>()
            .init_resource::<GeneRng>()
            .add_systems(
                crate::metabolism::MetabolicSchedule,
                (
                    mutation_system.in_set(MetabolicSet::Genome),
                    (genome_repair_system, pigment_decay_system).in_set(MetabolicSet::Build),
                    uv_damage_system
                        .in_set(MetabolicSet::Exchange)
                        .after(crate::exchange::heat_damage_system),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(7)
    }

    fn genome_with(kinds: &[BlockKind]) -> Genome {
        let mut genome = Genome::default();
        for &kind in kinds {
            genome.add_gene(kind);
            genome.express_gene(kind);
        }
        genome
    }

    #[test]
    fn the_shield_grows_with_pigments_and_is_never_one() {
        assert_eq!(shield(0.0), 0.0);
        assert_abs_diff_eq!(shield(SHIELD_HALF_SATURATION), 0.5);
        let mut last = 0.0;
        for p in [1.0, 5.0, 30.0, 300.0, 1e4, 1e9, f32::MAX] {
            let s = shield(p);
            assert!(s >= last && s < 1.0, "p {p}: {s}");
            last = s;
        }
    }

    #[test]
    fn the_shield_cuts_all_three_damages() {
        let s = shield(10.0);
        for (a, b) in [
            (uv_membrane_damage(5.0, 0.0, 1.0), uv_membrane_damage(5.0, s, 1.0)),
            (uv_genome_damage(5.0, 0.0, 1.0), uv_genome_damage(5.0, s, 1.0)),
            (uv_mutation_load(5.0, 0.0, 1.0), uv_mutation_load(5.0, s, 1.0)),
        ] {
            assert_abs_diff_eq!(b, a * (1.0 - s), epsilon = 1e-6);
            assert!(b > 0.0);
        }
        assert_eq!(uv_membrane_damage(0.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn translation_efficiency_is_monotonic_and_zero_when_fragmented() {
        assert_eq!(translation_efficiency(1.0), 1.0);
        assert_eq!(translation_efficiency(0.0), 0.0);
        assert_eq!(translation_efficiency(FRAGMENT_THRESHOLD - 0.001), 0.0);
        assert!(translation_efficiency(FRAGMENT_THRESHOLD) > 0.0);
        let mut last = 0.0;
        for i in 0..=100 {
            let e = translation_efficiency(i as f32 / 100.0);
            assert!(e >= last, "not monotonic at {i}");
            last = e;
        }
        assert!(is_fragmented(0.1) && !is_fragmented(0.5));
    }

    #[test]
    fn pigments_decay_exactly() {
        let once = decayed_pigments(10.0, 10.0);
        let split = (0..10).fold(10.0, |p, _| decayed_pigments(p, 1.0));
        assert_abs_diff_eq!(once, split, epsilon = 1e-4);
        assert!(once < 10.0 && once > 9.0, "slow decay: {once}");
    }

    #[test]
    fn load_mutates_one_gene_per_unit_and_keeps_the_rest() {
        let mut genome = genome_with(&[BlockKind::Chemosynthesis, BlockKind::Fermentation]);
        let (left, mutated) = apply_mutation_load(&mut genome, 0.9, &mut rng());
        assert_eq!((left, mutated.len()), (0.9, 0));
        let (left, mutated) = apply_mutation_load(&mut genome, 1.4, &mut rng());
        assert_abs_diff_eq!(left, 0.4, epsilon = 1e-6);
        assert_eq!(mutated.len(), 1);
        assert_eq!(genome.get_expressed_genes().len(), 1);
    }

    #[test]
    fn the_same_seed_picks_the_same_genes() {
        let kinds = [
            BlockKind::Chemosynthesis,
            BlockKind::AminoAcidBiosynthesis,
            BlockKind::LipidMetabolism,
            BlockKind::ProteinSynthesis,
            BlockKind::Fermentation,
        ];
        let run = |seed| {
            let mut genome = genome_with(&kinds);
            let (_, mutated) =
                apply_mutation_load(&mut genome, 3.0, &mut StdRng::seed_from_u64(seed));
            mutated
        };
        assert_eq!(run(1), run(1));
        assert_eq!(run(1).len(), 3);
    }

    #[test]
    fn a_cell_with_no_expressed_gene_keeps_only_the_fraction() {
        let mut genome = Genome::default();
        let (left, mutated) = apply_mutation_load(&mut genome, 5.3, &mut rng());
        assert_abs_diff_eq!(left, 0.3, epsilon = 1e-5);
        assert!(mutated.is_empty());
        // A gene that is restored later is not hit at once by the backlog.
        genome.add_gene(BlockKind::Fermentation);
        genome.express_gene(BlockKind::Fermentation);
        let (left, mutated) = apply_mutation_load(&mut genome, left, &mut rng());
        assert_abs_diff_eq!(left, 0.3, epsilon = 1e-5);
        assert!(mutated.is_empty());
    }

    #[test]
    fn repair_costs_atp_and_nucleotides_and_stops_at_the_gap() {
        let step = repair_step(0.5, 0.0, 0, 1000.0, 1000.0, 1.0);
        assert_abs_diff_eq!(step.integrity, REPAIR_WORK_PER_SEC);
        assert_abs_diff_eq!(step.atp, REPAIR_WORK_PER_SEC * REPAIR_ATP_PER_WORK);
        assert_abs_diff_eq!(
            step.nucleotides,
            REPAIR_WORK_PER_SEC * REPAIR_NUCLEOTIDES_PER_WORK
        );
        let near_full = repair_step(0.995, 0.0, 0, 1000.0, 1000.0, 10.0);
        assert_abs_diff_eq!(near_full.integrity, 0.005, epsilon = 1e-6);
        let full = repair_step(1.0, 0.0, 0, 1000.0, 1000.0, 10.0);
        assert_eq!(full, RepairStep::default());
    }

    #[test]
    fn repair_stops_when_a_pool_runs_out() {
        let no_nucleotides = repair_step(0.5, 0.0, 2, 1000.0, 0.0, 1.0);
        assert_eq!(no_nucleotides, RepairStep { progress: 0.0, ..default() });
        let no_atp = repair_step(0.5, 0.0, 0, 0.0, 1000.0, 1.0);
        assert_eq!(no_atp.integrity, 0.0);
        let little = repair_step(0.5, 0.0, 0, 1000.0, 3.0, 100.0);
        assert_abs_diff_eq!(little.nucleotides, 3.0, epsilon = 1e-4);
    }

    #[test]
    fn repair_restores_genes_after_enough_work() {
        let mut progress = 0.0;
        let mut genes = 0;
        // Only genes need repair: all work goes to them.
        for _ in 0..100 {
            let step = repair_step(1.0, progress, 2 - genes, 1000.0, 1000.0, 0.2);
            progress = step.progress;
            genes += step.genes;
        }
        assert_eq!(genes, 2);
        assert_abs_diff_eq!(progress, 0.0, epsilon = 2.0 * WORK_EPSILON);
    }

    #[test]
    fn a_gene_costs_its_full_work_and_a_short_progress_does_not_restore_it() {
        // 5 millionths of work short, and no nucleotides to finish it.
        let short = GENE_REPAIR_WORK * (1.0 - 5e-5);
        let step = repair_step(1.0, short, 1, 1000.0, 0.0, 1.0);
        assert_eq!(step.genes, 0);
        assert_eq!(step.progress, short);
        // With nucleotides, the rest is paid and the gene is restored.
        let step = repair_step(1.0, short, 1, 1000.0, 1000.0, 1.0);
        assert_eq!(step.genes, 1);
        let paid = step.nucleotides / REPAIR_NUCLEOTIDES_PER_WORK;
        assert_abs_diff_eq!(paid + short, GENE_REPAIR_WORK, epsilon = 1e-6);
    }

    #[test]
    fn the_total_work_for_genes_is_exact_over_many_ticks() {
        let (mut progress, mut genes, mut paid) = (0.0f32, 0u32, 0.0f32);
        while genes < 3 {
            let step = repair_step(1.0, progress, 3 - genes, 1e6, 1e6, 0.2);
            progress = step.progress;
            genes += step.genes;
            paid += step.nucleotides / REPAIR_NUCLEOTIDES_PER_WORK;
        }
        assert_abs_diff_eq!(paid, 3.0 * GENE_REPAIR_WORK, epsilon = 1e-5);
    }
}
