//! # Metabolism view
//!
//! A snapshot of a cell's metabolism for the Manage screen: pools with their
//! levels, pathways with their state and limit, and the edges (pipes) between
//! them with the flux per second. `build_view` is a pure function of plain
//! data, so it runs in headless tests. `update_local_view_system` keeps the
//! local player's view in the `LocalMetabolismView` resource, and the screen
//! only reads that resource.
//!
//! ## Flux
//!
//! The flux of a block edge is `turnover x profile[pool] / dt` per second:
//! the turnovers of the last metabolic tick, for a tick of `dt` seconds. It
//! agrees with the pool change of that tick (see `solve_tick`). A new view
//! smooths each flux against the previous view (`FLUX_SMOOTH_SECONDS`), so the
//! drawing does not flicker. Edges of kind `Other` come from the `FlowLedger`,
//! which smooths its own rates.
//!
//! ## Ghosts
//!
//! The map is fixed. A kind with a `block_spec` that the cell does not have is
//! a ghost pathway: `owned` is false, and it has edges with flux 0. Its name is
//! known only if the codex holds the gene.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use super::ledger::{FlowLedger, FlowSource};
use super::solver::{BlockInput, Limit, block_limits};
use super::{
    BlockStatus, BlockTurnover, CellTick, CurrencyPools, FlowmapTimingConfig, FluxProfile,
    Kinetics, MAX_SUBSTEP, MetabolicNode,
};
use crate::actions::Throttle;
use crate::blocks::block_spec;
use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, CellBlocks};
use crate::codex::Codex;
use crate::environment::{Channel, LocalEnvironment, external_saturation, limiting_channel};
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::Proteome;

/// Time constant of the flux smoothing, in seconds.
pub const FLUX_SMOOTH_SECONDS: f32 = 0.5;

/// A block of the cell, as the view needs it.
#[derive(Debug, Clone, Copy)]
pub struct BlockData<'a> {
    pub kind: BlockKind,
    pub profile: &'a FluxProfile,
    /// Turnovers in the last metabolic tick.
    pub turnover: f32,
    pub status: BlockStatus,
    /// `Drive x Throttle`, as the solver uses it.
    pub throttle: f32,
    pub kinetics: Kinetics,
}

/// The inputs of `build_view`.
pub struct ViewInput<'a> {
    pub genome: &'a Genome,
    pub codex: &'a HashSet<BlockKind>,
    pub proteome: &'a Proteome,
    pub blocks: &'a [BlockData<'a>],
    pub pools: &'a CurrencyPools,
    pub ledger: &'a FlowLedger,
    pub env: &'a LocalEnvironment,
    /// Length in seconds of the metabolic tick that gave the turnovers.
    pub dt: f32,
    /// The view of the last frame, for smoothing. `None`: no smoothing.
    pub previous: Option<&'a MetabolismView>,
    /// Seconds since the previous view.
    pub frame_dt: f32,
}

/// One pool node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoolView {
    pub currency: Currency,
    pub level: f32,
}

/// One pathway plate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathwayView {
    pub kind: BlockKind,
    /// False for a ghost: the cell does not have the gene.
    pub owned: bool,
    /// The player may see the name: the cell has the gene, or the codex does.
    pub known: bool,
    /// Gene state. `None` for a ghost.
    pub state: Option<BlockStatus>,
    pub enzymes: u32,
    /// Fraction of the best possible rate, in [0, 1] (smoothed).
    pub activity: f32,
    /// Drive x throttle of the block.
    pub throttle: f32,
    pub limit: Limit,
}

/// One end of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Node {
    Pool(Currency),
    Pathway(BlockKind),
    Outside(Channel),
    /// A flow that is not a pathway: upkeep, movement, actions, ...
    Source(FlowSource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// A pool that a pathway uses up.
    Input,
    /// A pool that a pathway makes.
    Output,
    /// A pool that a pathway needs but does not use up.
    Cofactor,
    /// An outside channel that a pathway needs.
    Outside,
    /// A flow from the ledger.
    Other,
}

/// A pipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeView {
    pub from: Node,
    pub to: Node,
    /// The pool that moves. `None` for an outside edge.
    pub currency: Option<Currency>,
    /// Amount per second, 0 or more, smoothed. 0 for cofactor and outside edges.
    pub flux: f32,
    pub kind: EdgeKind,
}

/// The view of one cell.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetabolismView {
    pub pools: Vec<PoolView>,
    pub pathways: Vec<PathwayView>,
    pub edges: Vec<EdgeView>,
}

impl MetabolismView {
    pub fn pathway(&self, kind: BlockKind) -> Option<&PathwayView> {
        self.pathways.iter().find(|p| p.kind == kind)
    }

    pub fn pool(&self, currency: Currency) -> Option<&PoolView> {
        self.pools.iter().find(|p| p.currency == currency)
    }

    /// The flux of the edge between two nodes for a currency, if there is one.
    pub fn flux(&self, from: Node, to: Node, currency: Currency) -> Option<f32> {
        self.edges
            .iter()
            .find(|e| e.from == from && e.to == to && e.currency == Some(currency))
            .map(|e| e.flux)
    }

    /// The net flux of the pathway edges into (+) and out of (-) one pool, per
    /// second: what the pool gains from pathways.
    pub fn pathway_net(&self, currency: Currency) -> f32 {
        self.edges
            .iter()
            .filter(|e| e.currency == Some(currency))
            .map(|e| match (e.kind, e.from, e.to) {
                (EdgeKind::Output, _, Node::Pool(_)) => e.flux,
                (EdgeKind::Input, Node::Pool(_), _) => -e.flux,
                _ => 0.0,
            })
            .sum()
    }
}

fn smooth(previous: f32, target: f32, alpha: f32) -> f32 {
    previous + (target - previous) * alpha
}

/// Builds the view. See the module docs.
pub fn build_view(input: &ViewInput) -> MetabolismView {
    let alpha = match input.previous {
        Some(_) if input.frame_dt > 0.0 => 1.0 - (-input.frame_dt / FLUX_SMOOTH_SECONDS).exp(),
        Some(_) => 0.0,
        None => 1.0,
    };
    let previous_edge = |edge: &EdgeView| {
        input
            .previous
            .and_then(|p| {
                p.edges
                    .iter()
                    .find(|e| e.from == edge.from && e.to == edge.to && e.currency == edge.currency)
            })
            .map_or(edge.flux, |e| e.flux)
    };

    let pools = Currency::ALL
        .iter()
        .map(|&currency| PoolView {
            currency,
            level: input.pools.get(currency),
        })
        .collect();

    // The limit of each block, with sharing, from the pools at this moment.
    let solver_inputs: Vec<BlockInput> = input
        .blocks
        .iter()
        .map(|b| BlockInput {
            profile: b.profile,
            enzymes: input.proteome.enzymes(b.kind) as f32,
            status: b.status,
            drive: b.throttle,
            k_half: b.kinetics.k_half,
            cofactors: b.kinetics.cofactors,
            external: external_saturation(input.env, b.kinetics.external, b.kinetics.k_half),
            external_channel: limiting_channel(input.env, b.kinetics.external, b.kinetics.k_half),
        })
        .collect();
    let limits = block_limits(&input.pools.pools, &solver_inputs, MAX_SUBSTEP);

    let mut pathways = Vec::new();
    let mut edges = Vec::new();
    let push_edges = |kind: BlockKind,
                          profile: &FluxProfile,
                          kinetics: &Kinetics,
                          turnover_rate: f32,
                          edges: &mut Vec<EdgeView>| {
        let mut flows: Vec<(Currency, f32)> = profile.0.iter().map(|(&c, &f)| (c, f)).collect();
        flows.sort_by_key(|&(c, _)| c);
        for (currency, flow) in flows {
            let (from, to, edge_kind) = if flow < 0.0 {
                (Node::Pool(currency), Node::Pathway(kind), EdgeKind::Input)
            } else {
                (Node::Pathway(kind), Node::Pool(currency), EdgeKind::Output)
            };
            let mut edge = EdgeView {
                from,
                to,
                currency: Some(currency),
                flux: flow.abs() * turnover_rate,
                kind: edge_kind,
            };
            edge.flux = smooth(previous_edge(&edge), edge.flux, alpha);
            edges.push(edge);
        }
        for &currency in kinetics.cofactors {
            edges.push(EdgeView {
                from: Node::Pool(currency),
                to: Node::Pathway(kind),
                currency: Some(currency),
                flux: 0.0,
                kind: EdgeKind::Cofactor,
            });
        }
        for &channel in kinetics.external {
            edges.push(EdgeView {
                from: Node::Outside(channel),
                to: Node::Pathway(kind),
                currency: None,
                flux: 0.0,
                kind: EdgeKind::Outside,
            });
        }
    };

    let mut owned: HashSet<BlockKind> = HashSet::new();
    for (i, block) in input.blocks.iter().enumerate() {
        owned.insert(block.kind);
        let rate = if input.dt > 0.0 { block.turnover / input.dt } else { 0.0 };
        let best = input.proteome.enzymes(block.kind) as f32
            * block.status.throughput()
            * block.throttle.max(0.0);
        let target = if best > 0.0 { (rate / best).clamp(0.0, 1.0) } else { 0.0 };
        let before = input
            .previous
            .and_then(|p| p.pathway(block.kind))
            .map_or(target, |p| p.activity);
        pathways.push(PathwayView {
            kind: block.kind,
            owned: true,
            known: true,
            state: Some(block.status),
            enzymes: input.proteome.enzymes(block.kind),
            activity: smooth(before, target, alpha),
            throttle: block.throttle,
            limit: limits[i],
        });
        push_edges(block.kind, block.profile, &block.kinetics, rate, &mut edges);
    }

    // Genes that have a block spec but no block yet (the block spawns next
    // frame): owned, from the gene state, with no flux.
    for kind in BlockKind::ALL {
        if owned.contains(&kind) {
            continue;
        }
        let Some(spec) = block_spec(kind) else { continue };
        let gene = input.genome.get_gene_state(&kind).cloned();
        let is_owned = gene.is_some();
        pathways.push(PathwayView {
            kind,
            owned: is_owned,
            known: is_owned || input.codex.contains(&kind),
            state: gene.map(|g| match g {
                GeneState::Expressed => BlockStatus::Active,
                GeneState::Mutated => BlockStatus::Mutated,
                GeneState::Silent => BlockStatus::Silent,
                GeneState::Enhanced => BlockStatus::Enhanced,
            }),
            enzymes: input.proteome.enzymes(kind),
            activity: 0.0,
            throttle: 0.0,
            limit: if is_owned { Limit::NoEnzymes } else { Limit::None },
        });
        push_edges(kind, &spec.profile, &spec.kinetics, 0.0, &mut edges);
    }
    pathways.sort_by_key(|p| p.kind);

    for (source, currency, rate) in input.ledger.rates() {
        let (from, to) = if rate > 0.0 {
            (Node::Source(source), Node::Pool(currency))
        } else {
            (Node::Pool(currency), Node::Source(source))
        };
        edges.push(EdgeView {
            from,
            to,
            currency: Some(currency),
            flux: rate.abs(),
            kind: EdgeKind::Other,
        });
    }

    MetabolismView { pools, pathways, edges }
}

// --- Bevy side ---

/// The metabolism view of the local player's cell. `None` until there is one.
#[derive(Resource, Default, Debug, Clone)]
pub struct LocalMetabolismView(pub Option<MetabolismView>);

/// Rebuilds the view of the local player's cell each frame.
#[allow(clippy::type_complexity)]
pub fn update_local_view_system(
    time: Res<Time>,
    timing: Res<FlowmapTimingConfig>,
    codex: Option<Res<Codex>>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<
        (
            &Genome,
            &Proteome,
            &CellBlocks,
            &CurrencyPools,
            &FlowLedger,
            &LocalEnvironment,
            &CellTick,
        ),
        With<Cell>,
    >,
    blocks: Query<(
        &MetabolicNode,
        &FluxProfile,
        &Kinetics,
        &super::Drive,
        &BlockTurnover,
        Option<&Throttle>,
    )>,
    mut view: ResMut<LocalMetabolismView>,
) {
    let Some(cell) = players.iter().next().and_then(|c| cells.get(c.0).ok()) else {
        if view.0.is_some() {
            view.0 = None;
        }
        return;
    };
    let (genome, proteome, cell_blocks, pools, ledger, env, tick) = cell;
    let mut data: Vec<BlockData> = cell_blocks
        .0
        .values()
        .filter_map(|&entity| {
            let (node, profile, kinetics, drive, turnover, throttle) = blocks.get(entity).ok()?;
            Some(BlockData {
                kind: node.kind,
                profile,
                turnover: turnover.0,
                status: node.status,
                throttle: drive.0 * throttle.map_or(1.0, |t| t.0),
                kinetics: *kinetics,
            })
        })
        .collect();
    data.sort_by_key(|b| b.kind);
    let codex_genes = codex.map(|c| c.genes.iter().copied().collect()).unwrap_or_default();
    let built = build_view(&ViewInput {
        genome,
        codex: &codex_genes,
        proteome,
        blocks: &data,
        pools,
        ledger,
        env,
        dt: tick.dt(timing.target_delta()),
        previous: view.0.as_ref(),
        frame_dt: time.delta_secs(),
    });
    view.0 = Some(built);
}

/// A map from pool to its net change per second, for tests and tooltips: the
/// pathway net plus the ledger rates.
pub fn net_rate(view: &MetabolismView, currency: Currency) -> f32 {
    let mut net = HashMap::<Currency, f32>::new();
    for e in &view.edges {
        let Some(c) = e.currency else { continue };
        match (e.kind, e.to) {
            (EdgeKind::Output, _) => *net.entry(c).or_default() += e.flux,
            (EdgeKind::Input, _) => *net.entry(c).or_default() -= e.flux,
            (EdgeKind::Other, Node::Pool(_)) => *net.entry(c).or_default() += e.flux,
            (EdgeKind::Other, _) => *net.entry(c).or_default() -= e.flux,
            _ => {}
        }
    }
    net.get(&currency).copied().unwrap_or(0.0)
}

/// Adds the local view resource and its system.
pub struct MetabolismViewPlugin;

impl Plugin for MetabolismViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalMetabolismView>()
            .add_systems(Update, update_local_view_system);
    }
}
