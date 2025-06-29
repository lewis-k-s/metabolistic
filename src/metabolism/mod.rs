use std::collections::HashMap;

use bevy::prelude::*;
use bevy::app::FixedUpdate;
use bevy::ecs::schedule::ScheduleLabel;

use crate::blocks::genome::{poll_genome_diff, BlockKind, Genome, GenomeDiffEvent, GeneState};
use crate::molecules::Currency;

// --- Components ---

/// Marker component for entities that are part of the metabolic system.
#[derive(Component, Default)]
pub struct MetabolicBlock;

/// Defines the flux profile (production/consumption) of a metabolic block.
/// Keys are Currency types, values are flux amounts (positive for production, negative for consumption).
#[derive(Component, Default, Debug, Clone)]
pub struct FluxProfile(pub HashMap<Currency, f32>);

// --- Resources ---

/// Dense vectors of nodes & edges used by solver.
#[derive(Resource, Default)]
pub struct MetabolicGraph {
    // Placeholder for actual graph data structures (e.g., adjacency list, matrix)
    pub nodes: Vec<Entity>,
    pub edges: Vec<Entity>,
}

/// True when edits require a graph rebuild.
#[derive(Resource, Default)]
pub struct FlowDirty(pub bool);

/// Per-node rates + flags picked up by gameplay & UI.
#[derive(Resource, Default)]
pub struct FluxResult(pub HashMap<Entity, f32>);

// --- Components (for ECS representation, mostly for editor/debug) ---

/// Status of a metabolic block, derived from genome expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockStatus {
    Active,
    Mutated,
    Silent,
}

/// Component for a node in the metabolic graph.
#[derive(Component)]
pub struct MetabolicNode {
    pub kind: BlockKind,
    pub status: BlockStatus,
}

/// Component for an edge in the metabolic graph.
#[derive(Component)]
pub struct MetabolicEdge;

// --- Schedules ---

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetabolicSchedule;

fn run_metabolic_schedule(world: &mut World) {
    world.run_schedule(MetabolicSchedule);
}

// --- Systems ---

pub fn rebuild_graph(
    mut metabolic_graph: ResMut<MetabolicGraph>,
    query_nodes: Query<Entity, With<MetabolicNode>>,
    query_edges: Query<Entity, With<MetabolicEdge>>,
) {
    metabolic_graph.nodes = query_nodes.iter().collect();
    metabolic_graph.edges = query_edges.iter().collect();
    info!("Rebuilding metabolic graph: {} nodes, {} edges", metabolic_graph.nodes.len(), metabolic_graph.edges.len());
}

pub fn solve_flux_system(
    metabolic_graph: Res<MetabolicGraph>,
    mut flux_result: ResMut<FluxResult>,
    query_blocks: Query<(&MetabolicNode, &FluxProfile)>,
) {
    info!("Solving metabolic flux for {} nodes and {} edges...", metabolic_graph.nodes.len(), metabolic_graph.edges.len());
    flux_result.0.clear();
    for node_entity in &metabolic_graph.nodes {
        if let Ok((node, flux_profile)) = query_blocks.get(*node_entity) {
            let mut total_flux_for_node = HashMap::new();

            for (currency, &amount) in flux_profile.0.iter() {
                // Apply BlockStatus modifiers
                let modified_amount = match node.status {
                    BlockStatus::Active => amount,
                    BlockStatus::Mutated => amount * 0.5, // Example: Mutated blocks have 50% flux
                    BlockStatus::Silent => 0.0,
                };
                *total_flux_for_node.entry(*currency).or_insert(0.0) += modified_amount;
            }
            // For now, we'll just store a single f32 value in FluxResult,
            // perhaps representing a sum or a specific currency's flux.
            // In a real scenario, FluxResult might need to be a HashMap<Entity, HashMap<Currency, f32>>
            // or a more complex structure. For simplicity, let's sum up all fluxes for now.
            let sum_flux: f32 = total_flux_for_node.values().sum();
            flux_result.0.insert(*node_entity, sum_flux);
        }
    }
}

pub fn apply_flux_results_system(
    flux_result: Res<FluxResult>,
    query_blocks: Query<(&MetabolicNode, &FluxProfile)>,
) {
    // Placeholder for applying flux results to gameplay/UI
    for (entity, &flux) in flux_result.0.iter() {
        if let Ok((node, flux_profile)) = query_blocks.get(*entity) {
            info!("Node {:?} (Kind: {:?}, Status: {:?}) has total flux: {} with profile: {:?}", entity, node.kind, node.status, flux, flux_profile.0);
        } else {
            warn!("Flux result for unknown node entity: {:?}", entity);
        }
    }
}

pub fn on_genome_diff(
    mut diff_reader: EventReader<GenomeDiffEvent>,
    genome: Res<Genome>,
    mut nodes: Query<&mut MetabolicNode>,
    mut dirty: ResMut<FlowDirty>,
) {
    if diff_reader.read().next().is_some() { // Corrected: use .read() instead of .iter()
        for mut node in &mut nodes {
            node.status = match genome.get_gene_state(&node.kind) {
                Some(GeneState::Expressed) => BlockStatus::Active,
                Some(GeneState::Mutated)   => BlockStatus::Mutated,
                _                          => BlockStatus::Silent,
            };
        }
        dirty.0 = true;
    }
}

// --- Plugin ---

pub struct MetabolicFlowPlugin;

impl Plugin for MetabolicFlowPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<MetabolicGraph>()
            .init_resource::<FlowDirty>()
            .init_resource::<FluxResult>()
            .add_schedule(Schedule::new(MetabolicSchedule))
            .add_systems(PreUpdate, poll_genome_diff)
            .add_systems(MetabolicSchedule, (
                on_genome_diff,
                apply_deferred,
                rebuild_graph.run_if(resource_changed::<FlowDirty>),
                solve_flux_system,
                apply_flux_results_system,
            ))
            .add_systems(Update, run_metabolic_schedule)
            .insert_resource(Time::<Fixed>::from_seconds(0.25));
    }
}