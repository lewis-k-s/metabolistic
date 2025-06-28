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
//! - **Mutation**: Random errors can temporarily disable gene tiles until repaired
//!
//! ## Usage
//!
//! Add the `GenomePlugin` to your Bevy app and use the genome system:
//! ```rust,no_run
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
//! - Applies changes to metabolic block entities
//! - Handles random mutations over time
//!
//! ## Controls (Demo)
//!
//! - Press 'G' to express the Sugar Catabolism gene
//! - Press 'H' to silence the Fermentation gene  
//! - Press 'J' to add a new Light Capture gene
//! - Press 'K' to spawn new metabolic block entities

use bevy::prelude::*;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents the different types of metabolic blocks that can be encoded in the genome
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, Serialize, Deserialize)]
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
        }
    }
}

/// The state of a gene tile in the genome
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GeneState {
    /// Gene is present but not expressed (no enzyme production)
    Silent,
    /// Gene is actively expressed (enzyme is being produced)
    Expressed,
    /// Gene is mutated and temporarily non-functional
    Mutated,
}

impl Default for GeneState {
    fn default() -> Self {
        GeneState::Silent
    }
}

/// Resource containing the entire chromosome of gene tiles
#[derive(Resource, Default)]
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
    pub fn add_gene(&mut self, block_kind: BlockKind) {
        self.table.insert(block_kind, GeneState::Silent);
    }

    /// Express a gene (activate the metabolic block)
    pub fn express_gene(&mut self, block_kind: BlockKind) -> bool {
        if let Some(state) = self.table.get_mut(&block_kind) {
            match state {
                GeneState::Silent => {
                    *state = GeneState::Expressed;
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Silence a gene (deactivate the metabolic block)
    pub fn silence_gene(&mut self, block_kind: BlockKind) -> bool {
        if let Some(state) = self.table.get_mut(&block_kind) {
            match state {
                GeneState::Expressed => {
                    *state = GeneState::Silent;
                    true
                }
                _ => false,
            }
        } else {
            false
        }
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

    /// Repair a mutated gene
    pub fn repair_gene(&mut self, block_kind: BlockKind) -> bool {
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
    fn update_previous_state(&mut self) {
        self.previous_table = self.table.clone();
    }

    /// Compute the diff between current and previous state
    fn compute_diff(&self) -> GenomeDiff {
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
}

/// Event containing changes in gene expression that affect metabolic blocks
#[derive(Event, Debug)]
pub struct GenomeDiffEvent {
    pub enabled: Vec<BlockKind>,
    pub disabled: Vec<BlockKind>,
}

/// A differential summary of genome changes
#[derive(Debug)]
pub struct GenomeDiff {
    pub enabled: Vec<BlockKind>,
    pub disabled: Vec<BlockKind>,
}

/// Marker component for metabolic blocks that can be controlled by the genome
#[derive(Component)]
pub struct MetabolicBlock {
    pub block_kind: BlockKind,
}

/// Component to enable/disable metabolic blocks
#[derive(Component)]
pub struct Enabled(pub bool);

impl Default for Enabled {
    fn default() -> Self {
        Enabled(false)
    }
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
        app.insert_resource(Genome::default())
            .insert_resource(GenomeOperationCosts::default())
            .add_event::<GenomeDiffEvent>()
            .add_systems(PreUpdate, poll_genome_diff)
            .add_systems(Update, apply_genome_diff)
            .add_systems(PostUpdate, random_mutation_system);
    }
}

/// System that compares current vs. previous genome snapshot and emits only the delta
pub fn poll_genome_diff(mut genome: ResMut<Genome>, mut diff_writer: EventWriter<GenomeDiffEvent>) {
    let diff = genome.compute_diff();

    if !diff.enabled.is_empty() || !diff.disabled.is_empty() {
        diff_writer.send(GenomeDiffEvent {
            enabled: diff.enabled,
            disabled: diff.disabled,
        });
    }

    // Update the previous state snapshot for next frame
    genome.update_previous_state();
}

/// System that receives genome diff events and toggles metabolic blocks accordingly
pub fn apply_genome_diff(
    mut diff_reader: EventReader<GenomeDiffEvent>,
    mut metabolic_blocks: Query<(&mut Enabled, &MetabolicBlock)>,
) {
    for diff in diff_reader.read() {
        for (mut enabled, metabolic_block) in metabolic_blocks.iter_mut() {
            if diff.enabled.contains(&metabolic_block.block_kind) {
                enabled.0 = true;
                info!("Enabled metabolic block: {:?}", metabolic_block.block_kind);
            }

            if diff.disabled.contains(&metabolic_block.block_kind) {
                enabled.0 = false;
                info!("Disabled metabolic block: {:?}", metabolic_block.block_kind);
            }
        }
    }
}

/// System that occasionally mutates genes to create gameplay challenges
pub fn random_mutation_system(mut genome: ResMut<Genome>, time: Res<Time>) {
    // Simple mutation system - in real implementation this would be more sophisticated
    const MUTATION_CHANCE_PER_SECOND: f32 = 0.01; // 1% chance per second per gene

    let delta_time = time.delta_secs();

    for (block_kind, _state) in genome.table.clone().iter() {
        if thread_rng().gen::<f32>() < MUTATION_CHANCE_PER_SECOND * delta_time {
            genome.mutate_gene(*block_kind);
            warn!("Gene {:?} has mutated!", block_kind);
        }
    }
}

/// Helper function to create a basic genome with some starting genes
pub fn create_starter_genome() -> Genome {
    let mut genome = Genome::default();

    // Add some basic metabolic pathways as starter genes
    genome.add_gene(BlockKind::SugarCatabolism);
    genome.add_gene(BlockKind::Fermentation);
    genome.add_gene(BlockKind::AminoAcidBiosynthesis);

    genome
}

/// Helper function to spawn a metabolic block entity
pub fn spawn_metabolic_block(commands: &mut Commands, block_kind: BlockKind) -> Entity {
    commands
        .spawn((
            MetabolicBlock { block_kind },
            Enabled::default(),
            Name::new(format!("Metabolic Block: {:?}", block_kind)),
        ))
        .id()
}
