//! # Genome Processing Block System
//!
//! This module implements the genome as a highly privileged processing block that manages
//! other metabolic processing blocks in the cellular simulation. It serves as the "tech-tree forge"
//! where players can earn, configure, or discard entire metabolic loops.
//!
//! ## Core Concepts
//!
//! - **Gene Tiles**: Each gene corresponds 1-to-1 with a metabolic block or throughput upgrade
//! - **Expression**: Paying ATP + nucleotides flips a gene tile to "expressed", making the block appear
//! - **Maintenance**: Each expressed tile has a flat ATP upkeep cost every tick (protein turnover)
//! - **Editing**: Swapping gene tiles costs ATP + reducing power (simulating recombination/repair)
//! - **Mutation**: UV can disable gene tiles until repaired (see `genome_damage`)
//!
//! ## Usage
//!
//! Add the `GenomePlugin` to your Bevy app and use the genome system:
//! ```rust,no_run,standalone_crate
//! use metabolistic3d::blocks::genome::{Genome, BlockKind};
//!
//! // Create and manipulate a genome
//! let mut genome = Genome::default();
//! genome.add_gene(BlockKind::SugarCatabolism);
//! genome.express_gene(BlockKind::SugarCatabolism);
//! ```
//!
//! The system automatically:
//! - Tracks gene expression changes via `GenomeDiffEvent`
//! - Syncs each cell's metabolic blocks with its genome (see `blocks`)
//!
//! ## Controls (Demo)
//!
//! - Press 'G' to express the Sugar Catabolism gene
//! - Press 'H' to silence the Fermentation gene  
//! - Press 'J' to add a new Light Capture gene

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents the different types of metabolic blocks that can be encoded in the genome
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component, Serialize, Deserialize,
)]
pub enum BlockKind {
    LightCapture,
    SugarCatabolism,
    OrganicAcidOxidation,
    Respiration,
    Fermentation,
    NitrogenSulfurAssimilation,
    AminoAcidBiosynthesis,
    LipidMetabolism,
    NucleotideCofactorSynthesis,
    SecondaryMetabolites,
    AromaticPrecursorSynthesis,
    Polymerization,
    /// Stores free fatty acids as storage beads and releases them again.
    FatStorage,
    /// Removes waste from the cell in vesicles.
    VesicleExport,
    /// Makes ATP, reducing power and carbon from vent minerals and heat.
    Chemosynthesis,
    /// Ribosomes: make protein units from amino acids and ATP.
    ProteinSynthesis,
}

impl BlockKind {
    /// Human-readable description of each metabolic block
    pub fn description(&self) -> &'static str {
        match self {
            BlockKind::LightCapture => "Capture light to produce ATP and NADPH",
            BlockKind::SugarCatabolism => "Break down sugars into pyruvate",
            BlockKind::OrganicAcidOxidation => "Oxidize organic acids via the TCA cycle",
            BlockKind::Respiration => "Use NADH to generate large amounts of ATP",
            BlockKind::Fermentation => "Anaerobic ATP production with redox balance",
            BlockKind::NitrogenSulfurAssimilation => "Assimilate nitrogen and sulfur sources",
            BlockKind::AminoAcidBiosynthesis => "Produce amino acids from precursors",
            BlockKind::LipidMetabolism => "Synthesize and degrade fatty acids",
            BlockKind::NucleotideCofactorSynthesis => "Generate nucleotides and cofactors",
            BlockKind::SecondaryMetabolites => "Produce pigments and toxins",
            BlockKind::AromaticPrecursorSynthesis => "Create aromatic precursors",
            BlockKind::Polymerization => "Polymerize lignin and other biopolymers",
            BlockKind::FatStorage => "Store free lipids as lipid beads",
            BlockKind::VesicleExport => "Export waste in vesicles",
            BlockKind::Chemosynthesis => "Make energy and carbon from vent minerals and heat",
            BlockKind::ProteinSynthesis => "Make proteins from amino acids",
        }
    }
}

/// The state of a gene tile in the genome
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum GeneState {
    /// Gene is present but not expressed (no enzyme production)
    #[default]
    Silent,
    /// Gene is actively expressed (enzyme is being produced)
    Expressed,
    /// Gene is mutated and temporarily non-functional
    Mutated,
    /// Gene had a beneficial mutation (only from induced mutation): its block
    /// runs faster. A later mutation turns it into `Mutated`, and repair then
    /// gives `Expressed`: the bonus is lost.
    Enhanced,
}

/// Resource/Component containing the entire chromosome of gene tiles
/// Each `Cell` entity owns one `Genome` component. It is the only source of truth
/// for which metabolic blocks the cell has.
#[derive(Component, Default, Clone, Debug)]
pub struct Genome {
    pub table: HashMap<BlockKind, GeneState>,
    /// Track previous state for diff computation
    previous_table: HashMap<BlockKind, GeneState>,
}

/// Serializable representation of a gene
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneRecord {
    pub kind: BlockKind,
    pub state: GeneState,
    pub description: String,
}

/// Data format used to save or load a genome in JSON form
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenomeSaveData {
    pub genes: Vec<GeneRecord>,
}

impl From<&Genome> for GenomeSaveData {
    fn from(genome: &Genome) -> Self {
        let genes = genome
            .table
            .iter()
            .map(|(kind, state)| GeneRecord {
                kind: *kind,
                state: state.clone(),
                description: kind.description().to_string(),
            })
            .collect();
        Self { genes }
    }
}

impl From<GenomeSaveData> for Genome {
    fn from(data: GenomeSaveData) -> Self {
        let table = data
            .genes
            .into_iter()
            .map(|record| (record.kind, record.state))
            .collect();
        Genome {
            table,
            previous_table: HashMap::new(),
        }
    }
}

impl GenomeSaveData {
    /// Serialize the genome to a JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a genome from a JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Genome {
    /// Convert this genome into JSON using `GenomeSaveData`
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        GenomeSaveData::from(self).to_json()
    }

    /// Create a genome from a JSON string produced by [`GenomeSaveData`]
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        GenomeSaveData::from_json(json).map(Into::into)
    }

    /// Add a new gene tile to the genome
    pub fn add_gene(&mut self, block_kind: BlockKind) -> &mut Self {
        self.table.insert(block_kind, GeneState::Silent);
        self
    }

    /// Express a gene (activate the metabolic block)
    pub fn express_gene(&mut self, block_kind: BlockKind) -> &mut Self {
        if let Some(state) = self.table.get_mut(&block_kind)
            && *state == GeneState::Silent
        {
            *state = GeneState::Expressed;
        }
        self
    }

    /// Silence a gene (deactivate the metabolic block)
    pub fn silence_gene(&mut self, block_kind: BlockKind) -> &mut Self {
        if let Some(state) = self.table.get_mut(&block_kind)
            && *state == GeneState::Expressed
        {
            *state = GeneState::Silent;
        }
        self
    }

    /// Mutate a gene (temporarily disable it)
    pub fn mutate_gene(&mut self, block_kind: BlockKind) -> bool {
        if let Some(state) = self.table.get_mut(&block_kind) {
            *state = GeneState::Mutated;
            true
        } else {
            false
        }
    }

    /// Turns an expressed gene into an enhanced one (a beneficial mutation).
    pub fn enhance_gene(&mut self, block_kind: BlockKind) -> bool {
        match self.table.get_mut(&block_kind) {
            Some(state) if *state == GeneState::Expressed => {
                *state = GeneState::Enhanced;
                true
            }
            _ => false,
        }
    }

    /// Repair a mutated gene back to expressed. Only expressed genes mutate,
    /// so this restores what UV damaged.
    pub fn restore_gene(&mut self, block_kind: BlockKind) -> bool {
        match self.table.get_mut(&block_kind) {
            Some(state) if *state == GeneState::Mutated => {
                *state = GeneState::Expressed;
                true
            }
            _ => false,
        }
    }

    /// Turns a mutated gene into a silent one. For editing tools. Repair after
    /// UV uses `restore_gene`.
    pub fn silence_mutated_gene(&mut self, block_kind: BlockKind) -> bool {
        if let Some(state) = self.table.get_mut(&block_kind) {
            match state {
                GeneState::Mutated => {
                    *state = GeneState::Silent;
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Get the current state of a gene
    pub fn get_gene_state(&self, block_kind: &BlockKind) -> Option<&GeneState> {
        self.table.get(block_kind)
    }

    /// Genes that a mutation can hit: expressed and enhanced ones. An enhanced
    /// gene that mutates loses its bonus; repair then gives `Expressed`.
    pub fn get_mutable_genes(&self) -> Vec<BlockKind> {
        self.table
            .iter()
            .filter(|(_, state)| matches!(state, GeneState::Expressed | GeneState::Enhanced))
            .map(|(kind, _)| *kind)
            .collect()
    }

    /// Get all expressed genes
    pub fn get_expressed_genes(&self) -> Vec<BlockKind> {
        self.table
            .iter()
            .filter_map(|(block_kind, state)| {
                if matches!(state, GeneState::Expressed) {
                    Some(*block_kind)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Internal method to update the previous state snapshot
    pub(crate) fn update_previous_state(&mut self) {
        self.previous_table = self.table.clone();
    }

    /// Compute the diff between current and previous state
    pub(crate) fn compute_diff(&self) -> GenomeDiff {
        let mut enabled = Vec::new();
        let mut disabled = Vec::new();

        for (block_kind, current_state) in &self.table {
            let previous_state = self.previous_table.get(block_kind);

            match (previous_state, current_state) {
                // Gene became expressed
                (Some(GeneState::Silent | GeneState::Mutated), GeneState::Expressed)
                | (None, GeneState::Expressed) => {
                    enabled.push(*block_kind);
                }
                // Gene stopped being expressed
                (Some(GeneState::Expressed), GeneState::Silent | GeneState::Mutated) => {
                    disabled.push(*block_kind);
                }
                _ => {} // No change in expression status
            }
        }
        GenomeDiff { enabled, disabled }
    }

    /// Check if any gene state has changed (used for metabolic system updates)
    pub fn has_any_changes(&self) -> bool {
        for (block_kind, current_state) in &self.table {
            let previous_state = self.previous_table.get(block_kind);
            if previous_state != Some(current_state) {
                return true;
            }
        }
        false
    }
}

/// Event containing changes in gene expression of one cell
#[derive(Message, Debug)]
pub struct GenomeDiffEvent {
    /// The cell whose genome changed
    pub cell: Entity,
    pub enabled: Vec<BlockKind>,
    pub disabled: Vec<BlockKind>,
}

/// Forces every cell to resync its metabolic blocks with its genome.
/// Normal genome edits do not need it: block sync reacts to `Changed<Genome>`.
#[derive(Message, Debug)]
pub struct MetabolicUpdateEvent;

/// A differential summary of genome changes
#[derive(Debug)]
pub struct GenomeDiff {
    pub enabled: Vec<BlockKind>,
    pub disabled: Vec<BlockKind>,
}

/// Currency costs for genome operations
#[derive(Resource)]
pub struct GenomeOperationCosts {
    /// ATP cost for gene expression (transcription/translation)
    pub expression_atp_cost: f32,
    /// Nucleotide cost for gene expression
    pub expression_nucleotide_cost: f32,
    /// ATP maintenance cost per expressed gene per tick
    pub maintenance_atp_cost: f32,
    /// ATP + reducing power cost for gene editing/swapping
    pub editing_atp_cost: f32,
    pub editing_reducing_power_cost: f32,
}

impl Default for GenomeOperationCosts {
    fn default() -> Self {
        Self {
            expression_atp_cost: 10.0,
            expression_nucleotide_cost: 5.0,
            maintenance_atp_cost: 1.0,
            editing_atp_cost: 20.0,
            editing_reducing_power_cost: 5.0,
        }
    }
}

/// Plugin that manages the genome system
pub struct GenomePlugin;

impl Plugin for GenomePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GenomeOperationCosts::default())
            .add_message::<GenomeDiffEvent>()
            .add_message::<MetabolicUpdateEvent>()
            .add_systems(PreUpdate, poll_genome_diff);
    }
}

/// Compares each changed genome with its previous snapshot and emits the delta.
pub fn poll_genome_diff(
    mut genomes: Query<(Entity, &mut Genome), Changed<Genome>>,
    mut diff_writer: MessageWriter<GenomeDiffEvent>,
) {
    for (cell, mut genome) in &mut genomes {
        let diff = genome.compute_diff();
        if !diff.enabled.is_empty() || !diff.disabled.is_empty() {
            diff_writer.write(GenomeDiffEvent {
                cell,
                enabled: diff.enabled,
                disabled: diff.disabled,
            });
        }
        // Do not mark the genome as changed again for the snapshot update.
        genome.bypass_change_detection().update_previous_state();
    }
}

/// The starter genome of a vent cell: chemosynthesis for energy and carbon,
/// biosynthesis of amino acids and lipids, protein synthesis, and nucleotide
/// synthesis for genome repair. Fermentation is present but silent. There is no
/// pigment gene: the cell must find it.
pub fn create_starter_genome() -> Genome {
    let mut genome = Genome::default();
    for kind in [
        BlockKind::Chemosynthesis,
        BlockKind::AminoAcidBiosynthesis,
        BlockKind::LipidMetabolism,
        BlockKind::ProteinSynthesis,
        BlockKind::NucleotideCofactorSynthesis,
    ] {
        genome.add_gene(kind);
        genome.express_gene(kind);
    }
    genome.add_gene(BlockKind::Fermentation);
    genome
}
