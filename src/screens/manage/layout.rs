//! The fixed map of the Manage screen: where each pool and pathway sits, and
//! how the pipes run. A pure table, with no Bevy systems, so it has tests.
//!
//! Units are sprite pixels, +y up. The map camera shows `VIEW_SIZE` pixels
//! (two screen pixels for each one at 1280x720). The map uses the left part
//! (`x < MAP_RIGHT`); the right column of the window holds the run inset and
//! the side panel (Bevy UI).
//!
//! Lanes from top to bottom (see `docs/metabolism-view.md`): the membrane
//! band, intake pools, energy pathways, hub pools, build pathways, products,
//! users, and the ribosome. Waste, ROS and O2 have their own column on the
//! right. Pipes run in the corridors between lanes and in the gutters between
//! columns, so they never cross a node.

use bevy::prelude::*;

use crate::blocks::block_spec;
use crate::blocks::genome::BlockKind;
use crate::molecules::Currency;

/// The size of the map camera view, in sprite pixels.
pub const VIEW_SIZE: Vec2 = Vec2::new(640.0, 360.0);
/// The map ends here on the right. The window right of it is for the inset and
/// the side panel.
pub const MAP_RIGHT: f32 = 140.0;
pub const MAP_LEFT: f32 = -VIEW_SIZE.x / 2.0;

/// Centre of the membrane band, and its height (the `wall_membrane` tile).
pub const BAND_Y: f32 = 162.0;
pub const BAND_HEIGHT: f32 = 26.0;
/// The lower edge of the band: the pipes from outside start here.
pub const BAND_BOTTOM: f32 = BAND_Y - BAND_HEIGHT / 2.0;
/// Width of one `wall_membrane` tile.
pub const BAND_TILE: f32 = 16.0;
/// The sockets are 24 px wide; they never come closer than this.
pub const MIN_SLOT_SPACING: f32 = 26.0;

/// Size of a pool node (icon, gauge and name) and of a pathway plate.
pub const POOL_SIZE: Vec2 = Vec2::new(24.0, 24.0);
pub const PLATE_SIZE: Vec2 = Vec2::new(64.0, 30.0);
pub const RIBOSOME_SIZE: Vec2 = Vec2::new(24.0, 24.0);

/// y of each lane.
const LANE_INTAKE: f32 = 128.0;
const LANE_ENERGY: f32 = 86.0;
const LANE_HUB: f32 = 44.0;
const LANE_BUILD: f32 = 2.0;
const LANE_PRODUCTS: f32 = -40.0;
const LANE_USERS: f32 = -82.0;
/// Lanes from top to bottom, for the pipe corridors.
const LANES: [f32; 6] = [
    LANE_INTAKE,
    LANE_ENERGY,
    LANE_HUB,
    LANE_BUILD,
    LANE_PRODUCTS,
    LANE_USERS,
];

/// x of the four columns of the left block, and of the right column.
const COLUMNS: [f32; 4] = [-270.0, -190.0, -110.0, -30.0];
const RIGHT_COLUMN: f32 = 86.0;

/// The ribosome sits under the first column, under the ProteinSynthesis plate.
pub const RIBOSOME_POS: Vec2 = Vec2::new(COLUMNS[0], -126.0);

/// A node of the map: its centre and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub pos: Vec2,
    pub size: Vec2,
}

impl Place {
    const fn new(x: f32, y: f32, size: Vec2) -> Self {
        Self {
            pos: Vec2::new(x, y),
            size,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_center_size(self.pos, self.size)
    }

    /// The edge where a pipe leaves or enters, going up (`up`) or down.
    pub fn edge_y(&self, up: bool) -> f32 {
        self.pos.y + if up { self.size.y / 2.0 } else { -self.size.y / 2.0 }
    }
}

/// The place of a pool. Every currency has one.
pub fn pool_place(currency: Currency) -> Place {
    use Currency::*;
    let (x, y) = match currency {
        // Intake lane.
        Minerals => (COLUMNS[0], LANE_INTAKE),
        Metals => (COLUMNS[1], LANE_INTAKE),
        O2 => (RIGHT_COLUMN, LANE_INTAKE),
        // Hub lane.
        ATP => (COLUMNS[0], LANE_HUB),
        ReducingPower => (COLUMNS[1], LANE_HUB),
        CarbonSkeletons => (COLUMNS[2], LANE_HUB),
        Pyruvate => (COLUMNS[3], LANE_HUB),
        ROS => (RIGHT_COLUMN, LANE_HUB),
        // Products lane.
        AminoAcids => (COLUMNS[0], LANE_PRODUCTS),
        Lipids => (COLUMNS[1], LANE_PRODUCTS),
        Nucleotides => (COLUMNS[2], LANE_PRODUCTS),
        Pigments => (COLUMNS[3], LANE_PRODUCTS),
        Waste => (RIGHT_COLUMN, LANE_PRODUCTS),
    };
    Place::new(x, y, POOL_SIZE)
}

/// The place of a pathway plate. `None` for a kind without a `block_spec`:
/// those genes are not on the map.
pub fn plate_place(kind: BlockKind) -> Option<Place> {
    use BlockKind::*;
    let (x, y) = match kind {
        Chemosynthesis => (COLUMNS[0], LANE_ENERGY),
        LightCapture => (COLUMNS[1], LANE_ENERGY),
        Respiration => (COLUMNS[2], LANE_ENERGY),
        Fermentation => (COLUMNS[3], LANE_ENERGY),
        Catalase => (RIGHT_COLUMN, LANE_ENERGY),
        AminoAcidBiosynthesis => (COLUMNS[0], LANE_BUILD),
        LipidMetabolism => (COLUMNS[1], LANE_BUILD),
        NucleotideCofactorSynthesis => (COLUMNS[2], LANE_BUILD),
        SecondaryMetabolites => (COLUMNS[3], LANE_BUILD),
        ProteinSynthesis => (COLUMNS[0], LANE_USERS),
        FatStorage => (COLUMNS[1], LANE_USERS),
        Cuticle => (COLUMNS[2], LANE_USERS),
        VesicleExport => (RIGHT_COLUMN, LANE_USERS),
        _ => return None,
    };
    Some(Place::new(x, y, PLATE_SIZE))
}

/// The ribosome.
pub fn ribosome_place() -> Place {
    Place {
        pos: RIBOSOME_POS,
        size: RIBOSOME_SIZE,
    }
}

/// Every kind that has a place on the map: exactly the kinds with a spec.
pub fn mapped_kinds() -> Vec<BlockKind> {
    BlockKind::ALL
        .into_iter()
        .filter(|&k| block_spec(k).is_some())
        .collect()
}

/// A short name for the label under a pool.
pub fn pool_label(currency: Currency) -> &'static str {
    match currency {
        Currency::ATP => "ATP",
        Currency::ReducingPower => "NADH",
        Currency::Pyruvate => "Pyr",
        Currency::CarbonSkeletons => "C",
        Currency::AminoAcids => "AA",
        Currency::Nucleotides => "NT",
        Currency::Lipids => "Lip",
        Currency::Metals => "Fe",
        Currency::Waste => "Waste",
        Currency::Minerals => "Min",
        Currency::Pigments => "Pig",
        Currency::O2 => "O2",
        Currency::ROS => "ROS",
    }
}

/// The places of the membrane sockets for `count` slots: evenly spaced over
/// the band. More slots than fit at `MIN_SLOT_SPACING` are not drawn (the
/// extra ones still exist in the game; the panel counts all).
pub fn slot_positions(count: usize) -> Vec<Vec2> {
    let width = MAP_RIGHT - MAP_LEFT;
    let fit = (width / MIN_SLOT_SPACING).floor() as usize;
    let shown = count.min(fit);
    if shown == 0 {
        return Vec::new();
    }
    let step = width / shown as f32;
    (0..shown)
        .map(|i| Vec2::new(MAP_LEFT + step * (i as f32 + 0.5), BAND_Y))
        .collect()
}

/// x of the gutters: the free columns between the nodes, for vertical pipes.
fn gutters() -> Vec<f32> {
    let mut xs = vec![MAP_LEFT + 10.0];
    for pair in COLUMNS.windows(2) {
        xs.push((pair[0] + pair[1]) / 2.0);
    }
    xs.push((COLUMNS[3] + RIGHT_COLUMN) / 2.0);
    xs.push(RIGHT_COLUMN + PLATE_SIZE.x / 2.0 + 12.0);
    xs
}

fn lane_of(y: f32) -> Option<usize> {
    LANES.iter().position(|&l| (l - y).abs() < 1.0)
}

/// y of the corridor next to lane `lane`, on the side `down`.
fn corridor(lane: usize, down: bool, jitter: f32) -> f32 {
    let other = if down { lane + 1 } else { lane - 1 };
    (LANES[lane] + LANES[other]) / 2.0 + jitter
}

/// The route of a pipe from node `a` to node `b`, as a polyline of axis-aligned
/// segments. `slot` (0 to 3) shifts the runs of different pipes apart. It goes
/// out of `a` vertically, runs in the corridor next to `a`'s lane, in a gutter
/// if the lanes are not next to each other, and enters `b` vertically.
pub fn route(a: Place, b: Place, slot: usize) -> Vec<Vec2> {
    let j = (slot % 4) as f32 - 1.5; // -1.5, -0.5, 0.5, 1.5
    let (Some(la), Some(lb)) = (lane_of(a.pos.y), lane_of(b.pos.y)) else {
        return vec![a.pos, b.pos];
    };
    if la == lb {
        // Two nodes of one lane: a straight run (not used by the map).
        return vec![a.pos, b.pos];
    }
    let down = lb > la;
    let ax = a.pos.x + j * 4.0;
    let bx = b.pos.x + j * 4.0;
    let mut points = vec![Vec2::new(ax, a.edge_y(!down))];
    let first = corridor(la, down, j * 3.0);
    if lb == la + 1 || la == lb + 1 {
        points.push(Vec2::new(ax, first));
        points.push(Vec2::new(bx, first));
    } else {
        // Several lanes apart: down a gutter.
        let gx = gutters()
            .into_iter()
            .min_by(|x, y| (x - ax).abs().total_cmp(&(y - ax).abs()))
            .unwrap_or(ax)
            + j * 3.0;
        let last = corridor(lb, !down, j * 3.0);
        points.push(Vec2::new(ax, first));
        points.push(Vec2::new(gx, first));
        points.push(Vec2::new(gx, last));
        points.push(Vec2::new(bx, last));
    }
    points.push(Vec2::new(bx, b.edge_y(down)));
    points
}

/// A pipe from the membrane band straight down to a node (outside energy,
/// minerals that a transporter takes in). `x_shift` moves it off the node's
/// centre. The route stops at the node's upper edge.
pub fn route_from_band(to: Place, x_shift: f32) -> Vec<Vec2> {
    let x = to.pos.x + x_shift;
    vec![Vec2::new(x, BAND_BOTTOM), Vec2::new(x, to.edge_y(true))]
}

/// The same, going out: from a node up to the band.
pub fn route_to_band(from: Place, x_shift: f32) -> Vec<Vec2> {
    let mut points = route_from_band(from, x_shift);
    points.reverse();
    points
}

/// A short stub on the left of a pool, for a flow that is not a pathway.
/// `index` is the place in the stack of that pool's stubs. `incoming` says if
/// the flow goes into the pool. The route has two points; the label icon sits
/// at the far end (`stub_icon_pos`).
pub fn stub_route(pool: Place, index: usize, incoming: bool) -> Vec<Vec2> {
    let y = pool.pos.y + 7.0 - 7.0 * index as f32;
    let near = Vec2::new(pool.pos.x - pool.size.x / 2.0, y);
    let far = Vec2::new(near.x - 16.0, y);
    if incoming { vec![far, near] } else { vec![near, far] }
}

/// The centre of the label icon of a stub.
pub fn stub_icon_pos(pool: Place, index: usize) -> Vec2 {
    let y = pool.pos.y + 7.0 - 7.0 * index as f32;
    Vec2::new(pool.pos.x - pool.size.x / 2.0 - 20.0, y)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn all_nodes() -> Vec<(String, Rect)> {
        let mut nodes: Vec<(String, Rect)> = Currency::ALL
            .iter()
            .map(|&c| (format!("pool {c:?}"), pool_place(c).rect()))
            .collect();
        for kind in mapped_kinds() {
            nodes.push((format!("plate {kind:?}"), plate_place(kind).unwrap().rect()));
        }
        nodes.push(("ribosome".into(), ribosome_place().rect()));
        nodes
    }

    #[test]
    fn every_block_with_a_spec_has_a_place_and_no_other_does() {
        for kind in BlockKind::ALL {
            assert_eq!(
                plate_place(kind).is_some(),
                block_spec(kind).is_some(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn every_currency_of_a_profile_has_a_place() {
        for kind in mapped_kinds() {
            let spec = block_spec(kind).unwrap();
            let kinetics_pools = spec.kinetics.cofactors.iter();
            for currency in spec.profile.0.keys().chain(kinetics_pools) {
                let place = pool_place(*currency);
                assert!(place.rect().size().min_element() > 0.0, "{currency:?}");
            }
        }
        for currency in Currency::ALL {
            let rect = pool_place(currency).rect();
            assert!(rect.min.x >= MAP_LEFT && rect.max.x <= MAP_RIGHT, "{currency:?}");
        }
    }

    #[test]
    fn no_two_nodes_overlap() {
        let nodes = all_nodes();
        for (i, (name_a, a)) in nodes.iter().enumerate() {
            for (name_b, b) in &nodes[i + 1..] {
                let overlap = !a.intersect(*b).is_empty();
                assert!(!overlap, "{name_a} overlaps {name_b}");
            }
        }
    }

    #[test]
    fn nodes_stay_inside_the_map_and_under_the_band() {
        for (name, rect) in all_nodes() {
            assert!(rect.min.x >= MAP_LEFT && rect.max.x <= MAP_RIGHT + 60.0, "{name}");
            assert!(rect.max.y < BAND_BOTTOM && rect.min.y > -VIEW_SIZE.y / 2.0, "{name}");
        }
    }

    #[test]
    fn routes_are_axis_aligned_and_end_on_the_nodes() {
        for kind in mapped_kinds() {
            let plate = plate_place(kind).unwrap();
            for currency in Currency::ALL {
                let pool = pool_place(currency);
                // Skip pairs in the same lane: the map has no such pipe.
                if (plate.pos.y - pool.pos.y).abs() < 1.0 {
                    continue;
                }
                for slot in 0..4 {
                    let points = route(pool, plate, slot);
                    for w in points.windows(2) {
                        assert!(
                            (w[0].x - w[1].x).abs() < 1e-3 || (w[0].y - w[1].y).abs() < 1e-3,
                            "{currency:?} -> {kind:?} slot {slot}: {points:?}"
                        );
                    }
                    let last = points.last().unwrap();
                    assert!(plate.rect().inflate(1.0).contains(*last), "{kind:?}");
                }
            }
        }
    }

    #[test]
    fn a_route_never_crosses_a_node() {
        let nodes = all_nodes();
        for kind in mapped_kinds() {
            let plate = plate_place(kind).unwrap();
            let spec = block_spec(kind).unwrap();
            for currency in spec.profile.0.keys() {
                let pool = pool_place(*currency);
                for slot in 0..4 {
                    let points = route(pool, plate, slot);
                    for w in points.windows(2) {
                        for (name, rect) in &nodes {
                            // The two end nodes are touched at the ends.
                            if rect.inflate(0.5).contains(points[0])
                                || rect.inflate(0.5).contains(*points.last().unwrap())
                            {
                                continue;
                            }
                            let seg = Rect::from_corners(w[0], w[1]).inflate(0.5);
                            assert!(
                                seg.intersect(*rect).is_empty(),
                                "{currency:?} -> {kind:?} crosses {name}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn slots_are_spaced_inside_the_band() {
        assert!(slot_positions(0).is_empty());
        for count in [1, 6, 12, 40] {
            let spots = slot_positions(count);
            assert!(!spots.is_empty() && spots.len() <= count);
            for pair in spots.windows(2) {
                assert!(pair[1].x - pair[0].x >= MIN_SLOT_SPACING - 0.01);
            }
            assert!(spots.iter().all(|p| p.x > MAP_LEFT && p.x < MAP_RIGHT));
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
