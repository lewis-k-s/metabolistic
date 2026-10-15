//! Protein synthesis metabolic block (ribosomes).
//!
//! AminoAcids + ATP -> protein units. The block's enzymes are its ribosomes.
//! It runs only while the cell's synthesis queue has orders: a regulator sets
//! its `Drive` to 0 when the queue is empty. Genome damage slows it:
//! the drive is multiplied by the translation efficiency, which is 0 for a
//! fragmented genome (see `genome_damage`). Its turnovers in a tick are the
//! protein units that `advance_protein_queue_system` spends on the queue.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellBlocks, OwnedBy};
use crate::genome_damage::{GenomeIntegrity, translation_efficiency};
use crate::metabolism::{BlockTurnover, Drive, FluxProfile};
use crate::molecules::Currency;
use crate::proteins::{
    AMINO_ACIDS_PER_UNIT, ATP_PER_UNIT, Proteome, SynthesisQueue, advance_synthesis,
};

/// Marker for protein synthesis blocks.
#[derive(Component)]
pub struct ProteinSynthesisBlock;

/// Per second, one ribosome at full saturation: 1 protein unit.
pub fn flux_profile() -> FluxProfile {
    let mut profile = FluxProfile::default();
    profile.set_demand(Currency::AminoAcids, AMINO_ACIDS_PER_UNIT);
    profile.set_demand(Currency::ATP, ATP_PER_UNIT);
    profile
}

/// Protein units per second that the ribosomes can make, if the inputs
/// suffice. It is the estimate behind the editor's time to finish.
pub fn potential_rate(
    ribosomes: u32,
    status: crate::metabolism::BlockStatus,
    throttle: f32,
    genome_integrity: f32,
) -> f32 {
    ribosomes as f32 * status.throughput() * throttle.max(0.0) * translation_efficiency(genome_integrity)
}

/// Drive of the block: on while the queue has orders.
pub fn synthesis_drive(queue: &SynthesisQueue) -> f32 {
    if queue.is_empty() { 0.0 } else { 1.0 }
}

/// Drive of the block with genome damage: the queue drive times the
/// translation efficiency.
pub fn damaged_synthesis_drive(queue: &SynthesisQueue, genome_integrity: f32) -> f32 {
    synthesis_drive(queue) * translation_efficiency(genome_integrity)
}

/// Turns protein synthesis on or off from each cell's queue, and slows it down
/// when the genome is damaged.
pub fn regulate_protein_synthesis_system(
    cells: Query<(&SynthesisQueue, &GenomeIntegrity), With<Cell>>,
    mut blocks: Query<(&OwnedBy, &mut Drive), With<ProteinSynthesisBlock>>,
) {
    for (owner, mut drive) in &mut blocks {
        if let Ok((queue, integrity)) = cells.get(owner.0) {
            let wanted = damaged_synthesis_drive(queue, integrity.0);
            if drive.0 != wanted {
                drive.0 = wanted;
            }
        }
    }
}

/// Spends the protein units of the last tick on each cell's queue.
pub fn advance_protein_queue_system(
    mut cells: Query<(&CellBlocks, &mut SynthesisQueue, &mut Proteome), With<Cell>>,
    turnovers: Query<&BlockTurnover>,
) {
    for (blocks, mut queue, mut proteome) in &mut cells {
        let units = blocks
            .get(BlockKind::ProteinSynthesis)
            .and_then(|block| turnovers.get(block).ok())
            .map_or(0.0, |t| t.0);
        if units > 0.0 {
            for target in advance_synthesis(&mut queue, &mut proteome, units) {
                debug!("Protein finished: {target:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metabolism::BlockStatus;

    #[test]
    fn the_potential_rate_follows_ribosomes_status_throttle_and_genome() {
        assert_eq!(potential_rate(2, BlockStatus::Active, 1.0, 1.0), 2.0);
        assert_eq!(potential_rate(2, BlockStatus::Mutated, 1.0, 1.0), 1.0);
        assert_eq!(potential_rate(2, BlockStatus::Active, 0.5, 1.0), 1.0);
        assert_eq!(potential_rate(2, BlockStatus::Active, 1.0, 0.1), 0.0, "fragmented");
        assert_eq!(potential_rate(0, BlockStatus::Active, 1.0, 1.0), 0.0);
    }
}
