//! # Genome map
//!
//! The layout of the genome view of the codex (`screens::codex`). Pure data and
//! geometry, with no Bevy systems, so it has tests.
//!
//! The genome is one circular chromosome, as in a bacterium. It is folded into
//! rows, as the nucleoid folds its loop: row 0 runs to the right, a half turn
//! takes the strand down, row 1 runs to the left, and so on. A return path (the
//! spine) on the left closes the loop. The origin of replication (`ori`) sits
//! on the spine and the terminus (`ter`) on the right turn halfway down.
//!
//! Each codex entry is a locus on a row: a gene, or the gene of a membrane
//! protein. Entries of one category are neighbours, like an operon, with a
//! linker between categories. The product icon stands above the locus and the
//! gene symbol is below it.
//!
//! Units are sprite pixels, +y up, with the centre of the rows at the origin.
//! The strand has an arc length `s` that runs once round the loop. The wobble
//! (`wobble`) and the helix phase (`helix_phase`) are periodic in `s`, so the
//! loop has no seam. The shader `assets/shaders/genome.wgsl` uses the same
//! segments and the same formulas.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::math::{Rect, Vec2};

use crate::blocks::genome::GeneState;
use crate::codex_catalog::{Category, CodexEntry, EntryId, EntryState, all_entries};

/// Strand length of one locus.
pub const LOCUS_LEN: f32 = 26.0;
/// Linker between two operons (categories) on one row.
pub const OPERON_GAP: f32 = 12.0;
/// Linker at each end of a row, before the turn.
pub const END_PAD: f32 = 6.0;
/// Distance between two rows. The turns are half circles of half this.
pub const ROW_PITCH: f32 = 64.0;
pub const TURN_RADIUS: f32 = ROW_PITCH / 2.0;
/// Space between the turns on the left and the spine.
pub const SPINE_GAP: f32 = 14.0;
/// The bottom of an icon, above the strand centre.
pub const ICON_RISE: f32 = 6.0;
/// The tallest icon (a membrane protein that points up).
pub const ICON_MAX_HEIGHT: f32 = 32.0;
/// The centre of the gene symbol, below the strand centre.
pub const SYMBOL_DROP: f32 = 10.0;
/// The operon bar, below the strand centre, and its name below the bar.
pub const OPERON_DROP: f32 = 16.0;
pub const OPERON_NAME_DROP: f32 = 22.0;
/// Free space round the drawing.
pub const MARGIN: f32 = 12.0;
/// Half the height of the helix: each backbone is this far from the centre.
pub const HELIX_AMPLITUDE: f32 = 3.0;
/// Strand length of one helix turn (rounded so the loop has whole turns).
pub const HELIX_PITCH: f32 = 13.0;
/// Turns of the helix per second (the DNA turns slowly in place).
pub const HELIX_SPIN: f32 = 0.35;
/// The wobble: two travelling waves along the strand, in pixels.
pub const WOBBLE_AMPLITUDE: [f32; 2] = [1.3, 0.9];
/// Wave length of the short wave (rounded so the loop has whole waves).
pub const WOBBLE_WAVE: f32 = 120.0;
/// Angular speed of each wave, radians per second.
pub const WOBBLE_SPEED: [f32; 2] = [0.9, 0.45];
/// The most loci and segments that the shader takes.
pub const MAX_LOCI: usize = 40;
pub const MAX_SEGMENTS: usize = 16;
/// The row counts that the layout can use: even, so the last row ends on the
/// left, next to the spine.
pub const ROW_CHOICES: [usize; 3] = [2, 4, 6];

/// A piece of the strand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Line { from: Vec2, to: Vec2 },
    /// `start` is the angle of the first point; `sweep` is signed (+ is
    /// counterclockwise).
    Arc { centre: Vec2, radius: f32, start: f32, sweep: f32 },
}

impl Segment {
    pub fn length(&self) -> f32 {
        match *self {
            Segment::Line { from, to } => from.distance(to),
            Segment::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }

    /// The point at distance `u` from the start.
    pub fn point(&self, u: f32) -> Vec2 {
        match *self {
            Segment::Line { from, to } => from + (to - from).normalize_or_zero() * u,
            Segment::Arc { centre, radius, start, sweep } => {
                let a = start + sweep.signum() * u / radius;
                centre + radius * Vec2::new(a.cos(), a.sin())
            }
        }
    }

    /// The left normal (the direction of travel turned by +90 degrees) at `u`.
    pub fn normal(&self, u: f32) -> Vec2 {
        let tangent = match *self {
            Segment::Line { from, to } => (to - from).normalize_or_zero(),
            Segment::Arc { radius, start, sweep, .. } => {
                let a = start + sweep.signum() * u / radius;
                sweep.signum() * Vec2::new(-a.sin(), a.cos())
            }
        };
        tangent.perp()
    }

    /// The nearest point of the segment to `p`: its distance from the start
    /// and the signed distance of `p` along the left normal.
    pub fn closest(&self, p: Vec2) -> (f32, f32) {
        let u = match *self {
            Segment::Line { from, to } => {
                let d = (to - from).normalize_or_zero();
                (p - from).dot(d).clamp(0.0, from.distance(to))
            }
            Segment::Arc { centre, radius, start, sweep } => {
                let a = (p.y - centre.y).atan2(p.x - centre.x);
                let along = if sweep > 0.0 { (a - start).rem_euclid(TAU) } else { (start - a).rem_euclid(TAU) };
                if along <= sweep.abs() {
                    along * radius
                } else {
                    // Outside the arc: the nearer end.
                    let len = self.length();
                    if p.distance(self.point(0.0)) <= p.distance(self.point(len)) { 0.0 } else { len }
                }
            }
        };
        (u, (p - self.point(u)).dot(self.normal(u)))
    }
}

/// One entry on the genome.
#[derive(Debug, Clone, PartialEq)]
pub struct Locus {
    pub id: EntryId,
    pub category: Category,
    pub row: usize,
    /// The x range on the row (left to right on the screen).
    pub x0: f32,
    pub x1: f32,
    /// The y of the row.
    pub y: f32,
    /// The arc-length range on the strand (`s0 < s1`).
    pub s0: f32,
    pub s1: f32,
}

impl Locus {
    pub fn mid_s(&self) -> f32 {
        (self.s0 + self.s1) / 2.0
    }
}

/// The loci of one category on one row.
#[derive(Debug, Clone, PartialEq)]
pub struct Operon {
    pub category: Category,
    pub row: usize,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// The category goes on from the row above: its name is shown there.
    pub continued: bool,
}

/// The whole drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct GenomeLayout {
    pub rows: usize,
    /// Each segment with the arc length at its start.
    pub segments: Vec<(f32, Segment)>,
    /// Arc length of the whole loop.
    pub length: f32,
    pub loci: Vec<Locus>,
    pub operons: Vec<Operon>,
    pub ori: Vec2,
    pub ter: Vec2,
    /// Everything that is drawn, with the margin.
    pub bounds: Rect,
    /// Whole waves of the short wobble round the loop.
    pub waves: f32,
    /// Whole helix turns round the loop.
    pub turns: f32,
}

/// The entries in genome order: by category (screen order), then catalog order.
pub fn genome_order() -> Vec<CodexEntry> {
    let entries = all_entries();
    Category::ALL
        .into_iter()
        .flat_map(|c| entries.iter().filter(move |e| e.category == c).cloned())
        .collect()
}

/// The length of a row of loci: the loci and the linkers between categories.
fn row_length(row: &[(EntryId, Category)]) -> f32 {
    let gaps = row.windows(2).filter(|w| w[0].1 != w[1].1).count();
    row.len() as f32 * LOCUS_LEN + gaps as f32 * OPERON_GAP
}

/// Splits the entries into `rows` rows of nearly equal count.
fn split_rows(entries: &[(EntryId, Category)], rows: usize) -> Vec<Vec<(EntryId, Category)>> {
    let per_row = entries.len().div_ceil(rows.max(1)).max(1);
    let mut out: Vec<Vec<_>> = entries.chunks(per_row).map(|c| c.to_vec()).collect();
    out.resize(rows, Vec::new());
    out
}

impl GenomeLayout {
    /// The layout of `entries` (in genome order) on `rows` rows (even, 2 or
    /// more).
    pub fn new(entries: &[(EntryId, Category)], rows: usize) -> Self {
        let rows = (rows.max(2) / 2) * 2;
        let split = split_rows(entries, rows);
        let longest = split.iter().map(|r| row_length(r)).fold(0.0, f32::max);
        let half = longest / 2.0 + END_PAD;
        let row_y = |k: usize| ((rows - 1) as f32 / 2.0 - k as f32) * ROW_PITCH;

        // The strand: rows and turns, then the closing path on the left.
        let mut pieces = Vec::new();
        for k in 0..rows {
            let y = row_y(k);
            let rightward = k % 2 == 0;
            let (a, b) = if rightward { (-half, half) } else { (half, -half) };
            pieces.push(Segment::Line { from: Vec2::new(a, y), to: Vec2::new(b, y) });
            if k + 1 < rows {
                let mid = Vec2::new(b, y - ROW_PITCH / 2.0);
                // A right turn goes clockwise, a left turn counterclockwise.
                let sweep = if rightward { -PI } else { PI };
                pieces.push(Segment::Arc { centre: mid, radius: TURN_RADIUS, start: FRAC_PI_2, sweep });
            }
        }
        let (top, bottom) = (row_y(0), row_y(rows - 1));
        let left = Vec2::new(-half, 0.0);
        let ori;
        let spine_x;
        if rows == 2 {
            // A half turn closes the loop: a stadium.
            let centre = Vec2::new(-half, (top + bottom) / 2.0);
            pieces.push(Segment::Arc { centre, radius: TURN_RADIUS, start: -FRAC_PI_2, sweep: -PI });
            ori = centre - Vec2::new(TURN_RADIUS, 0.0);
            spine_x = -half - TURN_RADIUS;
        } else {
            // A quarter turn, the spine up, a quarter turn back to row 0.
            let q = TURN_RADIUS + SPINE_GAP;
            let low = Vec2::new(left.x, bottom + q);
            let high = Vec2::new(left.x, top - q);
            pieces.push(Segment::Arc { centre: low, radius: q, start: -FRAC_PI_2, sweep: -FRAC_PI_2 });
            pieces.push(Segment::Line { from: Vec2::new(-half - q, low.y), to: Vec2::new(-half - q, high.y) });
            pieces.push(Segment::Arc { centre: high, radius: q, start: PI, sweep: -FRAC_PI_2 });
            ori = Vec2::new(-half - q, (low.y + high.y) / 2.0);
            spine_x = -half - q;
        }
        let mut segments = Vec::new();
        let mut s = 0.0;
        for piece in pieces {
            segments.push((s, piece));
            s += piece.length();
        }
        let length = s;
        // The terminus: on the right turn halfway down.
        // A right turn follows an even row.
        let ter_row = (rows / 2 - 1) / 2 * 2;
        let ter = Vec2::new(half + TURN_RADIUS, row_y(ter_row) - ROW_PITCH / 2.0);

        // The loci. Rows read left to right on the screen. A row that runs
        // left has its arc length growing to the left.
        let mut loci = Vec::new();
        let mut operons: Vec<Operon> = Vec::new();
        for (k, row) in split.iter().enumerate() {
            let y = row_y(k);
            let line_s = segments.iter().filter(|(_, seg)| matches!(seg, Segment::Line { .. })).nth(k).map_or(0.0, |(s, _)| *s);
            let rightward = k % 2 == 0;
            let mut x = -row_length(row) / 2.0;
            let mut last: Option<Category> = None;
            for &(id, category) in row {
                if last.is_some_and(|c| c != category) {
                    x += OPERON_GAP;
                }
                let (x0, x1) = (x, x + LOCUS_LEN);
                let (s0, s1) = if rightward {
                    (line_s + x0 + half, line_s + x1 + half)
                } else {
                    (line_s + half - x1, line_s + half - x0)
                };
                loci.push(Locus { id, category, row: k, x0, x1, y, s0, s1 });
                match operons.last_mut() {
                    Some(op) if op.row == k && op.category == category => op.x1 = x1,
                    _ => {
                        let continued = operons.last().is_some_and(|op| op.category == category);
                        operons.push(Operon { category, row: k, x0, x1, y, continued });
                    }
                }
                last = Some(category);
                x = x1;
            }
        }

        let min = Vec2::new(spine_x - HELIX_AMPLITUDE - 2.0, bottom - OPERON_NAME_DROP - 6.0) - Vec2::splat(MARGIN);
        let max = Vec2::new(half + TURN_RADIUS + HELIX_AMPLITUDE + 2.0, top + ICON_RISE + ICON_MAX_HEIGHT) + Vec2::splat(MARGIN);
        Self {
            rows,
            segments,
            length,
            loci,
            operons,
            ori,
            ter,
            bounds: Rect::from_corners(min.floor(), max.ceil()),
            waves: (length / WOBBLE_WAVE).round().max(1.0),
            turns: (length / HELIX_PITCH).round().max(1.0),
        }
    }

    /// The layout of every codex entry.
    pub fn of_catalog(rows: usize) -> Self {
        let entries: Vec<_> = genome_order().iter().map(|e| (e.id, e.category)).collect();
        Self::new(&entries, rows)
    }

    /// The segment that holds arc length `s` (wrapped round the loop), and the
    /// distance into it.
    fn segment_at(&self, s: f32) -> (&Segment, f32) {
        let s = s.rem_euclid(self.length);
        let i = self.segments.iter().rposition(|(s0, _)| *s0 <= s).unwrap_or(0);
        let (s0, seg) = &self.segments[i];
        (seg, s - s0)
    }

    /// The point of the strand centre at arc length `s`, at rest.
    pub fn point(&self, s: f32) -> Vec2 {
        let (seg, u) = self.segment_at(s);
        seg.point(u)
    }

    /// The left normal at arc length `s`.
    pub fn normal(&self, s: f32) -> Vec2 {
        let (seg, u) = self.segment_at(s);
        seg.normal(u)
    }

    /// The wobble at arc length `s` and time `t`: an offset along the left
    /// normal, in pixels.
    pub fn wobble(&self, s: f32, t: f32) -> f32 {
        let x = s / self.length;
        WOBBLE_AMPLITUDE[0] * (TAU * self.waves * x - WOBBLE_SPEED[0] * t).sin()
            + WOBBLE_AMPLITUDE[1] * (TAU * 2.0 * x + WOBBLE_SPEED[1] * t).sin()
    }

    /// The helix phase at arc length `s` and time `t`, in radians.
    pub fn helix_phase(&self, s: f32, t: f32) -> f32 {
        TAU * (self.turns * s / self.length + HELIX_SPIN * t)
    }

    /// The strand centre at arc length `s`, moved by the wobble.
    pub fn strand_point(&self, s: f32, t: f32) -> Vec2 {
        self.point(s) + self.normal(s) * self.wobble(s, t)
    }

    /// The nearest point of the strand at rest: arc length and signed distance
    /// along the left normal.
    pub fn closest(&self, p: Vec2) -> (f32, f32) {
        self.segments
            .iter()
            .map(|(s0, seg)| {
                let (u, d) = seg.closest(p);
                (s0 + u, d, p.distance(seg.point(u)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map_or((0.0, f32::INFINITY), |(s, d, _)| (s, d))
    }

    /// Where the product icon of locus `i` stands (the middle of its bottom
    /// edge) at time `t`. It moves with the strand.
    pub fn icon_foot(&self, i: usize, t: f32) -> Vec2 {
        let locus = &self.loci[i];
        let s = locus.mid_s();
        Vec2::new((locus.x0 + locus.x1) / 2.0, self.strand_point(s, t).y + ICON_RISE)
    }

    /// The locus under the point `p` at time `t`: the column of the locus,
    /// from the gene symbol up to the top of the icon.
    pub fn pick(&self, p: Vec2, t: f32) -> Option<usize> {
        self.loci.iter().enumerate().find_map(|(i, locus)| {
            let y = self.strand_point(locus.mid_s(), t).y;
            let inside = p.x >= locus.x0 && p.x < locus.x1 && p.y >= y - SYMBOL_DROP - 5.0 && p.y <= y + ICON_RISE + ICON_MAX_HEIGHT;
            inside.then_some(i)
        })
    }

    /// The index of the locus of an entry.
    pub fn locus_of(&self, id: EntryId) -> Option<usize> {
        self.loci.iter().position(|l| l.id == id)
    }
}

/// The row count that shows the genome largest in an area of `area` pixels.
pub fn best_rows(area: Vec2) -> usize {
    ROW_CHOICES
        .into_iter()
        .max_by(|a, b| fit_scale(*a, area).total_cmp(&fit_scale(*b, area)))
        .unwrap_or(4)
}

/// The scale at which the catalog layout on `rows` rows fills `area`.
pub fn fit_scale(rows: usize, area: Vec2) -> f32 {
    let size = GenomeLayout::of_catalog(rows).bounds.size();
    (area / size).min_element()
}

// --- How a locus looks ---

/// The colour ramp of a locus. The shader takes the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// DNA between genes.
    Linker = 0,
    Expressed = 1,
    Enhanced = 2,
    /// Damaged by UV: half rate.
    Mutated = 3,
    /// In the genome, switched off.
    Silent = 4,
    /// In the codex but not in this genome: a ghost of the sequence.
    InCodex = 5,
    /// Not found yet.
    Unknown = 6,
    /// The gene of a membrane protein.
    Protein = 7,
}

impl Look {
    pub const COUNT: usize = 8;
}

/// The look of a locus. A membrane protein is always in the genome (the cell
/// can make it from the start).
pub fn look(id: EntryId, state: &EntryState) -> Look {
    match (id, &state.gene) {
        (EntryId::Protein(_), _) => Look::Protein,
        (_, Some(GeneState::Expressed)) => Look::Expressed,
        (_, Some(GeneState::Enhanced)) => Look::Enhanced,
        (_, Some(GeneState::Mutated)) => Look::Mutated,
        (_, Some(GeneState::Silent)) => Look::Silent,
        (_, None) if state.in_codex => Look::InCodex,
        (_, None) => Look::Unknown,
    }
}

/// Bits of the flags of a locus in the shader.
pub mod flags {
    /// The product exists (enzymes or proteins): the locus glows.
    pub const ACTIVE: u32 = 1;
    /// The filter leaves it out: drawn dim.
    pub const DIMMED: u32 = 2;
    pub const HOVERED: u32 = 4;
    pub const SELECTED: u32 = 8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::genome::BlockKind;
    use crate::proteins::MembraneProtein;

    fn catalog(rows: usize) -> GenomeLayout {
        GenomeLayout::of_catalog(rows)
    }

    #[test]
    fn the_loop_is_closed_and_smooth() {
        for rows in ROW_CHOICES {
            let layout = catalog(rows);
            assert!(layout.segments.len() <= MAX_SEGMENTS, "{rows} rows: {} segments", layout.segments.len());
            for w in layout.segments.windows(2) {
                let (s0, a) = w[0];
                let (s1, b) = w[1];
                assert!((s0 + a.length() - s1).abs() < 1e-3);
                assert!(a.point(a.length()).distance(b.point(0.0)) < 1e-3, "{rows} rows: a gap between {a:?} and {b:?}");
                assert!(a.normal(a.length()).distance(b.normal(0.0)) < 1e-3, "{rows} rows: a kink between {a:?} and {b:?}");
            }
            let (_, first) = layout.segments[0];
            let (_, last) = *layout.segments.last().unwrap();
            assert!(last.point(last.length()).distance(first.point(0.0)) < 1e-3, "{rows} rows: the loop is open");
        }
    }

    #[test]
    fn every_entry_has_one_locus_on_a_row() {
        let n = genome_order().len();
        assert!(n <= MAX_LOCI, "the shader takes {MAX_LOCI} loci, the catalog has {n}");
        for rows in ROW_CHOICES {
            let layout = catalog(rows);
            assert_eq!(layout.loci.len(), n);
            for locus in &layout.loci {
                // The middle of the locus is on its row, at its x.
                let p = layout.point(locus.mid_s());
                assert!(p.distance(Vec2::new((locus.x0 + locus.x1) / 2.0, locus.y)) < 1e-3, "{locus:?} at {p}");
                assert!(locus.s1 - locus.s0 - LOCUS_LEN < 1e-3);
            }
            // No two loci overlap.
            let mut ranges: Vec<_> = layout.loci.iter().map(|l| (l.s0, l.s1)).collect();
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(ranges.windows(2).all(|w| w[0].1 <= w[1].0 + 1e-3));
        }
    }

    #[test]
    fn operons_keep_a_category_together() {
        let layout = catalog(4);
        let order: Vec<Category> = layout.loci.iter().map(|l| l.category).collect();
        let mut seen = Vec::new();
        for c in order {
            if seen.last() != Some(&c) {
                assert!(!seen.contains(&c), "{c:?} is split by another category");
                seen.push(c);
            }
        }
        assert!(layout.operons.iter().all(|o| o.x0 < o.x1));
        // A category that goes on to the next row is named once.
        let named: Vec<_> = layout.operons.iter().filter(|o| !o.continued).map(|o| o.category).collect();
        let mut unique = named.clone();
        unique.dedup();
        assert_eq!(named, unique);
    }

    #[test]
    fn closest_finds_the_strand_from_either_side() {
        let layout = catalog(4);
        for locus in &layout.loci {
            let s = locus.mid_s();
            let p = layout.point(s);
            let n = layout.normal(s);
            let (found, d) = layout.closest(p + n * 2.0);
            assert!((found - s).abs() < 1e-2 && (d - 2.0).abs() < 1e-3, "{locus:?}: {found} {d}");
            let (_, d) = layout.closest(p - n * 2.0);
            assert!((d + 2.0).abs() < 1e-3);
        }
        // On a turn too.
        let (s0, arc) = layout.segments[1];
        let mid = arc.length() / 2.0;
        let (found, d) = layout.closest(arc.point(mid) + arc.normal(mid) * 1.5);
        assert!((found - (s0 + mid)).abs() < 1e-2 && (d - 1.5).abs() < 1e-3);
    }

    #[test]
    fn the_wobble_is_small_and_has_no_seam() {
        let layout = catalog(4);
        for t in [0.0, 1.3, 7.7] {
            assert!((layout.wobble(0.0, t) - layout.wobble(layout.length, t)).abs() < 1e-3);
            for i in 0..50 {
                let s = layout.length * i as f32 / 50.0;
                assert!(layout.wobble(s, t).abs() <= WOBBLE_AMPLITUDE.iter().sum::<f32>() + 1e-4);
            }
        }
        // The helix has whole turns, so its phase matches at the seam.
        let a = layout.helix_phase(0.0, 2.0).rem_euclid(TAU);
        let b = layout.helix_phase(layout.length, 2.0).rem_euclid(TAU);
        assert!((a - b).abs() < 1e-2 || (a - b).abs() > TAU - 1e-2);
    }

    #[test]
    fn pick_finds_the_icon_and_the_symbol_of_a_locus() {
        let layout = catalog(4);
        let i = layout.locus_of(EntryId::Protein(MembraneProtein::Pilus)).unwrap();
        for t in [0.0, 3.0] {
            let foot = layout.icon_foot(i, t);
            assert_eq!(layout.pick(foot + Vec2::new(0.0, 10.0), t), Some(i));
            assert_eq!(layout.pick(foot - Vec2::new(0.0, ICON_RISE + SYMBOL_DROP), t), Some(i));
        }
        // Between two operons there is nothing.
        let row0: Vec<_> = layout.loci.iter().filter(|l| l.row == 0).collect();
        let gap = row0.windows(2).find(|w| w[0].category != w[1].category).unwrap();
        assert_eq!(layout.pick(Vec2::new(gap[0].x1 + OPERON_GAP / 2.0, gap[0].y), 0.0), None);
    }

    #[test]
    fn everything_fits_in_the_bounds() {
        for rows in ROW_CHOICES {
            let layout = catalog(rows);
            let b = layout.bounds;
            for i in 0..layout.loci.len() {
                let foot = layout.icon_foot(i, 0.0);
                assert!(b.contains(foot + Vec2::new(0.0, ICON_MAX_HEIGHT)) && b.contains(foot - Vec2::new(0.0, OPERON_NAME_DROP + ICON_RISE)));
            }
            assert!(b.contains(layout.ori) && b.contains(layout.ter));
        }
    }

    #[test]
    fn a_wide_area_takes_fewer_rows_than_a_tall_one() {
        assert_eq!(best_rows(Vec2::new(2000.0, 300.0)), 2);
        assert!(best_rows(Vec2::new(600.0, 900.0)) >= 4);
    }

    #[test]
    fn looks_follow_the_gene_state() {
        let state = |gene, in_codex| EntryState { known: true, in_codex, gene, count: 0, queued: 0, chosen: false };
        let gene = EntryId::Gene(BlockKind::Fermentation);
        assert_eq!(look(gene, &state(Some(GeneState::Silent), false)), Look::Silent);
        assert_eq!(look(gene, &state(Some(GeneState::Mutated), true)), Look::Mutated);
        assert_eq!(look(gene, &state(None, true)), Look::InCodex);
        assert_eq!(look(gene, &state(None, false)), Look::Unknown);
        assert_eq!(look(EntryId::Protein(MembraneProtein::Toxin), &state(None, false)), Look::Protein);
    }
}
