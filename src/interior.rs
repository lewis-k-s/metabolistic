//! # Interior pieces and display modes
//!
//! A cell sprite shows its pathways as small pieces inside the membrane (see
//! "Interior pieces" in `docs/art-direction.md`). This module decides which
//! pieces a cell shows. It has no graphics: `world::interior_view` spawns the
//! sprites from the plan.
//!
//! - `PIECES`: one line for each piece. To add a piece, add a line here and an
//!   entry to `INTERIORS` in `art/gen_sprites.py`.
//! - `interior_plan`: a pure function. It scores the candidates, keeps the best,
//!   and gives each one a fixed slot. A piece that is shown already gets a bonus
//!   and keeps its slot, so the picture does not jump between frames.
//! - `CellDisplay`: the display mode (Minimal, Standard, Full). It sets how many
//!   interior pieces and how many membrane overlays a cell shows.

use bevy::prelude::*;

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::proteins::Proteome;

/// How much of the cell the sprite shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// Membrane overlays only.
    Minimal,
    /// The best few interior pieces and the membrane overlays.
    #[default]
    Standard,
    /// Every piece and more overlays. For debugging.
    Full,
}

/// Caps of the membrane overlays of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayCaps {
    /// Most overlays of one protein kind.
    pub per_protein: u32,
    /// Most overlays in all.
    pub total: usize,
}

impl DisplayMode {
    /// The next mode, in a cycle.
    pub fn next(self) -> Self {
        match self {
            Self::Minimal => Self::Standard,
            Self::Standard => Self::Full,
            Self::Full => Self::Minimal,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Minimal => "Minimal",
            Self::Standard => "Standard",
            Self::Full => "Full",
        }
    }

    /// Most interior pieces of the player's cell.
    pub fn max_pieces(self) -> usize {
        match self {
            Self::Minimal => 0,
            Self::Standard => 3,
            Self::Full => usize::MAX,
        }
    }

    pub fn overlay_caps(self) -> OverlayCaps {
        match self {
            Self::Minimal | Self::Standard => OverlayCaps { per_protein: 2, total: 6 },
            Self::Full => OverlayCaps { per_protein: 3, total: 8 },
        }
    }
}

/// How long the HUD names the mode after a change, in seconds.
pub const MODE_NOTICE_SECONDS: f32 = 2.5;

/// The display mode of all cells. It is a resource: the game has no settings
/// file yet.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct CellDisplay {
    pub mode: DisplayMode,
    /// Seconds left that the HUD shows the mode.
    pub notice: f32,
}

impl CellDisplay {
    /// Goes to the next mode and starts the HUD notice.
    pub fn cycle(&mut self) {
        self.mode = self.mode.next();
        self.notice = MODE_NOTICE_SECONDS;
    }

    /// The HUD text: empty when the notice is over.
    pub fn notice_text(&self) -> String {
        if self.notice > 0.0 {
            format!("Cell view: {} (V)", self.mode.name())
        } else {
            String::new()
        }
    }
}

/// One interior piece of the art: the gene it shows and its sprite.
#[derive(Debug, Clone, Copy)]
pub struct PieceDef {
    pub kind: BlockKind,
    /// The sheet name in the manifest.
    pub sprite: &'static str,
    /// A ring piece fills the body and has no slot.
    pub ring: bool,
}

/// Every piece, in priority order for ties.
pub const PIECES: [PieceDef; 8] = [
    PieceDef { kind: BlockKind::Chemosynthesis, sprite: "interior_sulfur", ring: false },
    PieceDef { kind: BlockKind::LightCapture, sprite: "interior_thylakoid", ring: false },
    PieceDef { kind: BlockKind::Respiration, sprite: "interior_cristae", ring: false },
    PieceDef { kind: BlockKind::FatStorage, sprite: "interior_lipid", ring: false },
    PieceDef { kind: BlockKind::ProteinSynthesis, sprite: "interior_ribosome", ring: false },
    PieceDef { kind: BlockKind::Catalase, sprite: "interior_peroxisome", ring: false },
    PieceDef { kind: BlockKind::SecondaryMetabolites, sprite: "interior_pigment", ring: true },
    PieceDef { kind: BlockKind::Cuticle, sprite: "interior_cuticle", ring: true },
];

/// The piece of a gene, if the art has one.
pub fn piece_def(kind: BlockKind) -> Option<&'static PieceDef> {
    PIECES.iter().find(|p| p.kind == kind)
}

/// Slot centres for a body of radius `SLOT_BASE_RADIUS`, in pixels, y up. The
/// order is the fill order: the first three spread round the body. They sit on
/// a ring of 12 px, 45 degrees apart, so pieces of at most 10 px do not touch.
pub const SLOT_OFFSETS: [Vec2; 8] = [
    Vec2::new(-9.0, 9.0),
    Vec2::new(9.0, -9.0),
    Vec2::new(9.0, 9.0),
    Vec2::new(-9.0, -9.0),
    Vec2::new(12.0, 0.0),
    Vec2::new(-12.0, 0.0),
    Vec2::new(0.0, 12.0),
    Vec2::new(0.0, -12.0),
];
/// The body radius that `SLOT_OFFSETS` fit, in pixels (the player's).
pub const SLOT_BASE_RADIUS: f32 = 20.0;

/// The centre of a slot on a body of `radius` pixels, on a whole pixel.
pub fn slot_offset(slot: usize, radius: f32) -> Vec2 {
    (SLOT_OFFSETS[slot % SLOT_OFFSETS.len()] * (radius / SLOT_BASE_RADIUS)).round()
}

/// How a piece is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// The gene works.
    Normal,
    /// The gene is mutated: dim and glitched.
    Glitch,
}

/// A piece that a cell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub kind: BlockKind,
    pub look: Look,
    /// The slot of a spot piece. A ring piece has none.
    pub slot: Option<usize>,
}

/// What a cell may show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most pieces in all.
    pub pieces: usize,
    /// True if ring pieces may show.
    pub rings: bool,
}

/// The kind of cell that is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRole {
    Player,
    /// A creature with a real genome.
    Npc,
    /// A cell with its own art (the gate cell): no pieces.
    Plain,
}

/// An NPC body must be at least this big (pixels of radius) to show pieces.
pub const NPC_MIN_RADIUS: f32 = 18.0;
/// Most pieces of an NPC.
pub const NPC_MAX_PIECES: usize = 2;

/// What a cell of this role and size shows in this mode. An NPC never goes
/// above Standard and shows no rings, to protect the frame time.
pub fn limits(mode: DisplayMode, role: CellRole, radius: f32) -> Limits {
    match role {
        CellRole::Player => Limits { pieces: mode.max_pieces(), rings: true },
        CellRole::Npc if radius >= NPC_MIN_RADIUS => Limits {
            pieces: mode.max_pieces().min(NPC_MAX_PIECES),
            rings: false,
        },
        _ => Limits { pieces: 0, rings: false },
    }
}

/// A piece that could show, with the numbers that rank it.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub kind: BlockKind,
    pub state: GeneState,
    pub enzymes: u32,
    /// How hard the block works, 0 to 1 (its throttle).
    pub activity: f32,
}

/// Most enzymes that count in the score.
const SCORE_ENZYME_CAP: u32 = 4;
/// Bonus for a piece that is shown already: scores within this margin keep
/// the current picture.
pub const STICKY_BONUS: f32 = 0.4;

/// The candidates of a cell: each piece whose gene is expressed (or mutated)
/// and has an enzyme. `activity` gives the level of a block.
pub fn candidates(genome: &Genome, proteome: &Proteome, activity: impl Fn(BlockKind) -> f32) -> Vec<Candidate> {
    PIECES
        .iter()
        .filter_map(|def| {
            let state = genome.table.get(&def.kind)?.clone();
            let enzymes = proteome.enzymes(def.kind);
            (state != GeneState::Silent && enzymes > 0).then(|| Candidate {
                kind: def.kind,
                state,
                enzymes,
                activity: activity(def.kind).clamp(0.0, 1.0),
            })
        })
        .collect()
}

/// The score of a candidate. A mutated gene counts half.
pub fn score(c: &Candidate) -> f32 {
    let weight = if c.state == GeneState::Mutated { 0.5 } else { 1.0 };
    let enzymes = 1.0 + 0.25 * c.enzymes.min(SCORE_ENZYME_CAP) as f32;
    weight * (0.6 + 0.4 * c.activity) * enzymes
}

/// The pieces that a cell shows, sorted like `PIECES`.
///
/// Keeps the best `limits.pieces` candidates. A piece in `previous` gets
/// `STICKY_BONUS` and keeps its slot; a new spot piece takes the free slot with
/// the lowest number. A spot piece with no free slot is dropped.
pub fn interior_plan(
    genome: &Genome,
    proteome: &Proteome,
    activity: impl Fn(BlockKind) -> f32,
    previous: &[Piece],
    limits: Limits,
) -> Vec<Piece> {
    plan_from(&candidates(genome, proteome, activity), previous, limits)
}

/// `interior_plan`, from ready candidates.
pub fn plan_from(candidates: &[Candidate], previous: &[Piece], limits: Limits) -> Vec<Piece> {
    let was_shown = |kind| previous.iter().any(|p| p.kind == kind);
    let mut ranked: Vec<(usize, f32, &Candidate)> = candidates
        .iter()
        .filter_map(|c| {
            let index = PIECES.iter().position(|p| p.kind == c.kind)?;
            if PIECES[index].ring && !limits.rings {
                return None;
            }
            let bonus = if was_shown(c.kind) { STICKY_BONUS } else { 0.0 };
            Some((index, score(c) + bonus, c))
        })
        .collect();
    // Best first; the table order breaks ties.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.truncate(limits.pieces);
    ranked.sort_by_key(|r| r.0);

    let mut used = [false; SLOT_OFFSETS.len()];
    let old_slot = |kind| previous.iter().find(|p| p.kind == kind).and_then(|p| p.slot);
    // Pieces that keep a slot claim it first.
    for (index, _, c) in &ranked {
        if !PIECES[*index].ring
            && let Some(slot) = old_slot(c.kind)
            && slot < used.len()
        {
            used[slot] = true;
        }
    }
    let mut plan = Vec::new();
    for (index, _, c) in ranked {
        let look = if c.state == GeneState::Mutated { Look::Glitch } else { Look::Normal };
        let slot = if PIECES[index].ring {
            None
        } else if let Some(slot) = old_slot(c.kind).filter(|s| *s < used.len()) {
            Some(slot)
        } else {
            let Some(free) = used.iter().position(|u| !u) else {
                continue;
            };
            used[free] = true;
            Some(free)
        };
        plan.push(Piece { kind: c.kind, look, slot });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proteins::ProteinTarget;

    fn genome(states: &[(BlockKind, GeneState)]) -> Genome {
        let mut g = Genome::default();
        for (kind, state) in states {
            g.table.insert(*kind, state.clone());
        }
        g
    }

    fn proteome(kinds: &[(BlockKind, u32)]) -> Proteome {
        let mut p = Proteome::default();
        for &(kind, n) in kinds {
            for _ in 0..n {
                p.add(ProteinTarget::Enzyme(kind));
            }
        }
        p
    }

    const ALL: Limits = Limits { pieces: usize::MAX, rings: true };

    fn full_cell() -> (Genome, Proteome) {
        let genes: Vec<_> = PIECES.iter().map(|p| (p.kind, GeneState::Expressed)).collect();
        let enzymes: Vec<_> = PIECES.iter().map(|p| (p.kind, 1)).collect();
        (genome(&genes), proteome(&enzymes))
    }

    fn kinds(plan: &[Piece]) -> Vec<BlockKind> {
        plan.iter().map(|p| p.kind).collect()
    }

    #[test]
    fn silent_genes_and_genes_without_enzymes_show_nothing() {
        let g = genome(&[
            (BlockKind::Chemosynthesis, GeneState::Silent),
            (BlockKind::LightCapture, GeneState::Expressed),
        ]);
        let p = proteome(&[(BlockKind::Chemosynthesis, 3)]);
        assert!(interior_plan(&g, &p, |_| 1.0, &[], ALL).is_empty());
    }

    #[test]
    fn a_mutated_gene_shows_the_glitch_look() {
        let g = genome(&[(BlockKind::Chemosynthesis, GeneState::Mutated)]);
        let p = proteome(&[(BlockKind::Chemosynthesis, 1)]);
        let plan = interior_plan(&g, &p, |_| 1.0, &[], ALL);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].look, Look::Glitch);
        let g = genome(&[(BlockKind::Chemosynthesis, GeneState::Enhanced)]);
        assert_eq!(interior_plan(&g, &p, |_| 1.0, &[], ALL)[0].look, Look::Normal);
    }

    #[test]
    fn the_limit_keeps_the_best_scores() {
        let (g, _) = full_cell();
        // Respiration has many enzymes and runs hard; the rest have one and idle.
        let p = proteome(&PIECES.iter().map(|d| (d.kind, if d.kind == BlockKind::Respiration { 4 } else { 1 })).collect::<Vec<_>>());
        let activity = |k| if k == BlockKind::Catalase { 0.9 } else { 0.1 };
        let plan = interior_plan(&g, &p, activity, &[], Limits { pieces: 2, rings: true });
        let kinds = kinds(&plan);
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&BlockKind::Respiration));
        assert!(kinds.contains(&BlockKind::Catalase));
    }

    #[test]
    fn a_mutated_gene_ranks_below_a_working_one() {
        let g = genome(&[
            (BlockKind::Chemosynthesis, GeneState::Mutated),
            (BlockKind::LightCapture, GeneState::Expressed),
        ]);
        let p = proteome(&[(BlockKind::Chemosynthesis, 1), (BlockKind::LightCapture, 1)]);
        let plan = interior_plan(&g, &p, |_| 1.0, &[], Limits { pieces: 1, rings: true });
        assert_eq!(kinds(&plan), vec![BlockKind::LightCapture]);
    }

    #[test]
    fn full_mode_shows_every_piece_in_its_own_slot() {
        let (g, p) = full_cell();
        let plan = interior_plan(&g, &p, |_| 1.0, &[], ALL);
        assert_eq!(plan.len(), PIECES.len());
        let mut slots: Vec<_> = plan.iter().filter_map(|p| p.slot).collect();
        let spots = PIECES.iter().filter(|d| !d.ring).count();
        assert_eq!(slots.len(), spots);
        slots.sort();
        slots.dedup();
        assert_eq!(slots.len(), spots, "no two pieces share a slot");
        for piece in &plan {
            assert_eq!(piece.slot.is_none(), piece_def(piece.kind).unwrap().ring);
        }
    }

    #[test]
    fn rings_need_permission() {
        let (g, p) = full_cell();
        let plan = interior_plan(&g, &p, |_| 1.0, &[], Limits { pieces: 99, rings: false });
        assert!(plan.iter().all(|piece| piece.slot.is_some()));
        assert_eq!(plan.len(), PIECES.iter().filter(|d| !d.ring).count());
    }

    #[test]
    fn a_shown_piece_stays_when_scores_are_close() {
        let g = genome(&[
            (BlockKind::Chemosynthesis, GeneState::Expressed),
            (BlockKind::LightCapture, GeneState::Expressed),
        ]);
        let p = proteome(&[(BlockKind::Chemosynthesis, 1), (BlockKind::LightCapture, 1)]);
        let one = Limits { pieces: 1, rings: true };
        // Light capture is a little busier, but sulfur is shown: it stays.
        let activity = |k| if k == BlockKind::LightCapture { 0.9 } else { 0.8 };
        let shown = [Piece { kind: BlockKind::Chemosynthesis, look: Look::Normal, slot: Some(0) }];
        assert_eq!(kinds(&interior_plan(&g, &p, activity, &shown, one)), vec![BlockKind::Chemosynthesis]);
        // Without the history the busier one wins.
        assert_eq!(kinds(&interior_plan(&g, &p, activity, &[], one)), vec![BlockKind::LightCapture]);
        // A clear difference still wins.
        let activity = |k| if k == BlockKind::LightCapture { 1.0 } else { 0.0 };
        assert_eq!(kinds(&interior_plan(&g, &p, activity, &shown, one)), vec![BlockKind::LightCapture]);
    }

    #[test]
    fn pieces_keep_their_slots_when_another_piece_goes() {
        let (g, p) = full_cell();
        let first = interior_plan(&g, &p, |_| 1.0, &[], ALL);
        // Silence the first spot piece.
        let mut g2 = g.clone();
        g2.table.insert(first[0].kind, GeneState::Silent);
        let second = interior_plan(&g2, &p, |_| 1.0, &first, ALL);
        for piece in &second {
            let before = first.iter().find(|f| f.kind == piece.kind).unwrap();
            assert_eq!(before.slot, piece.slot, "{:?}", piece.kind);
        }
        // The slot that was free goes to a new piece, the lowest first.
        let third = interior_plan(&g, &p, |_| 1.0, &second, ALL);
        let back = third.iter().find(|t| t.kind == first[0].kind).unwrap();
        assert_eq!(back.slot, first[0].slot);
    }

    #[test]
    fn the_plan_is_the_same_for_the_same_input() {
        let (g, p) = full_cell();
        let a = interior_plan(&g, &p, |_| 0.5, &[], ALL);
        let b = interior_plan(&g, &p, |_| 0.5, &a, ALL);
        assert_eq!(a, b);
    }

    #[test]
    fn modes_cycle_and_set_the_caps() {
        let mut mode = DisplayMode::Minimal;
        let mut seen = Vec::new();
        for _ in 0..3 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(seen, vec![DisplayMode::Standard, DisplayMode::Full, DisplayMode::Minimal]);
        assert_eq!(DisplayMode::Minimal.max_pieces(), 0);
        assert_eq!(DisplayMode::Standard.max_pieces(), 3);
        assert_eq!(DisplayMode::Standard.overlay_caps(), OverlayCaps { per_protein: 2, total: 6 });
        assert!(DisplayMode::Full.overlay_caps().total > 6);
    }

    #[test]
    fn the_hud_names_the_mode_for_a_short_time() {
        let mut display = CellDisplay::default();
        assert!(display.notice_text().is_empty());
        display.cycle();
        assert!(display.notice_text().contains("Full"));
        display.notice = 0.0;
        assert!(display.notice_text().is_empty());
    }

    #[test]
    fn npc_limits_protect_the_frame_time() {
        let big = limits(DisplayMode::Full, CellRole::Npc, 20.0);
        assert_eq!(big, Limits { pieces: NPC_MAX_PIECES, rings: false });
        assert_eq!(limits(DisplayMode::Minimal, CellRole::Npc, 20.0).pieces, 0);
        assert_eq!(limits(DisplayMode::Standard, CellRole::Npc, 10.0).pieces, 0);
        assert_eq!(limits(DisplayMode::Full, CellRole::Plain, 36.0).pieces, 0);
        assert_eq!(limits(DisplayMode::Standard, CellRole::Player, 20.0), Limits { pieces: 3, rings: true });
    }

    #[test]
    fn slots_are_on_whole_pixels_and_do_not_overlap() {
        for radius in [18.0, 20.0, 26.0] {
            let offsets: Vec<_> = (0..SLOT_OFFSETS.len()).map(|s| slot_offset(s, radius)).collect();
            for o in &offsets {
                assert_eq!(*o, o.round());
            }
            for (i, a) in offsets.iter().enumerate() {
                for b in &offsets[i + 1..] {
                    assert!(a.distance(*b) >= 8.0, "{a} {b}");
                }
            }
        }
    }
}
