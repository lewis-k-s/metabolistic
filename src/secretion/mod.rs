//! # Secretion: area clouds
//!
//! A cell can release a substance into the water round it. The substance is a
//! channel of the `EnvGrid`: it spreads, drifts with the water and decays.
//! Its effect comes from the level that a cell samples at its position, so a
//! secretion needs no aim. The plan is `docs/secretion-abilities-plan.md`.
//!
//! | Secretion | Gene | Channel | Effect |
//! |---|---|---|---|
//! | Microcin (`microcin`) | `cvaC` | `Microcin` | damages prey; prey flee it |
//! | Slime (`slime`) | `pelA` | `Slime` | more drag for other cells |
//! | Siderophore (`siderophore`) | `entF` | `Siderophore`, `FeSiderophore` | dissolves rust into Metals |
//! | Autoinducer (`autoinducer`) | `luxS` | `Autoinducer` | symbionts gather; predators listen |
//!
//! ## The mechanic
//!
//! 1. **A gene, not a membrane protein.** Each secretion is a gene with a
//!    block. It uses no membrane slot.
//! 2. **A block whose turnover goes out.** The block's `FluxProfile` has
//!    demands only (ATP and one material). The solver runs it, so the costs
//!    are real flux, and a short pool slows it down. Its `Drive` is 0 unless
//!    the secretion is on (`secretion_drive_system`).
//! 3. **The source step.** At the end of the tick, `secretion_exchange_system`
//!    puts `turnover x yield` into the grid at the cell (`EnvGrid::deposit`).
//! 4. **Effects** read the sampled level (see the submodules).
//!
//! ## Keys
//!
//! The `SecretionBar` has two keys, Q and E (`KEY_LABELS`). A key toggles its
//! secretion on and off (`ToggleSecretion`). The bar fills itself with the
//! secretion genes of the cell (`sync_bar`); the Manage screen can cycle a key.
//!
//! The rules are pure functions. The systems fetch data and call them.

pub mod autoinducer;
pub mod microcin;
pub mod siderophore;
pub mod slime;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, CellBlocks, OwnedBy};
use crate::envgrid::{EnvGrid, FieldParams};
use crate::environment::Channel;
use crate::metabolism::{BlockTurnover, Drive, FluxProfile, MetabolicSchedule, MetabolicSet};
use crate::molecules::Currency;

// --- Secretions ---

/// A substance that a cell can secrete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Secretion {
    Microcin,
    Slime,
    Siderophore,
    Autoinducer,
}

impl Secretion {
    pub const ALL: [Secretion; 4] = [
        Secretion::Microcin,
        Secretion::Slime,
        Secretion::Siderophore,
        Secretion::Autoinducer,
    ];

    /// Position in `ALL`.
    pub const fn index(self) -> usize {
        match self {
            Secretion::Microcin => 0,
            Secretion::Slime => 1,
            Secretion::Siderophore => 2,
            Secretion::Autoinducer => 3,
        }
    }

    /// The gene that makes it.
    pub const fn gene(self) -> BlockKind {
        match self {
            Secretion::Microcin => BlockKind::Microcin,
            Secretion::Slime => BlockKind::Slime,
            Secretion::Siderophore => BlockKind::Siderophore,
            Secretion::Autoinducer => BlockKind::Autoinducer,
        }
    }

    /// The secretion of a gene, if it is a secretion gene.
    pub fn from_gene(kind: BlockKind) -> Option<Secretion> {
        Secretion::ALL.into_iter().find(|s| s.gene() == kind)
    }

    /// The grid channel that the cell puts the product into.
    pub const fn channel(self) -> Channel {
        match self {
            Secretion::Microcin => Channel::Microcin,
            Secretion::Slime => Channel::Slime,
            Secretion::Siderophore => Channel::Siderophore,
            Secretion::Autoinducer => Channel::Autoinducer,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Secretion::Microcin => "Microcin",
            Secretion::Slime => "Slime",
            Secretion::Siderophore => "Siderophore",
            Secretion::Autoinducer => "AI-2 call",
        }
    }

    /// What one turnover of the block uses: (currency, amount). One enzyme at
    /// full saturation does one turnover per second.
    pub fn costs(self) -> &'static [(Currency, f32)] {
        match self {
            Secretion::Microcin => microcin::COSTS,
            Secretion::Slime => slime::COSTS,
            Secretion::Siderophore => siderophore::COSTS,
            Secretion::Autoinducer => autoinducer::COSTS,
        }
    }

    /// Grid units that one turnover puts out.
    pub fn yield_per_turnover(self) -> f32 {
        match self {
            Secretion::Microcin => microcin::YIELD,
            Secretion::Slime => slime::YIELD,
            Secretion::Siderophore => siderophore::YIELD,
            Secretion::Autoinducer => autoinducer::YIELD,
        }
    }

    /// The flux profile of the block: demands only.
    pub fn flux_profile(self) -> FluxProfile {
        let mut profile = FluxProfile::default();
        for &(currency, amount) in self.costs() {
            profile.set_demand(currency, amount);
        }
        profile
    }
}

/// How a secreted channel moves in a field. `water_drift` is the drift of the
/// water (the `Minerals` drift of the field). Each product drifts with it,
/// except slime, a polymer gel, which drifts only a little.
pub fn cloud_params(channel: Channel, water_drift: Vec2) -> FieldParams {
    let (diffusion, decay, drift) = match channel {
        Channel::Microcin => (microcin::DIFFUSION, microcin::DECAY, water_drift),
        Channel::Slime => (slime::DIFFUSION, slime::DECAY, water_drift * slime::DRIFT_SHARE),
        Channel::Siderophore => (siderophore::DIFFUSION, siderophore::DECAY, water_drift),
        Channel::FeSiderophore => (siderophore::FE_DIFFUSION, siderophore::FE_DECAY, water_drift),
        Channel::Autoinducer => (autoinducer::DIFFUSION, autoinducer::DECAY, water_drift),
        _ => return FieldParams::STILL,
    };
    FieldParams {
        diffusion,
        decay,
        drift,
        open_drift_edges: true,
    }
}

/// Does the genome carry the gene in a state that makes its product (or its
/// immunity, or its receptor)? Every state except Silent.
pub fn has_active_gene(genome: &Genome, kind: BlockKind) -> bool {
    matches!(
        genome.get_gene_state(&kind),
        Some(GeneState::Expressed | GeneState::Mutated | GeneState::Enhanced)
    )
}

// --- The bar ---

/// Number of secretion keys.
pub const KEY_COUNT: usize = 2;
/// The binding of each key, for the HUD and the editor.
pub const KEY_LABELS: [&str; KEY_COUNT] = ["Q", "E"];

/// The secretions on the keys Q and E.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretionBar {
    pub keys: [Option<Secretion>; KEY_COUNT],
}

/// Which secretions are on (indexed by `Secretion::index`).
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Secreting(pub [bool; Secretion::ALL.len()]);

impl Secreting {
    pub fn is_on(&self, secretion: Secretion) -> bool {
        self.0[secretion.index()]
    }

    pub fn set(&mut self, secretion: Secretion, on: bool) {
        self.0[secretion.index()] = on;
    }
}

/// The secretion genes of a genome that can make their product, in the order
/// of `Secretion::ALL`.
pub fn available_secretions(genome: &Genome) -> Vec<Secretion> {
    Secretion::ALL
        .into_iter()
        .filter(|s| genome.table.contains_key(&s.gene()))
        .collect()
}

/// The bar after a genome change: a key whose gene left the genome is
/// cleared, and each empty key gets the next secretion that is not on the bar.
pub fn sync_bar(bar: &SecretionBar, genome: &Genome) -> SecretionBar {
    let options = available_secretions(genome);
    let mut keys = bar.keys.map(|k| k.filter(|s| options.contains(s)));
    for slot in 0..KEY_COUNT {
        if keys[slot].is_none() {
            keys[slot] = options.iter().copied().find(|s| !keys.contains(&Some(*s)));
        }
    }
    SecretionBar { keys }
}

/// The next secretion of a key when the player cycles it in Manage: each
/// option in order, then empty. A secretion on the other key is skipped.
pub fn next_on_key(bar: &SecretionBar, key: usize, options: &[Secretion]) -> Option<Secretion> {
    let other: Vec<Secretion> = (0..KEY_COUNT)
        .filter(|k| *k != key)
        .filter_map(|k| bar.keys[k])
        .collect();
    let free: Vec<Secretion> = options.iter().copied().filter(|s| !other.contains(s)).collect();
    crate::actions::next_assignment(bar.keys.get(key).copied().flatten(), &free)
}

/// Why a toggle was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleRefused {
    /// The key is empty.
    EmptyKey,
    /// The gene is missing or silent.
    NoGene,
}

/// Toggles the secretion on `key`. Returns the new state (true: on).
pub fn toggle_key(
    bar: &SecretionBar,
    secreting: &mut Secreting,
    genome: &Genome,
    key: usize,
) -> Result<bool, ToggleRefused> {
    let secretion = bar.keys.get(key).copied().flatten().ok_or(ToggleRefused::EmptyKey)?;
    let on = !secreting.is_on(secretion);
    if on && !has_active_gene(genome, secretion.gene()) {
        return Err(ToggleRefused::NoGene);
    }
    secreting.set(secretion, on);
    Ok(on)
}

/// The cost of a secretion per second at full rate, for the HUD: "1.5 ATP +
/// 0.4 AminoAcids".
pub fn cost_words(secretion: Secretion) -> String {
    secretion
        .costs()
        .iter()
        .map(|(c, a)| format!("{a} {c:?}"))
        .collect::<Vec<_>>()
        .join(" + ")
}

/// The HUD line of a key: "[Q] Microcin ON (1.5 ATP + 0.4 AminoAcids /s)".
/// Empty when the key is empty.
pub fn key_line(key: usize, bar: &SecretionBar, secreting: &Secreting, genome: &Genome) -> String {
    let Some(secretion) = bar.keys.get(key).copied().flatten() else {
        return String::new();
    };
    let label = KEY_LABELS[key];
    let state = if !has_active_gene(genome, secretion.gene()) {
        "silent".to_string()
    } else if secreting.is_on(secretion) {
        format!("ON ({} /s)", cost_words(secretion))
    } else {
        "off".to_string()
    };
    format!("[{label}] {}: {state}", secretion.name())
}

// --- Source step ---

/// The drive of a secretion block: 1 while the secretion is on, else 0. The
/// pathway level (`Throttle`) still multiplies it in the solver.
pub fn secretion_drive(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

/// Grid units that `turnover` turnovers put out.
pub fn secreted_amount(secretion: Secretion, turnover: f32) -> f32 {
    turnover.max(0.0) * secretion.yield_per_turnover()
}

/// Marks the block of a secretion gene.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretionBlock(pub Secretion);

// --- Messages and systems ---

/// Asks a cell to toggle the secretion on a key (0 is Q, 1 is E).
#[derive(Message, Debug, Clone, Copy)]
pub struct ToggleSecretion {
    pub cell: Entity,
    pub key: usize,
}

/// Applies `ToggleSecretion` messages.
pub fn toggle_secretion_system(
    mut orders: MessageReader<ToggleSecretion>,
    mut cells: Query<(&SecretionBar, &mut Secreting, &Genome), With<Cell>>,
) {
    for order in orders.read() {
        let Ok((bar, mut secreting, genome)) = cells.get_mut(order.cell) else {
            continue;
        };
        let mut next = *secreting;
        if toggle_key(bar, &mut next, genome, order.key).is_ok() && next != *secreting {
            *secreting = next;
        }
    }
}

/// Keeps the bar in line with the genome, and turns off a secretion whose
/// gene left the genome.
pub fn sync_secretion_bar_system(
    mut cells: Query<(Ref<Genome>, &mut SecretionBar, &mut Secreting), With<Cell>>,
) {
    for (genome, mut bar, mut secreting) in &mut cells {
        if !genome.is_changed() {
            continue;
        }
        let synced = sync_bar(&bar, &genome);
        if synced != *bar {
            *bar = synced;
        }
        for secretion in Secretion::ALL {
            if secreting.is_on(secretion) && !genome.table.contains_key(&secretion.gene()) {
                secreting.set(secretion, false);
            }
        }
    }
}

/// Sets the drive of each secretion block from its cell's `Secreting`.
pub fn secretion_drive_system(
    cells: Query<&Secreting, With<Cell>>,
    mut blocks: Query<(&OwnedBy, &SecretionBlock, &mut Drive)>,
) {
    for (owner, block, mut drive) in &mut blocks {
        let on = cells.get(owner.0).is_ok_and(|s| s.is_on(block.0));
        let wanted = secretion_drive(on);
        if drive.0 != wanted {
            drive.0 = wanted;
        }
    }
}

/// The source step: what each secretion block made in the tick goes into the
/// grid at the cell.
pub fn secretion_exchange_system(
    mut grid: ResMut<EnvGrid>,
    cells: Query<(&Transform, &CellBlocks), With<Cell>>,
    blocks: Query<(&SecretionBlock, &BlockTurnover)>,
) {
    for (transform, cell_blocks) in &cells {
        let position = transform.translation.truncate();
        for secretion in Secretion::ALL {
            let Some((block, turnover)) =
                cell_blocks.get(secretion.gene()).and_then(|b| blocks.get(b).ok())
            else {
                continue;
            };
            let amount = secreted_amount(block.0, turnover.0);
            if amount > 0.0 {
                grid.deposit(block.0.channel(), position, amount);
            }
        }
    }
}

// --- Drawing data ---

/// Level of each cloud at which its texel is full (255), in the order of the
/// two cloud maps: A is microcin, slime and AI-2; B is free siderophore and
/// the iron complex.
/// The numbers follow the levels next to a still producer with one enzyme.
pub const CLOUD_FULL: [(Channel, f32); 5] = [
    (Channel::Microcin, 1.5),
    (Channel::Slime, 3.0),
    (Channel::Autoinducer, 0.7),
    (Channel::Siderophore, 1.0),
    // The complex is thin (a few Metals spread out), so it shows early.
    (Channel::FeSiderophore, 0.1),
];

/// The two RGBA8 cloud maps of a grid, one texel for each grid cell, row 0 at
/// the top (the highest world y), like `lighting::render_lightmap`. Map A: R
/// microcin, G slime, B AI-2. Map B: R free siderophore, G iron complex. Each
/// level is scaled by its `CLOUD_FULL`. Unused bytes are 0.
pub fn render_cloudmaps(grid: &EnvGrid) -> [Vec<u8>; 2] {
    let dims = grid.dims();
    let mut maps = [vec![0u8; dims.len() * 4], vec![0u8; dims.len() * 4]];
    for (k, (channel, full)) in CLOUD_FULL.into_iter().enumerate() {
        if grid.is_dormant(channel) {
            continue;
        }
        let (map, byte) = if k < 3 { (0, k) } else { (1, k - 3) };
        let layer = grid.layer(channel);
        for row in 0..dims.height {
            let y = dims.height - 1 - row;
            for x in 0..dims.width {
                let level = layer[dims.index(x, y)];
                let t = (row * dims.width + x) * 4 + byte;
                maps[map][t] = (level / full * 255.0).clamp(0.0, 255.0).round() as u8;
            }
        }
    }
    maps
}

/// Does any secreted channel have content? The cloud view skips its texture
/// writes while none has.
pub fn any_cloud(grid: &EnvGrid) -> bool {
    Channel::SECRETED.iter().any(|c| !grid.is_dormant(*c))
}

pub struct SecretionPlugin;

impl Plugin for SecretionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ToggleSecretion>()
            .add_systems(Update, (sync_secretion_bar_system, toggle_secretion_system).chain())
            .add_systems(
                MetabolicSchedule,
                (
                    secretion_drive_system.in_set(MetabolicSet::Regulate),
                    (
                        secretion_exchange_system,
                        microcin::microcin_system,
                        siderophore::chelate_rust_system,
                        siderophore::siderophore_uptake_system,
                        autoinducer::clump_system,
                    )
                        .chain()
                        .in_set(MetabolicSet::Exchange)
                        .before(crate::exchange::step_grid_system)
                        .run_if(resource_exists::<EnvGrid>),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genome_with(kinds: &[(BlockKind, GeneState)]) -> Genome {
        let mut genome = Genome::default();
        for (kind, state) in kinds {
            genome.table.insert(*kind, state.clone());
        }
        genome
    }

    #[test]
    fn every_secretion_maps_to_its_gene_and_channel() {
        for s in Secretion::ALL {
            assert_eq!(Secretion::from_gene(s.gene()), Some(s));
            assert!(s.channel().is_secreted());
            assert_eq!(Secretion::ALL[s.index()], s);
            assert!(!s.costs().is_empty());
            assert!(s.costs().iter().any(|(c, _)| *c == Currency::ATP), "{s:?} costs ATP");
        }
        assert_eq!(Secretion::from_gene(BlockKind::Chemosynthesis), None);
    }

    #[test]
    fn the_bar_fills_itself_and_drops_a_lost_gene() {
        let genome = genome_with(&[
            (BlockKind::Slime, GeneState::Expressed),
            (BlockKind::Microcin, GeneState::Expressed),
        ]);
        let bar = sync_bar(&SecretionBar::default(), &genome);
        assert_eq!(bar.keys, [Some(Secretion::Microcin), Some(Secretion::Slime)]);

        // The player put slime on Q: a sync keeps it.
        let mine = SecretionBar { keys: [Some(Secretion::Slime), None] };
        assert_eq!(sync_bar(&mine, &genome).keys, [Some(Secretion::Slime), Some(Secretion::Microcin)]);

        // Slime leaves the genome: Q is cleared and refilled with nothing new.
        let less = genome_with(&[(BlockKind::Microcin, GeneState::Expressed)]);
        let after = sync_bar(&bar, &less);
        assert_eq!(after.keys, [Some(Secretion::Microcin), None]);
    }

    #[test]
    fn a_key_cycles_through_the_free_secretions() {
        let options = [Secretion::Microcin, Secretion::Slime, Secretion::Siderophore];
        let bar = SecretionBar { keys: [Some(Secretion::Microcin), Some(Secretion::Slime)] };
        // Q cycles, skipping slime (it is on E).
        assert_eq!(next_on_key(&bar, 0, &options), Some(Secretion::Siderophore));
        let bar = SecretionBar { keys: [Some(Secretion::Siderophore), Some(Secretion::Slime)] };
        assert_eq!(next_on_key(&bar, 0, &options), None);
        let bar = SecretionBar { keys: [None, Some(Secretion::Slime)] };
        assert_eq!(next_on_key(&bar, 0, &options), Some(Secretion::Microcin));
    }

    #[test]
    fn a_key_toggles_and_a_silent_gene_cannot_start() {
        let genome = genome_with(&[(BlockKind::Microcin, GeneState::Expressed)]);
        let bar = SecretionBar { keys: [Some(Secretion::Microcin), None] };
        let mut on = Secreting::default();
        assert_eq!(toggle_key(&bar, &mut on, &genome, 0), Ok(true));
        assert!(on.is_on(Secretion::Microcin));
        assert_eq!(toggle_key(&bar, &mut on, &genome, 0), Ok(false));
        assert_eq!(toggle_key(&bar, &mut on, &genome, 1), Err(ToggleRefused::EmptyKey));

        let silent = genome_with(&[(BlockKind::Microcin, GeneState::Silent)]);
        assert_eq!(toggle_key(&bar, &mut on, &silent, 0), Err(ToggleRefused::NoGene));
        // A mutated gene still runs, at half rate.
        let mutated = genome_with(&[(BlockKind::Microcin, GeneState::Mutated)]);
        assert_eq!(toggle_key(&bar, &mut on, &mutated, 0), Ok(true));
        // Switching off always works.
        assert_eq!(toggle_key(&bar, &mut on, &silent, 0), Ok(false));
    }

    #[test]
    fn the_drive_and_the_amount_follow_the_switch_and_the_turnover() {
        assert_eq!(secretion_drive(true), 1.0);
        assert_eq!(secretion_drive(false), 0.0);
        assert_eq!(secreted_amount(Secretion::Microcin, 0.0), 0.0);
        assert_eq!(secreted_amount(Secretion::Microcin, -1.0), 0.0);
        assert!(
            (secreted_amount(Secretion::Slime, 2.0) - 2.0 * slime::YIELD).abs() < 1e-6,
            "linear in turnover"
        );
    }

    #[test]
    fn clouds_drift_with_the_water_and_slime_drifts_less() {
        let water = Vec2::new(0.7, 0.25);
        let microcin = cloud_params(Channel::Microcin, water);
        let slime = cloud_params(Channel::Slime, water);
        assert_eq!(microcin.drift, water);
        assert!(slime.drift.length() < water.length());
        assert!(slime.decay < microcin.decay, "a gel stays longer");
        for channel in Channel::SECRETED {
            let p = cloud_params(channel, water);
            assert!(p.diffusion > 0.0 && p.diffusion <= 5.0 && p.decay > 0.0, "{channel:?}");
        }
        assert_eq!(cloud_params(Channel::Heat, water), FieldParams::STILL);
    }

    #[test]
    fn the_cloud_maps_put_each_channel_in_its_byte() {
        let mut grid = EnvGrid::centered(4, 3);
        for channel in Channel::SECRETED {
            grid.set_params(channel, FieldParams::STILL);
        }
        grid.step(0.1);
        assert!(!any_cloud(&grid), "all dormant");
        assert!(render_cloudmaps(&grid).iter().all(|m| m.iter().all(|&b| b == 0)));
        // Grid cell (1, 2) is the top row of the image, column 1.
        grid.set_level(Channel::Slime, 1, 2, 1.5);
        grid.set_level(Channel::FeSiderophore, 1, 2, 30.0);
        assert!(any_cloud(&grid));
        let [a, b] = render_cloudmaps(&grid);
        let texel = 4; // column 1 of row 0
        assert_eq!(a[texel + 1], 128, "slime at half of its full level");
        assert_eq!(b[texel + 1], 255, "the complex is clamped");
        assert_eq!(a[texel], 0);
    }

    #[test]
    fn the_hud_line_says_the_state() {
        let genome = genome_with(&[(BlockKind::Slime, GeneState::Expressed)]);
        let bar = SecretionBar { keys: [Some(Secretion::Slime), None] };
        let mut on = Secreting::default();
        assert_eq!(key_line(0, &bar, &on, &genome), "[Q] Slime: off");
        on.set(Secretion::Slime, true);
        assert!(key_line(0, &bar, &on, &genome).starts_with("[Q] Slime: ON ("));
        assert_eq!(key_line(1, &bar, &on, &genome), "");
    }
}
