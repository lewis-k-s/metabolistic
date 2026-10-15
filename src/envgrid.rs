//! # Environment grid
//!
//! `EnvGrid` is a coarse grid with one `f32` layer for each `Channel`: heat,
//! minerals, light (UV) and O2. Each channel has sources (emitters),
//! diffusion, a drift and a decay. Cells sample the grid at their position and
//! take minerals from it (see `exchange`).
//!
//! Bevy systems fetch data; the functions here change it. All step functions
//! work on plain slices, so tests need no `App`.
//!
//! ## Step
//!
//! A step of `dt` seconds has equal sub-steps of at most `MAX_SUBSTEP` seconds.
//! A sub-step of length `h` does these in order:
//!
//! 1. sources: add `emission x h`;
//! 2. drift: move a fraction of each grid cell to its downwind neighbours;
//! 3. diffusion: exchange a fraction of the difference with each neighbour;
//! 4. decay: multiply by `exp(-decay x h)`.
//!
//! Drift and diffusion only move mass between grid cells, and the edge of the
//! grid is closed (unless a channel opens its drift edges, so that a plume can
//! leave the field). So with no sources and no decay, the total of a layer
//! does not change. Levels never go below 0, because the moved fractions in each
//! sub-step add up to at most 1.
//!
//! ## Walls
//!
//! A grid cell can be a wall (the `walls` mask of `EnvGrid`). A wall cell is
//! inert: it gets no source, nothing flows into it or out of it, so a wall
//! blocks drift and diffusion. Nothing is created or lost, so the total of a
//! layer is still conserved. The `_masked` functions take the mask; the others
//! call them with no mask.
//!
//! ## Leaky cells
//!
//! A grid cell can also be *leaky* (a microbial mat): it is not a wall, but
//! diffusion across its edges is multiplied by a factor below 1 (the lower of
//! the two cells at an edge). The flow between two cells stays equal and
//! opposite, so the total is still conserved.
//!
//! Ticks of 0.1 s, 0.2 s or 0.8 s all use sub-steps of 0.05 s, so the tick
//! length does not change the result.

use bevy::prelude::*;

use crate::environment::Channel;

/// Side of a grid cell in world units (pixels). The player radius is 30.
/// **Playtest.**
pub const CELL_SIZE: f32 = 32.0;

/// Largest sub-step of `EnvGrid::step`, in seconds.
pub const MAX_SUBSTEP: f32 = 0.05;

/// Largest share of a grid cell that diffusion moves to one neighbour in one
/// sub-step. With 4 neighbours, 0.25 keeps levels at 0 or more.
const MAX_DIFFUSION_SHARE: f32 = 0.25;

/// Width and height of a grid, in grid cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridDims {
    pub width: usize,
    pub height: usize,
}

impl GridDims {
    pub fn len(&self) -> usize {
        self.width * self.height
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Index of the grid cell at column `x`, row `y`.
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }
}

/// How one channel moves in the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldParams {
    /// Diffusion per second: the share of the level difference that goes to
    /// each neighbour in one second. At most 5 (see `MAX_SUBSTEP`).
    pub diffusion: f32,
    /// Decay per second: the level falls like `exp(-decay x t)`.
    pub decay: f32,
    /// Drift in grid cells per second. Fast drift is capped at 1 cell per
    /// sub-step.
    pub drift: Vec2,
    /// If true, drift carries content out of the grid at the downwind edges
    /// (it is lost). If false, the edge is closed and content piles up there.
    pub open_drift_edges: bool,
}

impl FieldParams {
    /// No movement and no decay.
    pub const STILL: FieldParams = FieldParams {
        diffusion: 0.0,
        decay: 0.0,
        drift: Vec2::ZERO,
        open_drift_edges: false,
    };
}

// --- Pure step functions ---

/// Number of equal sub-steps for a step of `dt` seconds.
pub fn substep_count(dt: f32) -> u32 {
    // The small margin stops float error from adding a step (0.2 / 0.05 > 4).
    ((dt / MAX_SUBSTEP) - 1e-4).ceil().max(1.0) as u32
}

/// Adds `emission x h` to each grid cell.
pub fn add_sources(layer: &mut [f32], emission: &[f32], h: f32) {
    for (level, &rate) in layer.iter_mut().zip(emission) {
        *level += rate * h;
    }
}

/// Like `add_sources`, but wall cells get nothing.
pub fn add_sources_masked(layer: &mut [f32], emission: &[f32], walls: Option<&[bool]>, h: f32) {
    let Some(walls) = walls else {
        return add_sources(layer, emission, h);
    };
    for ((level, &rate), &wall) in layer.iter_mut().zip(emission).zip(walls) {
        if !wall {
            *level += rate * h;
        }
    }
}

/// Moves each grid cell's content downwind. `drift` is in grid cells per
/// second. With `open_edges` false, content never leaves the grid: a cell at
/// the edge keeps what would go out. With `open_edges` true, that content is
/// lost.
pub fn advect(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    drift: Vec2,
    open_edges: bool,
    h: f32,
) {
    advect_masked(layer, scratch, dims, drift, open_edges, None, h);
}

/// `advect` with a wall mask: content does not enter or leave a wall cell.
pub fn advect_masked(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    drift: Vec2,
    open_edges: bool,
    walls: Option<&[bool]>,
    h: f32,
) {
    let is_wall = |x: usize, y: usize| walls.is_some_and(|w| w[dims.index(x, y)]);
    let mut fx = drift.x.abs() * h;
    let mut fy = drift.y.abs() * h;
    let total = fx + fy;
    if total <= 0.0 {
        return;
    }
    if total > 1.0 {
        fx /= total;
        fy /= total;
    }
    let step_x: isize = if drift.x >= 0.0 { 1 } else { -1 };
    let step_y: isize = if drift.y >= 0.0 { 1 } else { -1 };

    scratch.clear();
    scratch.resize(layer.len(), 0.0);
    for y in 0..dims.height {
        for x in 0..dims.width {
            let i = dims.index(x, y);
            let content = layer[i];
            if is_wall(x, y) {
                scratch[i] += content;
                continue;
            }
            let mut stay = content;
            let nx = x as isize + step_x;
            if fx > 0.0 {
                let inside = nx >= 0 && (nx as usize) < dims.width;
                let blocked = inside && is_wall(nx as usize, y);
                if !blocked && (inside || open_edges) {
                    let moved = content * fx;
                    if inside {
                        scratch[dims.index(nx as usize, y)] += moved;
                    }
                    stay -= moved;
                }
            }
            let ny = y as isize + step_y;
            if fy > 0.0 {
                let inside = ny >= 0 && (ny as usize) < dims.height;
                let blocked = inside && is_wall(x, ny as usize);
                if !blocked && (inside || open_edges) {
                    let moved = content * fy;
                    if inside {
                        scratch[dims.index(x, ny as usize)] += moved;
                    }
                    stay -= moved;
                }
            }
            scratch[i] += stay;
        }
    }
    layer.copy_from_slice(scratch);
}

/// Diffusion. `share` is the part of the difference that each neighbour gets,
/// at most `MAX_DIFFUSION_SHARE`. Edge cells have fewer neighbours, so the
/// edge is closed.
pub fn diffuse(layer: &mut [f32], scratch: &mut Vec<f32>, dims: GridDims, share: f32) {
    diffuse_masked(layer, scratch, dims, share, None);
}

/// `diffuse` with a wall mask: no flow to or from a wall cell.
pub fn diffuse_masked(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    share: f32,
    walls: Option<&[bool]>,
) {
    diffuse_weighted(layer, scratch, dims, share, walls, None);
}

/// `diffuse_masked` with a leak factor for each grid cell (0 to 1; 1 is open).
/// The flow across the edge of two cells is multiplied by the lower factor of
/// the two, so it is the same in both directions and mass is conserved.
pub fn diffuse_weighted(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    share: f32,
    walls: Option<&[bool]>,
    leak: Option<&[f32]>,
) {
    let is_wall = |x: usize, y: usize| walls.is_some_and(|w| w[dims.index(x, y)]);
    let factor = |a: usize, b: usize| leak.map_or(1.0, |l| l[a].min(l[b]));
    let share = share.clamp(0.0, MAX_DIFFUSION_SHARE);
    if share <= 0.0 {
        return;
    }
    scratch.clear();
    scratch.extend_from_slice(layer);
    for y in 0..dims.height {
        for x in 0..dims.width {
            let i = dims.index(x, y);
            let here = layer[i];
            if is_wall(x, y) {
                continue; // scratch already holds the unchanged level
            }
            let mut flow = 0.0;
            if x > 0 && !is_wall(x - 1, y) {
                let j = dims.index(x - 1, y);
                flow += (layer[j] - here) * factor(i, j);
            }
            if x + 1 < dims.width && !is_wall(x + 1, y) {
                let j = dims.index(x + 1, y);
                flow += (layer[j] - here) * factor(i, j);
            }
            if y > 0 && !is_wall(x, y - 1) {
                let j = dims.index(x, y - 1);
                flow += (layer[j] - here) * factor(i, j);
            }
            if y + 1 < dims.height && !is_wall(x, y + 1) {
                let j = dims.index(x, y + 1);
                flow += (layer[j] - here) * factor(i, j);
            }
            scratch[i] = here + share * flow;
        }
    }
    layer.copy_from_slice(scratch);
}

/// Exact decay over `h` seconds.
pub fn decay_layer(layer: &mut [f32], decay: f32, h: f32) {
    if decay <= 0.0 {
        return;
    }
    let keep = (-decay * h).exp();
    for level in layer {
        *level *= keep;
    }
}

/// One step of `dt` seconds for one layer. See the module docs.
pub fn step_layer(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    emission: &[f32],
    params: &FieldParams,
    dt: f32,
) {
    step_layer_masked(layer, scratch, dims, emission, params, None, dt);
}

/// `step_layer` with a wall mask (see the module docs).
pub fn step_layer_masked(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    emission: &[f32],
    params: &FieldParams,
    walls: Option<&[bool]>,
    dt: f32,
) {
    step_layer_leaky(layer, scratch, dims, emission, params, walls, None, dt);
}

/// `step_layer_masked` with leak factors (see the module docs).
#[allow(clippy::too_many_arguments)]
pub fn step_layer_leaky(
    layer: &mut [f32],
    scratch: &mut Vec<f32>,
    dims: GridDims,
    emission: &[f32],
    params: &FieldParams,
    walls: Option<&[bool]>,
    leak: Option<&[f32]>,
    dt: f32,
) {
    if dt <= 0.0 {
        return;
    }
    let n = substep_count(dt);
    let h = dt / n as f32;
    for _ in 0..n {
        add_sources_masked(layer, emission, walls, h);
        advect_masked(layer, scratch, dims, params.drift, params.open_drift_edges, walls, h);
        diffuse_weighted(layer, scratch, dims, params.diffusion * h, walls, leak);
        decay_layer(layer, params.decay, h);
    }
}

// --- Sampling ---

/// The four grid cells around a world position, with bilinear weights that add
/// up to 1. Grid cell `(x, y)` has its centre at `origin + (x + 0.5, y + 0.5) x
/// CELL_SIZE`. Outside the grid the position is clamped to the nearest centre.
pub fn bilinear_taps(dims: GridDims, origin: Vec2, pos: Vec2) -> [(usize, f32); 4] {
    let max_x = dims.width.saturating_sub(1) as f32;
    let max_y = dims.height.saturating_sub(1) as f32;
    let gx = ((pos.x - origin.x) / CELL_SIZE - 0.5).clamp(0.0, max_x);
    let gy = ((pos.y - origin.y) / CELL_SIZE - 0.5).clamp(0.0, max_y);
    let x0 = gx.floor() as usize;
    let y0 = gy.floor() as usize;
    let x1 = (x0 + 1).min(dims.width.saturating_sub(1));
    let y1 = (y0 + 1).min(dims.height.saturating_sub(1));
    let tx = gx - x0 as f32;
    let ty = gy - y0 as f32;
    [
        (dims.index(x0, y0), (1.0 - tx) * (1.0 - ty)),
        (dims.index(x1, y0), tx * (1.0 - ty)),
        (dims.index(x0, y1), (1.0 - tx) * ty),
        (dims.index(x1, y1), tx * ty),
    ]
}

/// The level of a layer at a world position (bilinear).
pub fn sample_layer(layer: &[f32], dims: GridDims, origin: Vec2, pos: Vec2) -> f32 {
    bilinear_taps(dims, origin, pos)
        .iter()
        .map(|&(i, w)| layer[i] * w)
        .sum()
}

/// Removes up to `amount` from the grid cells around a world position, in
/// proportion to the bilinear weights. A cell never goes below 0. Returns what
/// was removed, so a caller can credit exactly that to a cell.
pub fn take_from_layer(
    layer: &mut [f32],
    dims: GridDims,
    origin: Vec2,
    pos: Vec2,
    amount: f32,
) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }
    let mut taken = 0.0;
    for (i, w) in bilinear_taps(dims, origin, pos) {
        // Taps may repeat at the edge. Each one takes from the current level.
        let part = (amount * w).min(layer[i]).max(0.0);
        layer[i] -= part;
        taken += part;
    }
    taken
}

/// Adds up to `amount` to the grid cells around a world position, in proportion
/// to the bilinear weights. Wall cells get nothing: their share goes to the
/// other cells. Returns what was added (0 if all four cells are walls), so a
/// caller can take exactly that from a cell.
pub fn add_to_layer(
    layer: &mut [f32],
    dims: GridDims,
    origin: Vec2,
    pos: Vec2,
    walls: Option<&[bool]>,
    amount: f32,
) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }
    let taps = bilinear_taps(dims, origin, pos);
    let open = |i: usize| !walls.is_some_and(|w| w[i]);
    let weight: f32 = taps.iter().filter(|&&(i, _)| open(i)).map(|&(_, w)| w).sum();
    if weight <= 0.0 {
        return 0.0;
    }
    for (i, w) in taps {
        if open(i) {
            layer[i] += amount * w / weight;
        }
    }
    amount
}

// --- The resource ---

/// The environment grid. One layer for each `Channel`.
#[derive(Resource, Debug, Clone)]
pub struct EnvGrid {
    dims: GridDims,
    /// World position of the lower-left corner of the grid.
    origin: Vec2,
    layers: [Vec<f32>; Channel::ALL.len()],
    /// Emission per second for each grid cell and channel.
    emission: [Vec<f32>; Channel::ALL.len()],
    params: [FieldParams; Channel::ALL.len()],
    /// Channels that are settled: they are static, so `step` skips them.
    settled: [bool; Channel::ALL.len()],
    scratch: Vec<f32>,
    /// Wall cells: inert, they block drift and diffusion.
    walls: Vec<bool>,
    wall_count: usize,
    /// Diffusion factor of each grid cell (1 is open). See "Leaky cells".
    leak: Vec<f32>,
    leak_count: usize,
}

impl EnvGrid {
    /// An empty grid of `width x height` grid cells. `origin` is the world
    /// position of its lower-left corner.
    pub fn new(width: usize, height: usize, origin: Vec2) -> Self {
        let dims = GridDims { width, height };
        let blank = || vec![0.0; dims.len()];
        Self {
            dims,
            origin,
            layers: std::array::from_fn(|_| blank()),
            emission: std::array::from_fn(|_| blank()),
            params: [FieldParams::STILL; Channel::ALL.len()],
            settled: [false; Channel::ALL.len()],
            scratch: Vec::new(),
            walls: vec![false; dims.len()],
            wall_count: 0,
            leak: vec![1.0; dims.len()],
            leak_count: 0,
        }
    }

    /// Makes grid cell `(x, y)` a wall or opens it again. The levels in the
    /// cell are set to 0 and stay 0 while it is a wall (a wall holds no
    /// content), so the mass that was in it leaves the books: call this
    /// before the warm-up, or accept the loss. Settled channels (light) stay
    /// settled, so opening a wall does not change them.
    pub fn set_wall(&mut self, x: usize, y: usize, wall: bool) {
        let i = self.dims.index(x, y);
        if self.walls[i] == wall {
            return;
        }
        self.walls[i] = wall;
        if wall {
            self.wall_count += 1;
            for layer in &mut self.layers {
                layer[i] = 0.0;
            }
        } else {
            self.wall_count -= 1;
        }
    }

    /// Sets the diffusion factor of grid cell `(x, y)`, from 0 to 1. A cell
    /// with a factor below 1 is leaky: a microbial mat.
    pub fn set_leak(&mut self, x: usize, y: usize, factor: f32) {
        let i = self.dims.index(x, y);
        let factor = factor.clamp(0.0, 1.0);
        let was = self.leak[i] < 1.0;
        self.leak[i] = factor;
        match (was, factor < 1.0) {
            (false, true) => self.leak_count += 1,
            (true, false) => self.leak_count -= 1,
            _ => {}
        }
    }

    /// The diffusion factor of grid cell `(x, y)`.
    pub fn leak(&self, x: usize, y: usize) -> f32 {
        self.leak[self.dims.index(x, y)]
    }

    pub fn is_wall(&self, x: usize, y: usize) -> bool {
        self.walls[self.dims.index(x, y)]
    }

    /// The grid cell `(x, y)` that holds a world position, or `None` outside
    /// the grid.
    pub fn cell_at(&self, pos: Vec2) -> Option<(usize, usize)> {
        let local = (pos - self.origin) / CELL_SIZE;
        if local.x < 0.0 || local.y < 0.0 {
            return None;
        }
        let (x, y) = (local.x as usize, local.y as usize);
        (x < self.dims.width && y < self.dims.height).then_some((x, y))
    }

    /// The wall mask, row by row.
    pub fn walls(&self) -> &[bool] {
        &self.walls
    }

    pub fn wall_count(&self) -> usize {
        self.wall_count
    }

    /// A grid of `width x height` cells, centred on the world origin.
    pub fn centered(width: usize, height: usize) -> Self {
        let origin = -Vec2::new(width as f32, height as f32) * CELL_SIZE * 0.5;
        Self::new(width, height, origin)
    }

    pub fn dims(&self) -> GridDims {
        self.dims
    }

    pub fn origin(&self) -> Vec2 {
        self.origin
    }

    /// Size of the grid in world units.
    pub fn world_size(&self) -> Vec2 {
        Vec2::new(self.dims.width as f32, self.dims.height as f32) * CELL_SIZE
    }

    /// World position of the centre of grid cell `(x, y)`.
    pub fn cell_center(&self, x: usize, y: usize) -> Vec2 {
        self.origin + (Vec2::new(x as f32, y as f32) + 0.5) * CELL_SIZE
    }

    pub fn params(&self, channel: Channel) -> &FieldParams {
        &self.params[channel.index()]
    }

    pub fn set_params(&mut self, channel: Channel, params: FieldParams) {
        self.params[channel.index()] = params;
        self.settled[channel.index()] = false;
    }

    /// Marks a channel as settled: it has reached its steady state and has
    /// no drift, so `step` skips it from now on. Use it for static channels
    /// such as light shafts, after the warm-up. Adding an emitter or setting
    /// the params un-settles the channel. `set_level` does not.
    pub fn settle(&mut self, channel: Channel) {
        self.settled[channel.index()] = true;
    }

    pub fn is_settled(&self, channel: Channel) -> bool {
        self.settled[channel.index()]
    }

    /// The levels of one channel, row by row.
    pub fn layer(&self, channel: Channel) -> &[f32] {
        &self.layers[channel.index()]
    }

    /// The level of one channel in grid cell `(x, y)`.
    pub fn level(&self, channel: Channel, x: usize, y: usize) -> f32 {
        self.layers[channel.index()][self.dims.index(x, y)]
    }

    /// Sets the level of grid cell `(x, y)`. Negative levels become 0.
    pub fn set_level(&mut self, channel: Channel, x: usize, y: usize, level: f32) {
        let i = self.dims.index(x, y);
        self.layers[channel.index()][i] = level.max(0.0);
    }

    /// The sum of one channel over the grid.
    pub fn total(&self, channel: Channel) -> f32 {
        self.layers[channel.index()].iter().sum()
    }

    /// The lowest level of one channel.
    pub fn min_level(&self, channel: Channel) -> f32 {
        self.layers[channel.index()]
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min)
    }

    /// Adds an emitter: `rate` per second in total, spread over the grid cells
    /// within `radius` of `center`. The share of a cell falls off linearly
    /// with distance. If no cell centre is within `radius`, the nearest cell
    /// gets all of it.
    pub fn add_emitter(&mut self, channel: Channel, center: Vec2, radius: f32, rate: f32) {
        self.settled[channel.index()] = false;
        let mut weights: Vec<(usize, f32)> = Vec::new();
        let mut nearest = (0, f32::INFINITY);
        for y in 0..self.dims.height {
            for x in 0..self.dims.width {
                let distance = self.cell_center(x, y).distance(center);
                let i = self.dims.index(x, y);
                if distance < nearest.1 {
                    nearest = (i, distance);
                }
                if distance < radius {
                    weights.push((i, 1.0 - distance / radius));
                }
            }
        }
        if weights.is_empty() {
            weights.push((nearest.0, 1.0));
        }
        let sum: f32 = weights.iter().map(|&(_, w)| w).sum();
        let emission = &mut self.emission[channel.index()];
        for (i, w) in weights {
            emission[i] += rate * w / sum;
        }
    }

    /// Adds a rectangular emitter: every grid cell whose centre is inside
    /// `[min, max]` emits `rate_per_cell` per second. A light shaft is a tall
    /// rectangle. If no cell centre is inside, nothing is added.
    pub fn add_rect_emitter(&mut self, channel: Channel, min: Vec2, max: Vec2, rate_per_cell: f32) {
        self.settled[channel.index()] = false;
        for y in 0..self.dims.height {
            for x in 0..self.dims.width {
                let c = self.cell_center(x, y);
                if c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y {
                    let i = self.dims.index(x, y);
                    self.emission[channel.index()][i] += rate_per_cell;
                }
            }
        }
    }

    /// Total emission per second of one channel.
    pub fn emission_total(&self, channel: Channel) -> f32 {
        self.emission[channel.index()].iter().sum()
    }

    /// The level of a channel at a world position (bilinear).
    pub fn sample(&self, channel: Channel, pos: Vec2) -> f32 {
        sample_layer(&self.layers[channel.index()], self.dims, self.origin, pos)
    }

    /// Removes up to `amount` of a channel around a world position. Returns
    /// what was removed.
    pub fn take(&mut self, channel: Channel, pos: Vec2, amount: f32) -> f32 {
        take_from_layer(
            &mut self.layers[channel.index()],
            self.dims,
            self.origin,
            pos,
            amount,
        )
    }

    /// Adds `amount` of a channel around a world position (wall cells get
    /// nothing). Returns what was added, which is all of `amount` unless the
    /// four cells round the position are walls.
    pub fn deposit(&mut self, channel: Channel, pos: Vec2, amount: f32) -> f32 {
        let walls = (self.wall_count > 0).then_some(self.walls.as_slice());
        add_to_layer(
            &mut self.layers[channel.index()],
            self.dims,
            self.origin,
            pos,
            walls,
            amount,
        )
    }

    /// Runs all channels for `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        for channel in Channel::ALL {
            let i = channel.index();
            if self.settled[i] {
                continue;
            }
            let walls = (self.wall_count > 0).then_some(self.walls.as_slice());
            let leak = (self.leak_count > 0).then_some(self.leak.as_slice());
            step_layer_leaky(
                &mut self.layers[i],
                &mut self.scratch,
                self.dims,
                &self.emission[i],
                &self.params[i],
                walls,
                leak,
                dt,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const DIMS: GridDims = GridDims {
        width: 12,
        height: 9,
    };

    /// A layer with some uneven content.
    fn lumpy() -> Vec<f32> {
        (0..DIMS.len())
            .map(|i| ((i * 37) % 11) as f32 * 3.0)
            .collect()
    }

    fn sum(layer: &[f32]) -> f32 {
        layer.iter().sum()
    }

    #[test]
    fn a_settled_channel_is_skipped_until_an_emitter_is_added() {
        let mut grid = EnvGrid::centered(6, 6);
        grid.set_params(
            Channel::Light,
            FieldParams { diffusion: 0.5, decay: 0.3, ..FieldParams::STILL },
        );
        grid.add_emitter(Channel::Light, Vec2::ZERO, 40.0, 5.0);
        for _ in 0..100 {
            grid.step(1.0);
        }
        let settled_layer = grid.layer(Channel::Light).to_vec();
        grid.settle(Channel::Light);
        assert!(grid.is_settled(Channel::Light));
        grid.step(1.0);
        assert_eq!(grid.layer(Channel::Light), settled_layer.as_slice());
        grid.add_emitter(Channel::Light, Vec2::ZERO, 40.0, 5.0);
        assert!(!grid.is_settled(Channel::Light));
        grid.step(1.0);
        assert_ne!(grid.layer(Channel::Light), settled_layer.as_slice());
    }

    #[test]
    fn diffusion_conserves_the_total_and_levels_out() {
        let mut layer = lumpy();
        let mut scratch = Vec::new();
        let before = sum(&layer);
        for _ in 0..500 {
            diffuse(&mut layer, &mut scratch, DIMS, 0.2);
        }
        assert_abs_diff_eq!(sum(&layer), before, epsilon = before * 1e-4);
        let mean = before / DIMS.len() as f32;
        for level in &layer {
            assert_abs_diff_eq!(*level, mean, epsilon = mean * 0.05);
        }
    }

    #[test]
    fn drift_conserves_the_total_and_moves_mass_downwind() {
        let mut layer = vec![0.0; DIMS.len()];
        layer[DIMS.index(2, 4)] = 100.0;
        let mut scratch = Vec::new();
        for _ in 0..40 {
            advect(&mut layer, &mut scratch, DIMS, Vec2::new(2.0, 0.0), false, 0.1);
        }
        assert_abs_diff_eq!(sum(&layer), 100.0, epsilon = 1e-3);
        let centre: f32 = (0..DIMS.width)
            .map(|x| x as f32 * layer[DIMS.index(x, 4)])
            .sum::<f32>()
            / 100.0;
        assert!(centre > 5.0, "mass moved to +x, centre at {centre}");
        // The edge is closed: the right column holds what arrived.
        assert!(layer[DIMS.index(DIMS.width - 1, 4)] > 1.0);
    }

    #[test]
    fn open_edges_let_mass_leave_but_never_go_negative() {
        let mut layer = vec![5.0; DIMS.len()];
        let mut scratch = Vec::new();
        for _ in 0..100 {
            advect(&mut layer, &mut scratch, DIMS, Vec2::new(3.0, 1.0), true, 0.1);
            assert!(layer.iter().all(|&v| v >= 0.0));
        }
        assert!(sum(&layer) < 1.0, "everything drifted out: {}", sum(&layer));
    }

    #[test]
    fn fast_drift_is_capped_and_stays_positive() {
        let mut layer = lumpy();
        let before = sum(&layer);
        let mut scratch = Vec::new();
        advect(&mut layer, &mut scratch, DIMS, Vec2::new(-50.0, 30.0), false, 0.05);
        assert_abs_diff_eq!(sum(&layer), before, epsilon = before * 1e-5);
        assert!(layer.iter().all(|&v| v >= 0.0));
    }

    #[test]
    fn no_sources_and_no_decay_conserve_the_total() {
        let params = FieldParams {
            diffusion: 2.0,
            decay: 0.0,
            drift: Vec2::new(1.5, -0.7),
            open_drift_edges: false,
        };
        let mut layer = lumpy();
        let emission = vec![0.0; DIMS.len()];
        let mut scratch = Vec::new();
        let before = sum(&layer);
        for _ in 0..200 {
            step_layer(&mut layer, &mut scratch, DIMS, &emission, &params, 0.2);
        }
        assert_abs_diff_eq!(sum(&layer), before, epsilon = before * 1e-4);
    }

    #[test]
    fn levels_never_go_negative() {
        // Strong diffusion and drift, a spike, and decay.
        let params = FieldParams {
            diffusion: 5.0,
            decay: 0.3,
            drift: Vec2::new(30.0, 30.0),
            open_drift_edges: true,
        };
        let mut layer = vec![0.0; DIMS.len()];
        layer[DIMS.index(5, 5)] = 1000.0;
        let emission = vec![0.0; DIMS.len()];
        let mut scratch = Vec::new();
        for dt in [0.05, 0.2, 1.0, 3.0] {
            for _ in 0..20 {
                step_layer(&mut layer, &mut scratch, DIMS, &emission, &params, dt);
                assert!(layer.iter().all(|&v| v >= 0.0 && v.is_finite()));
            }
        }
    }

    #[test]
    fn decay_is_exact() {
        let mut layer = vec![8.0; DIMS.len()];
        let emission = vec![0.0; DIMS.len()];
        let params = FieldParams {
            decay: 0.5,
            ..FieldParams::STILL
        };
        let mut scratch = Vec::new();
        step_layer(&mut layer, &mut scratch, DIMS, &emission, &params, 2.0);
        assert_abs_diff_eq!(layer[0], 8.0 * (-1.0f32).exp(), epsilon = 1e-4);
    }

    #[test]
    fn source_and_decay_reach_the_balance() {
        // One emitter, no movement: the level goes to emission / decay.
        let mut layer = vec![0.0; 4];
        let dims = GridDims {
            width: 2,
            height: 2,
        };
        let emission = vec![3.0, 0.0, 0.0, 0.0];
        let params = FieldParams {
            decay: 0.5,
            ..FieldParams::STILL
        };
        let mut scratch = Vec::new();
        for _ in 0..200 {
            step_layer(&mut layer, &mut scratch, dims, &emission, &params, 0.2);
        }
        // Steady state of a source with exact decay: about E / k (the source
        // is added before the decay in each sub-step, so a little above).
        assert_abs_diff_eq!(layer[0], 6.0, epsilon = 0.2);
    }

    #[test]
    fn tick_length_does_not_change_the_result() {
        let params = FieldParams {
            diffusion: 1.5,
            decay: 0.1,
            drift: Vec2::new(1.0, 0.5),
            open_drift_edges: false,
        };
        let mut emission = vec![0.0; DIMS.len()];
        emission[DIMS.index(3, 3)] = 5.0;
        let run = |dt: f32, steps: usize| {
            let mut layer = lumpy();
            let mut scratch = Vec::new();
            for _ in 0..steps {
                step_layer(&mut layer, &mut scratch, DIMS, &emission, &params, dt);
            }
            layer
        };
        let one = run(1.0, 1);
        for (dt, steps) in [(0.5, 2), (0.2, 5), (0.1, 10), (0.05, 20)] {
            let other = run(dt, steps);
            for (a, b) in one.iter().zip(&other) {
                assert_abs_diff_eq!(*a, *b, epsilon = 1e-3 + a.abs() * 1e-4);
            }
        }
    }

    #[test]
    fn sample_is_bilinear_and_exact_at_centres() {
        let origin = Vec2::new(-100.0, -50.0);
        let dims = GridDims {
            width: 3,
            height: 2,
        };
        let layer = vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0];
        let centre = |x: usize, y: usize| {
            origin + (Vec2::new(x as f32, y as f32) + 0.5) * CELL_SIZE
        };
        assert_abs_diff_eq!(sample_layer(&layer, dims, origin, centre(1, 0)), 10.0);
        assert_abs_diff_eq!(sample_layer(&layer, dims, origin, centre(2, 1)), 50.0);
        // Halfway between two centres in x, and in y.
        let mid_x = (centre(0, 0) + centre(1, 0)) * 0.5;
        assert_abs_diff_eq!(sample_layer(&layer, dims, origin, mid_x), 5.0, epsilon = 1e-4);
        let mid_y = (centre(0, 0) + centre(0, 1)) * 0.5;
        assert_abs_diff_eq!(sample_layer(&layer, dims, origin, mid_y), 15.0, epsilon = 1e-4);
        // Outside the grid it clamps to the nearest centre.
        assert_abs_diff_eq!(
            sample_layer(&layer, dims, origin, Vec2::new(1e6, 1e6)),
            50.0
        );
        assert_abs_diff_eq!(
            sample_layer(&layer, dims, origin, Vec2::new(-1e6, -1e6)),
            0.0
        );
    }

    #[test]
    fn take_removes_what_it_returns_and_never_goes_below_zero() {
        let origin = Vec2::ZERO;
        let dims = GridDims {
            width: 3,
            height: 3,
        };
        let mut layer = vec![1.0; 9];
        let pos = Vec2::new(1.5, 1.5) * CELL_SIZE; // the centre cell
        let before = sum(&layer);
        let got = take_from_layer(&mut layer, dims, origin, pos, 0.4);
        assert_abs_diff_eq!(got, 0.4, epsilon = 1e-5);
        assert_abs_diff_eq!(sum(&layer), before - got, epsilon = 1e-5);
        // Ask for more than the grid holds.
        let got = take_from_layer(&mut layer, dims, origin, pos, 100.0);
        assert!(got < 100.0);
        assert!(layer.iter().all(|&v| v >= 0.0));
        assert_eq!(take_from_layer(&mut layer, dims, origin, pos, -1.0), 0.0);
    }

    #[test]
    fn emitter_adds_its_rate_and_the_grid_total_follows() {
        let mut grid = EnvGrid::centered(10, 10);
        grid.add_emitter(Channel::Minerals, Vec2::new(10.0, 10.0), 90.0, 4.0);
        assert_abs_diff_eq!(grid.emission_total(Channel::Minerals), 4.0, epsilon = 1e-4);
        // An emitter smaller than a grid cell still goes into one cell.
        grid.add_emitter(Channel::Heat, Vec2::new(5.0, 5.0), 1.0, 2.0);
        assert_abs_diff_eq!(grid.emission_total(Channel::Heat), 2.0, epsilon = 1e-4);
        grid.step(1.0);
        assert_abs_diff_eq!(grid.total(Channel::Minerals), 4.0, epsilon = 1e-3);
        assert!(grid.min_level(Channel::Minerals) >= 0.0);
    }

    #[test]
    fn grid_samples_a_set_level_at_the_cell_centre() {
        let mut grid = EnvGrid::centered(4, 4);
        grid.set_level(Channel::Heat, 2, 1, 9.0);
        let pos = grid.cell_center(2, 1);
        assert_abs_diff_eq!(grid.sample(Channel::Heat, pos), 9.0, epsilon = 1e-5);
        assert_eq!(grid.sample(Channel::Minerals, pos), 0.0);
        assert_eq!(grid.world_size(), Vec2::new(128.0, 128.0));
    }

    // --- Walls ---

    /// A wall column at x = 5 splits the test grid.
    fn wall_mask() -> Vec<bool> {
        (0..DIMS.len()).map(|i| i % DIMS.width == 5).collect()
    }

    #[test]
    fn walls_conserve_mass_and_nothing_crosses() {
        let mask = wall_mask();
        // Content on the left side only (columns 0-4), none in the wall.
        let mut layer: Vec<f32> = (0..DIMS.len())
            .map(|i| if i % DIMS.width < 5 { 1.0 + (i % 7) as f32 } else { 0.0 })
            .collect();
        let before = sum(&layer);
        let mut scratch = Vec::new();
        let params = FieldParams {
            diffusion: 1.0,
            decay: 0.0,
            drift: Vec2::new(2.0, 0.5),
            open_drift_edges: false,
        };
        for _ in 0..400 {
            step_layer_masked(&mut layer, &mut scratch, DIMS, &vec![0.0; DIMS.len()], &params, Some(&mask), 0.2);
        }
        assert_abs_diff_eq!(sum(&layer), before, epsilon = 1e-2);
        for (i, level) in layer.iter().enumerate() {
            if i % DIMS.width >= 5 {
                assert_eq!(*level, 0.0, "cell {i} is wall or behind it");
            }
        }
    }

    #[test]
    fn a_wall_with_a_gap_lets_content_through() {
        let mut mask = wall_mask();
        mask[DIMS.index(5, 4)] = false; // a hole
        let mut layer = vec![0.0; DIMS.len()];
        layer[DIMS.index(2, 4)] = 100.0;
        let mut scratch = Vec::new();
        let params = FieldParams { diffusion: 1.0, ..FieldParams::STILL };
        for _ in 0..400 {
            step_layer_masked(&mut layer, &mut scratch, DIMS, &vec![0.0; DIMS.len()], &params, Some(&mask), 0.2);
        }
        let right: f32 = (0..DIMS.len()).filter(|i| i % DIMS.width > 5).map(|i| layer[i]).sum();
        assert!(right > 1.0, "some content passed the hole: {right}");
        assert_abs_diff_eq!(sum(&layer), 100.0, epsilon = 1e-2);
    }

    #[test]
    fn a_wall_cell_gets_no_emission_and_the_grid_tracks_walls() {
        let mut grid = EnvGrid::centered(6, 6);
        grid.set_params(Channel::Minerals, FieldParams { diffusion: 0.5, ..FieldParams::STILL });
        grid.add_emitter(Channel::Minerals, grid.cell_center(2, 2), 10.0, 5.0);
        grid.set_wall(2, 2, true);
        assert!(grid.is_wall(2, 2) && grid.wall_count() == 1);
        for _ in 0..50 {
            grid.step(1.0);
        }
        assert_eq!(grid.level(Channel::Minerals, 2, 2), 0.0);
        assert_eq!(grid.cell_at(grid.cell_center(2, 2)), Some((2, 2)));
        assert_eq!(grid.cell_at(Vec2::new(1e6, 0.0)), None);
        grid.set_wall(2, 2, false);
        assert_eq!(grid.wall_count(), 0);
    }
}
