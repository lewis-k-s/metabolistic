//! # Environment shapes
//!
//! A shape is a free-form piece of the environment: rock, a membrane or a
//! microbial mat. It has a transform (`at`, `rotation` in degrees, `scale`)
//! and a geometry in local units. Tiles of a level file stay supported; a
//! shape does not need to follow the 32 px grid.
//!
//! ## Geometry
//!
//! | `type` | Fields | Meaning |
//! |---|---|---|
//! | `polygon` | `points` | A closed polygon (may be concave). |
//! | `boulder` | `radius`, `sides`, `seed` | An irregular convex rock. |
//! | `chimney` | `width`, `height`, `wall` | A U-shaped tube around the origin. The mouth is open towards +y before the rotation. |
//! | `ridge` | `points`, `thickness` | A polyline with a thickness. |
//! | `segment` | `points`, `thickness`, `pores` | A membrane polyline. `pores` are distances along it. |
//!
//! ## What the engine does with a shape
//!
//! Every geometry is cut into **convex parts** (`Shape::parts`, in world
//! units). The parts are used for three things:
//!
//! - **The grid.** `Shape::tiles` marks each tile whose centre is inside a part.
//!   Rock gives `WallKind::Rock` tiles, a mat gives `WallKind::Mat` tiles. The
//!   fields, `layout::validate` and the creature rules use these tiles.
//! - **Physics (rock).** The parts are the exact collider (see
//!   `world::shapes_view`), not the tiles.
//! - **Drawing.** The shaders draw the parts.
//!
//! A membrane shape gives only tiles (`Wall`, and `Pore` at the pores). Tiles
//! keep lysis and the pore rules working. The functions are pure.

use bevy::math::Vec2;
use serde::{Deserialize, Serialize};

use crate::envgrid::CELL_SIZE;
use crate::walls::WallKind;

type Point = [f32; 2];

/// What a shape is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    /// Solid rock. It cannot be lysed.
    Rock,
    /// Tissue wall, animated by a shader. Tiles in the grid, so it can be cut.
    Membrane,
    /// A microbial mat: no body, cells crawl through.
    Mat,
}

/// The geometry of a shape, in local units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Geometry {
    Polygon { points: Vec<Point> },
    Boulder { radius: f32, sides: u32, seed: u32 },
    Chimney { width: f32, height: f32, wall: f32 },
    Ridge { points: Vec<Point>, thickness: f32 },
    Segment {
        points: Vec<Point>,
        thickness: f32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pores: Vec<f32>,
    },
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}
fn one() -> Point {
    [1.0, 1.0]
}
fn is_one(v: &Point) -> bool {
    *v == [1.0, 1.0]
}

/// One shape of a level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub kind: ShapeKind,
    /// World position of the local origin.
    pub at: Point,
    /// Degrees, counter-clockwise.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: Point,
    pub geometry: Geometry,
}

fn v(p: &Point) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a - o).perp_dot(b - o)
}

fn signed_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    (0..n).map(|i| points[i].perp_dot(points[(i + 1) % n])).sum::<f32>() * 0.5
}

/// A small deterministic random number in [0, 1) from a seed and an index.
fn hash01(seed: u32, i: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B1) ^ i.wrapping_mul(0x85EB_CA6B).wrapping_add(0x27D4_EB2F);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// The convex hull, counter-clockwise (monotone chain).
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = points.to_vec();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut hull: Vec<Vec2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Ear clipping: the triangles of a simple polygon.
pub fn triangulate(points: &[Vec2]) -> Vec<[Vec2; 3]> {
    let mut idx: Vec<usize> = (0..points.len()).collect();
    if signed_area(points) < 0.0 {
        idx.reverse();
    }
    let mut out = Vec::new();
    let inside = |p: Vec2, a: Vec2, b: Vec2, c: Vec2| cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0;
    while idx.len() > 3 {
        let n = idx.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (points[idx[(i + n - 1) % n]], points[idx[i]], points[idx[(i + 1) % n]]);
            cross(a, b, c) > 1e-6 && !idx.iter().any(|&j| {
                let p = points[j];
                p != a && p != b && p != c && inside(p, a, b, c)
            })
        });
        let Some(i) = ear else {
            // A degenerate polygon: fan the rest.
            for k in 1..idx.len() - 1 {
                out.push([points[idx[0]], points[idx[k]], points[idx[k + 1]]]);
            }
            return out;
        };
        out.push([points[idx[(i + n - 1) % n]], points[idx[i]], points[idx[(i + 1) % n]]]);
        idx.remove(i);
    }
    if idx.len() == 3 {
        out.push([points[idx[0]], points[idx[1]], points[idx[2]]]);
    }
    out
}

/// A polyline with a thickness, as convex parts: one quad a segment and a
/// hexagon on each inner joint.
fn thick_line(points: &[Vec2], thickness: f32) -> Vec<Vec<Vec2>> {
    let half = thickness * 0.5;
    let mut parts = Vec::new();
    for w in points.windows(2) {
        let dir = (w[1] - w[0]).normalize_or_zero();
        let n = dir.perp() * half;
        parts.push(vec![w[0] - n, w[1] - n, w[1] + n, w[0] + n]);
    }
    for p in points.iter().skip(1).take(points.len().saturating_sub(2)) {
        parts.push((0..6).map(|k| *p + Vec2::from_angle(k as f32 * std::f32::consts::TAU / 6.0) * half).collect());
    }
    parts
}

impl Geometry {
    /// Convex parts in local units, counter-clockwise.
    pub fn local_parts(&self) -> Vec<Vec<Vec2>> {
        match self {
            Geometry::Polygon { points } => {
                let pts: Vec<Vec2> = points.iter().map(v).collect();
                triangulate(&pts).into_iter().map(|t| t.to_vec()).collect()
            }
            Geometry::Boulder { radius, sides, seed } => {
                let n = (*sides).max(5);
                let pts: Vec<Vec2> = (0..n)
                    .map(|k| {
                        let step = std::f32::consts::TAU / n as f32;
                        let angle = (k as f32 + (hash01(*seed, k) - 0.5) * 0.5) * step;
                        Vec2::from_angle(angle) * radius * (0.72 + 0.34 * hash01(*seed, k + 100))
                    })
                    .collect();
                vec![convex_hull(&pts)]
            }
            Geometry::Chimney { width, height, wall } => {
                let (hw, hh) = (width * 0.5, height * 0.5);
                let rect = |x0: f32, y0: f32, x1: f32, y1: f32| vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
                vec![
                    rect(-hw, -hh, -hw + wall, hh),
                    rect(hw - wall, -hh, hw, hh),
                    rect(-hw, -hh, hw, -hh + wall),
                ]
            }
            Geometry::Ridge { points, thickness } | Geometry::Segment { points, thickness, .. } => {
                let pts: Vec<Vec2> = points.iter().map(v).collect();
                thick_line(&pts, *thickness)
            }
        }
    }

    /// The local polyline of a segment or a ridge, if it has one.
    pub fn local_line(&self) -> Option<Vec<Vec2>> {
        match self {
            Geometry::Ridge { points, .. } | Geometry::Segment { points, .. } => Some(points.iter().map(v).collect()),
            _ => None,
        }
    }
}

impl Shape {
    /// Local point to world point.
    pub fn to_world(&self, local: Vec2) -> Vec2 {
        let rot = Vec2::from_angle(self.rotation.to_radians());
        v(&self.at) + rot.rotate(local * v(&self.scale))
    }

    /// The convex parts in world units, counter-clockwise.
    pub fn parts(&self) -> Vec<Vec<Vec2>> {
        self.geometry
            .local_parts()
            .into_iter()
            .map(|part| {
                let mut world: Vec<Vec2> = part.into_iter().map(|p| self.to_world(p)).collect();
                if signed_area(&world) < 0.0 {
                    world.reverse();
                }
                world
            })
            .filter(|p| p.len() >= 3)
            .collect()
    }

    /// The polyline in world units, if the geometry has one.
    pub fn line(&self) -> Option<Vec<Vec2>> {
        self.geometry.local_line().map(|l| l.into_iter().map(|p| self.to_world(p)).collect())
    }

    /// True if a world point is inside any part.
    pub fn contains(&self, point: Vec2) -> bool {
        self.parts().iter().any(|part| inside_convex(part, point))
    }

    /// The tiles of the shape on a centred grid of `width` x `height` cells.
    /// See the module docs.
    pub fn tiles(&self, width: usize, height: usize) -> Vec<((usize, usize), WallKind)> {
        let origin = -Vec2::new(width as f32, height as f32) * CELL_SIZE * 0.5;
        let tile_of = |p: Vec2| {
            let t = ((p - origin) / CELL_SIZE).floor();
            (t.x >= 0.0 && t.y >= 0.0 && (t.x as usize) < width && (t.y as usize) < height).then(|| (t.x as usize, t.y as usize))
        };
        if self.kind == ShapeKind::Membrane {
            return self.membrane_tiles(&tile_of);
        }
        let kind = if self.kind == ShapeKind::Rock { WallKind::Rock } else { WallKind::Mat };
        let parts = self.parts();
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in parts.iter().flatten() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let mut out = Vec::new();
        if lo.x > hi.x {
            return out;
        }
        let (t0, t1) = (((lo - origin) / CELL_SIZE).floor(), ((hi - origin) / CELL_SIZE).ceil());
        for ty in (t0.y.max(0.0) as usize)..=(t1.y.max(0.0) as usize).min(height.saturating_sub(1)) {
            for tx in (t0.x.max(0.0) as usize)..=(t1.x.max(0.0) as usize).min(width.saturating_sub(1)) {
                let centre = origin + (Vec2::new(tx as f32, ty as f32) + 0.5) * CELL_SIZE;
                if parts.iter().any(|part| inside_convex(part, centre)) {
                    out.push(((tx, ty), kind));
                }
            }
        }
        out
    }

    fn membrane_tiles(&self, tile_of: &dyn Fn(Vec2) -> Option<(usize, usize)>) -> Vec<((usize, usize), WallKind)> {
        let Some(line) = self.line() else { return Vec::new() };
        let pore_at: Vec<f32> = match &self.geometry {
            Geometry::Segment { pores, .. } => pores.iter().map(|d| d * self.scale[0].abs().max(self.scale[1].abs())).collect(),
            _ => Vec::new(),
        };
        let mut out: Vec<((usize, usize), WallKind)> = Vec::new();
        let mut walked = 0.0;
        for w in line.windows(2) {
            let len = w[0].distance(w[1]);
            let steps = (len / 8.0).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let d = walked + len * s as f32 / steps as f32;
                let Some(tile) = tile_of(w[0].lerp(w[1], s as f32 / steps as f32)) else { continue };
                let pore = pore_at.iter().any(|p| (p - d).abs() < CELL_SIZE * 0.5);
                match out.iter_mut().find(|(t, _)| *t == tile) {
                    Some(entry) => {
                        if pore {
                            entry.1 = WallKind::Pore;
                        }
                    }
                    None => out.push((tile, if pore { WallKind::Pore } else { WallKind::Wall })),
                }
            }
            walked += len;
        }
        out
    }
}

/// Depth that marks an edge as inner: no outline there.
pub const INNER_DEPTH: f32 = 1.0e4;

/// One triangle for the shaders: a fan triangle `[a, b, centre]` of a part.
/// `depth` is the distance in pixels to the outer edge `ab` at each corner
/// (0 at `a` and `b`, the distance of the centre at the centre; `INNER_DEPTH`
/// if the edge is covered by another part, so no outline is drawn there).
/// `normal` is the face normal of a low roof over the part: the centre is
/// raised, so each triangle faces a different way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facet {
    pub points: [Vec2; 3],
    pub depth: [f32; 3],
    pub normal: [f32; 3],
}

impl Shape {
    /// The facets of all parts, for the rock and mat shaders.
    pub fn facets(&self) -> Vec<Facet> {
        let parts = self.parts();
        let mut out = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let centre = part.iter().copied().sum::<Vec2>() / part.len() as f32;
            let n = part.len();
            for i in 0..n {
                let (a, b) = (part[i], part[(i + 1) % n]);
                let edge = b - a;
                let len = edge.length();
                if len < 1e-3 {
                    continue;
                }
                let outward = Vec2::new(edge.y, -edge.x) / len;
                let d = ((centre - a).dot(-outward)).max(0.01);
                let probe = (a + b) * 0.5 + outward * 0.75;
                let covered = parts.iter().enumerate().any(|(j, other)| j != index && inside_convex(other, probe));
                let height = d * 0.85;
                let normal = bevy::math::Vec3::new(edge.x, edge.y, 0.0).cross(bevy::math::Vec3::new(centre.x - a.x, centre.y - a.y, height)).normalize_or_zero();
                out.push(Facet {
                    points: [a, b, centre],
                    depth: [0.0, 0.0, if covered { INNER_DEPTH } else { d }],
                    normal: normal.to_array(),
                });
            }
        }
        out
    }
}

/// True if `point` is inside or on a convex counter-clockwise polygon.
pub fn inside_convex(part: &[Vec2], point: Vec2) -> bool {
    let n = part.len();
    (0..n).all(|i| cross(part[i], part[(i + 1) % n], point) >= -1e-4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(kind: ShapeKind, geometry: Geometry) -> Shape {
        Shape { kind, at: [0.0, 0.0], rotation: 0.0, scale: [1.0, 1.0], geometry }
    }

    #[test]
    fn a_square_polygon_gives_two_triangles_that_cover_it() {
        let sq = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let tris = triangulate(&sq);
        assert_eq!(tris.len(), 2);
        let area: f32 = tris.iter().map(|t| signed_area(t).abs()).sum();
        assert!((area - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_concave_polygon_is_covered_by_convex_parts() {
        let l = Geometry::Polygon { points: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 10.0], [10.0, 10.0], [10.0, 40.0], [0.0, 40.0]] };
        let s = shape(ShapeKind::Rock, l);
        assert!(s.contains(Vec2::new(5.0, 30.0)));
        assert!(s.contains(Vec2::new(30.0, 5.0)));
        assert!(!s.contains(Vec2::new(30.0, 30.0)), "the notch of the L is empty");
    }

    #[test]
    fn a_chimney_is_a_u_that_opens_towards_plus_y() {
        let mut s = shape(ShapeKind::Rock, Geometry::Chimney { width: 100.0, height: 100.0, wall: 20.0 });
        assert!(s.contains(Vec2::new(-45.0, 0.0)) && s.contains(Vec2::new(45.0, 0.0)), "side walls");
        assert!(s.contains(Vec2::new(0.0, -45.0)), "the bottom is closed");
        assert!(!s.contains(Vec2::new(0.0, 20.0)), "the inside is empty");
        s.rotation = 90.0; // the mouth now points to -x
        assert!(s.contains(Vec2::new(45.0, 0.0)), "the closed end is on the +x side");
        assert!(!s.contains(Vec2::new(-45.0, 0.0)), "the mouth is open");
    }

    #[test]
    fn a_boulder_is_convex_and_the_same_for_the_same_seed() {
        let g = Geometry::Boulder { radius: 60.0, sides: 9, seed: 4 };
        assert_eq!(g.local_parts(), g.local_parts());
        let part = &g.local_parts()[0];
        assert!(part.len() >= 5);
        assert!(signed_area(part) > 0.0);
        assert_ne!(g.local_parts(), Geometry::Boulder { radius: 60.0, sides: 9, seed: 5 }.local_parts());
    }

    #[test]
    fn rock_tiles_follow_the_centre_rule_and_rotation_moves_them() {
        let g = Geometry::Polygon { points: vec![[-100.0, -20.0], [100.0, -20.0], [100.0, 20.0], [-100.0, 20.0]] };
        let flat = shape(ShapeKind::Rock, g.clone()).tiles(64, 64);
        let mut up = shape(ShapeKind::Rock, g);
        up.rotation = 90.0;
        let tall = up.tiles(64, 64);
        assert!(flat.iter().all(|(_, k)| *k == WallKind::Rock));
        let spread = |t: &[((usize, usize), WallKind)]| {
            let xs: Vec<_> = t.iter().map(|((x, _), _)| *x).collect();
            xs.iter().max().unwrap() - xs.iter().min().unwrap()
        };
        assert!(spread(&flat) > spread(&tall));
        assert!(!flat.is_empty() && !tall.is_empty());
    }

    #[test]
    fn a_membrane_segment_gives_a_connected_line_with_pores() {
        let mut s = shape(ShapeKind::Membrane, Geometry::Segment { points: vec![[-200.0, 0.0], [200.0, 0.0]], thickness: 16.0, pores: vec![200.0] });
        s.at = [16.0, 16.0];
        let tiles = s.tiles(64, 64);
        assert!(tiles.len() >= 12);
        assert_eq!(tiles.iter().filter(|(_, k)| *k == WallKind::Pore).count(), 1);
        assert!(tiles.iter().all(|((_, y), _)| *y == tiles[0].0.1));
    }

    #[test]
    fn facets_face_different_ways_and_inner_edges_have_no_outline() {
        let s = shape(ShapeKind::Rock, Geometry::Ridge { points: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], thickness: 20.0 });
        let facets = s.facets();
        assert!(facets.len() >= 12);
        assert!(facets.iter().all(|f| f.normal[2] > 0.0), "the roof faces up");
        assert!(facets.iter().any(|f| f.depth[2] >= INNER_DEPTH), "edges inside the union are inner");
        assert!(facets.iter().any(|f| f.depth[2] < 20.0), "outer edges keep their depth");
        let first = facets[0].normal;
        assert!(facets.iter().any(|f| (f.normal[0] - first[0]).abs() > 0.1 || (f.normal[1] - first[1]).abs() > 0.1));
    }

    #[test]
    fn serde_round_trip_keeps_the_defaults_out() {
        let s = shape(ShapeKind::Mat, Geometry::Ridge { points: vec![[0.0, 0.0], [10.0, 0.0]], thickness: 8.0 });
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("rotation") && !text.contains("scale"));
        assert_eq!(serde_json::from_str::<Shape>(&text).unwrap(), s);
    }
}
