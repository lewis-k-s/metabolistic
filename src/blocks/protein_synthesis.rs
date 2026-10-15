//! Protein synthesis metabolic block (ribosomes).
//!
//! AminoAcids + ATP -> protein units. The block's enzymes are its ribosomes.
//! It runs only while the cell's synthesis queue has orders: a regulator sets
//! its `Drive` to 0 when the queue is empty. Its turnovers in a tick are the
//! protein units that `advance_protein_queue_system` spends on the queue.

use bevy::prelude::*;

use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellBlocks, OwnedBy};
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

/// Drive of the block: on while the queue has orders.
pub fn synthesis_drive(queue: &SynthesisQueue) -> f32 {
    if queue.is_empty() { 0.0 } else { 1.0 }
}

/// Turns protein synthesis on or off from each cell's queue.
pub fn regulate_protein_synthesis_system(
    cells: Query<&SynthesisQueue, With<Cell>>,
    mut blocks: Query<(&OwnedBy, &mut Drive), With<ProteinSynthesisBlock>>,
) {
    for (owner, mut drive) in &mut blocks {
        if let Ok(queue) = cells.get(owner.0) {
            let wanted = synthesis_drive(queue);
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
