//! # Protein economy
//!
//! The protein synthesis block turns AminoAcids + ATP into protein units. The
//! cell spends units on the orders in its `SynthesisQueue`. Each finished
//! protein goes into the cell's `Proteome`:
//!
//! - as an **enzyme** of a block: more enzymes make the block faster;
//! - as a **membrane protein**: transporters, flagella, adhesins, toxins,
//!   receptors. Each one needs a membrane slot (see `cell::membrane_slots`).
//!
//! Proteins do not decay. The cell can recycle a protein for about half of its
//! amino acids.
//!
//! The pure functions here change the queue and the proteome. The editor sends
//! `QueueProtein` and `RecycleProtein` messages; `apply_protein_commands`
//! applies them.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blocks::genome::BlockKind;
use crate::cell::{Cell, CellSize, membrane_slots};
use crate::metabolism::{CurrencyPools, FlowLedger, FlowSource};
use crate::molecules::Currency;

/// Protein units in one protein.
pub const UNITS_PER_PROTEIN: f32 = 10.0;
/// Amino acids in one protein unit.
pub const AMINO_ACIDS_PER_UNIT: f32 = 1.0;
/// ATP to make one protein unit.
pub const ATP_PER_UNIT: f32 = 2.0;
/// Fraction of a protein's amino acids that recycling gives back.
pub const RECYCLE_FRACTION: f32 = 0.5;

/// A protein that sits in the membrane and gives the cell an action or a
/// passive effect. Actions come in later milestones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MembraneProtein {
    /// Moves substances between the outside and the cell.
    Transporter,
    /// Gives thrust.
    Flagellum,
    /// Holds on to surfaces and lets the cell pass through walls slowly.
    Adhesin,
    /// Damages other cells.
    Toxin,
    /// Senses the outside.
    Receptor,
    /// Cuts world membranes (the action Lyse).
    Lytic,
    /// A type IV pilus: shoots out, sticks, and reels in (see `pilus`).
    Pilus,
    /// A type VI secretion spear: stuns a cell in contact (see `stun`).
    Injector,
}

impl MembraneProtein {
    /// Every membrane protein, in the order that screens list them.
    pub const ALL: [MembraneProtein; 8] = [
        MembraneProtein::Transporter,
        MembraneProtein::Flagellum,
        MembraneProtein::Adhesin,
        MembraneProtein::Toxin,
        MembraneProtein::Pilus,
        MembraneProtein::Injector,
        MembraneProtein::Lytic,
        MembraneProtein::Receptor,
    ];
}

/// The name of the overlay sheet (a sprite of `manifest.json`) that shows a
/// membrane protein. The one place that maps a protein to its art: the Manage
/// screen and the Run screen both use it.
pub fn overlay_sheet(protein: MembraneProtein) -> &'static str {
    match protein {
        // The only transporter that exists takes up minerals.
        MembraneProtein::Transporter => "overlay_transporter_mineral",
        MembraneProtein::Flagellum => "overlay_flagellum",
        MembraneProtein::Adhesin => "overlay_adhesin",
        MembraneProtein::Toxin => "overlay_toxin",
        MembraneProtein::Lytic => "overlay_lytic",
        MembraneProtein::Pilus => "overlay_pilus",
        MembraneProtein::Injector => "overlay_injector",
        MembraneProtein::Receptor => "overlay_receptor",
    }
}

/// What a protein is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProteinTarget {
    /// An enzyme of a metabolic block.
    Enzyme(BlockKind),
    /// A membrane protein.
    Membrane(MembraneProtein),
}

/// The finished proteins of a cell. This is the only source of truth for
/// enzyme counts: the solver reads them by block kind.
#[derive(Component, Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Proteome {
    pub enzymes: HashMap<BlockKind, u32>,
    pub membrane: HashMap<MembraneProtein, u32>,
}

impl Proteome {
    /// A proteome with the given enzyme counts.
    pub fn with_enzymes(enzymes: &[(BlockKind, u32)]) -> Self {
        Self {
            enzymes: enzymes.iter().copied().collect(),
            ..default()
        }
    }

    /// Enzymes allocated to a block kind.
    pub fn enzymes(&self, kind: BlockKind) -> u32 {
        self.enzymes.get(&kind).copied().unwrap_or(0)
    }

    /// Number of proteins of a target.
    pub fn count(&self, target: ProteinTarget) -> u32 {
        match target {
            ProteinTarget::Enzyme(kind) => self.enzymes(kind),
            ProteinTarget::Membrane(p) => self.membrane.get(&p).copied().unwrap_or(0),
        }
    }

    /// Membrane slots in use.
    pub fn membrane_total(&self) -> u32 {
        self.membrane.values().sum()
    }

    /// Adds one protein.
    pub fn add(&mut self, target: ProteinTarget) {
        match target {
            ProteinTarget::Enzyme(kind) => *self.enzymes.entry(kind).or_insert(0) += 1,
            ProteinTarget::Membrane(p) => *self.membrane.entry(p).or_insert(0) += 1,
        }
    }

    /// Removes one protein. Returns false if there is none.
    pub fn remove(&mut self, target: ProteinTarget) -> bool {
        let count = match target {
            ProteinTarget::Enzyme(kind) => self.enzymes.get_mut(&kind),
            ProteinTarget::Membrane(p) => self.membrane.get_mut(&p),
        };
        match count {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        }
    }
}

/// Proteins ordered but not yet built, oldest first.
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct SynthesisQueue {
    pub orders: VecDeque<ProteinTarget>,
    /// Protein units made but not yet spent on a finished protein. Units made
    /// after the last order stay here for the next order.
    pub progress: f32,
}

impl SynthesisQueue {
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

/// Why an order was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// All membrane slots are in use or ordered.
    NoMembraneSlot,
}

/// Membrane slots in use and in total. Membrane proteins in the queue count as
/// used, as in `queue_protein`.
pub fn membrane_slot_usage(queue: &SynthesisQueue, proteome: &Proteome, slots: u32) -> (u32, u32) {
    let queued = queue
        .orders
        .iter()
        .filter(|o| matches!(o, ProteinTarget::Membrane(_)))
        .count() as u32;
    (proteome.membrane_total() + queued, slots)
}

/// Protein units still to make for the whole queue.
pub fn units_left(queue: &SynthesisQueue) -> f32 {
    (queue.orders.len() as f32 * UNITS_PER_PROTEIN - queue.progress).max(0.0)
}

/// Seconds until the queue is done at `units_per_sec`. `None` if the queue is
/// empty or the rate is 0.
pub fn synthesis_eta(queue: &SynthesisQueue, units_per_sec: f32) -> Option<f32> {
    (!queue.is_empty() && units_per_sec > 0.0).then(|| units_left(queue) / units_per_sec)
}

/// Seconds until the first order is done at `units_per_sec`.
pub fn first_order_eta(queue: &SynthesisQueue, units_per_sec: f32) -> Option<f32> {
    (!queue.is_empty() && units_per_sec > 0.0)
        .then(|| (UNITS_PER_PROTEIN - queue.progress).max(0.0) / units_per_sec)
}

/// Adds an order to the queue. A membrane protein needs a free slot. Slots of
/// membrane proteins already in the queue count as used.
pub fn queue_protein(
    queue: &mut SynthesisQueue,
    proteome: &Proteome,
    slots: u32,
    target: ProteinTarget,
) -> Result<(), QueueError> {
    if let ProteinTarget::Membrane(_) = target {
        let queued = queue
            .orders
            .iter()
            .filter(|o| matches!(o, ProteinTarget::Membrane(_)))
            .count() as u32;
        if proteome.membrane_total() + queued >= slots {
            return Err(QueueError::NoMembraneSlot);
        }
    }
    queue.orders.push_back(target);
    Ok(())
}

/// Spends `units` new protein units on the queue. Returns the finished proteins.
pub fn advance_synthesis(
    queue: &mut SynthesisQueue,
    proteome: &mut Proteome,
    units: f32,
) -> Vec<ProteinTarget> {
    // Tolerance for float error in the sum of many small unit amounts.
    const EPS: f32 = 1e-4;
    queue.progress += units.max(0.0);
    let mut finished = Vec::new();
    while queue.progress + EPS >= UNITS_PER_PROTEIN {
        let Some(target) = queue.orders.pop_front() else {
            break;
        };
        queue.progress = (queue.progress - UNITS_PER_PROTEIN).max(0.0);
        proteome.add(target);
        finished.push(target);
    }
    finished
}

/// Amino acids that recycling one protein gives back.
pub fn recycle_yield() -> f32 {
    RECYCLE_FRACTION * UNITS_PER_PROTEIN * AMINO_ACIDS_PER_UNIT
}

/// Removes one protein and returns the amino acids it gives back, or `None`
/// if the cell has no such protein.
pub fn recycle_protein(proteome: &mut Proteome, target: ProteinTarget) -> Option<f32> {
    proteome.remove(target).then(recycle_yield)
}

/// Asks a cell to add a protein to its synthesis queue.
#[derive(Message, Debug, Clone, Copy)]
pub struct QueueProtein {
    pub cell: Entity,
    pub target: ProteinTarget,
}

/// Asks a cell to recycle one protein for amino acids.
#[derive(Message, Debug, Clone, Copy)]
pub struct RecycleProtein {
    pub cell: Entity,
    pub target: ProteinTarget,
}

/// Applies `QueueProtein` and `RecycleProtein` messages.
pub fn apply_protein_commands(
    mut queue_orders: MessageReader<QueueProtein>,
    mut recycle_orders: MessageReader<RecycleProtein>,
    mut cells: Query<
        (
            &mut SynthesisQueue,
            &mut Proteome,
            &CellSize,
            &mut CurrencyPools,
            &mut FlowLedger,
        ),
        With<Cell>,
    >,
) {
    for order in queue_orders.read() {
        let Ok((mut queue, proteome, size, ..)) = cells.get_mut(order.cell) else {
            continue;
        };
        if let Err(err) = queue_protein(&mut queue, &proteome, membrane_slots(size), order.target) {
            debug!("Order {:?} refused: {err:?}", order.target);
        }
    }
    for order in recycle_orders.read() {
        let Ok((_, mut proteome, _, mut pools, mut ledger)) = cells.get_mut(order.cell) else {
            continue;
        };
        if let Some(amino_acids) = recycle_protein(&mut proteome, order.target) {
            ledger.apply(&mut pools, FlowSource::Recycle, Currency::AminoAcids, amino_acids);
        }
    }
}

/// Adds the protein messages and applies them each frame.
pub struct ProteinPlugin;

impl Plugin for ProteinPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<QueueProtein>()
            .add_message::<RecycleProtein>()
            .add_systems(Update, apply_protein_commands);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAGELLUM: ProteinTarget = ProteinTarget::Membrane(MembraneProtein::Flagellum);
    const CHEMO: ProteinTarget = ProteinTarget::Enzyme(BlockKind::Chemosynthesis);

    #[test]
    fn queue_builds_proteins_in_order() {
        let mut queue = SynthesisQueue::default();
        let mut proteome = Proteome::default();
        queue_protein(&mut queue, &proteome, 6, CHEMO).unwrap();
        queue_protein(&mut queue, &proteome, 6, FLAGELLUM).unwrap();

        assert!(advance_synthesis(&mut queue, &mut proteome, 9.0).is_empty());
        assert_eq!(
            advance_synthesis(&mut queue, &mut proteome, 1.0),
            vec![CHEMO]
        );
        assert_eq!(
            advance_synthesis(&mut queue, &mut proteome, 15.0),
            vec![FLAGELLUM]
        );
        assert_eq!(proteome.enzymes(BlockKind::Chemosynthesis), 1);
        assert_eq!(proteome.count(FLAGELLUM), 1);
        assert!(queue.is_empty());
        assert_eq!(queue.progress, 5.0, "spare units stay for the next order");
    }

    #[test]
    fn membrane_orders_need_free_slots() {
        let mut queue = SynthesisQueue::default();
        let mut proteome = Proteome::default();
        proteome.add(FLAGELLUM);
        queue_protein(&mut queue, &proteome, 2, FLAGELLUM).unwrap();
        assert_eq!(
            queue_protein(&mut queue, &proteome, 2, FLAGELLUM),
            Err(QueueError::NoMembraneSlot)
        );
        // Enzymes do not use membrane slots.
        queue_protein(&mut queue, &proteome, 2, CHEMO).unwrap();
    }

    #[test]
    fn slot_usage_counts_queued_membrane_proteins_only() {
        let mut queue = SynthesisQueue::default();
        let mut proteome = Proteome::default();
        proteome.add(FLAGELLUM);
        queue_protein(&mut queue, &proteome, 6, FLAGELLUM).unwrap();
        queue_protein(&mut queue, &proteome, 6, CHEMO).unwrap();
        assert_eq!(membrane_slot_usage(&queue, &proteome, 6), (2, 6));
    }

    #[test]
    fn the_eta_follows_the_queue_and_the_rate() {
        let mut queue = SynthesisQueue::default();
        assert_eq!(synthesis_eta(&queue, 1.0), None, "empty queue");
        let proteome = Proteome::default();
        queue_protein(&mut queue, &proteome, 6, CHEMO).unwrap();
        queue_protein(&mut queue, &proteome, 6, CHEMO).unwrap();
        queue.progress = 4.0;
        assert_eq!(units_left(&queue), 16.0);
        assert_eq!(synthesis_eta(&queue, 2.0), Some(8.0));
        assert_eq!(first_order_eta(&queue, 2.0), Some(3.0));
        assert_eq!(synthesis_eta(&queue, 0.0), None, "no synthesis");
    }

    #[test]
    fn recycling_gives_back_half_the_amino_acids() {
        let mut proteome = Proteome::with_enzymes(&[(BlockKind::Chemosynthesis, 1)]);
        let back = recycle_protein(&mut proteome, CHEMO).unwrap();
        assert_eq!(back, 0.5 * UNITS_PER_PROTEIN * AMINO_ACIDS_PER_UNIT);
        assert_eq!(proteome.enzymes(BlockKind::Chemosynthesis), 0);
        assert_eq!(recycle_protein(&mut proteome, CHEMO), None);
    }
}
