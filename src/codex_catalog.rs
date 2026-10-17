//! # The codex catalog
//!
//! The catalog is the list of everything the player can learn about: one
//! `CodexEntry` for each gene (`BlockKind::ALL`) and for each membrane protein
//! (`MembraneProtein::ALL`). The Codex screen (`screens::codex`) shows it.
//!
//! The catalog is data. Numbers come from the code that uses them (block specs,
//! protein costs, action constants), so they cannot drift. Only the words are
//! written by hand, in `gene_facts` and `protein_facts`. Each of those is one
//! `match` without a wildcard arm: a new `BlockKind` or `MembraneProtein`
//! does not compile until its facts are written. That is the whole work to
//! add an entry. Add the icon in `gene_icon` or `overlay_sheet` too, or the
//! entry gets a labelled placeholder tile (and the art list in
//! `docs/art-direction.md` grows).
//!
//! The rules for state and edits are pure functions of a `CodexContext` (a
//! copy of the data that they need). The screen only fetches the data and
//! sends the messages.

use std::collections::{BTreeSet, HashMap};

use crate::actions::{
    ADHESIN_ATP_PER_SEC, BURST_ATP, BURST_SECONDS, BURST_THRUST_MULTIPLIER, TOXIN_ATP, TOXIN_METALS,
};
use crate::blocks::block_spec;
use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::environment::Channel;
use crate::exchange::{BASE_UPTAKE_PER_SEC, TRANSPORTER_UPTAKE_PER_SEC};
use crate::molecules::Currency;
use crate::movement::FLAGELLUM_THRUST;
use crate::prepare::{START_BUDGET, can_add, gene_cost, is_selectable};
use crate::pilus::{PILUS_EXTEND_ATP, PILUS_RANGE, PILUS_RETRACT_ATP_PER_SEC, PILUS_RETRACT_SPEED};
use crate::stun::{INJECTOR_ATP, INJECTOR_COOLDOWN, INJECTOR_METALS, STUN_SECONDS};
use crate::proteins::{
    AMINO_ACIDS_PER_UNIT, ATP_PER_UNIT, MembraneProtein, ProteinTarget, Proteome, SynthesisQueue,
    UNITS_PER_PROTEIN, membrane_slot_usage, overlay_sheet,
};
use crate::stage::Stage;
use crate::walls::{ADHESIN_CRAWL_SPEED, LYSE_ATP};

/// The pathway icons that exist in `assets/sprites`.
pub const PATHWAY_ICONS: [(BlockKind, &str); 13] = [
    (BlockKind::Chemosynthesis, "sprites/pathway_icon_chemosynthesis.png"),
    (BlockKind::Fermentation, "sprites/pathway_icon_fermentation.png"),
    (BlockKind::AminoAcidBiosynthesis, "sprites/pathway_icon_amino_acid_biosynthesis.png"),
    (BlockKind::LipidMetabolism, "sprites/pathway_icon_lipid_metabolism.png"),
    (BlockKind::ProteinSynthesis, "sprites/pathway_icon_protein_synthesis.png"),
    (BlockKind::FatStorage, "sprites/pathway_icon_fat_storage.png"),
    (BlockKind::VesicleExport, "sprites/pathway_icon_vesicle_export.png"),
    (BlockKind::LightCapture, "sprites/pathway_icon_light_capture.png"),
    (BlockKind::Respiration, "sprites/pathway_icon_respiration.png"),
    (BlockKind::Catalase, "sprites/pathway_icon_catalase.png"),
    (BlockKind::Cuticle, "sprites/pathway_icon_cuticle.png"),
    (BlockKind::NucleotideCofactorSynthesis, "sprites/pathway_icon_nucleotide_cofactor_synthesis.png"),
    (BlockKind::SecondaryMetabolites, "sprites/pathway_icon_secondary_metabolites.png"),
];

/// The icon file of a gene (a path under `assets/`), if it has art.
pub fn gene_icon(kind: BlockKind) -> Option<&'static str> {
    PATHWAY_ICONS.iter().find(|(k, _)| *k == kind).map(|(_, p)| *p)
}

// --- Entries ---

/// What an entry is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryId {
    Gene(BlockKind),
    Protein(MembraneProtein),
}

/// The group of an entry on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Energy,
    Biosynthesis,
    Storage,
    Defence,
    Motility,
    Interaction,
    /// Genes with no block yet.
    Planned,
}

impl Category {
    /// Every category, in the order of the screen.
    pub const ALL: [Category; 7] = [
        Category::Energy,
        Category::Biosynthesis,
        Category::Storage,
        Category::Defence,
        Category::Motility,
        Category::Interaction,
        Category::Planned,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Energy => "Energy",
            Category::Biosynthesis => "Biosynthesis",
            Category::Storage => "Storage and waste",
            Category::Defence => "Defence",
            Category::Motility => "Motility",
            Category::Interaction => "Interaction",
            Category::Planned => "Planned",
        }
    }
}

/// Where the player gets an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The starter cell has it.
    Starter,
    /// Engulf a creature that carries the gene.
    Engulf,
    /// Engulf the gate cell of the stage.
    Gate,
    /// The key gene of the stage: some prey carry it and the stage keeps them
    /// alive until the player has it.
    KeyPrey,
    /// The protein synthesis block makes it from amino acids and ATP.
    Synthesis,
    /// No way to get it yet.
    NotYet,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Starter => "Starter cell",
            Source::Engulf => "Engulf a creature",
            Source::Gate => "Gate cell",
            Source::KeyPrey => "Key prey",
            Source::Synthesis => "Protein synthesis",
            Source::NotYet => "Not available yet",
        }
    }
}

/// The art of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// A still image: a path under `assets/`.
    Image(&'static str),
    /// A sheet of `manifest.json`. The screen shows frame 0 of the `idle` clip.
    Overlay(&'static str),
    /// No art yet: the screen shows a labelled tile.
    Missing,
}

/// What an entry costs.
#[derive(Debug, Clone, PartialEq)]
pub struct Cost {
    /// Prepare points for a starting gene. `None` for a protein, and for a gene
    /// that cannot be chosen.
    pub prepare_points: Option<u32>,
    /// Protein units to build one enzyme or one membrane protein.
    pub build_units: f32,
    /// Amino acids for that.
    pub build_amino: f32,
    /// ATP for that.
    pub build_atp: f32,
    /// What one use costs.
    pub per_use: Vec<(Currency, f32)>,
    /// What each second of use costs.
    pub per_second: Vec<(Currency, f32)>,
}

/// One entry of the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CodexEntry {
    pub id: EntryId,
    pub name: &'static str,
    pub category: Category,
    /// The first stage where the player can get it. `None`: never yet.
    pub stage: Option<Stage>,
    pub sources: Vec<Source>,
    /// What it does, in one line.
    pub effect: String,
    /// Flows per second for one enzyme at saturation. Negative: used.
    pub flows: Vec<(Currency, f32)>,
    /// Pools that the block needs but does not use up.
    pub cofactors: Vec<Currency>,
    /// Outside channels that the block needs.
    pub outside: Vec<Channel>,
    pub cost: Cost,
    /// A real organism or protein that the entry is based on.
    pub note: &'static str,
    pub icon: Icon,
}

impl CodexEntry {
    pub fn is_gene(&self) -> bool {
        matches!(self.id, EntryId::Gene(_))
    }
}

/// The words and the place of a gene.
struct Facts {
    name: &'static str,
    category: Category,
    stage: Option<Stage>,
    sources: &'static [Source],
    note: &'static str,
}

/// The hand-written facts of a gene. No wildcard arm: a new gene needs facts.
fn gene_facts(kind: BlockKind) -> Facts {
    use Category::*;
    use Source::*;
    let (name, category, stage, sources, note): (_, _, _, &'static [Source], _) = match kind {
        BlockKind::LightCapture => (
            "Light capture",
            Energy,
            Some(Stage::Vents),
            &[Gate, Engulf],
            "Cyanobacteria such as Synechocystis use photosystems to turn light into ATP and reducing power, and they release O2.",
        ),
        BlockKind::SugarCatabolism => (
            "Sugar catabolism",
            Planned,
            None,
            &[NotYet],
            "In glycolysis, Escherichia coli splits glucose into two pyruvate and makes a little ATP.",
        ),
        BlockKind::OrganicAcidOxidation => (
            "Organic acid oxidation",
            Planned,
            None,
            &[NotYet],
            "The TCA cycle of bacteria and mitochondria burns acetyl-CoA to CO2 and loads electron carriers.",
        ),
        BlockKind::Respiration => (
            "Respiration",
            Energy,
            Some(Stage::ShallowWater),
            &[Gate],
            "Aerobic bacteria such as E. coli pass electrons to O2 through a membrane chain. This gives far more ATP than fermentation.",
        ),
        BlockKind::Fermentation => (
            "Fermentation",
            Energy,
            Some(Stage::Vents),
            &[Engulf],
            "Lactobacillus turns pyruvate into lactate when there is no O2. Yeast makes ethanol in the same way.",
        ),
        BlockKind::NitrogenSulfurAssimilation => (
            "Nitrogen and sulfur assimilation",
            Planned,
            None,
            &[NotYet],
            "E. coli takes up ammonia with glutamine synthetase. Rhizobia fix N2 with nitrogenase.",
        ),
        BlockKind::AminoAcidBiosynthesis => (
            "Amino acid biosynthesis",
            Biosynthesis,
            Some(Stage::Vents),
            &[Starter],
            "E. coli makes all 20 amino acids from glucose and ammonia.",
        ),
        BlockKind::LipidMetabolism => (
            "Lipid metabolism",
            Biosynthesis,
            Some(Stage::Vents),
            &[Starter],
            "Bacteria build membrane lipids with fatty acid synthase enzymes, the FAS II system.",
        ),
        BlockKind::NucleotideCofactorSynthesis => (
            "Nucleotide and cofactor synthesis",
            Biosynthesis,
            Some(Stage::Vents),
            &[Starter],
            "All cells build purines and pyrimidines from small precursors. They also make cofactors such as NAD+.",
        ),
        BlockKind::SecondaryMetabolites => (
            "Pigments",
            Defence,
            Some(Stage::Vents),
            &[KeyPrey, Engulf],
            "Cyanobacteria make scytonemin, a sheath pigment that blocks UV. Streptomyces makes the blue pigment actinorhodin.",
        ),
        BlockKind::AromaticPrecursorSynthesis => (
            "Aromatic precursors",
            Planned,
            None,
            &[NotYet],
            "The shikimate pathway of bacteria and plants makes the precursors of aromatic amino acids.",
        ),
        BlockKind::Polymerization => (
            "Polymerization",
            Planned,
            None,
            &[NotYet],
            "Land plants join phenol units into lignin. It makes wood stiff.",
        ),
        BlockKind::FatStorage => (
            "Fat storage",
            Storage,
            Some(Stage::Vents),
            &[Engulf],
            "Rhodococcus and Mycobacterium keep triacylglycerol in lipid droplets for later use.",
        ),
        BlockKind::VesicleExport => (
            "Vesicle export",
            Storage,
            Some(Stage::Vents),
            &[Engulf],
            "Gram-negative bacteria such as E. coli shed outer-membrane vesicles. They carry waste and other cargo out.",
        ),
        BlockKind::Chemosynthesis => (
            "Chemosynthesis",
            Energy,
            Some(Stage::Vents),
            &[Starter],
            "Symbionts of the giant tubeworm Riftia oxidise hydrogen sulfide from vents to make ATP and fix carbon.",
        ),
        BlockKind::ProteinSynthesis => (
            "Protein synthesis",
            Biosynthesis,
            Some(Stage::Vents),
            &[Starter],
            "The 70S ribosome of bacteria reads mRNA and joins amino acids into proteins.",
        ),
        BlockKind::Catalase => (
            "Catalase",
            Defence,
            Some(Stage::ShallowWater),
            &[KeyPrey],
            "Catalases such as KatE of E. coli turn hydrogen peroxide into water and O2.",
        ),
        BlockKind::Cuticle => (
            "Cuticle",
            Defence,
            Some(Stage::Land),
            &[KeyPrey],
            "Land plants cover their surface with a waxy cuticle. It keeps water in.",
        ),
        BlockKind::Multicellularity => (
            "Multicellularity",
            Planned,
            Some(Stage::Land),
            &[Gate],
            "Volvox cells live in a hollow colony. Dictyostelium cells join into a slug when food is short.",
        ),
    };
    Facts { name, category, stage, sources, note }
}

/// The hand-written facts of a membrane protein. No wildcard arm.
fn protein_facts(protein: MembraneProtein) -> (Facts, String) {
    use Category::*;
    let facts = |name, category, note| Facts {
        name,
        category,
        stage: Some(Stage::Vents),
        sources: &[Source::Synthesis],
        note,
    };
    match protein {
        MembraneProtein::Transporter => (
            facts(
                "Transporter",
                Interaction,
                "ABC transporters of E. coli move nutrients across the membrane. Each one is a protein in the membrane.",
            ),
            format!(
                "Takes up vent minerals. Each one adds {TRANSPORTER_UPTAKE_PER_SEC} per second to the base uptake of {BASE_UPTAKE_PER_SEC}. It also lets the cell pass a pore in a wall."
            ),
        ),
        MembraneProtein::Flagellum => (
            facts(
                "Flagellum",
                Motility,
                "The flagellar motor of E. coli turns a long filament. Protons flowing through the motor drive it.",
            ),
            format!(
                "Adds {FLAGELLUM_THRUST} thrust. The burst action gives x{BURST_THRUST_MULTIPLIER} thrust for {BURST_SECONDS} s."
            ),
        ),
        MembraneProtein::Adhesin => (
            facts(
                "Adhesin",
                Motility,
                "FimH, at the tip of the type 1 pilus of E. coli, sticks to sugars on the surface of host cells.",
            ),
            format!(
                "Hold to grab a surface. The cell crawls through a wall at {ADHESIN_CRAWL_SPEED} px per second."
            ),
        ),
        MembraneProtein::Toxin => (
            facts(
                "Toxin",
                Interaction,
                "Alpha-hemolysin of Staphylococcus aureus makes pores in the membrane of other cells.",
            ),
            "Fires a droplet that damages the cell that it hits.".to_string(),
        ),
        MembraneProtein::Pilus => (
            facts(
                "Type IV pilus",
                Motility,
                "Myxococcus and Bdellovibrio extend a type IV pilus (the ATPase PilB adds subunits). The tip sticks to a surface or to prey. The ATPase PilT then removes subunits, the fibre shrinks, and it pulls with a force of many piconewtons. This is twitching motility.",
            ),
            format!(
                "Press to shoot along the aim (range {PILUS_RANGE} px). Hold to reel in at {PILUS_RETRACT_SPEED} px per second. A smaller cell comes to you, so you can engulf it. A bigger cell or a wall pulls you to it. Release to let go."
            ),
        ),
        MembraneProtein::Injector => (
            facts(
                "Contractile injector",
                Interaction,
                "The type VI secretion system is a spear. A sheath contracts and pushes a tube with toxic effectors into a cell that touches it. Many bacteria use it against rivals and prey.",
            ),
            format!(
                "Needs contact, in a cone round the aim. Stuns the cell for {STUN_SECONDS} s: no thrust, no AI, no engulfing. Cooldown {INJECTOR_COOLDOWN} s. Stunned prey cannot escape you."
            ),
        ),
        MembraneProtein::Lytic => (
            facts(
                "Lytic protein",
                Interaction,
                "Lysozyme cuts the peptidoglycan wall of bacteria. Phage lysins burst a host cell from inside.",
            ),
            "Cuts a hole in a world membrane (the action Lyse).".to_string(),
        ),
        MembraneProtein::Receptor => (
            facts(
                "Receptor",
                Interaction,
                "The chemoreceptors Tar and Tsr of E. coli sense amino acids and sugars in the water.",
            ),
            "Senses the outside. It has no effect yet.".to_string(),
        ),
    }
}

/// What building one protein costs. The same for every protein.
fn build_cost(prepare_points: Option<u32>) -> Cost {
    Cost {
        prepare_points,
        build_units: UNITS_PER_PROTEIN,
        build_amino: UNITS_PER_PROTEIN * AMINO_ACIDS_PER_UNIT,
        build_atp: UNITS_PER_PROTEIN * ATP_PER_UNIT,
        per_use: Vec::new(),
        per_second: Vec::new(),
    }
}

/// The entry of a gene.
fn gene_entry(kind: BlockKind) -> CodexEntry {
    let facts = gene_facts(kind);
    let spec = block_spec(kind);
    let mut flows: Vec<(Currency, f32)> = spec
        .as_ref()
        .map(|s| s.profile.0.iter().map(|(c, r)| (*c, *r)).collect())
        .unwrap_or_default();
    flows.sort_by_key(|(c, _)| *c);
    let (cofactors, outside) = spec
        .as_ref()
        .map(|s| (s.kinetics.cofactors.to_vec(), s.kinetics.external.to_vec()))
        .unwrap_or_default();
    CodexEntry {
        id: EntryId::Gene(kind),
        name: facts.name,
        category: facts.category,
        stage: facts.stage,
        sources: facts.sources.to_vec(),
        effect: kind.description().to_string(),
        flows,
        cofactors,
        outside,
        cost: build_cost((gene_cost(kind) > 0).then(|| gene_cost(kind))),
        note: facts.note,
        icon: gene_icon(kind).map_or(Icon::Missing, Icon::Image),
    }
}

/// The entry of a membrane protein.
fn protein_entry(protein: MembraneProtein) -> CodexEntry {
    let (facts, effect) = protein_facts(protein);
    let mut cost = build_cost(None);
    match protein {
        MembraneProtein::Flagellum => cost.per_use.push((Currency::ATP, BURST_ATP)),
        MembraneProtein::Adhesin => cost.per_second.push((Currency::ATP, ADHESIN_ATP_PER_SEC)),
        MembraneProtein::Toxin => {
            cost.per_use.push((Currency::ATP, TOXIN_ATP));
            cost.per_use.push((Currency::Metals, TOXIN_METALS));
        }
        MembraneProtein::Lytic => cost.per_use.push((Currency::ATP, LYSE_ATP)),
        MembraneProtein::Pilus => {
            cost.per_use.push((Currency::ATP, PILUS_EXTEND_ATP));
            cost.per_second.push((Currency::ATP, PILUS_RETRACT_ATP_PER_SEC));
        }
        MembraneProtein::Injector => {
            cost.per_use.push((Currency::ATP, INJECTOR_ATP));
            cost.per_use.push((Currency::Metals, INJECTOR_METALS));
        }
        MembraneProtein::Transporter | MembraneProtein::Receptor => {}
    }
    CodexEntry {
        id: EntryId::Protein(protein),
        name: facts.name,
        category: facts.category,
        stage: facts.stage,
        sources: facts.sources.to_vec(),
        effect,
        flows: Vec::new(),
        cofactors: Vec::new(),
        outside: Vec::new(),
        cost,
        note: facts.note,
        icon: Icon::Overlay(overlay_sheet(protein)),
    }
}

/// The entry for an id.
pub fn entry(id: EntryId) -> CodexEntry {
    match id {
        EntryId::Gene(kind) => gene_entry(kind),
        EntryId::Protein(protein) => protein_entry(protein),
    }
}

/// Every entry: genes in `BlockKind::ALL` order, then membrane proteins.
pub fn all_entries() -> Vec<CodexEntry> {
    BlockKind::ALL
        .into_iter()
        .map(|k| gene_entry(k))
        .chain(MembraneProtein::ALL.into_iter().map(protein_entry))
        .collect()
}

// --- State of an entry ---

/// What the screen is for. It decides which edits are possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditMode {
    /// Look only (the menu).
    #[default]
    Browse,
    /// A run is on (Manage): proteins can be queued.
    Run,
    /// Before a run: genes can go in the loadout.
    Prepare,
}

/// A copy of the data that the codex rules need. The screen builds it from the
/// ECS each frame and compares it with the last one to know when to redraw.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodexContext {
    pub mode: EditMode,
    /// Genes in the codex file.
    pub codex_genes: BTreeSet<BlockKind>,
    /// The genome that is shown: the cell's, or the starter genome plus the
    /// loadout.
    pub genes: HashMap<BlockKind, GeneState>,
    pub proteome: Proteome,
    pub queue: SynthesisQueue,
    /// Membrane slots of the cell.
    pub slots: u32,
    /// The loadout that is chosen in Prepare.
    pub chosen: BTreeSet<BlockKind>,
}

impl CodexContext {
    /// The context outside a run: the starter cell plus the chosen genes.
    pub fn without_run(mode: EditMode, codex_genes: BTreeSet<BlockKind>, chosen: BTreeSet<BlockKind>) -> Self {
        let mut genome = crate::blocks::genome::create_starter_genome();
        let mut proteome = crate::shared::starter_proteome();
        crate::prepare::apply_loadout(&mut genome, &mut proteome, &chosen);
        Self {
            mode,
            codex_genes,
            genes: genome.table,
            proteome,
            queue: SynthesisQueue::default(),
            slots: 0,
            chosen,
        }
    }

    /// The context of a cell in a run.
    pub fn of_cell(
        codex_genes: BTreeSet<BlockKind>,
        genome: &Genome,
        proteome: &Proteome,
        queue: &SynthesisQueue,
        slots: u32,
    ) -> Self {
        Self {
            mode: EditMode::Run,
            codex_genes,
            genes: genome.table.clone(),
            proteome: proteome.clone(),
            queue: queue.clone(),
            slots,
            chosen: BTreeSet::new(),
        }
    }
}

/// What the player knows and has of an entry.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryState {
    /// False: the screen shows a silhouette and "?".
    pub known: bool,
    pub in_codex: bool,
    /// The state of the gene in the genome that is shown.
    pub gene: Option<GeneState>,
    /// Enzymes of a gene, or proteins of a membrane protein kind.
    pub count: u32,
    /// Orders of this entry in the synthesis queue.
    pub queued: u32,
    /// The gene is in the Prepare loadout.
    pub chosen: bool,
}

/// The state of an entry. A gene is known if it is in the codex or in the
/// genome. A membrane protein is always known: the cell can make it from the
/// start.
pub fn entry_state(id: EntryId, ctx: &CodexContext) -> EntryState {
    match id {
        EntryId::Gene(kind) => {
            let gene = ctx.genes.get(&kind).cloned();
            let in_codex = ctx.codex_genes.contains(&kind);
            let target = ProteinTarget::Enzyme(kind);
            EntryState {
                known: in_codex || gene.is_some(),
                in_codex,
                gene,
                count: ctx.proteome.count(target),
                queued: queued(&ctx.queue, target),
                chosen: ctx.chosen.contains(&kind),
            }
        }
        EntryId::Protein(protein) => {
            let target = ProteinTarget::Membrane(protein);
            EntryState {
                known: true,
                in_codex: false,
                gene: None,
                count: ctx.proteome.count(target),
                queued: queued(&ctx.queue, target),
                chosen: false,
            }
        }
    }
}

fn queued(queue: &SynthesisQueue, target: ProteinTarget) -> u32 {
    queue.orders.iter().filter(|o| **o == target).count() as u32
}

/// The badge on a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Unknown,
    InCodex,
    Expressed,
    Enhanced,
    Mutated,
    Silent,
    /// A membrane protein: the count is in the state.
    Protein,
}

impl Badge {
    pub fn text(self) -> &'static str {
        match self {
            Badge::Unknown => "unknown",
            Badge::InCodex => "in codex",
            Badge::Expressed => "expressed",
            Badge::Enhanced => "enhanced",
            Badge::Mutated => "mutated",
            Badge::Silent => "silent",
            Badge::Protein => "protein",
        }
    }
}

pub fn badge(id: EntryId, state: &EntryState) -> Badge {
    match (id, &state.gene) {
        (EntryId::Protein(_), _) => Badge::Protein,
        (_, Some(GeneState::Expressed)) => Badge::Expressed,
        (_, Some(GeneState::Enhanced)) => Badge::Enhanced,
        (_, Some(GeneState::Mutated)) => Badge::Mutated,
        (_, Some(GeneState::Silent)) => Badge::Silent,
        (_, None) if state.in_codex => Badge::InCodex,
        (_, None) => Badge::Unknown,
    }
}

// --- Filter and groups ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Genes,
    Proteins,
    /// In the current genome or proteome, or in the codex.
    Owned,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Genes, Filter::Proteins, Filter::Owned];

    pub fn name(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Genes => "Genes",
            Filter::Proteins => "Membrane proteins",
            Filter::Owned => "Owned",
        }
    }

    pub fn accepts(self, id: EntryId, state: &EntryState) -> bool {
        match self {
            Filter::All => true,
            Filter::Genes => matches!(id, EntryId::Gene(_)),
            Filter::Proteins => matches!(id, EntryId::Protein(_)),
            Filter::Owned => state.gene.is_some() || state.in_codex || state.count > 0,
        }
    }
}

/// The entries that pass the filter, grouped by category in screen order.
/// A category with no entry is left out.
pub fn grouped(filter: Filter, ctx: &CodexContext) -> Vec<(Category, Vec<CodexEntry>)> {
    let entries = all_entries();
    Category::ALL
        .into_iter()
        .filter_map(|category| {
            let list: Vec<CodexEntry> = entries
                .iter()
                .filter(|e| e.category == category && filter.accepts(e.id, &entry_state(e.id, ctx)))
                .cloned()
                .collect();
            (!list.is_empty()).then_some((category, list))
        })
        .collect()
}

// --- Edits ---

/// A change that the detail panel can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Put one protein in the synthesis queue (`proteins::QueueProtein`).
    Queue(ProteinTarget),
    /// Add or remove a gene in the Prepare loadout (`prepare::toggle`).
    ToggleLoadout(BlockKind),
}

/// One button of the detail panel.
#[derive(Debug, Clone, PartialEq)]
pub struct EditOption {
    pub edit: Edit,
    pub label: String,
    pub enabled: bool,
    /// Why the button is off, or other help.
    pub note: Option<String>,
}

/// The edit buttons for an entry. No new rules: they use the rules of
/// `proteins` (slots) and `prepare` (budget).
pub fn edit_options(id: EntryId, ctx: &CodexContext) -> Vec<EditOption> {
    match (ctx.mode, id) {
        (EditMode::Run, EntryId::Gene(kind)) => {
            if block_spec(kind).is_none() {
                return Vec::new();
            }
            let has_gene = ctx.genes.contains_key(&kind);
            vec![EditOption {
                edit: Edit::Queue(ProteinTarget::Enzyme(kind)),
                label: "Queue enzyme".to_string(),
                enabled: has_gene,
                note: (!has_gene).then(|| "The cell needs the gene first.".to_string()),
            }]
        }
        (EditMode::Run, EntryId::Protein(protein)) => {
            let (used, total) = membrane_slot_usage(&ctx.queue, &ctx.proteome, ctx.slots);
            let free = used < total;
            vec![EditOption {
                edit: Edit::Queue(ProteinTarget::Membrane(protein)),
                label: "Queue protein".to_string(),
                enabled: free,
                note: Some(format!("Membrane slots: {used}/{total}.")),
            }]
        }
        (EditMode::Prepare, EntryId::Gene(kind)) => {
            let codex = crate::codex::Codex { genes: ctx.codex_genes.clone() };
            if !is_selectable(kind, &codex) {
                return Vec::new();
            }
            let chosen = ctx.chosen.contains(&kind);
            let enabled = chosen || can_add(&ctx.chosen, kind, &codex, START_BUDGET);
            vec![EditOption {
                edit: Edit::ToggleLoadout(kind),
                label: if chosen { "Remove from loadout" } else { "Add to loadout" }.to_string(),
                enabled,
                note: Some(format!(
                    "Points: {}/{START_BUDGET}.",
                    crate::prepare::spent(&ctx.chosen)
                )),
            }]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(mode: EditMode, genes: &[BlockKind]) -> CodexContext {
        CodexContext::without_run(mode, genes.iter().copied().collect(), BTreeSet::new())
    }

    #[test]
    fn every_gene_and_protein_has_an_entry() {
        let entries = all_entries();
        assert_eq!(entries.len(), BlockKind::ALL.len() + MembraneProtein::ALL.len());
        for kind in BlockKind::ALL {
            assert!(entries.iter().any(|e| e.id == EntryId::Gene(kind)), "{kind:?}");
        }
        for protein in MembraneProtein::ALL {
            assert!(entries.iter().any(|e| e.id == EntryId::Protein(protein)), "{protein:?}");
        }
        for e in &entries {
            assert!(!e.name.is_empty() && !e.effect.is_empty() && !e.note.is_empty(), "{:?}", e.id);
            assert!(!e.sources.is_empty(), "{:?}", e.id);
        }
    }

    #[test]
    fn planned_genes_are_exactly_the_ones_without_a_block() {
        for kind in BlockKind::ALL {
            let e = entry(EntryId::Gene(kind));
            assert_eq!(e.category == Category::Planned, block_spec(kind).is_none(), "{kind:?}");
            assert_eq!(e.cost.prepare_points.is_some(), block_spec(kind).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn every_icon_exists_in_the_assets() {
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string("assets/sprites/anim/manifest.json").unwrap()).unwrap();
        for e in all_entries() {
            match e.icon {
                Icon::Image(path) => {
                    assert!(std::path::Path::new("assets").join(path).exists(), "{:?}: {path}", e.id);
                }
                Icon::Overlay(sheet) => {
                    let file = manifest[sheet]["idle"]["file"].as_str();
                    let file = file.unwrap_or_else(|| panic!("{:?}: no idle clip in {sheet}", e.id));
                    let path = std::path::Path::new("assets/sprites/anim").join(file);
                    assert!(path.exists(), "{:?}: {}", e.id, path.display());
                }
                Icon::Missing => {}
            }
        }
    }

    #[test]
    fn genes_without_art_are_the_planned_ones_and_multicellularity() {
        // Keep this list in step with "Missing art" in docs/art-direction.md.
        let missing: Vec<_> = BlockKind::ALL
            .into_iter()
            .filter(|k| gene_icon(*k).is_none())
            .collect();
        for kind in &missing {
            assert!(block_spec(*kind).is_none(), "{kind:?} has a block but no icon");
        }
    }

    #[test]
    fn numbers_come_from_the_code() {
        let toxin = entry(EntryId::Protein(MembraneProtein::Toxin));
        assert_eq!(toxin.cost.per_use, vec![(Currency::ATP, TOXIN_ATP), (Currency::Metals, TOXIN_METALS)]);
        let chemo = entry(EntryId::Gene(BlockKind::Chemosynthesis));
        assert!(chemo.flows.contains(&(Currency::ATP, crate::blocks::chemosynthesis::ATP_RATE)));
        assert!(chemo.cofactors.contains(&Currency::Metals));
        assert!(chemo.outside.contains(&Channel::Heat));
        assert_eq!(chemo.cost.build_atp, UNITS_PER_PROTEIN * ATP_PER_UNIT);
    }

    #[test]
    fn gene_states_follow_codex_and_genome() {
        let ctx = ctx_with(EditMode::Browse, &[BlockKind::Fermentation, BlockKind::Respiration]);
        // Starter genome: chemosynthesis expressed, fermentation silent.
        let s = entry_state(EntryId::Gene(BlockKind::Chemosynthesis), &ctx);
        assert_eq!(badge(EntryId::Gene(BlockKind::Chemosynthesis), &s), Badge::Expressed);
        assert_eq!(s.count, 2);
        let s = entry_state(EntryId::Gene(BlockKind::Fermentation), &ctx);
        assert_eq!(badge(EntryId::Gene(BlockKind::Fermentation), &s), Badge::Silent);
        let s = entry_state(EntryId::Gene(BlockKind::Respiration), &ctx);
        assert_eq!(badge(EntryId::Gene(BlockKind::Respiration), &s), Badge::InCodex);
        let s = entry_state(EntryId::Gene(BlockKind::Cuticle), &ctx);
        assert!(!s.known);
        assert_eq!(badge(EntryId::Gene(BlockKind::Cuticle), &s), Badge::Unknown);
    }

    #[test]
    fn the_owned_filter_and_groups() {
        let ctx = ctx_with(EditMode::Browse, &[BlockKind::Respiration]);
        let all: usize = grouped(Filter::All, &ctx).iter().map(|(_, l)| l.len()).sum();
        assert_eq!(all, all_entries().len());
        let proteins: usize = grouped(Filter::Proteins, &ctx).iter().map(|(_, l)| l.len()).sum();
        assert_eq!(proteins, MembraneProtein::ALL.len());
        let owned = grouped(Filter::Owned, &ctx);
        let owned_ids: Vec<_> = owned.iter().flat_map(|(_, l)| l.iter().map(|e| e.id)).collect();
        assert!(owned_ids.contains(&EntryId::Gene(BlockKind::Respiration)));
        assert!(owned_ids.contains(&EntryId::Gene(BlockKind::Chemosynthesis)));
        assert!(!owned_ids.contains(&EntryId::Gene(BlockKind::Cuticle)));
        assert!(!owned_ids.contains(&EntryId::Protein(MembraneProtein::Toxin)));
    }

    #[test]
    fn run_edits_queue_proteins_within_the_slots() {
        let mut ctx = ctx_with(EditMode::Run, &[]);
        ctx.slots = 1;
        let toxin = EntryId::Protein(MembraneProtein::Toxin);
        assert!(edit_options(toxin, &ctx)[0].enabled);
        ctx.queue.orders.push_back(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        assert!(!edit_options(toxin, &ctx)[0].enabled, "the slot is ordered");
        // An enzyme needs the gene in the genome. A planned gene has no button.
        let respiration = EntryId::Gene(BlockKind::Respiration);
        assert!(!edit_options(respiration, &ctx)[0].enabled);
        ctx.genes.insert(BlockKind::Respiration, GeneState::Silent);
        assert!(edit_options(respiration, &ctx)[0].enabled);
        assert!(edit_options(EntryId::Gene(BlockKind::Polymerization), &ctx).is_empty());
    }

    #[test]
    fn prepare_edits_follow_the_budget() {
        let mut ctx = ctx_with(
            EditMode::Prepare,
            &[BlockKind::Fermentation, BlockKind::FatStorage, BlockKind::VesicleExport],
        );
        let ferment = EntryId::Gene(BlockKind::Fermentation);
        assert_eq!(edit_options(ferment, &ctx)[0].label, "Add to loadout");
        // A gene that is not in the codex cannot be chosen.
        assert!(edit_options(EntryId::Gene(BlockKind::Cuticle), &ctx).is_empty());
        ctx.chosen = [BlockKind::Fermentation, BlockKind::FatStorage].into_iter().collect();
        assert_eq!(edit_options(ferment, &ctx)[0].label, "Remove from loadout");
        assert!(edit_options(ferment, &ctx)[0].enabled);
        let third = EntryId::Gene(BlockKind::VesicleExport);
        assert!(!edit_options(third, &ctx)[0].enabled, "over budget");
        // Proteins have no Prepare edit; Browse has no edits.
        assert!(edit_options(EntryId::Protein(MembraneProtein::Toxin), &ctx).is_empty());
        ctx.mode = EditMode::Browse;
        assert!(edit_options(ferment, &ctx).is_empty());
    }

    #[test]
    fn a_chosen_gene_shows_as_expressed_with_an_enzyme() {
        let ctx = CodexContext::without_run(
            EditMode::Prepare,
            [BlockKind::Fermentation].into_iter().collect(),
            [BlockKind::Fermentation].into_iter().collect(),
        );
        let s = entry_state(EntryId::Gene(BlockKind::Fermentation), &ctx);
        assert_eq!(badge(EntryId::Gene(BlockKind::Fermentation), &s), Badge::Expressed);
        assert_eq!(s.count, 1);
        assert!(s.chosen);
    }
}
