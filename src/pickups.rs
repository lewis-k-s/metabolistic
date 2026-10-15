//! # Pickups
//!
//! A `GeneOrb` gives the player's cell a gene when the cell touches it.
//!
//! **Stand-in:** real gene transfer is M5 (engulf a cell and take one of its
//! genes). Until then, the orb gives the gene directly, with one enzyme, so that
//! the block can run at once. A gene that the cell has but that is silent or
//! mutated is switched on or restored instead.

use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, CellSize};
use crate::movement::world_radius;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::{ProteinTarget, Proteome};

/// Radius of an orb in world units (its sprite frame is 24 px wide).
pub const ORB_RADIUS: f32 = 12.0;

/// A pickup that gives one gene. Stand-in for horizontal gene transfer (M5).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneOrb {
    pub gene: BlockKind,
}

/// True if the orb has something to give: the gene is absent, silent or
/// mutated. An expressed gene needs nothing.
pub fn can_take_gene(genome: &Genome, kind: BlockKind) -> bool {
    genome.get_gene_state(&kind) != Some(&GeneState::Expressed)
}

/// Gives a gene. An absent gene is added and expressed. A silent gene is
/// expressed. A mutated gene is restored. The cell gets one enzyme for the
/// block if it has none. Returns false (and changes nothing) if the gene is
/// already expressed.
pub fn grant_gene(genome: &mut Genome, proteome: &mut Proteome, kind: BlockKind) -> bool {
    match genome.get_gene_state(&kind) {
        Some(GeneState::Expressed) => return false,
        Some(GeneState::Mutated) => {
            genome.restore_gene(kind);
        }
        Some(GeneState::Silent) => {
            genome.express_gene(kind);
        }
        None => {
            genome.add_gene(kind);
            genome.express_gene(kind);
        }
    }
    if proteome.enzymes(kind) == 0 {
        proteome.add(ProteinTarget::Enzyme(kind));
    }
    true
}

/// A cell takes the gene of each orb that it touches. The orb is used up.
pub fn collect_gene_orbs_system(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    mut cells: Query<(&Transform, &CellSize, &mut Genome, &mut Proteome), With<Cell>>,
    orbs: Query<(Entity, &GeneOrb, &Transform)>,
    mut commands: Commands,
) {
    for controlled in &players {
        let Ok((cell_transform, size, mut genome, mut proteome)) = cells.get_mut(controlled.0) else {
            continue;
        };
        for (entity, orb, orb_transform) in &orbs {
            let near = cell_transform
                .translation
                .truncate()
                .distance(orb_transform.translation.truncate())
                <= world_radius(size) + ORB_RADIUS;
            if near && can_take_gene(&genome, orb.gene) {
                grant_gene(&mut genome, &mut proteome, orb.gene);
                commands.entity(entity).despawn();
            }
        }
    }
}

pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, collect_gene_orbs_system);
    }
}
