//! # Prepare: the starting genome
//!
//! Before a run the player picks starting genes from the codex. The starter
//! genome is free. Each codex gene costs `gene_cost` points; the budget is
//! `START_BUDGET`. A gene with no block yet (`Respiration`, M8) cannot do
//! anything, so it costs 0 and cannot be chosen. A chosen gene starts
//! `Expressed` with one enzyme (`apply_loadout`).
//!
//! The rules are pure functions; the screen is in `screens::prepare`.

use bevy::prelude::*;
use std::collections::BTreeSet;

use crate::blocks::block_spec;
use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::codex::Codex;
use crate::proteins::{ProteinTarget, Proteome};

/// Points to spend on codex genes at the start of a run.
pub const START_BUDGET: u32 = 2;

/// What a codex gene costs. A gene with no block costs 0.
pub fn gene_cost(kind: BlockKind) -> u32 {
    u32::from(block_spec(kind).is_some())
}

/// Can the player choose this gene? It must have a block, must not be in the
/// starter genome as an expressed gene, and must be in the codex.
pub fn is_selectable(kind: BlockKind, codex: &Codex) -> bool {
    gene_cost(kind) > 0
        && codex.genes.contains(&kind)
        && crate::blocks::genome::create_starter_genome().get_gene_state(&kind) != Some(&GeneState::Expressed)
}

/// Points that a choice costs.
pub fn spent(chosen: &BTreeSet<BlockKind>) -> u32 {
    chosen.iter().map(|k| gene_cost(*k)).sum()
}

/// Can `kind` be added without going over the budget?
pub fn can_add(chosen: &BTreeSet<BlockKind>, kind: BlockKind, codex: &Codex, budget: u32) -> bool {
    is_selectable(kind, codex) && !chosen.contains(&kind) && spent(chosen) + gene_cost(kind) <= budget
}

/// Adds the gene if it is not chosen (and fits), else removes it. Returns true
/// if the choice changed.
pub fn toggle(chosen: &mut BTreeSet<BlockKind>, kind: BlockKind, codex: &Codex, budget: u32) -> bool {
    if chosen.remove(&kind) {
        true
    } else if can_add(chosen, kind, codex, budget) {
        chosen.insert(kind);
        true
    } else {
        false
    }
}

/// Drops genes that are not allowed (not in the codex, or over the budget).
/// Genes are dropped from the end, so a short list stays whole.
pub fn sanitize(chosen: &BTreeSet<BlockKind>, codex: &Codex, budget: u32) -> BTreeSet<BlockKind> {
    let mut result = BTreeSet::new();
    for kind in chosen {
        if can_add(&result, *kind, codex, budget) {
            result.insert(*kind);
        }
    }
    result
}

/// The codex genes in the order of the screen: selectable first.
pub fn codex_list(codex: &Codex) -> Vec<(BlockKind, bool)> {
    let mut list: Vec<_> = codex.genes.iter().map(|k| (*k, is_selectable(*k, codex))).collect();
    list.sort_by_key(|(kind, selectable)| (!*selectable, *kind));
    list
}

/// Adds the chosen genes to a starter cell: expressed, with one enzyme each.
pub fn apply_loadout(genome: &mut Genome, proteome: &mut Proteome, chosen: &BTreeSet<BlockKind>) {
    for kind in chosen {
        genome.table.insert(*kind, GeneState::Expressed);
        if proteome.count(ProteinTarget::Enzyme(*kind)) == 0 {
            proteome.add(ProteinTarget::Enzyme(*kind));
        }
    }
}

/// The choice on the Prepare screen.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct PrepareChoice(pub BTreeSet<BlockKind>);

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_of(genes: &[BlockKind]) -> Codex {
        let mut codex = Codex::default();
        codex.bank(genes);
        codex
    }

    #[test]
    fn genes_with_a_block_cost_one_and_others_nothing() {
        assert_eq!(gene_cost(BlockKind::Fermentation), 1);
        assert_eq!(gene_cost(BlockKind::SecondaryMetabolites), 1);
        assert_eq!(gene_cost(BlockKind::Respiration), 0, "no block until M8");
        // M7: photosynthesis and catalase have blocks now.
        assert_eq!(gene_cost(BlockKind::LightCapture), 1);
        assert_eq!(gene_cost(BlockKind::Catalase), 1);
    }

    #[test]
    fn only_codex_genes_with_a_block_can_be_chosen() {
        let codex = codex_of(&[BlockKind::Fermentation, BlockKind::Respiration]);
        assert!(is_selectable(BlockKind::Fermentation, &codex));
        assert!(!is_selectable(BlockKind::Respiration, &codex), "no block");
        assert!(!is_selectable(BlockKind::FatStorage, &codex), "not in the codex");
        // A starter gene is free already.
        let codex = codex_of(&[BlockKind::Chemosynthesis]);
        assert!(!is_selectable(BlockKind::Chemosynthesis, &codex));
    }

    #[test]
    fn the_budget_is_two_points() {
        let codex = codex_of(&[BlockKind::Fermentation, BlockKind::FatStorage, BlockKind::VesicleExport]);
        let mut chosen = BTreeSet::new();
        assert!(toggle(&mut chosen, BlockKind::Fermentation, &codex, START_BUDGET));
        assert!(toggle(&mut chosen, BlockKind::FatStorage, &codex, START_BUDGET));
        assert_eq!(spent(&chosen), 2);
        assert!(!toggle(&mut chosen, BlockKind::VesicleExport, &codex, START_BUDGET), "over budget");
        assert_eq!(chosen.len(), 2);
        // Taking one back frees a point.
        assert!(toggle(&mut chosen, BlockKind::Fermentation, &codex, START_BUDGET));
        assert!(toggle(&mut chosen, BlockKind::VesicleExport, &codex, START_BUDGET));
        assert!(!chosen.contains(&BlockKind::Fermentation));
        // A gene with no block is refused and costs nothing.
        let codex = codex_of(&[BlockKind::Respiration]);
        assert!(!toggle(&mut chosen, BlockKind::Respiration, &codex, START_BUDGET));
    }

    #[test]
    fn sanitize_drops_unknown_and_excess_genes() {
        let codex = codex_of(&[BlockKind::Fermentation, BlockKind::FatStorage, BlockKind::VesicleExport]);
        let want: BTreeSet<_> = [BlockKind::Fermentation, BlockKind::FatStorage, BlockKind::VesicleExport, BlockKind::Respiration]
            .into_iter()
            .collect();
        let got = sanitize(&want, &codex, START_BUDGET);
        assert_eq!(spent(&got), 2);
        assert!(!got.contains(&BlockKind::Respiration));
    }

    #[test]
    fn chosen_genes_start_expressed_with_one_enzyme() {
        let mut genome = crate::blocks::genome::create_starter_genome();
        let mut proteome = crate::shared::starter_proteome();
        let chosen: BTreeSet<_> = [BlockKind::Fermentation, BlockKind::FatStorage].into_iter().collect();
        assert_eq!(genome.get_gene_state(&BlockKind::Fermentation), Some(&GeneState::Silent));
        apply_loadout(&mut genome, &mut proteome, &chosen);
        for kind in &chosen {
            assert_eq!(genome.get_gene_state(kind), Some(&GeneState::Expressed));
            assert_eq!(proteome.count(ProteinTarget::Enzyme(*kind)), 1);
        }
    }

    #[test]
    fn the_list_puts_selectable_genes_first() {
        let codex = codex_of(&[BlockKind::Respiration, BlockKind::Fermentation]);
        assert_eq!(
            codex_list(&codex),
            vec![(BlockKind::Fermentation, true), (BlockKind::Respiration, false)]
        );
    }
}
