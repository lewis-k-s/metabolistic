//! # Metabolic Blocks
//!
//! A metabolic block is an entity owned by one cell (`OwnedBy`). It has a
//! `MetabolicNode` (kind and status), a `FluxProfile` (flows per second for one
//! enzyme at saturation), `Kinetics` (half-saturation, cofactors, outside
//! channels), a `Drive` (regulation) and a `BlockTurnover` (last tick). The
//! number of enzymes is in the cell's `Proteome`. The flux engine in
//! `metabolism` runs all blocks of a cell each tick.
//!
//! ## Genome sync
//!
//! The cell's `Genome` decides its blocks. `sync_cell_blocks_system` runs when a
//! genome changes, or on `MetabolicUpdateEvent`:
//! - Each implemented gene in the genome has one block. Its status follows the
//!   gene state: Expressed = Active, Mutated = half rate, Silent = no flux.
//! - A block whose gene leaves the genome is despawned. Its enzymes stay in
//!   the proteome.
//!
//! ## Block kinds
//!
//! - **Chemosynthesis** (`ChemosynthesisBlock`): Minerals + Heat (outside) ->
//!   ATP + ReducingPower + CarbonSkeletons. Metals are a cofactor.
//! - **Photosynthesis** (`photosynthesis`, gene `LightCapture`): Light (outside) ->
//!   ATP + ReducingPower + CarbonSkeletons + O2 + a little ROS.
//! - **Catalase** (`catalase`): ROS + ATP -> O2.
//! - **Respiration** (`respiration`, M8): CarbonSkeletons + O2 + ReducingPower ->
//!   much more ATP + a little ROS.
//! - **Cuticle** (`cuticle`, M8): uses Lipids; its strength lowers water loss.
//! - **Amino acid biosynthesis**, **lipid metabolism** (`biosynthesis`):
//!   CarbonSkeletons + ATP + ReducingPower -> AminoAcids or Lipids.
//! - **Protein synthesis** (`ProteinSynthesisBlock`): AminoAcids + ATP ->
//!   protein units for the synthesis queue. Regulated: on while the queue has orders.
//! - **Fermentation** (`FermentationBlock`): Pyruvate + ReducingPower -> ATP + Waste
//! - **Fat storage** (`FatStorageBlock`): regulated. Stores Lipids as
//!   StoredLipids above the lipid toxicity threshold and releases them below it.
//! - **Nucleotide synthesis** (`nucleotides`): CarbonSkeletons + ATP +
//!   ReducingPower -> Nucleotides, up to a target. Genome repair uses them.
//! - **Pigments** (`secondary_metabolites`): CarbonSkeletons + ATP -> Pigments,
//!   up to a target. They screen UV.
//! - **Vesicle export** (`VesicleExportBlock`): removes Waste from the cell.
//! - **Secretions** (`secretion::SecretionBlock`, genes `Microcin`, `Slime`,
//!   `Siderophore`, `Autoinducer`): ATP and a material go out into the grid as
//!   a cloud. `Drive` 0 until the player switches the secretion on.

pub mod biosynthesis;
pub mod catalase;
pub mod chemosynthesis;
pub mod cuticle;
pub mod fat_storage;
pub mod fermentation;
pub mod genome;
pub mod nucleotides;
pub mod photosynthesis;
pub mod protein_synthesis;
pub mod refill;
pub mod respiration;
pub mod secondary_metabolites;
pub mod vesicle_export;

use bevy::prelude::*;

use crate::cell::{Cell, CellBlocks, OwnedBy};
use crate::metabolism::{
    BlockStatus, BlockTurnover, Drive, FluxProfile, Kinetics, MetabolicBlock, MetabolicNode,
    MetabolicSet,
};
use genome::{BlockKind, Genome, MetabolicUpdateEvent};

/// Plugin that keeps each cell's blocks in sync with its genome and runs block
/// regulation and the synthesis queue in the metabolic tick.
pub struct MetabolicBlocksPlugin;

impl Plugin for MetabolicBlocksPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<MetabolicUpdateEvent>()
            .add_systems(Update, sync_cell_blocks_system)
            .add_systems(
                crate::metabolism::MetabolicSchedule,
                (
                    (
                        fat_storage::regulate_fat_storage_system,
                        protein_synthesis::regulate_protein_synthesis_system,
                        refill::regulate_refill_system::<nucleotides::NucleotideSynthesisBlock>,
                        refill::regulate_refill_system::<secondary_metabolites::PigmentBlock>,
                    )
                        .in_set(MetabolicSet::Regulate),
                    (
                        protein_synthesis::advance_protein_queue_system,
                        crate::membrane::membrane_repair_system,
                    )
                        .in_set(MetabolicSet::Build),
                ),
            );
    }
}

/// The data that a block kind starts with.
#[derive(Debug, Clone)]
pub struct BlockSpec {
    pub profile: FluxProfile,
    pub kinetics: Kinetics,
}

impl BlockSpec {
    fn new(profile: FluxProfile) -> Self {
        Self {
            profile,
            kinetics: Kinetics::default(),
        }
    }
}

/// The spec of a block kind, or `None` if the kind has no implementation yet.
/// Regulated blocks that only move pools (fat storage) have an empty profile.
pub fn block_spec(kind: BlockKind) -> Option<BlockSpec> {
    match kind {
        BlockKind::Chemosynthesis => Some(BlockSpec {
            profile: chemosynthesis::flux_profile(),
            kinetics: chemosynthesis::kinetics(),
        }),
        BlockKind::LightCapture => Some(BlockSpec {
            profile: photosynthesis::flux_profile(),
            kinetics: photosynthesis::kinetics(),
        }),
        BlockKind::Catalase => Some(BlockSpec {
            profile: catalase::flux_profile(),
            kinetics: catalase::kinetics(),
        }),
        BlockKind::Respiration => Some(BlockSpec {
            profile: respiration::flux_profile(),
            kinetics: respiration::kinetics(),
        }),
        BlockKind::Cuticle => Some(BlockSpec {
            profile: cuticle::flux_profile(),
            kinetics: cuticle::kinetics(),
        }),
        BlockKind::AminoAcidBiosynthesis => {
            Some(BlockSpec::new(biosynthesis::amino_acid_profile()))
        }
        BlockKind::LipidMetabolism => Some(BlockSpec::new(biosynthesis::lipid_profile())),
        BlockKind::ProteinSynthesis => Some(BlockSpec::new(protein_synthesis::flux_profile())),
        BlockKind::Fermentation => Some(BlockSpec::new(fermentation::flux_profile())),
        BlockKind::VesicleExport => Some(BlockSpec::new(vesicle_export::flux_profile())),
        BlockKind::FatStorage => Some(BlockSpec::new(FluxProfile::default())),
        BlockKind::NucleotideCofactorSynthesis => {
            Some(BlockSpec::new(nucleotides::flux_profile()))
        }
        BlockKind::SecondaryMetabolites => {
            Some(BlockSpec::new(secondary_metabolites::flux_profile()))
        }
        BlockKind::Microcin | BlockKind::Slime | BlockKind::Siderophore | BlockKind::Autoinducer => {
            crate::secretion::Secretion::from_gene(kind).map(|s| BlockSpec::new(s.flux_profile()))
        }
        _ => None,
    }
}

/// Changes needed to bring a cell's blocks in line with its genome.
#[derive(Debug, Default, PartialEq)]
pub struct BlockPlan {
    pub spawn: Vec<(BlockKind, BlockStatus)>,
    pub update: Vec<(BlockKind, BlockStatus)>,
    pub despawn: Vec<BlockKind>,
}

/// Plans block changes from the genome and the blocks the cell has now.
pub fn plan_blocks(genome: &Genome, existing: &CellBlocks) -> BlockPlan {
    let mut plan = BlockPlan::default();

    let mut genes: Vec<_> = genome.table.iter().collect();
    genes.sort_by_key(|(kind, _)| **kind);
    for (&kind, state) in genes {
        if block_spec(kind).is_none() {
            continue;
        }
        let status = BlockStatus::from(state.clone());
        if existing.contains(kind) {
            plan.update.push((kind, status));
        } else {
            plan.spawn.push((kind, status));
        }
    }

    let mut owned: Vec<BlockKind> = existing.0.keys().copied().collect();
    owned.sort();
    plan.despawn = owned
        .into_iter()
        .filter(|kind| !genome.table.contains_key(kind))
        .collect();
    plan
}

/// Spawns one block for `cell`. Returns `None` for kinds with no implementation.
pub fn spawn_block(
    commands: &mut Commands,
    cell: Entity,
    kind: BlockKind,
    status: BlockStatus,
) -> Option<Entity> {
    let spec = block_spec(kind)?;
    let mut entity = commands.spawn((
        MetabolicBlock,
        MetabolicNode { kind, status },
        spec.profile,
        spec.kinetics,
        Drive::default(),
        crate::actions::Throttle::default(),
        BlockTurnover::default(),
        OwnedBy(cell),
        Name::new(format!("{kind:?} block")),
    ));
    match kind {
        BlockKind::Chemosynthesis => {
            entity.insert(chemosynthesis::ChemosynthesisBlock);
        }
        BlockKind::LightCapture => {
            entity.insert(photosynthesis::PhotosynthesisBlock);
        }
        BlockKind::Catalase => {
            entity.insert(catalase::CatalaseBlock);
        }
        BlockKind::Respiration => {
            entity.insert(respiration::RespirationBlock);
        }
        BlockKind::Cuticle => {
            entity.insert(cuticle::CuticleBlock);
        }
        BlockKind::AminoAcidBiosynthesis => {
            entity.insert(biosynthesis::AminoAcidBiosynthesisBlock);
        }
        BlockKind::LipidMetabolism => {
            entity.insert(biosynthesis::LipidMetabolismBlock);
        }
        BlockKind::ProteinSynthesis => {
            entity.insert(protein_synthesis::ProteinSynthesisBlock);
        }
        BlockKind::Fermentation => {
            entity.insert(fermentation::FermentationBlock);
        }
        BlockKind::FatStorage => {
            entity.insert(fat_storage::FatStorageBlock::default());
        }
        BlockKind::VesicleExport => {
            entity.insert(vesicle_export::VesicleExportBlock);
        }
        BlockKind::NucleotideCofactorSynthesis => {
            entity.insert(nucleotides::NucleotideSynthesisBlock);
        }
        BlockKind::SecondaryMetabolites => {
            entity.insert(secondary_metabolites::PigmentBlock);
        }
        BlockKind::Microcin | BlockKind::Slime | BlockKind::Siderophore | BlockKind::Autoinducer => {
            if let Some(secretion) = crate::secretion::Secretion::from_gene(kind) {
                // Off until the player switches it on.
                entity.insert((crate::secretion::SecretionBlock(secretion), Drive(0.0)));
            }
        }
        _ => {}
    }
    Some(entity.id())
}

/// Applies `plan_blocks` to every cell whose genome changed.
pub fn sync_cell_blocks_system(
    mut force: MessageReader<MetabolicUpdateEvent>,
    mut cells: Query<(Entity, Ref<Genome>, &mut CellBlocks), With<Cell>>,
    mut nodes: Query<&mut MetabolicNode>,
    mut commands: Commands,
) {
    let forced = force.read().count() > 0;
    for (cell, genome, mut blocks) in &mut cells {
        if !forced && !genome.is_changed() {
            continue;
        }
        let plan = plan_blocks(&genome, &blocks);
        if plan.spawn.is_empty() && plan.despawn.is_empty() {
            // Status-only changes: do not mark CellBlocks as changed.
            for (kind, status) in plan.update {
                set_status(&blocks, &mut nodes, kind, status);
            }
            continue;
        }
        for kind in plan.despawn {
            if let Some(block) = blocks.0.remove(&kind) {
                commands.entity(block).despawn();
            }
        }
        for (kind, status) in plan.update {
            set_status(&blocks, &mut nodes, kind, status);
        }
        for (kind, status) in plan.spawn {
            if let Some(block) = spawn_block(&mut commands, cell, kind, status) {
                blocks.0.insert(kind, block);
            }
        }
    }
}

fn set_status(
    blocks: &CellBlocks,
    nodes: &mut Query<&mut MetabolicNode>,
    kind: BlockKind,
    status: BlockStatus,
) {
    if let Some(mut node) = blocks.get(kind).and_then(|e| nodes.get_mut(e).ok())
        && node.status != status
    {
        node.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genome::GeneState;

    #[test]
    fn plan_spawns_implemented_genes_only() {
        let mut genome = Genome::default();
        genome.add_gene(BlockKind::Fermentation);
        genome.express_gene(BlockKind::Fermentation);
        genome.add_gene(BlockKind::Multicellularity); // no block
        let plan = plan_blocks(&genome, &CellBlocks::default());
        assert_eq!(
            plan.spawn,
            vec![(BlockKind::Fermentation, BlockStatus::Active)]
        );
        assert!(plan.update.is_empty());
        assert!(plan.despawn.is_empty());
    }

    #[test]
    fn plan_updates_status_and_despawns_removed_genes() {
        let mut genome = Genome::default();
        genome.add_gene(BlockKind::Fermentation);
        genome
            .table
            .insert(BlockKind::Fermentation, GeneState::Mutated);
        let mut existing = CellBlocks::default();
        existing
            .0
            .insert(BlockKind::Fermentation, Entity::from_raw_u32(1).unwrap());
        existing
            .0
            .insert(BlockKind::VesicleExport, Entity::from_raw_u32(2).unwrap());
        let plan = plan_blocks(&genome, &existing);
        assert!(plan.spawn.is_empty());
        assert_eq!(
            plan.update,
            vec![(BlockKind::Fermentation, BlockStatus::Mutated)]
        );
        assert_eq!(plan.despawn, vec![BlockKind::VesicleExport]);
    }
}
