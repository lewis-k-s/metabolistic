use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ecs::schedule::ScheduleLabel;

use crate::blocks::genome::{poll_genome_diff, BlockKind, Genome, MetabolicUpdateEvent, GeneState};
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
    // Track currency dependencies between blocks
    pub dependencies: HashMap<Entity, Vec<Entity>>, // entity -> list of entities it depends on
}

/// Central currency pools managed by the metabolic flow system.
/// This replaces individual currency resources for flow-based calculations.
#[derive(Resource, Default, Debug)]
pub struct CurrencyPools {
    pub pools: HashMap<Currency, f32>,
}

impl CurrencyPools {
    /// Get the amount of a specific currency
    pub fn get(&self, currency: Currency) -> f32 {
        self.pools.get(&currency).copied().unwrap_or(0.0)
    }
    
    /// Set the amount of a specific currency
    pub fn set(&mut self, currency: Currency, amount: f32) {
        self.pools.insert(currency, amount.max(0.0)); // Prevent negative currencies
    }
    
    /// Add to a currency (positive) or subtract (negative)
    pub fn modify(&mut self, currency: Currency, delta: f32) {
        let current = self.get(currency);
        self.set(currency, current + delta);
    }
    
    /// Check if there's enough of a currency available
    pub fn can_consume(&self, currency: Currency, amount: f32) -> bool {
        self.get(currency) >= amount
    }
    
    /// Initialize with default starting amounts
    pub fn with_defaults() -> Self {
        let mut pools = HashMap::new();
        pools.insert(Currency::ATP, 100.0);
        pools.insert(Currency::ReducingPower, 50.0);
        pools.insert(Currency::AcetylCoA, 20.0);
        pools.insert(Currency::CarbonSkeletons, 30.0);
        pools.insert(Currency::FreeFattyAcids, 10.0);
        pools.insert(Currency::StorageBeads, 0.0);
        pools.insert(Currency::Pyruvate, 25.0);
        pools.insert(Currency::OrganicWaste, 0.0);
        
        Self { pools }
    }
}

/// True when edits require a graph rebuild.
#[derive(Resource, Default)]
pub struct FlowDirty(pub bool);

/// Per-node flux results with currency-specific changes.
#[derive(Resource, Default)]
pub struct FluxResult {
    /// Total flux per entity (for backward compatibility)
    pub entity_flux: HashMap<Entity, f32>,
    /// Currency changes to be applied: Currency -> total delta
    pub currency_changes: HashMap<Currency, f32>,
}

// --- Components (for ECS representation, mostly for editor/debug) ---

/// Status of a metabolic block, derived from genome expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockStatus {
    Active,
    Mutated,
    Silent,
}

impl From<GeneState> for BlockStatus {
    fn from(gene_state: GeneState) -> Self {
        match gene_state {
            GeneState::Expressed => BlockStatus::Active,
            GeneState::Mutated   => BlockStatus::Mutated,
            GeneState::Silent    => BlockStatus::Silent,
        }
    }
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
    query_flux_profiles: Query<(Entity, &FluxProfile)>,
) {
    metabolic_graph.nodes = query_nodes.iter().collect();
    metabolic_graph.edges = query_edges.iter().collect();
    
    // Build dependency graph based on currency flows
    metabolic_graph.dependencies.clear();
    
    // For each node, find which other nodes produce currencies it consumes
    for (consumer_entity, consumer_flux) in query_flux_profiles.iter() {
        let mut dependencies = Vec::new();
        
        // Find currencies this block consumes (negative flux)
        let consumed_currencies: Vec<Currency> = consumer_flux.0.iter()
            .filter(|(_, &amount)| amount < 0.0)
            .map(|(&currency, _)| currency)
            .collect();
        
        // Find other blocks that produce these currencies
        for (producer_entity, producer_flux) in query_flux_profiles.iter() {
            if producer_entity == consumer_entity {
                continue; // Skip self
            }
            
            // Check if this producer produces any currency the consumer needs
            for &currency in &consumed_currencies {
                if let Some(&amount) = producer_flux.0.get(&currency) {
                    if amount > 0.0 { // Positive flux = production
                        dependencies.push(producer_entity);
                        break; // Only need to add dependency once per producer
                    }
                }
            }
        }
        
        metabolic_graph.dependencies.insert(consumer_entity, dependencies);
    }
    
    info!("Rebuilding metabolic graph: {} nodes, {} edges, {} dependencies", 
          metabolic_graph.nodes.len(), 
          metabolic_graph.edges.len(),
          metabolic_graph.dependencies.len());
}

pub fn solve_flux_system(
    metabolic_graph: Res<MetabolicGraph>,
    mut flux_result: ResMut<FluxResult>,
    currency_pools: Res<CurrencyPools>,
    query_blocks: Query<(&MetabolicNode, &FluxProfile)>,
) {
    info!("Solving metabolic flux for {} nodes and {} edges...", metabolic_graph.nodes.len(), metabolic_graph.edges.len());
    
    flux_result.entity_flux.clear();
    flux_result.currency_changes.clear();
    
    // Topologically sort nodes to respect dependencies
    let sorted_nodes = topological_sort(&metabolic_graph);
    
    for node_entity in sorted_nodes {
        if let Ok((node, flux_profile)) = query_blocks.get(node_entity) {
            let mut total_flux_for_node = 0.0;
            let mut can_execute = true;

            // Check if all required currencies are available
            for (currency, &amount) in flux_profile.0.iter() {
                if amount < 0.0 { // Consumption
                    let required = -amount;
                    // Apply BlockStatus modifiers to required amount
                    let modified_required = match node.status {
                        BlockStatus::Active => required,
                        BlockStatus::Mutated => required * 0.5,
                        BlockStatus::Silent => 0.0,
                    };
                    
                    if modified_required > 0.0 {
                        let available = currency_pools.get(*currency) + 
                                       flux_result.currency_changes.get(currency).unwrap_or(&0.0);
                        if available < modified_required {
                            can_execute = false;
                            break;
                        }
                    }
                }
            }

            if can_execute {
                // Apply flux changes
                for (currency, &amount) in flux_profile.0.iter() {
                    let modified_amount = match node.status {
                        BlockStatus::Active => amount,
                        BlockStatus::Mutated => amount * 0.5,
                        BlockStatus::Silent => 0.0,
                    };
                    
                    if modified_amount != 0.0 {
                        *flux_result.currency_changes.entry(*currency).or_insert(0.0) += modified_amount;
                        total_flux_for_node += modified_amount;
                    }
                }
            }
            
            flux_result.entity_flux.insert(node_entity, total_flux_for_node);
        }
    }
}

/// Topological sort of metabolic nodes respecting dependencies
fn topological_sort(graph: &MetabolicGraph) -> Vec<Entity> {
    let mut sorted = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut visiting = std::collections::HashSet::new();
    
    fn visit(
        node: Entity,
        graph: &MetabolicGraph,
        visited: &mut std::collections::HashSet<Entity>,
        visiting: &mut std::collections::HashSet<Entity>,
        sorted: &mut Vec<Entity>,
    ) {
        if visited.contains(&node) {
            return;
        }
        if visiting.contains(&node) {
            // Cycle detected, just skip for now
            return;
        }
        
        visiting.insert(node);
        
        // Visit dependencies first
        if let Some(deps) = graph.dependencies.get(&node) {
            for &dep in deps {
                visit(dep, graph, visited, visiting, sorted);
            }
        }
        
        visiting.remove(&node);
        visited.insert(node);
        sorted.push(node);
    }
    
    // Visit all nodes
    for &node in &graph.nodes {
        visit(node, graph, &mut visited, &mut visiting, &mut sorted);
    }
    
    sorted
}

/// Apply calculated currency changes to the central currency pools
pub fn apply_currency_changes_system(
    flux_result: Res<FluxResult>,
    mut currency_pools: ResMut<CurrencyPools>,
) {
    for (&currency, &delta) in flux_result.currency_changes.iter() {
        if delta != 0.0 {
            currency_pools.modify(currency, delta);
            info!("Applied currency change: {:?} delta: {:.2} (new total: {:.2})", 
                  currency, delta, currency_pools.get(currency));
        }
    }
}

pub fn apply_flux_results_system(
    flux_result: Res<FluxResult>,
    query_blocks: Query<(&MetabolicNode, &FluxProfile)>,
) {
    // Logging and debugging information about flux results
    for (entity, &flux) in flux_result.entity_flux.iter() {
        if let Ok((node, flux_profile)) = query_blocks.get(*entity) {
            info!("Node {:?} (Kind: {:?}, Status: {:?}) has total flux: {:.2} with profile: {:?}", 
                  entity, node.kind, node.status, flux, flux_profile.0);
        } else {
            warn!("Flux result for unknown node entity: {:?}", entity);
        }
    }
    
    // Log currency changes summary
    if !flux_result.currency_changes.is_empty() {
        info!("Currency changes this cycle: {:?}", flux_result.currency_changes);
    }
}

pub fn on_genome_diff(
    mut diff_reader: EventReader<MetabolicUpdateEvent>,
    genome: Res<Genome>,
    mut nodes: Query<&mut MetabolicNode>,
    mut dirty: ResMut<FlowDirty>,
) {
    if diff_reader.read().next().is_some() {
        for mut node in &mut nodes {
            node.status = genome.get_gene_state(&node.kind)
                .cloned()
                .unwrap_or(GeneState::Silent)
                .into();
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
            .insert_resource(CurrencyPools::with_defaults())
            .add_schedule(Schedule::new(MetabolicSchedule))
            .add_systems(PreUpdate, poll_genome_diff)
            .add_systems(MetabolicSchedule, (
                on_genome_diff,
                apply_deferred,
                rebuild_graph.run_if(resource_changed::<FlowDirty>),
                solve_flux_system,
                apply_currency_changes_system,
                apply_flux_results_system,
            ).chain()) // Chain ensures proper ordering
            .add_systems(Update, run_metabolic_schedule)
            .insert_resource(Time::<Fixed>::from_seconds(0.25));
    }
}