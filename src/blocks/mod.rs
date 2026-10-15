//! # Metabolic Blocks
//!
//! A metabolic block is an entity owned by one cell (`OwnedBy`). It has a
//! `MetabolicNode` (kind and status) and a `FluxProfile` (rates per second). The
//! flux engine in `metabolism` runs all blocks of a cell each tick.
//!
//! ## Genome sync
//!
//! The cell's `Genome` decides its blocks. `sync_cell_blocks_system` runs when a
//! genome changes, or on `MetabolicUpdateEvent`:
//! - Each implemented gene in the genome has one block. Its status follows the
//!   gene state: Expressed = Active, Mutated = half rate, Silent = no flux.
//! - A block whose gene leaves the genome is despawned.
//!
//! ## Block kinds
//!
//! - **Fermentation** (`FermentationBlock`): Pyruvate + ReducingPower -> ATP + OrganicWaste
//! - **Fat storage** (`FatStorageBlock`): regulated. Stores FreeFattyAcids as
//!   StorageBeads above the lipid toxicity threshold and releases them below it.
//! - **Vesicle export** (`VesicleExportBlock`): removes OrganicWaste from the cell.

pub mod fat_storage;
pub mod fermentation;
pub mod genome;
pub mod vesicle_export;

use bevy::prelude::*;

use crate::cell::{Cell, CellBlocks, OwnedBy};
use crate::metabolism::{BlockStatus, FluxProfile, MetabolicBlock, MetabolicNode, MetabolicSet};
use genome::{BlockKind, Genome, MetabolicUpdateEvent};

/// Plugin that keeps each cell's blocks in sync with its genome and runs block
/// regulation in the metabolic tick.
pub struct MetabolicBlocksPlugin;

impl Plugin for MetabolicBlocksPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<MetabolicUpdateEvent>()
            .add_systems(Update, sync_cell_blocks_system)
            .add_systems(
                crate::metabolism::MetabolicSchedule,
                fat_storage::regulate_fat_storage_system.in_set(MetabolicSet::Regulate),
            );
    }
}

/// The full-rate flux profile of a block kind, or `None` if the kind has no
/// implementation yet. Regulated blocks start with an empty profile.
pub fn block_flux_profile(kind: BlockKind) -> Option<FluxProfile> {
    match kind {
        BlockKind::Fermentation => Some(fermentation::flux_profile()),
        BlockKind::VesicleExport => Some(vesicle_export::flux_profile()),
        BlockKind::FatStorage => Some(FluxProfile::default()),
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
        if block_flux_profile(kind).is_none() {
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
    let profile = block_flux_profile(kind)?;
    let mut entity = commands.spawn((
        MetabolicBlock,
        MetabolicNode { kind, status },
        profile,
        OwnedBy(cell),
        Name::new(format!("{kind:?} block")),
    ));
    match kind {
        BlockKind::Fermentation => {
            entity.insert(fermentation::FermentationBlock);
        }
        BlockKind::FatStorage => {
            entity.insert(fat_storage::FatStorageBlock::default());
        }
        BlockKind::VesicleExport => {
            entity.insert(vesicle_export::VesicleExportBlock);
        }
        _ => {}
    }
    Some(entity.id())
}

/// Applies `plan_blocks` to every cell whose genome changed.
pub fn sync_cell_blocks_system(
    mut force: EventReader<MetabolicUpdateEvent>,
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
    if let Some(mut node) = blocks.get(kind).and_then(|e| nodes.get_mut(e).ok()) {
        if node.status != status {
            node.status = status;
        }
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
        genome.add_gene(BlockKind::LightCapture); // not implemented
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
            .insert(BlockKind::Fermentation, Entity::from_raw(1));
        existing
            .0
            .insert(BlockKind::VesicleExport, Entity::from_raw(2));
        let plan = plan_blocks(&genome, &existing);
        assert!(plan.spawn.is_empty());
        assert_eq!(
            plan.update,
            vec![(BlockKind::Fermentation, BlockStatus::Mutated)]
        );
        assert_eq!(plan.despawn, vec![BlockKind::VesicleExport]);
    }
}
