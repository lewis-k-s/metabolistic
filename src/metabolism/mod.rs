use std::collections::HashMap;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use crate::blocks::genome::{poll_genome_diff, BlockKind, GeneState, Genome, MetabolicUpdateEvent};
use crate::cell::Cell;
pub use crate::molecules::CurrencyPools; // Re-export for backward compatibility
use crate::molecules::{Currency, EnvironmentalCurrencyPools};

// --- Components ---

/// Marker component for entities that are part of the metabolic system.
#[derive(Component, Default)]
pub struct MetabolicBlock;

/// Defines the flux profile (production/consumption) of a metabolic block.
/// Keys are Currency types, values are flux amounts (positive for production, negative for consumption).
#[derive(Component, Default, Debug, Clone)]
pub struct FluxProfile(pub HashMap<Currency, f32>);

impl FluxProfile {
    /// Set demand (consumption) for a specific currency
    /// Demand is represented as a negative value in the flux profile
    pub fn set_demand(&mut self, currency: Currency, amount: f32) {
        self.0.insert(currency, -amount.abs());
    }

    /// Set production for a specific currency
    /// Production is represented as a positive value in the flux profile
    pub fn set_production(&mut self, currency: Currency, amount: f32) {
        self.0.insert(currency, amount.abs());
    }

    /// Check if this profile has demand (consumption) for a given currency
    /// Returns true if the currency exists and has a negative value
    pub fn has_demand_for(&self, currency: Currency) -> bool {
        self.0.get(&currency).is_some_and(|&amount| amount < 0.0)
    }

    /// Get the demand amount for a specific currency
    /// Returns the absolute value of consumption, or 0.0 if no demand
    pub fn get_demand(&self, currency: Currency) -> f32 {
        self.0
            .get(&currency)
            .map(|&amount| if amount < 0.0 { -amount } else { 0.0 })
            .unwrap_or(0.0)
    }

    /// Check if this profile produces a given currency
    /// Returns true if the currency exists and has a positive value
    pub fn produces(&self, currency: Currency) -> bool {
        self.0.get(&currency).is_some_and(|&amount| amount > 0.0)
    }

    /// Get the production amount for a specific currency
    /// Returns the production amount, or 0.0 if no production
    pub fn get_production(&self, currency: Currency) -> f32 {
        self.0
            .get(&currency)
            .map(|&amount| if amount > 0.0 { amount } else { 0.0 })
            .unwrap_or(0.0)
    }

    /// Get all currencies this profile has demand for
    pub fn get_demanded_currencies(&self) -> Vec<Currency> {
        self.0
            .iter()
            .filter_map(
                |(&currency, &amount)| {
                    if amount < 0.0 {
                        Some(currency)
                    } else {
                        None
                    }
                },
            )
            .collect()
    }

    /// Get all currencies this profile produces
    pub fn get_produced_currencies(&self) -> Vec<Currency> {
        self.0
            .iter()
            .filter_map(
                |(&currency, &amount)| {
                    if amount > 0.0 {
                        Some(currency)
                    } else {
                        None
                    }
                },
            )
            .collect()
    }

    /// Clear all flux values
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Scale all flux values by a factor
    pub fn scale(&mut self, factor: f32) {
        for amount in self.0.values_mut() {
            *amount *= factor;
        }
    }
}

// --- Resources ---

/// Configuration for flowmap and metabolic block update timing
#[derive(Resource, Clone)]
pub struct FlowmapTimingConfig {
    /// Target updates per second for metabolic systems (default: 5.0)
    pub updates_per_second: f32,
    /// Accumulated time since last metabolic update
    pub accumulator: f32,
}

impl Default for FlowmapTimingConfig {
    fn default() -> Self {
        Self {
            updates_per_second: 5.0, // 5 updates per second
            accumulator: 0.0,
        }
    }
}

impl FlowmapTimingConfig {
    /// Create config with custom update rate
    pub fn new(updates_per_second: f32) -> Self {
        Self {
            updates_per_second: updates_per_second.max(0.1), // Minimum 0.1 Hz
            accumulator: 0.0,
        }
    }

    /// Get the target delta time between updates
    pub fn target_delta(&self) -> f32 {
        1.0 / self.updates_per_second
    }

    /// Check if enough time has accumulated for an update
    pub fn should_update(&self) -> bool {
        self.accumulator >= self.target_delta()
    }

    /// Consume accumulated time for one update cycle
    pub fn consume_update(&mut self) {
        self.accumulator -= self.target_delta();
        // Keep any remaining time for next cycle
        self.accumulator = self.accumulator.max(0.0);
    }

    /// Add time to the accumulator
    pub fn add_time(&mut self, delta: f32) {
        self.accumulator += delta;
    }

    /// Set new update rate
    pub fn set_rate(&mut self, updates_per_second: f32) {
        self.updates_per_second = updates_per_second.max(0.1);
    }
}

/// Dense vectors of nodes & edges used by solver.
/// Used as Component (per-cell) for individual cell metabolic graphs
#[derive(Component, Default, Clone)]
pub struct MetabolicGraph {
    // Placeholder for actual graph data structures (e.g., adjacency list, matrix)
    pub nodes: Vec<Entity>,
    pub edges: Vec<Entity>,
    // Track currency dependencies between blocks
    pub dependencies: HashMap<Entity, Vec<Entity>>, // entity -> list of entities it depends on
}

/// True when edits require a graph rebuild.
#[derive(Resource, Default)]
pub struct FlowDirty(pub bool);

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
            GeneState::Mutated => BlockStatus::Mutated,
            GeneState::Silent => BlockStatus::Silent,
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

/// System to run the metabolic schedule at the configured timing rate
fn run_metabolic_schedule_timed(world: &mut World) {
    // Get timing config and time resources
    let delta_time = world.resource::<Time>().delta_secs();

    // Add time to accumulator
    {
        let mut timing_config = world.resource_mut::<FlowmapTimingConfig>();
        timing_config.add_time(delta_time);

        // Only run schedule if enough time has accumulated
        if !timing_config.should_update() {
            return;
        }

        timing_config.consume_update();
        info!(
            "Running MetabolicSchedule at {:.2} Hz (target), actual interval: {:.3}s",
            timing_config.updates_per_second,
            timing_config.target_delta()
        );
    }

    // Run the metabolic schedule
    world.run_schedule(MetabolicSchedule);
}

// --- Systems ---

pub fn on_genome_diff(
    mut diff_reader: EventReader<MetabolicUpdateEvent>,
    genome: Res<Genome>,
    mut nodes: Query<&mut MetabolicNode>,
    mut dirty: ResMut<FlowDirty>,
) {
    if diff_reader.read().next().is_some() {
        for mut node in &mut nodes {
            node.status = genome
                .get_gene_state(&node.kind)
                .cloned()
                .unwrap_or(GeneState::Silent)
                .into();
        }
        dirty.0 = true;
    }
}

// --- Metabolic Graph Management Systems ---

/// System to rebuild metabolic graphs for cells that have changed
pub fn rebuild_cell_metabolic_graphs_system(mut cell_query: Query<&mut Cell>) {
    for mut cell in cell_query.iter_mut() {
        // Clear existing graph
        cell.metabolic_graph.nodes.clear();
        cell.metabolic_graph.edges.clear();
        cell.metabolic_graph.dependencies.clear();

        // For now, we'll skip the complex block lookup since we need to redesign this approach
        // In the future, we'd query the block entities directly
        let owned_blocks: Vec<(Entity, &MetabolicNode, &FluxProfile)> = Vec::new();

        // Add nodes to the graph
        for (entity, _, _) in &owned_blocks {
            cell.metabolic_graph.nodes.push(*entity);
        }

        // Build dependency graph based on currency flows
        for (consumer_entity, _, consumer_flux) in &owned_blocks {
            let mut dependencies = Vec::new();

            // Find currencies this block consumes (negative flux)
            let consumed_currencies: Vec<Currency> = consumer_flux
                .0
                .iter()
                .filter(|(_, &amount)| amount < 0.0)
                .map(|(&currency, _)| currency)
                .collect();

            // Find other blocks that produce these currencies
            for (producer_entity, _, producer_flux) in &owned_blocks {
                if producer_entity == consumer_entity {
                    continue; // Skip self
                }

                // Check if this producer produces any currency the consumer needs
                for &currency in &consumed_currencies {
                    if let Some(&amount) = producer_flux.0.get(&currency) {
                        if amount > 0.0 {
                            // Positive flux = production
                            dependencies.push(*producer_entity);
                            break; // Only need to add dependency once per producer
                        }
                    }
                }
            }

            cell.metabolic_graph
                .dependencies
                .insert(*consumer_entity, dependencies);
        }

        info!(
            "Rebuilt metabolic graph for cell: {} nodes, {} dependencies",
            cell.metabolic_graph.nodes.len(),
            cell.metabolic_graph.dependencies.len()
        );
    }
}

/// System to solve flux for cells with updated metabolic graphs
pub fn solve_cell_flux_system(
    mut cell_query: Query<
        (Entity, &MetabolicGraph, &mut CurrencyPools),
        (With<Cell>, Changed<MetabolicGraph>),
    >,
    blocks_query: Query<(&MetabolicNode, &FluxProfile)>,
) {
    for (cell_entity, metabolic_graph, mut currency_pools) in cell_query.iter_mut() {
        let mut currency_changes = HashMap::new();

        // Topologically sort nodes to respect dependencies
        let sorted_nodes = topological_sort_cell_graph(metabolic_graph);

        for node_entity in sorted_nodes {
            if let Ok((node, flux_profile)) = blocks_query.get(node_entity) {
                let mut can_execute = true;

                // Check if all required currencies are available
                for (currency, &amount) in flux_profile.0.iter() {
                    if amount < 0.0 {
                        // Consumption
                        let required = -amount;
                        // Apply BlockStatus modifiers to required amount
                        let modified_required = match node.status {
                            BlockStatus::Active => required,
                            BlockStatus::Mutated => required * 0.5,
                            BlockStatus::Silent => 0.0,
                        };

                        if modified_required > 0.0 {
                            let available = currency_pools.get(*currency)
                                + currency_changes.get(currency).unwrap_or(&0.0);
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
                            *currency_changes.entry(*currency).or_insert(0.0) += modified_amount;
                        }
                    }
                }
            }
        }

        // Apply currency changes to this cell's currency pools
        for (&currency, &delta) in currency_changes.iter() {
            if delta != 0.0 {
                currency_pools.modify(currency, delta);
                info!(
                    "Applied currency change for cell {:?}: {:?} delta: {:.2} (new total: {:.2})",
                    cell_entity,
                    currency,
                    delta,
                    currency_pools.get(currency)
                );
            }
        }
    }
}

/// Topological sort helper for cell metabolic graphs
fn topological_sort_cell_graph(graph: &MetabolicGraph) -> Vec<Entity> {
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

// --- Cell-Based ECS Systems ---

/// System to handle gene expression requests for cells using change detection
/// This replaces individual Cell.express_gene() calls with batch processing
pub fn cell_gene_expression_system(
    cell_query: Query<Entity, (With<Cell>, Changed<Genome>)>,
    mut commands: Commands,
) {
    for cell_entity in cell_query.iter() {
        // This system runs only when genomes have changed
        // Individual gene expression is handled through events or commands
        // For now, we'll add a system that detects genome changes and logs them
        info!(
            "Cell {:?} genome changed, blocks may need to be respawned",
            cell_entity
        );

        // Trigger metabolic block spawning/despawning by sending a command
        commands.trigger_targets(MetabolicUpdateEvent, cell_entity);
    }
}

/// System to process currency operations for cells using queries
/// This replaces individual Cell.set_currency(), Cell.modify_currency() calls
pub fn cell_currency_management_system(
    mut cell_query: Query<(Entity, &mut CurrencyPools), With<Cell>>,
    // Add event listeners here for currency modification requests if needed
) {
    // This system will be expanded to handle currency modification events
    // For now, it serves as the foundation for batch currency processing
    for (_entity, _currency_pools) in cell_query.iter_mut() {
        // Batch currency operations will be processed here
        // This enables parallel processing of independent cells
    }
}

/// System to synchronize metabolic blocks with genome state for all cells
/// This replaces Cell.sync_blocks_with_genome() method
pub fn sync_cell_blocks_with_genome_system(
    mut cell_query: Query<(Entity, &mut Cell), Changed<Genome>>,
    mut commands: Commands,
) {
    for (cell_entity, mut cell) in cell_query.iter_mut() {
        // Check each implemented block kind
        for block_kind in [
            BlockKind::Fermentation,
            BlockKind::LipidMetabolism,
            // Add other block kinds as they're implemented
        ] {
            let gene_state = cell.genome.get_gene_state(&block_kind);
            let should_exist =
                matches!(gene_state, Some(GeneState::Expressed | GeneState::Mutated));

            // Check if block already exists for this cell
            let block_exists = cell.metabolic_blocks.contains_key(&block_kind);

            if should_exist && !block_exists {
                let block_entity =
                    spawn_metabolic_block_for_cell(&mut commands, cell_entity, block_kind);
                if block_entity != Entity::PLACEHOLDER {
                    cell.metabolic_blocks.insert(block_kind, block_entity);
                }
            } else if !should_exist && block_exists {
                // Despawn the block
                if let Some(block_entity) = cell.metabolic_blocks.remove(&block_kind) {
                    commands.entity(block_entity).despawn();
                    info!("Despawned metabolic block {:?}", block_kind);
                }
            }
        }
    }
}

/// System to run metabolic cycles for all cells with change detection
/// This replaces Cell.run_metabolic_cycle() method with batch processing
pub fn cell_metabolic_cycle_system(mut cell_query: Query<&mut Cell, Changed<CurrencyPools>>) {
    for cell in cell_query.iter_mut() {
        // Get all blocks from this cell
        let owned_blocks: Vec<_> = cell
            .metabolic_blocks
            .iter()
            .map(|(&_, &entity)| entity)
            .collect();

        if owned_blocks.is_empty() {
            continue;
        }

        // Simple flux calculation for this cell's blocks
        let _currency_changes: HashMap<Currency, f32> = HashMap::new();

        // For now, we'll skip the complex block processing since we need to redesign this approach
        // In the future, we'd query the block entities directly
        continue;
    }
}

/// Helper function to spawn a metabolic block for a specific cell
fn spawn_metabolic_block_for_cell(
    commands: &mut Commands,
    cell_entity: Entity,
    block_kind: BlockKind,
) -> Entity {
    let mut flux_profile = FluxProfile::default();

    // Configure flux profile based on block type
    let block_entity = match block_kind {
        BlockKind::Fermentation => {
            flux_profile.set_demand(Currency::Pyruvate, 1.0);
            flux_profile.set_demand(Currency::ReducingPower, 1.0);
            flux_profile.set_production(Currency::ATP, 1.0);
            flux_profile.set_production(Currency::OrganicWaste, 1.0);

            commands
                .spawn((
                    crate::blocks::fermentation::FermentationBlock,
                    MetabolicBlock,
                    MetabolicNode {
                        kind: block_kind,
                        status: BlockStatus::Active,
                    },
                    flux_profile,
                    Name::new("Fermentation Block"),
                ))
                .id()
        }
        BlockKind::LipidMetabolism => commands
            .spawn((
                crate::blocks::vesicle_export::VesicleExportBlock,
                Name::new("Vesicle Export Block"),
            ))
            .id(),
        _ => {
            warn!("Block kind {:?} not implemented in cell spawn", block_kind);
            return Entity::PLACEHOLDER;
        }
    };

    // Store the block in the cell's metabolic_blocks
    // Note: This would ideally be done in the caller, but for now we'll do it here
    info!(
        "Spawned metabolic block {:?} for cell {:?}",
        block_kind, cell_entity
    );

    block_entity
}

// --- Environmental Currency Exchange Systems ---

/// System to handle waste export from cells to the environment
/// Cells can dump organic waste into the environmental pools
pub fn cell_waste_export_system(
    mut cell_query: Query<&mut CurrencyPools, With<Cell>>,
    mut env_pools: ResMut<EnvironmentalCurrencyPools>,
) {
    for mut currency_pools in cell_query.iter_mut() {
        let waste_amount = currency_pools.get(Currency::OrganicWaste);

        // If cell has significant waste, export some to environment
        if waste_amount > 20.0 {
            let export_amount = waste_amount * 0.1; // Export 10% of excess waste
            currency_pools.modify(Currency::OrganicWaste, -export_amount);
            env_pools.modify(Currency::OrganicWaste, export_amount);

            info!(
                "Cell exported {:.2} organic waste to environment",
                export_amount
            );
        }
    }
}

/// System to handle nutrient absorption from environment to cells
/// Cells can absorb external carbon sources and other nutrients
pub fn cell_nutrient_absorption_system(
    mut cell_query: Query<&mut CurrencyPools, With<Cell>>,
    mut env_pools: ResMut<EnvironmentalCurrencyPools>,
) {
    for mut currency_pools in cell_query.iter_mut() {
        // Absorb carbon skeletons from environment if cell is low
        let cell_carbon = currency_pools.get(Currency::CarbonSkeletons);
        let env_carbon = env_pools.get(Currency::CarbonSkeletons);

        if cell_carbon < 10.0 && env_carbon > 5.0 {
            let absorption_amount = 2.0_f32.min(env_carbon); // Absorb up to 2.0 units
            currency_pools.modify(Currency::CarbonSkeletons, absorption_amount);
            env_pools.modify(Currency::CarbonSkeletons, -absorption_amount);

            info!(
                "Cell absorbed {:.2} carbon skeletons from environment",
                absorption_amount
            );
        }

        // Absorb pyruvate from environment if available
        let cell_pyruvate = currency_pools.get(Currency::Pyruvate);
        let env_pyruvate = env_pools.get(Currency::Pyruvate);

        if cell_pyruvate < 5.0 && env_pyruvate > 1.0 {
            let absorption_amount = 1.0_f32.min(env_pyruvate); // Absorb up to 1.0 unit
            currency_pools.modify(Currency::Pyruvate, absorption_amount);
            env_pools.modify(Currency::Pyruvate, -absorption_amount);

            info!(
                "Cell absorbed {:.2} pyruvate from environment",
                absorption_amount
            );
        }
    }
}

/// System to handle environmental currency decay and processing
/// Environmental waste can break down over time, external nutrients can be replenished
pub fn environmental_currency_processing_system(mut env_pools: ResMut<EnvironmentalCurrencyPools>) {
    // Organic waste naturally degrades over time (simulating bacterial breakdown)
    let current_waste = env_pools.get(Currency::OrganicWaste);
    if current_waste > 0.0 {
        let decay_amount = current_waste * 0.02; // 2% decay per cycle
        env_pools.modify(Currency::OrganicWaste, -decay_amount);

        // Some waste conversion to useful nutrients (partial recycling)
        let recycled_carbon = decay_amount * 0.3; // 30% becomes carbon skeletons
        env_pools.modify(Currency::CarbonSkeletons, recycled_carbon);

        info!(
            "Environmental waste decay: -{:.2} waste, +{:.2} carbon",
            decay_amount, recycled_carbon
        );
    }

    // Environmental nutrient replenishment (simulating external nutrient sources)
    let current_carbon = env_pools.get(Currency::CarbonSkeletons);
    if current_carbon < 100.0 {
        let replenishment = 1.0; // Slow steady replenishment
        env_pools.modify(Currency::CarbonSkeletons, replenishment);

        info!("Environmental carbon replenishment: +{:.2}", replenishment);
    }

    let current_pyruvate = env_pools.get(Currency::Pyruvate);
    if current_pyruvate < 20.0 {
        let replenishment = 0.5; // Slow steady replenishment
        env_pools.modify(Currency::Pyruvate, replenishment);

        info!(
            "Environmental pyruvate replenishment: +{:.2}",
            replenishment
        );
    }
}

// --- Plugin ---

// DefaultCell setup removed - tests now use player-cell spawn utilities

pub struct MetabolicFlowPlugin;

impl Plugin for MetabolicFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlowDirty>()
            .init_resource::<FlowmapTimingConfig>()
            .add_schedule(Schedule::new(MetabolicSchedule))
            // No default cell setup - tests now use player-cell spawn utilities
            .add_systems(PreUpdate, poll_genome_diff)
            .add_systems(
                MetabolicSchedule,
                (
                    on_genome_diff,
                    apply_deferred,
                    sync_cell_blocks_with_genome_system, // Cell-based block synchronization
                    rebuild_cell_metabolic_graphs_system, // New: ECS-based metabolic graph rebuilding
                    solve_cell_flux_system,               // New: ECS-based flux solving
                    cell_metabolic_cycle_system,          // Legacy: Cell-based metabolic processing
                )
                    .chain(),
            ) // Chain ensures proper ordering
            .add_systems(
                Update,
                (
                    cell_gene_expression_system,     // New: Handle gene expression changes
                    cell_currency_management_system, // New: Handle currency operations
                    cell_waste_export_system.run_if(resource_exists::<EnvironmentalCurrencyPools>), // New: Export waste to environment
                    cell_nutrient_absorption_system
                        .run_if(resource_exists::<EnvironmentalCurrencyPools>), // New: Absorb nutrients from environment
                    environmental_currency_processing_system
                        .run_if(resource_exists::<EnvironmentalCurrencyPools>), // New: Environmental processing
                    run_metabolic_schedule_timed, // New: Timing-controlled metabolic schedule execution
                ),
            );
    }
}
