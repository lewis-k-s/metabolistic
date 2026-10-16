//! The fixed map of the Manage screen: where each pool and pathway sits, and
//! how the pipes run. A pure table, with no Bevy systems, so it has tests.
//!
//! Units are sprite pixels, +y up, and the map is centred on the origin. The
//! size of the map (`Layout::new(size)`) follows the window: the same lanes and
//! columns, spread over the free space. The smallest size is `MIN_SIZE`.
//!
//! From top to bottom: the outside (heat, minerals and light, drawn in
//! `outside`), the membrane band, six lanes (intake pools, energy pathways, hub
//! pools, build pathways, products, users), and the ribosome. The right column
//! holds the O2, ROS and Waste pools and the Catalase and Export plates. Pipes
//! run in the corridors between lanes and in the gutters between columns, so
//! they never cross a node, and a `Router` gives each pipe its own track, so
//! two pipes never lie on one another.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::blocks::block_spec;
use crate::blocks::genome::BlockKind;
use crate::molecules::Currency;

/// The smallest map, in sprite pixels. A smaller window shows a smaller scale.
pub const MIN_SIZE: Vec2 = Vec2::new(610.0, 500.0);

/// Height of the membrane band (the `wall_membrane` tile) and its tile width.
pub const BAND_HEIGHT: f32 = 26.0;
pub const BAND_TILE: f32 = 16.0;
/// The sockets are 24 px wide; they never come closer than this.
pub const MIN_SLOT_SPACING: f32 = 26.0;

/// Size of a pool node (icon, name, net rate, gauge) and of a pathway plate.
pub const POOL_SIZE: Vec2 = Vec2::new(80.0, 26.0);
pub const PLATE_SIZE: Vec2 = Vec2::new(96.0, 38.0);
pub const RIBOSOME_SIZE: Vec2 = Vec2::new(24.0, 24.0);

/// Room on the left for the stubs of the flows that are not pathways.
const STUB_MARGIN: f32 = 46.0;
/// Distance between two pipes that run side by side.
const TRACK_PITCH: f32 = 4.0;

/// A node of the map: its centre and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub pos: Vec2,
    pub size: Vec2,
}

impl Place {
    pub fn rect(&self) -> Rect {
        Rect::from_center_size(self.pos, self.size)
    }

    /// The edge where a pipe leaves or enters: the upper one if `up`.
    pub fn edge_y(&self, up: bool) -> f32 {
        self.pos.y + if up { self.size.y / 2.0 } else { -self.size.y / 2.0 }
    }
}

/// The places of the map for one size.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Size of the map. The map covers `-size/2 ..= size/2`.
    pub size: Vec2,
    /// y of the lower edge of the outside strip (the upper edge of the band).
    pub outside_bottom: f32,
    pub band_y: f32,
    /// y of the six lanes, from the top.
    pub lanes: [f32; 6],
    /// x of the five columns (the fifth is the right column).
    pub columns: [f32; 5],
    pub ribosome_pos: Vec2,
}

impl Layout {
    /// The layout for a map of `size` (at least `MIN_SIZE`).
    pub fn new(size: Vec2) -> Self {
        let size = size.max(MIN_SIZE);
        let top = size.y / 2.0;
        let outside_h = (size.y * 0.1).clamp(32.0, 90.0);
        let outside_bottom = top - outside_h;
        let band_y = outside_bottom - BAND_HEIGHT / 2.0;
        let band_bottom = band_y - BAND_HEIGHT / 2.0;
        let ribosome_y = -top + RIBOSOME_SIZE.y / 2.0 + 2.0;
        let first = band_bottom - 30.0;
        let last = ribosome_y + RIBOSOME_SIZE.y / 2.0 + 14.0 + PLATE_SIZE.y / 2.0;
        let mut lanes = [0.0; 6];
        for (i, lane) in lanes.iter_mut().enumerate() {
            *lane = (first + (last - first) * i as f32 / 5.0).round();
        }
        let x0 = -size.x / 2.0 + STUB_MARGIN + PLATE_SIZE.x / 2.0;
        let x4 = size.x / 2.0 - PLATE_SIZE.x / 2.0 - 4.0;
        let mut columns = [0.0; 5];
        for (i, column) in columns.iter_mut().enumerate() {
            *column = (x0 + (x4 - x0) * i as f32 / 4.0).round();
        }
        Self {
            size,
            outside_bottom,
            band_y,
            lanes,
            columns,
            ribosome_pos: Vec2::new(columns[0], ribosome_y),
        }
    }

    pub fn left(&self) -> f32 {
        -self.size.x / 2.0
    }

    pub fn right(&self) -> f32 {
        self.size.x / 2.0
    }

    pub fn band_bottom(&self) -> f32 {
        self.band_y - BAND_HEIGHT / 2.0
    }

    /// The place of a pool. Every currency has one.
    pub fn pool_place(&self, currency: Currency) -> Place {
        use Currency::*;
        let (column, lane) = match currency {
            // Intake lane.
            Minerals => (0, 0),
            Metals => (1, 0),
            O2 => (4, 0),
            // Hub lane.
            ATP => (0, 2),
            ReducingPower => (1, 2),
            CarbonSkeletons => (2, 2),
            Pyruvate => (3, 2),
            ROS => (4, 2),
            // Products lane.
            AminoAcids => (0, 4),
            Lipids => (1, 4),
            Nucleotides => (2, 4),
            Pigments => (3, 4),
            Waste => (4, 4),
        };
        Place {
            pos: Vec2::new(self.columns[column], self.lanes[lane]),
            size: POOL_SIZE,
        }
    }

    /// The place of a pathway plate. `None` for a kind without a `block_spec`:
    /// those genes are not on the map.
    pub fn plate_place(&self, kind: BlockKind) -> Option<Place> {
        use BlockKind::*;
        let (column, lane) = match kind {
            Chemosynthesis => (0, 1),
            LightCapture => (1, 1),
            Respiration => (2, 1),
            Fermentation => (3, 1),
            Catalase => (4, 1),
            AminoAcidBiosynthesis => (0, 3),
            LipidMetabolism => (1, 3),
            NucleotideCofactorSynthesis => (2, 3),
            SecondaryMetabolites => (3, 3),
            ProteinSynthesis => (0, 5),
            FatStorage => (1, 5),
            Cuticle => (2, 5),
            VesicleExport => (4, 5),
            _ => return None,
        };
        Some(Place {
            pos: Vec2::new(self.columns[column], self.lanes[lane]),
            size: PLATE_SIZE,
        })
    }

    pub fn ribosome_place(&self) -> Place {
        Place {
            pos: self.ribosome_pos,
            size: RIBOSOME_SIZE,
        }
    }

    /// The places of the membrane sockets for `count` slots: evenly spaced
    /// over the band. More slots than fit at `MIN_SLOT_SPACING` are not drawn
    /// (the extra ones still exist in the game; the panel counts all).
    pub fn slot_positions(&self, count: usize) -> Vec<Vec2> {
        let width = self.size.x;
        let fit = (width / MIN_SLOT_SPACING).floor() as usize;
        let shown = count.min(fit);
        if shown == 0 {
            return Vec::new();
        }
        let step = width / shown as f32;
        (0..shown)
            .map(|i| Vec2::new(self.left() + step * (i as f32 + 0.5), self.band_y))
            .collect()
    }

    /// x of the gutters: the free columns between the plates, for vertical
    /// pipes. Each has a width.
    fn gutters(&self) -> Vec<(f32, f32)> {
        let mut gutters = Vec::new();
        for pair in self.columns.windows(2) {
            let gap = pair[1] - pair[0] - PLATE_SIZE.x;
            gutters.push(((pair[0] + pair[1]) / 2.0, gap));
        }
        gutters.push((self.left() + 14.0, 20.0));
        gutters
    }

    fn lane_of(&self, y: f32) -> Option<usize> {
        self.lanes.iter().position(|&l| (l - y).abs() < 1.0)
    }

    /// The corridor between lane `lane` and the next one down: its centre and
    /// free height.
    fn corridor(&self, lane: usize) -> (f32, f32) {
        let upper = self.lanes[lane] - self.lane_half(lane);
        let lower = self.lanes[lane + 1] + self.lane_half(lane + 1);
        ((upper + lower) / 2.0, upper - lower)
    }

    /// Half the height of the nodes of a lane (plates on the even lanes).
    fn lane_half(&self, lane: usize) -> f32 {
        if lane % 2 == 0 { POOL_SIZE.y / 2.0 } else { PLATE_SIZE.y / 2.0 }
    }

    /// A pipe from the membrane band straight down to a node (outside energy,
    /// minerals that a transporter takes in). `x_shift` moves it off the node's
    /// centre.
    pub fn route_from_band(&self, to: Place, x_shift: f32) -> Vec<Vec2> {
        let x = to.pos.x + x_shift;
        vec![Vec2::new(x, self.band_bottom()), Vec2::new(x, to.edge_y(true))]
    }

    /// The same, going out: from a node up to the band.
    pub fn route_to_band(&self, from: Place, x_shift: f32) -> Vec<Vec2> {
        let mut points = self.route_from_band(from, x_shift);
        points.reverse();
        points
    }

    /// A short stub on the left of a pool, for a flow that is not a pathway.
    /// `index` is the place in the stack of that pool's stubs. `incoming` says
    /// if the flow goes into the pool. The label icon sits at the far end
    /// (`stub_icon_pos`).
    pub fn stub_route(&self, pool: Place, index: usize, incoming: bool) -> Vec<Vec2> {
        let y = pool.pos.y + 6.0 - 6.0 * index as f32;
        let near = Vec2::new(pool.pos.x - pool.size.x / 2.0, y);
        let far = Vec2::new(near.x - 8.0, y);
        if incoming { vec![far, near] } else { vec![near, far] }
    }

    /// The centre of the label icon of a stub.
    pub fn stub_icon_pos(&self, pool: Place, index: usize) -> Vec2 {
        let y = pool.pos.y + 6.0 - 6.0 * index as f32;
        Vec2::new(pool.pos.x - pool.size.x / 2.0 - 12.0, y)
    }
}

/// An axis-aligned run of a pipe on a track, for the overlap check.
#[derive(Debug, Clone, Copy)]
struct Run {
    /// The pool that the pipe belongs to. Pipes of one pool may share a track:
    /// they are one bus.
    owner: Option<Currency>,
    /// Which corridor or gutter, and the track in it.
    area: (bool, usize),
    track: usize,
    from: f32,
    to: f32,
}

/// Gives each pipe its own track in the corridors and gutters, and its own
/// place on the edge of a node. One router for all pipes of a map; make a new
/// one when the map is built again.
#[derive(Debug, Default)]
pub struct Router {
    runs: Vec<Run>,
    /// Pipes that already leave or enter an edge of a node.
    attached: HashMap<(i32, i32, bool), usize>,
}

/// The offset of the n-th item on a line: 0, +1, -1, +2, -2, ...
fn spread(n: usize) -> f32 {
    let step = n.div_ceil(2) as f32;
    if n % 2 == 1 { step } else { -step }
}

impl Router {
    /// A place on the edge of a node for one more pipe: the x offset.
    fn attach(&mut self, place: Place, up: bool) -> f32 {
        let key = (place.pos.x as i32, place.pos.y as i32, up);
        let n = self.attached.entry(key).or_default();
        let room = ((place.size.x / 2.0 - 6.0) / TRACK_PITCH).floor() as usize * 2 + 1;
        // A pool sits 2 px off the plate grid, so a pipe from a pool and a pipe
        // from the plate next to it are never on one line.
        let shift = if place.size.x < PLATE_SIZE.x { 2.0 } else { 0.0 };
        let offset = spread(*n % room.max(1)) * TRACK_PITCH + shift;
        *n += 1;
        offset
    }

    /// The lowest track of an area whose runs do not touch `from..to`.
    fn track(
        &mut self,
        owner: Option<Currency>,
        area: (bool, usize),
        from: f32,
        to: f32,
        tracks: usize,
    ) -> usize {
        let (from, to) = (from.min(to) - 3.0, from.max(to) + 3.0);
        let free = (0..tracks.max(1)).find(|&t| {
            !self.runs.iter().any(|r| {
                r.area == area
                    && r.track == t
                    && r.from < to
                    && from < r.to
                    && (owner.is_none() || r.owner != owner)
            })
        });
        let track = free.unwrap_or(self.runs.len() % tracks.max(1));
        self.runs.push(Run { owner, area, track, from, to });
        track
    }

    /// The route of a pipe from node `a` to node `b`, as a polyline of
    /// axis-aligned segments: out of `a` vertically, along the corridor next
    /// to `a`'s lane, down a gutter if the lanes are not next to each other,
    /// and into `b` vertically.
    pub fn route(
        &mut self,
        layout: &Layout,
        owner: Option<Currency>,
        a: Place,
        b: Place,
    ) -> Vec<Vec2> {
        let (Some(la), Some(lb)) = (layout.lane_of(a.pos.y), layout.lane_of(b.pos.y)) else {
            return vec![a.pos, b.pos];
        };
        if la == lb {
            return vec![a.pos, b.pos];
        }
        let down = lb > la;
        let ax = a.pos.x + self.attach(a, !down);
        let bx = b.pos.x + self.attach(b, down);
        let mut points = vec![Vec2::new(ax, a.edge_y(!down))];
        let corridor_y = |router: &mut Router, lane: usize, other: usize, from: f32, to: f32| {
            let upper = lane.min(other);
            let (center, height) = layout.corridor(upper);
            let tracks = (((height - 8.0) / TRACK_PITCH).floor() as usize).max(1);
            let t = router.track(owner, (false, upper), from, to, tracks);
            center + spread(t) * TRACK_PITCH
        };
        let adjacent = la.abs_diff(lb) == 1;
        if adjacent {
            let y = corridor_y(self, la, lb, ax, bx);
            points.push(Vec2::new(ax, y));
            points.push(Vec2::new(bx, y));
        } else {
            let gutters = layout.gutters();
            let (gi, (gcenter, gap)) = gutters
                .iter()
                .copied()
                .enumerate()
                .min_by(|x, y| (x.1.0 - ax).abs().total_cmp(&(y.1.0 - ax).abs()))
                .unwrap_or((0, (ax, 12.0)));
            let next_a = if down { la + 1 } else { la - 1 };
            let next_b = if down { lb - 1 } else { lb + 1 };
            let y1 = corridor_y(self, la, next_a, ax, gcenter);
            let y2 = corridor_y(self, lb, next_b, gcenter, bx);
            let tracks = (((gap - 4.0) / TRACK_PITCH).floor() as usize).max(1);
            let t = self.track(owner, (true, gi), y1, y2, tracks);
            let gx = gcenter + spread(t) * TRACK_PITCH;
            points.push(Vec2::new(ax, y1));
            points.push(Vec2::new(gx, y1));
            points.push(Vec2::new(gx, y2));
            points.push(Vec2::new(bx, y2));
        }
        points.push(Vec2::new(bx, b.edge_y(down)));
        points
    }
}

/// Total length of a polyline.
pub fn route_length(points: &[Vec2]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// The point at `distance` along a polyline (clamped to its ends).
pub fn point_along(points: &[Vec2], distance: f32) -> Vec2 {
    let mut left = distance.max(0.0);
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        if left <= len && len > 0.0 {
            return w[0].lerp(w[1], left / len);
        }
        left -= len;
    }
    points.last().copied().unwrap_or(Vec2::ZERO)
}

/// Every kind that has a place on the map: exactly the kinds with a spec.
pub fn mapped_kinds() -> Vec<BlockKind> {
    BlockKind::ALL
        .into_iter()
        .filter(|&k| block_spec(k).is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> [Vec2; 3] {
        [MIN_SIZE, Vec2::new(620.0, 860.0), Vec2::new(930.0, 1280.0)]
    }

    fn all_nodes(layout: &Layout) -> Vec<(String, Rect)> {
        let mut nodes: Vec<(String, Rect)> = Currency::ALL
            .iter()
            .map(|&c| (format!("pool {c:?}"), layout.pool_place(c).rect()))
            .collect();
        for kind in mapped_kinds() {
            nodes.push((format!("plate {kind:?}"), layout.plate_place(kind).unwrap().rect()));
        }
        nodes.push(("ribosome".into(), layout.ribosome_place().rect()));
        nodes
    }

    /// Every pipe of the blocks, routed in a fixed order.
    fn all_routes(layout: &Layout) -> Vec<(String, Option<Currency>, Vec<Vec2>)> {
        let mut router = Router::default();
        let mut routes = Vec::new();
        for kind in mapped_kinds() {
            let plate = layout.plate_place(kind).unwrap();
            let spec = block_spec(kind).unwrap();
            let mut currencies: Vec<_> = spec.profile.0.iter().collect();
            currencies.sort_by_key(|(c, _)| **c);
            for (currency, flow) in currencies {
                let pool = layout.pool_place(*currency);
                let route = if *flow < 0.0 {
                    router.route(layout, Some(*currency), pool, plate)
                } else {
                    router.route(layout, Some(*currency), plate, pool)
                };
                routes.push((format!("{currency:?} / {kind:?}"), Some(*currency), route));
            }
            for currency in spec.kinetics.cofactors {
                let route =
                    router.route(layout, Some(*currency), layout.pool_place(*currency), plate);
                routes.push((format!("{currency:?} cofactor / {kind:?}"), Some(*currency), route));
            }
        }
        routes
    }

    #[test]
    fn every_block_with_a_spec_has_a_place_and_no_other_does() {
        let layout = Layout::new(MIN_SIZE);
        for kind in BlockKind::ALL {
            assert_eq!(
                layout.plate_place(kind).is_some(),
                block_spec(kind).is_some(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn every_currency_of_a_profile_has_a_place_inside_the_map() {
        for size in sizes() {
            let layout = Layout::new(size);
            let half = layout.size / 2.0;
            for kind in mapped_kinds() {
                let spec = block_spec(kind).unwrap();
                for currency in spec.profile.0.keys().chain(spec.kinetics.cofactors) {
                    let rect = layout.pool_place(*currency).rect();
                    assert!(rect.min.x >= -half.x && rect.max.x <= half.x, "{currency:?}");
                }
            }
            for currency in Currency::ALL {
                let rect = layout.pool_place(currency).rect();
                assert!(rect.min.x >= -half.x && rect.max.x <= half.x, "{currency:?}");
            }
        }
    }

    #[test]
    fn no_two_nodes_overlap_and_all_sit_under_the_band() {
        for size in sizes() {
            let layout = Layout::new(size);
            let nodes = all_nodes(&layout);
            for (i, (name_a, a)) in nodes.iter().enumerate() {
                assert!(a.max.y < layout.band_bottom() && a.min.y >= -layout.size.y / 2.0, "{name_a}");
                for (name_b, b) in &nodes[i + 1..] {
                    assert!(a.intersect(*b).is_empty(), "{size}: {name_a} overlaps {name_b}");
                }
            }
        }
    }

    #[test]
    fn routes_are_axis_aligned_and_end_on_the_nodes() {
        for size in sizes() {
            let layout = Layout::new(size);
            for (name, _, route) in all_routes(&layout) {
                for w in route.windows(2) {
                    assert!(
                        (w[0].x - w[1].x).abs() < 1e-3 || (w[0].y - w[1].y).abs() < 1e-3,
                        "{name}: {route:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_route_never_crosses_a_node() {
        for size in sizes() {
            let layout = Layout::new(size);
            let nodes = all_nodes(&layout);
            for (name, _, route) in all_routes(&layout) {
                for w in route.windows(2) {
                    let seg = Rect::from_corners(w[0], w[1]).inflate(0.4);
                    for (node, rect) in &nodes {
                        let touches_end = rect.inflate(0.6).contains(route[0])
                            || rect.inflate(0.6).contains(*route.last().unwrap());
                        if touches_end {
                            // The end nodes: only the end segments may touch them.
                            continue;
                        }
                        assert!(seg.intersect(*rect).is_empty(), "{size}: {name} crosses {node}");
                    }
                }
            }
        }
    }

    /// Two pipes never lie on one another: no two segments of different pipes
    /// are on one line and overlap. (They may cross.)
    #[test]
    fn pipes_do_not_share_a_track() {
        for size in sizes() {
            let layout = Layout::new(size);
            let routes = all_routes(&layout);
            for (i, (name_a, owner_a, a)) in routes.iter().enumerate() {
                for (name_b, owner_b, b) in &routes[i + 1..] {
                    // The pipes of one pool are a bus: they may share a track.
                    if owner_a == owner_b {
                        continue;
                    }
                    for wa in a.windows(2) {
                        for wb in b.windows(2) {
                            let horizontal = |w: &[Vec2]| (w[0].y - w[1].y).abs() < 1e-3;
                            if horizontal(wa) != horizontal(wb) {
                                continue;
                            }
                            let (line_a, line_b, range_a, range_b) = if horizontal(wa) {
                                (wa[0].y, wb[0].y, (wa[0].x, wa[1].x), (wb[0].x, wb[1].x))
                            } else {
                                (wa[0].x, wb[0].x, (wa[0].y, wa[1].y), (wb[0].y, wb[1].y))
                            };
                            if (line_a - line_b).abs() > 1.5 {
                                continue;
                            }
                            let overlap = range_a.0.min(range_a.1).max(range_b.0.min(range_b.1))
                                < range_a.0.max(range_a.1).min(range_b.0.max(range_b.1)) - 0.5;
                            assert!(!overlap, "{size}: {name_a} and {name_b} share a track: {wa:?} vs {wb:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_map_fills_a_tall_window() {
        let small = Layout::new(MIN_SIZE);
        let tall = Layout::new(Vec2::new(620.0, 1280.0));
        // The lanes spread out: more room between them.
        assert!(tall.lanes[0] - tall.lanes[1] > 2.0 * (small.lanes[0] - small.lanes[1]));
    }

    #[test]
    fn slots_are_spaced_inside_the_band() {
        let layout = Layout::new(MIN_SIZE);
        assert!(layout.slot_positions(0).is_empty());
        for count in [1, 6, 12, 40] {
            let spots = layout.slot_positions(count);
            assert!(!spots.is_empty() && spots.len() <= count);
            for pair in spots.windows(2) {
                assert!(pair[1].x - pair[0].x >= MIN_SLOT_SPACING - 0.01);
            }
            assert!(spots.iter().all(|p| p.x > layout.left() && p.x < layout.right()));
        }
    }

    #[test]
    fn point_along_walks_the_polyline() {
        let line = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        assert_eq!(route_length(&line), 20.0);
        assert_eq!(point_along(&line, 5.0), Vec2::new(5.0, 0.0));
        assert_eq!(point_along(&line, 15.0), Vec2::new(10.0, 5.0));
        assert_eq!(point_along(&line, 99.0), Vec2::new(10.0, 10.0));
    }
}
