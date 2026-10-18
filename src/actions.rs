//! # Live control: the action bar
//!
//! Regulation is fast, like gene expression. There are two controls:
//!
//! **Pathway levels** (`PathwayLevels`): each expressed block runs at Off, Low
//! or High (`Level`). The player sets the level on the metabolic map of the
//! Manage screen (`SetPathwayLevel`), not with a hotkey. The level is the target
//! of the block's `Throttle`, which ramps to it at `RAMP_PER_SEC` (capped at the
//! gap, so it never overshoots and does not depend on the tick length). Each
//! level change costs `LEVEL_CHANGE_ATP`; with too little ATP it does not
//! happen. The `Throttle` multiplies the solver's drive, so regulated blocks
//! (fat storage, protein synthesis, refill blocks) keep setting `Drive` on
//! their own.
//!
//! **Action slots** (`ActionBar`): `SLOT_COUNT` slots on the mouse buttons
//! (`SLOT_LABELS`: left, right and the two side buttons), for active skills
//! only. A slot holds one `ActionKind`.
//!
//! **Flagellum burst** is not a slot: Shift fires it (`UseAction`). Thrust
//! x`BURST_THRUST_MULTIPLIER` for `BURST_SECONDS`, then a cooldown. Needs a
//! flagellum.
//!
//! **Two directions**: the `heading` is the move direction (WASD), and the
//! `aim` is the cursor. The flagellum trails the heading; the aimed actions
//! (toxin, pilus, injector, lyse) use the aim.
//!
//! - **Adhesin**: a toggle. While held, the body does not move (thrust is
//!   ignored and damping is high) and the cell pays ATP each second. Needs an
//!   adhesin.
//! - **Toxin**: fires a droplet in the aim direction (`FireToxin`). Costs ATP and
//!   Metals. Needs a toxin. There are no targets until M5.
//! - **Pilus**: a type IV pilus (see `pilus`). Press shoots it along the aim,
//!   hold keeps it attached, the mouse wheel reels it in and out, release lets
//!   go. Needs a pilus.
//! - **Injector**: a type VI secretion spear (see `stun`). Stuns a cell in
//!   contact, in a cone round the aim. Needs an injector.
//! - **Lyse**: cuts the wall tile in contact (`walls`). 5 ATP. Needs a lytic protein.
//! - **Induce mutation**: costs ATP, adds 1.0 to the `MutationLoad`, so a gene
//!   mutates. It has a chance to give an `Enhanced` gene (see `genome_damage`).
//!
//! The rules are pure functions and methods. The systems fetch data and call
//! them. The hotkeys and the droplet sprites are in `world` (full builds).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blocks::genome::{BlockKind, GeneState, Genome};
use crate::cell::{Cell, OwnedBy};
use crate::genome_damage::{InducedMutations, MutationLoad};
use crate::metabolism::{
    CellTick, CurrencyPools, FlowLedger, FlowSource, FlowmapTimingConfig, MetabolicNode, MetabolicSet,
};
use crate::molecules::Currency;
use crate::movement::thrust_force;
use crate::proteins::{MembraneProtein, ProteinTarget, Proteome};

// --- Tuning. All of these need **playtest**. ---

/// Number of action slots.
pub const SLOT_COUNT: usize = 4;
/// The binding of each slot, for the HUD and the editor: left and right mouse
/// buttons, then the back (mouse 4) and forward (mouse 5) side buttons.
pub const SLOT_LABELS: [&str; SLOT_COUNT] = ["LMB", "RMB", "M4", "M5"];
/// The binding of the flagellum burst, for the HUD.
pub const BURST_LABEL: &str = "Shift";
/// Drive of a pathway at level Low.
pub const LOW_LEVEL: f32 = 0.5;
/// Drive of a pathway at level High.
pub const HIGH_LEVEL: f32 = 1.0;
/// How fast a throttle moves to its target, per second.
pub const RAMP_PER_SEC: f32 = 1.0;
/// ATP for each level change.
pub const LEVEL_CHANGE_ATP: f32 = 1.0;

/// Thrust factor during a flagellum burst.
pub const BURST_THRUST_MULTIPLIER: f32 = 3.0;
/// Length of a burst in seconds.
pub const BURST_SECONDS: f32 = 1.5;
/// Seconds between the end of a burst and the next one.
pub const BURST_COOLDOWN_SECONDS: f32 = 4.0;
/// ATP for one burst.
pub const BURST_ATP: f32 = 3.0;

/// ATP per second while the adhesin holds.
pub const ADHESIN_ATP_PER_SEC: f32 = 0.2;
/// Linear damping per second while the adhesin holds. Stops the body in about
/// 0.1 s.
pub const ADHESIN_DAMPING: f32 = 60.0;
/// Seconds after the grab in which the overlay plays its `use` clip. After
/// that it loops `hold`.
pub const ADHESIN_GRAB_SECONDS: f32 = 0.6;

/// ATP for one toxin shot.
pub const TOXIN_ATP: f32 = 2.0;
/// Metals (a cofactor) for one toxin shot.
pub const TOXIN_METALS: f32 = 1.0;
/// Speed of a toxin droplet, in pixels per second.
pub const TOXIN_SPEED: f32 = 240.0;
/// Seconds that a toxin droplet flies before it is gone.
pub const TOXIN_LIFETIME: f32 = 1.2;
/// Seconds that the toxin overlay plays its `use` clip.
pub const TOXIN_FLASH_SECONDS: f32 = 0.6;

/// ATP for an induced mutation.
pub const INDUCE_ATP: f32 = 10.0;
/// Mutation load that an induced mutation adds. One whole unit mutates a gene.
pub const INDUCE_LOAD: f32 = 1.0;

// --- Levels ---

/// The level of a pathway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    Off,
    Low,
    High,
}

impl Level {
    /// The throttle target of this level.
    pub fn value(self) -> f32 {
        match self {
            Level::Off => 0.0,
            Level::Low => LOW_LEVEL,
            Level::High => HIGH_LEVEL,
        }
    }

    /// The next level when the player clicks the valve: Off -> Low -> High -> Off.
    pub fn next(self) -> Level {
        match self {
            Level::Off => Level::Low,
            Level::Low => Level::High,
            Level::High => Level::Off,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Off => "Off",
            Level::Low => "Low",
            Level::High => "High",
        }
    }
}

/// The level that the player set for each pathway. A pathway with no entry is
/// at High (normal).
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct PathwayLevels(pub HashMap<BlockKind, Level>);

impl PathwayLevels {
    pub fn level(&self, kind: BlockKind) -> Level {
        self.0.get(&kind).copied().unwrap_or(Level::High)
    }
}

/// The throttle of a block: the level that the player set, after the ramp.
/// 1 is normal. The solver multiplies the block's `Drive` by it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Throttle(pub f32);

impl Default for Throttle {
    fn default() -> Self {
        Self(HIGH_LEVEL)
    }
}

/// One step of `current` towards `target` at `rate` per second. It stops at the
/// target, so it never overshoots and the tick length does not matter.
pub fn ramp(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let step = (rate * dt).max(0.0);
    let gap = target - current;
    if gap.abs() <= step {
        target
    } else {
        current + step * gap.signum()
    }
}

/// Sets the level of a pathway and returns the ATP cost. The same level again
/// is free. Refused if the gene is missing, silent or mutated, or if there is
/// too little ATP (then nothing changes).
pub fn set_pathway_level(
    levels: &mut PathwayLevels,
    pools: &mut CurrencyPools,
    genome: &Genome,
    kind: BlockKind,
    level: Level,
) -> Result<f32, Refused> {
    if !matches!(
        genome.get_gene_state(&kind),
        Some(GeneState::Expressed | GeneState::Enhanced)
    ) {
        return Err(Refused::NoGene);
    }
    if levels.level(kind) == level {
        return Ok(0.0);
    }
    if pools.get(Currency::ATP) < LEVEL_CHANGE_ATP {
        return Err(Refused::NotEnoughAtp);
    }
    levels.0.insert(kind, level);
    pools.modify(Currency::ATP, -LEVEL_CHANGE_ATP);
    Ok(LEVEL_CHANGE_ATP)
}

// --- The bar ---

/// An active skill. A slot can hold all of them except the flagellum burst,
/// which has its own key (see `available_actions`). Pathway levels are not
/// here (see `SetPathwayLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    FlagellumBurst,
    Adhesin,
    Toxin,
    Pilus,
    Injector,
    Lyse,
    InduceMutation,
}

impl ActionKind {
    /// Does the action use the aim? The Run screen shows the aim line while
    /// such a slot is held.
    pub fn needs_aim(self) -> bool {
        matches!(
            self,
            ActionKind::Toxin | ActionKind::Pilus | ActionKind::Injector | ActionKind::Lyse
        )
    }

    /// The membrane protein that the action needs, if any.
    pub fn required_protein(self) -> Option<MembraneProtein> {
        match self {
            ActionKind::FlagellumBurst => Some(MembraneProtein::Flagellum),
            ActionKind::Adhesin => Some(MembraneProtein::Adhesin),
            ActionKind::Toxin => Some(MembraneProtein::Toxin),
            ActionKind::Pilus => Some(MembraneProtein::Pilus),
            ActionKind::Injector => Some(MembraneProtein::Injector),
            ActionKind::Lyse => Some(MembraneProtein::Lytic),
            _ => None,
        }
    }

    pub fn name(self) -> String {
        match self {
            ActionKind::FlagellumBurst => "Flagellum burst".into(),
            ActionKind::Adhesin => "Adhesin".into(),
            ActionKind::Toxin => "Toxin".into(),
            ActionKind::Pilus => "Pilus".into(),
            ActionKind::Injector => "Injector".into(),
            ActionKind::Lyse => "Lyse".into(),
            ActionKind::InduceMutation => "Induce mutation".into(),
        }
    }
}

/// The action slots of a cell.
#[derive(Component, Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionBar {
    pub slots: [Option<ActionKind>; SLOT_COUNT],
}

/// The bar of a new vent cell: induced mutation on the forward side button. The
/// other slots are free for the actions of membrane proteins.
pub fn starter_action_bar() -> ActionBar {
    ActionBar {
        slots: [None, None, None, Some(ActionKind::InduceMutation)],
    }
}

/// Everything the cell can put in a slot now: the actions of its membrane
/// proteins, then induced mutation. The flagellum burst is not in the list: it
/// is on Shift.
pub fn available_actions(proteome: &Proteome) -> Vec<ActionKind> {
    let mut out = Vec::new();
    for action in [
        ActionKind::Adhesin,
        ActionKind::Toxin,
        ActionKind::Pilus,
        ActionKind::Injector,
        ActionKind::Lyse,
    ] {
        let protein = action.required_protein().expect("these actions need a protein");
        if proteome.count(ProteinTarget::Membrane(protein)) > 0 {
            out.push(action);
        }
    }
    out.push(ActionKind::InduceMutation);
    out
}

/// The next content of a slot when the player cycles it: empty, then each
/// option in order, then empty again. A content that is not in the options
/// (for example the action of a recycled protein) is cleared.
pub fn next_assignment(current: Option<ActionKind>, options: &[ActionKind]) -> Option<ActionKind> {
    match current {
        None => options.first().copied(),
        Some(kind) => options
            .iter()
            .position(|o| *o == kind)
            .and_then(|i| options.get(i + 1).copied()),
    }
}

// --- Runtime state of the actions ---

/// Timers and flags of the membrane actions.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ActionState {
    /// Seconds of burst left.
    pub burst_left: f32,
    /// Seconds until the next burst is possible.
    pub burst_cooldown: f32,
    pub adhesin_held: bool,
    /// Seconds since the adhesin grabbed.
    pub held_for: f32,
    /// Seconds left of the toxin overlay's `use` clip.
    pub toxin_flash: f32,
    /// Seconds left of the pilus overlay's `use` clip (the extension).
    pub pilus_flash: f32,
    /// True while a pilus is attached and held (the overlay loops `hold`).
    pub pilus_taut: bool,
    /// True while a held pilus retracts (see `pilus::reel_step`).
    pub pilus_reeling: bool,
    /// Seconds left of the injector overlay's `use` clip.
    pub injector_flash: f32,
    /// Seconds until the injector can fire again.
    pub injector_cooldown: f32,
    /// The aim (a unit vector). Shots, lysis, the pilus and the injector use
    /// it. On the Run screen it follows the cursor (see `aim_toward`); without
    /// a cursor it is the last move direction.
    pub aim: Vec2,
    /// The last move direction (a unit vector). The flagellum trails it, and a
    /// burst without input goes along it.
    pub heading: Vec2,
    /// True while the body touches a wall tile (set by `walls`).
    pub on_wall: bool,
}

impl Default for ActionState {
    fn default() -> Self {
        Self {
            burst_left: 0.0,
            burst_cooldown: 0.0,
            adhesin_held: false,
            held_for: 0.0,
            toxin_flash: 0.0,
            pilus_flash: 0.0,
            pilus_taut: false,
            pilus_reeling: false,
            injector_flash: 0.0,
            injector_cooldown: 0.0,
            aim: Vec2::X,
            heading: Vec2::X,
            on_wall: false,
        }
    }
}

impl ActionState {
    /// The body ignores walls and pores: an adhesin holds while it touches a
    /// wall, so the cell crawls through it (see `walls`).
    pub fn ghosting(&self) -> bool {
        self.adhesin_held && self.on_wall
    }

    /// The pilus is no longer attached.
    pub fn let_go(&mut self) {
        self.pilus_taut = false;
        self.pilus_reeling = false;
    }

    pub fn bursting(&self) -> bool {
        self.burst_left > 0.0
    }

    /// A burst can start: none is on, the cooldown is over, nothing holds.
    pub fn can_burst(&self) -> bool {
        !self.bursting() && self.burst_cooldown <= 0.0 && !self.adhesin_held
    }

    pub fn thrust_multiplier(&self) -> f32 {
        if self.bursting() {
            BURST_THRUST_MULTIPLIER
        } else {
            1.0
        }
    }

    /// Moves the timers on by `dt` seconds. The cooldown starts when the burst
    /// ends, also in the middle of a long step.
    pub fn advance(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        let mut rest = dt;
        if self.burst_left > 0.0 {
            let used = rest.min(self.burst_left);
            self.burst_left -= used;
            rest -= used;
            if self.burst_left <= 0.0 {
                self.burst_left = 0.0;
                self.burst_cooldown = BURST_COOLDOWN_SECONDS;
            }
        }
        self.burst_cooldown = (self.burst_cooldown - rest).max(0.0);
        self.toxin_flash = (self.toxin_flash - dt).max(0.0);
        self.pilus_flash = (self.pilus_flash - dt).max(0.0);
        self.injector_flash = (self.injector_flash - dt).max(0.0);
        self.injector_cooldown = (self.injector_cooldown - dt).max(0.0);
        if self.adhesin_held {
            self.held_for += dt;
        }
    }

    /// Sets the heading from a move input, and the aim too (the cursor then
    /// overrides the aim). A zero input keeps both.
    pub fn steer(&mut self, input: Vec2) {
        if input.length_squared() > 1e-6 {
            self.heading = input.normalize();
            self.aim = self.heading;
        }
    }

    /// Points the aim from `from` to the world position `cursor`. A cursor too
    /// close to `from` keeps the old aim.
    pub fn aim_toward(&mut self, from: Vec2, cursor: Vec2) {
        self.aim = aim_toward(from, cursor, self.aim);
    }
}

/// Which action slots the player holds down now. The Run screen sets it from the
/// mouse buttons. Press actions use `UseSlot`; the pilus also reads the hold.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotHeld(pub [bool; SLOT_COUNT]);

/// The aimed proteins of a cell, the most recently used first. The protein in
/// front points exactly at the cursor; the others sit a little to the side
/// (`world::actions_view::cluster_row`). Using an aimed slot moves its protein
/// to the front.
#[derive(Component, Default, Debug, Clone, PartialEq, Eq)]
pub struct AimOrder(pub Vec<MembraneProtein>);

impl AimOrder {
    /// Moves `protein` to the front.
    pub fn bump(&mut self, protein: MembraneProtein) {
        self.0.retain(|p| *p != protein);
        self.0.insert(0, protein);
    }

    /// The place of each protein of `shown` in the cluster: the used ones by
    /// recency, then the never used ones in the order of `shown`. Returns the
    /// rank of `protein`, or `None` if it is not in `shown`.
    pub fn rank(&self, shown: &[MembraneProtein], protein: MembraneProtein) -> Option<usize> {
        let mut ranked: Vec<MembraneProtein> = self.0.iter().copied().filter(|p| shown.contains(p)).collect();
        for p in shown {
            if !ranked.contains(p) {
                ranked.push(*p);
            }
        }
        ranked.iter().position(|p| *p == protein)
    }
}

/// Cursor distance in pixels below which the aim does not change.
pub const AIM_DEAD_ZONE: f32 = 2.0;

/// The unit aim from `from` to `cursor`, or `fallback` if the cursor is in the
/// dead zone.
pub fn aim_toward(from: Vec2, cursor: Vec2, fallback: Vec2) -> Vec2 {
    let offset = cursor - from;
    if offset.length() < AIM_DEAD_ZONE {
        fallback
    } else {
        offset.normalize()
    }
}

/// The thrust force of a body. A held adhesin ignores the input. During a burst
/// the thrust is multiplied, and without input the burst goes along the
/// heading.
pub fn action_thrust(input: Vec2, capacity: f32, state: &ActionState) -> Vec2 {
    if state.adhesin_held {
        return Vec2::ZERO;
    }
    let direction = if state.bursting() && input.length_squared() < 1e-6 {
        state.heading
    } else {
        input
    };
    thrust_force(direction, capacity * state.thrust_multiplier())
}

/// Linear damping of the body: high while the adhesin holds.
pub fn action_damping(base: f32, state: &ActionState) -> f32 {
    if state.adhesin_held {
        base.max(ADHESIN_DAMPING)
    } else {
        base
    }
}

/// ATP that a held adhesin uses in `dt` seconds.
pub fn adhesin_cost(dt: f32) -> f32 {
    ADHESIN_ATP_PER_SEC * dt.max(0.0)
}

/// Opacity of a toxin droplet `age` seconds after the shot: it fades out.
pub fn droplet_alpha(age: f32) -> f32 {
    (1.0 - age / TOXIN_LIFETIME).clamp(0.0, 1.0)
}

/// One line of text for a slot: what it holds and its cooldown. For the HUD and
/// the editor.
pub fn slot_status(
    kind: Option<ActionKind>,
    state: &ActionState,
    proteome: &Proteome,
) -> String {
    let Some(kind) = kind else {
        return "empty".into();
    };
    if let Some(protein) = kind.required_protein()
        && !has_protein(proteome, protein)
    {
        return format!("{}: no protein", kind.name());
    }
    let detail = match kind {
        ActionKind::FlagellumBurst if state.bursting() => format!("BURST {:.1}s", state.burst_left),
        ActionKind::FlagellumBurst if state.burst_cooldown > 0.0 => {
            format!("cooldown {:.1}s", state.burst_cooldown)
        }
        ActionKind::FlagellumBurst => format!("ready ({BURST_ATP} ATP)"),
        ActionKind::Adhesin if state.adhesin_held => "HOLDING".into(),
        ActionKind::Adhesin => "ready".into(),
        ActionKind::Toxin => format!("{TOXIN_ATP} ATP + {TOXIN_METALS} Metals"),
        ActionKind::Pilus if state.pilus_reeling => "REELING".into(),
        ActionKind::Pilus if state.pilus_taut => "HOLDING (wheel to reel)".into(),
        ActionKind::Pilus => format!(
            "press to shoot ({} ATP), hold, wheel to reel",
            crate::pilus::PILUS_EXTEND_ATP
        ),
        ActionKind::Injector if state.injector_cooldown > 0.0 => {
            format!("cooldown {:.1}s", state.injector_cooldown)
        }
        ActionKind::Injector => format!(
            "ready ({} ATP + {} Metals)",
            crate::stun::INJECTOR_ATP,
            crate::stun::INJECTOR_METALS
        ),
        ActionKind::Lyse => format!("{} ATP", crate::walls::LYSE_ATP),
        ActionKind::InduceMutation => format!("{INDUCE_ATP} ATP, risky"),
    };
    format!("{}: {detail}", kind.name())
}

/// The HUD line of a slot: its binding and its status.
pub fn slot_line(slot: usize, bar: &ActionBar, state: &ActionState, proteome: &Proteome) -> String {
    let kind = bar.slots.get(slot).copied().flatten();
    format!("[{}] {}", SLOT_LABELS[slot], slot_status(kind, state, proteome))
}

/// The HUD line of the flagellum burst. Empty without a flagellum.
pub fn burst_line(state: &ActionState, proteome: &Proteome) -> String {
    if !has_protein(proteome, MembraneProtein::Flagellum) {
        return String::new();
    }
    format!(
        "[{BURST_LABEL}] {}",
        slot_status(Some(ActionKind::FlagellumBurst), state, proteome)
    )
}

// --- Using a slot ---

/// Why a use was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The cell has no protein for the action.
    NoProtein,
    /// The gene of the pathway is missing, silent or mutated.
    NoGene,
    NotEnoughAtp,
    NotEnoughMetals,
    Cooldown,
    /// No expressed gene can mutate.
    NoGeneToMutate,
}

/// What a use did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UseResult {
    Done,
    /// A toxin shot: spawn a droplet that flies in this direction.
    Fired(Vec2),
    /// A lysis order: the walls system cuts the tile in contact (and charges
    /// the ATP) if there is one.
    Lyse(Vec2),
    Refused(Refused),
}

/// The parts of a cell that an action changes.
pub struct ActionTarget<'a> {
    pub pools: &'a mut CurrencyPools,
    pub state: &'a mut ActionState,
    pub load: &'a mut MutationLoad,
    pub induced: &'a mut InducedMutations,
    pub genome: &'a Genome,
    pub proteome: &'a Proteome,
}

fn has_protein(proteome: &Proteome, protein: MembraneProtein) -> bool {
    proteome.count(ProteinTarget::Membrane(protein)) > 0
}

/// Does one use of an action.
pub fn use_action(kind: ActionKind, target: &mut ActionTarget) -> UseResult {
    if let Some(protein) = kind.required_protein()
        && !has_protein(target.proteome, protein)
    {
        return UseResult::Refused(Refused::NoProtein);
    }
    let atp = target.pools.get(Currency::ATP);
    match kind {
        ActionKind::FlagellumBurst => {
            if !target.state.can_burst() {
                return UseResult::Refused(Refused::Cooldown);
            }
            if atp < BURST_ATP {
                return UseResult::Refused(Refused::NotEnoughAtp);
            }
            target.pools.modify(Currency::ATP, -BURST_ATP);
            target.state.burst_left = BURST_SECONDS;
            UseResult::Done
        }
        ActionKind::Adhesin => {
            if target.state.adhesin_held {
                target.state.adhesin_held = false;
                target.state.held_for = 0.0;
                UseResult::Done
            } else if atp <= 0.0 {
                UseResult::Refused(Refused::NotEnoughAtp)
            } else {
                target.state.adhesin_held = true;
                target.state.held_for = 0.0;
                target.state.burst_left = 0.0;
                UseResult::Done
            }
        }
        ActionKind::Toxin => {
            if atp < TOXIN_ATP {
                return UseResult::Refused(Refused::NotEnoughAtp);
            }
            if target.pools.get(Currency::Metals) < TOXIN_METALS {
                return UseResult::Refused(Refused::NotEnoughMetals);
            }
            target.pools.modify(Currency::ATP, -TOXIN_ATP);
            target.pools.modify(Currency::Metals, -TOXIN_METALS);
            target.state.toxin_flash = TOXIN_FLASH_SECONDS;
            UseResult::Fired(target.state.aim)
        }
        // The pilus and the injector need positions, so `pilus` and `stun` do
        // these uses. They read the same `UseSlot` message.
        ActionKind::Pilus | ActionKind::Injector => UseResult::Done,
        ActionKind::Lyse => {
            if atp < crate::walls::LYSE_ATP {
                return UseResult::Refused(Refused::NotEnoughAtp);
            }
            UseResult::Lyse(target.state.aim)
        }
        ActionKind::InduceMutation => {
            if target.genome.get_mutable_genes().is_empty() {
                return UseResult::Refused(Refused::NoGeneToMutate);
            }
            if atp < INDUCE_ATP {
                return UseResult::Refused(Refused::NotEnoughAtp);
            }
            target.pools.modify(Currency::ATP, -INDUCE_ATP);
            target.load.0 += INDUCE_LOAD;
            target.induced.0 += 1;
            UseResult::Done
        }
    }
}

// --- Messages and systems ---

/// Moves the protein of each used aimed slot to the front of the `AimOrder`.
pub fn aim_order_system(mut orders: MessageReader<UseSlot>, mut cells: Query<(&ActionBar, &mut AimOrder)>) {
    for order in orders.read() {
        let Ok((bar, mut aim_order)) = cells.get_mut(order.cell) else {
            continue;
        };
        let Some(kind) = bar.slots.get(order.slot).copied().flatten() else {
            continue;
        };
        if let Some(protein) = kind.required_protein().filter(|_| kind.needs_aim())
            && aim_order.0.first() != Some(&protein)
        {
            aim_order.bump(protein);
        }
    }
}

/// Asks a cell to use an action slot (0-based).
#[derive(Message, Debug, Clone, Copy)]
pub struct UseSlot {
    pub cell: Entity,
    pub slot: usize,
}

/// Asks a cell to set the level of a pathway. The metabolic map of the Manage
/// screen sends it.
#[derive(Message, Debug, Clone, Copy)]
pub struct SetPathwayLevel {
    pub cell: Entity,
    pub kind: BlockKind,
    pub level: Level,
}

/// Asks a cell to use an action that is not in a slot: the flagellum burst on
/// Shift.
#[derive(Message, Debug, Clone, Copy)]
pub struct UseAction {
    pub cell: Entity,
    pub kind: ActionKind,
}

/// A toxin shot. The world spawns a droplet.
#[derive(Message, Debug, Clone, Copy)]
pub struct FireToxin {
    pub cell: Entity,
    pub direction: Vec2,
}

/// A lysis order. The walls system cuts the tile in contact.
#[derive(Message, Debug, Clone, Copy)]
pub struct LyseOrder {
    pub cell: Entity,
    pub direction: Vec2,
}

/// A toxin droplet in flight. `position` is exact; the transform of its sprite
/// is the nearest whole pixel (see `world::actions_view`). The `npc` module
/// checks hits.
#[derive(Component, Debug, Clone, Copy)]
pub struct ToxinDroplet {
    /// The cell that fired it. It cannot hit that cell.
    pub owner: Entity,
    pub position: Vec2,
    pub direction: Vec2,
    pub age: f32,
}

/// Applies `UseSlot` and `UseAction` messages.
#[allow(clippy::type_complexity)]
pub fn use_slot_system(
    mut orders: MessageReader<UseSlot>,
    mut direct: MessageReader<UseAction>,
    mut fired: MessageWriter<FireToxin>,
    mut lyse: MessageWriter<LyseOrder>,
    mut cells: Query<
        (
            &ActionBar,
            &mut CurrencyPools,
            &mut FlowLedger,
            &mut ActionState,
            &mut MutationLoad,
            &mut InducedMutations,
            &Genome,
            &Proteome,
        ),
        With<Cell>,
    >,
) {
    let mut uses: Vec<(Entity, ActionKind)> = orders
        .read()
        .filter_map(|order| {
            let (bar, ..) = cells.get(order.cell).ok()?;
            Some((order.cell, bar.slots.get(order.slot).copied().flatten()?))
        })
        .collect();
    uses.extend(direct.read().map(|order| (order.cell, order.kind)));
    for (cell, kind) in uses {
        let Ok((
            _,
            mut pools,
            mut ledger,
            mut state,
            mut load,
            mut induced,
            genome,
            proteome,
        )) = cells.get_mut(cell)
        else {
            continue;
        };
        let before = pools.clone();
        let mut target = ActionTarget {
            pools: &mut pools,
            state: &mut state,
            load: &mut load,
            induced: &mut induced,
            genome,
            proteome,
        };
        let result = use_action(kind, &mut target);
        ledger.record_diff(FlowSource::Action, &before, &pools);
        match result {
            UseResult::Fired(direction) => {
                fired.write(FireToxin {
                    cell,
                    direction,
                });
            }
            UseResult::Lyse(direction) => {
                lyse.write(LyseOrder {
                    cell,
                    direction,
                });
            }
            UseResult::Refused(why) => debug!("{} refused: {why:?}", kind.name()),
            UseResult::Done => {}
        }
    }
}

/// Applies `SetPathwayLevel` messages.
pub fn set_pathway_level_system(
    mut orders: MessageReader<SetPathwayLevel>,
    mut cells: Query<(&mut PathwayLevels, &mut CurrencyPools, &mut FlowLedger, &Genome), With<Cell>>,
) {
    for order in orders.read() {
        let Ok((mut levels, mut pools, mut ledger, genome)) = cells.get_mut(order.cell) else {
            continue;
        };
        let before = pools.clone();
        if let Err(why) = set_pathway_level(&mut levels, &mut pools, genome, order.kind, order.level) {
            debug!("Level of {:?} refused: {why:?}", order.kind);
        }
        ledger.record_diff(FlowSource::Action, &before, &pools);
    }
}

/// Moves the action timers on, in fixed steps (the same clock as the physics).
pub fn advance_action_state_system(time: Res<Time>, mut cells: Query<&mut ActionState, With<Cell>>) {
    for mut state in &mut cells {
        state.advance(time.delta_secs());
    }
}

/// Moves each block's throttle towards the level of its pathway.
pub fn throttle_ramp_system(
    timing: Res<FlowmapTimingConfig>,
    cells: Query<(&PathwayLevels, &CellTick), With<Cell>>,
    mut blocks: Query<(&OwnedBy, &MetabolicNode, &mut Throttle)>,
) {
    for (owner, node, mut throttle) in &mut blocks {
        let Ok((levels, tick)) = cells.get(owner.0) else {
            continue;
        };
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let level = levels.level(node.kind);
        let next = ramp(throttle.0, level.value(), RAMP_PER_SEC, dt);
        if next != throttle.0 {
            throttle.0 = next;
        }
    }
}

/// A held adhesin pays ATP each tick. With no ATP left, it lets go.
pub fn adhesin_upkeep_system(
    timing: Res<FlowmapTimingConfig>,
    mut cells: Query<(&mut ActionState, &mut CurrencyPools, &mut FlowLedger, &CellTick), With<Cell>>,
) {
    for (mut state, mut pools, mut ledger, tick) in &mut cells {
        let dt = tick.dt(timing.target_delta());
        if !state.adhesin_held || !tick.due {
            continue;
        }
        let cost = adhesin_cost(dt);
        if pools.get(Currency::ATP) < cost {
            state.adhesin_held = false;
            state.held_for = 0.0;
        } else {
            ledger.apply(&mut pools, FlowSource::Action, Currency::ATP, -cost);
        }
    }
}

pub struct ActionsPlugin;

impl Plugin for ActionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UseSlot>()
            .add_message::<UseAction>()
            .add_message::<SetPathwayLevel>()
            .add_message::<FireToxin>()
            .add_message::<LyseOrder>()
            .add_systems(Update, (use_slot_system, set_pathway_level_system, aim_order_system))
            .add_systems(FixedUpdate, advance_action_state_system)
            .add_systems(
                crate::metabolism::MetabolicSchedule,
                (
                    throttle_ramp_system.in_set(MetabolicSet::Regulate),
                    adhesin_upkeep_system.in_set(MetabolicSet::Exchange),
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn the_last_used_aimed_protein_leads_the_cluster() {
        use MembraneProtein::*;
        let shown = [Toxin, Pilus, Injector];
        let mut order = AimOrder::default();
        // Never used: the order of the plan.
        assert_eq!(order.rank(&shown, Toxin), Some(0));
        assert_eq!(order.rank(&shown, Injector), Some(2));
        order.bump(Injector);
        assert_eq!(order.rank(&shown, Injector), Some(0));
        assert_eq!(order.rank(&shown, Toxin), Some(1));
        order.bump(Pilus);
        order.bump(Injector);
        assert_eq!(order.0, vec![Injector, Pilus]);
        assert_eq!(order.rank(&shown, Pilus), Some(1));
        assert_eq!(order.rank(&shown, Toxin), Some(2));
        // A protein not shown has no place; a used one that is gone is skipped.
        assert_eq!(order.rank(&shown, Lytic), None);
        assert_eq!(order.rank(&[Toxin], Toxin), Some(0));
    }

    fn pools(atp: f32, metals: f32) -> CurrencyPools {
        let mut p = CurrencyPools::default();
        p.set(Currency::ATP, atp);
        p.set(Currency::Metals, metals);
        p
    }

    struct Rig {
        pools: CurrencyPools,
        state: ActionState,
        load: MutationLoad,
        induced: InducedMutations,
        genome: Genome,
        proteome: Proteome,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                pools: pools(100.0, 5.0),
                state: ActionState::default(),
                load: MutationLoad::default(),
                induced: InducedMutations::default(),
                genome: crate::blocks::genome::create_starter_genome(),
                proteome: Proteome::default(),
            }
        }

        fn use_action(&mut self, kind: ActionKind) -> UseResult {
            use_action(
                kind,
                &mut ActionTarget {
                    pools: &mut self.pools,
                    state: &mut self.state,
                    load: &mut self.load,
                    induced: &mut self.induced,
                    genome: &self.genome,
                    proteome: &self.proteome,
                },
            )
        }
    }

    #[test]
    fn a_pathway_without_a_level_runs_at_high() {
        assert_eq!(PathwayLevels::default().level(BlockKind::Chemosynthesis), Level::High);
    }

    #[test]
    fn levels_cycle_off_low_high() {
        assert_eq!(Level::Off.next(), Level::Low);
        assert_eq!(Level::Low.next(), Level::High);
        assert_eq!(Level::High.next(), Level::Off);
        assert_eq!(Level::Off.value(), 0.0);
        assert_eq!(Level::Low.value(), 0.5);
        assert_eq!(Level::High.value(), 1.0);
    }

    #[test]
    fn a_level_change_costs_atp_and_needs_atp() {
        let mut rig = Rig::new();
        let chemo = BlockKind::Chemosynthesis;
        let mut levels = PathwayLevels::default();
        let set = |levels: &mut PathwayLevels, rig: &mut Rig, level| {
            set_pathway_level(levels, &mut rig.pools, &rig.genome, chemo, level)
        };
        assert_eq!(set(&mut levels, &mut rig, Level::Off), Ok(LEVEL_CHANGE_ATP));
        assert_eq!(levels.level(chemo), Level::Off);
        assert_abs_diff_eq!(rig.pools.get(Currency::ATP), 100.0 - LEVEL_CHANGE_ATP);
        // The same level again is free.
        assert_eq!(set(&mut levels, &mut rig, Level::Off), Ok(0.0));
        assert_abs_diff_eq!(rig.pools.get(Currency::ATP), 100.0 - LEVEL_CHANGE_ATP);

        rig.pools.set(Currency::ATP, 0.5);
        assert_eq!(set(&mut levels, &mut rig, Level::Low), Err(Refused::NotEnoughAtp));
        assert_eq!(levels.level(chemo), Level::Off);
        assert_eq!(rig.pools.get(Currency::ATP), 0.5, "nothing is paid");
    }

    #[test]
    fn a_silent_gene_has_no_level() {
        let mut rig = Rig::new();
        let mut levels = PathwayLevels::default();
        // Fermentation is in the starter genome but silent.
        assert_eq!(
            set_pathway_level(&mut levels, &mut rig.pools, &rig.genome, BlockKind::Fermentation, Level::Off),
            Err(Refused::NoGene)
        );
        assert_eq!(levels.level(BlockKind::Fermentation), Level::High);
    }

    #[test]
    fn the_ramp_reaches_the_target_without_overshoot_for_any_tick_length() {
        for dt in [0.01, 0.2, 0.5, 3.0] {
            let mut value = 1.0;
            let mut t = 0.0;
            while value > 0.0 {
                let next = ramp(value, 0.0, RAMP_PER_SEC, dt);
                assert!(next >= 0.0 && next <= value, "dt {dt}");
                value = next;
                t += dt;
            }
            // One unit at 1 per second: a second, up to one step.
            assert!(t >= 1.0 - 1e-3 && t <= 1.0 + dt + 1e-3, "dt {dt}: {t}");
        }
        // Up and down hit the target exactly.
        assert_eq!(ramp(0.0, 0.5, 1.0, 10.0), 0.5);
        assert_eq!(ramp(1.0, 0.5, 1.0, 0.2), 0.8);
        assert_eq!(ramp(0.3, 0.3, 1.0, 0.2), 0.3);
    }

    #[test]
    fn available_actions_follow_the_proteome() {
        let mut proteome = Proteome::default();
        assert_eq!(available_actions(&proteome), vec![ActionKind::InduceMutation]);
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Toxin));
        // The flagellum burst is on Shift, not in a slot.
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        assert_eq!(
            available_actions(&proteome),
            vec![ActionKind::Toxin, ActionKind::InduceMutation]
        );
    }

    #[test]
    fn cycling_a_slot_visits_every_option_then_empties() {
        let options = [ActionKind::Adhesin, ActionKind::Toxin];
        let mut slot = None;
        slot = next_assignment(slot, &options);
        assert_eq!(slot, Some(ActionKind::Adhesin));
        slot = next_assignment(slot, &options);
        assert_eq!(slot, Some(ActionKind::Toxin));
        slot = next_assignment(slot, &options);
        assert_eq!(slot, None);
        assert_eq!(next_assignment(None, &[]), None);
        // A content that left the options is cleared.
        assert_eq!(next_assignment(Some(ActionKind::FlagellumBurst), &options), None);
    }

    #[test]
    fn a_burst_triples_the_thrust_for_its_time_then_has_a_cooldown() {
        let mut rig = Rig::new();
        rig.proteome
            .add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        assert_eq!(rig.use_action(ActionKind::FlagellumBurst), UseResult::Done);
        assert_abs_diff_eq!(rig.pools.get(Currency::ATP), 100.0 - BURST_ATP);

        let dt = 1.0 / 64.0;
        let mut boosted = 0.0;
        let mut cooldown_seen = false;
        for _ in 0..(8.0 / dt) as usize {
            let force = action_thrust(Vec2::X, 300.0, &rig.state);
            if rig.state.bursting() {
                assert_abs_diff_eq!(force.x, 900.0);
                boosted += dt;
            } else {
                assert_abs_diff_eq!(force.x, 300.0);
            }
            if !rig.state.bursting() && rig.state.burst_cooldown > 0.0 {
                cooldown_seen = true;
                assert_eq!(
                    rig.use_action(ActionKind::FlagellumBurst),
                    UseResult::Refused(Refused::Cooldown)
                );
            }
            rig.state.advance(dt);
        }
        assert!((boosted - BURST_SECONDS).abs() <= dt, "boosted {boosted}");
        assert!(cooldown_seen);
        // 8 s later the cooldown is over: 1.5 + 4 < 8.
        assert_eq!(rig.use_action(ActionKind::FlagellumBurst), UseResult::Done);
    }

    #[test]
    fn the_burst_and_cooldown_do_not_depend_on_the_step_length() {
        for dt in [0.01, 0.2, 1.0, 10.0] {
            let mut state = ActionState {
                burst_left: BURST_SECONDS,
                ..default()
            };
            let mut t = 0.0;
            while state.bursting() {
                state.advance(dt);
                t += dt;
            }
            assert!(t >= BURST_SECONDS - 1e-3 && t < BURST_SECONDS + dt + 1e-3);
        }
        // One long step across the end of the burst: cooldown starts at its end.
        let mut state = ActionState {
            burst_left: 1.0,
            ..default()
        };
        state.advance(2.0);
        assert_abs_diff_eq!(state.burst_cooldown, BURST_COOLDOWN_SECONDS - 1.0, epsilon = 1e-5);
    }

    #[test]
    fn a_burst_without_input_goes_along_the_heading_not_the_cursor() {
        let mut state = ActionState::default();
        state.steer(Vec2::new(0.0, 5.0));
        // The cursor is to the west; the cell last moved north.
        state.aim_toward(Vec2::ZERO, Vec2::new(-100.0, 0.0));
        state.burst_left = 1.0;
        assert_eq!(action_thrust(Vec2::ZERO, 300.0, &state), Vec2::new(0.0, 900.0));
        state.burst_left = 0.0;
        assert_eq!(action_thrust(Vec2::ZERO, 300.0, &state), Vec2::ZERO);
        state.steer(Vec2::ZERO);
        assert_eq!(state.heading, Vec2::Y, "zero input keeps the heading");
        assert_eq!(state.aim, Vec2::NEG_X, "zero input keeps the aim");
    }

    #[test]
    fn actions_need_their_protein() {
        let mut rig = Rig::new();
        for kind in [ActionKind::FlagellumBurst, ActionKind::Adhesin, ActionKind::Toxin] {
            assert_eq!(rig.use_action(kind), UseResult::Refused(Refused::NoProtein));
        }
        assert_eq!(rig.pools.get(Currency::ATP), 100.0);
    }

    #[test]
    fn the_adhesin_holds_the_body_and_costs_atp_each_second() {
        let mut rig = Rig::new();
        rig.proteome
            .add(ProteinTarget::Membrane(MembraneProtein::Adhesin));
        assert_eq!(rig.use_action(ActionKind::Adhesin), UseResult::Done);
        assert!(rig.state.adhesin_held);
        assert_eq!(action_thrust(Vec2::X, 300.0, &rig.state), Vec2::ZERO);
        assert_eq!(action_damping(6.0, &rig.state), ADHESIN_DAMPING);
        assert_abs_diff_eq!(adhesin_cost(5.0), 1.0, epsilon = 1e-6);
        // A second use lets go, at no cost.
        let atp = rig.pools.get(Currency::ATP);
        assert_eq!(rig.use_action(ActionKind::Adhesin), UseResult::Done);
        assert!(!rig.state.adhesin_held);
        assert_eq!(rig.pools.get(Currency::ATP), atp);
        assert_eq!(action_damping(6.0, &rig.state), 6.0);
        assert!(action_thrust(Vec2::X, 300.0, &rig.state).x > 0.0);
    }

    #[test]
    fn a_toxin_shot_costs_atp_and_metals_and_goes_along_the_aim() {
        let mut rig = Rig::new();
        rig.proteome
            .add(ProteinTarget::Membrane(MembraneProtein::Toxin));
        rig.state.aim_toward(Vec2::ZERO, Vec2::new(-50.0, 0.0));
        assert_eq!(
            rig.use_action(ActionKind::Toxin),
            UseResult::Fired(Vec2::NEG_X)
        );
        assert_abs_diff_eq!(rig.pools.get(Currency::ATP), 100.0 - TOXIN_ATP);
        assert_abs_diff_eq!(rig.pools.get(Currency::Metals), 5.0 - TOXIN_METALS);
        rig.pools.set(Currency::Metals, 0.5);
        assert_eq!(
            rig.use_action(ActionKind::Toxin),
            UseResult::Refused(Refused::NotEnoughMetals)
        );
    }

    #[test]
    fn slot_text_names_the_cooldown_or_missing_protein() {
        let mut state = ActionState::default();
        let mut proteome = Proteome::default();
        assert_eq!(slot_status(None, &state, &proteome), "empty");
        let burst = Some(ActionKind::FlagellumBurst);
        assert!(slot_status(burst, &state, &proteome).contains("no protein"));
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        assert!(slot_status(burst, &state, &proteome).contains("ready"));
        state.burst_left = 1.0;
        assert!(slot_status(burst, &state, &proteome).contains("BURST"));
        state.burst_left = 0.0;
        state.burst_cooldown = 2.5;
        assert!(slot_status(burst, &state, &proteome).contains("cooldown 2.5"));
    }

    #[test]
    fn hud_lines_name_the_binding() {
        let state = ActionState::default();
        let mut proteome = Proteome::default();
        let bar = starter_action_bar();
        assert_eq!(slot_line(0, &bar, &state, &proteome), "[LMB] empty");
        assert!(slot_line(3, &bar, &state, &proteome).starts_with("[M5] Induce mutation"));
        assert_eq!(burst_line(&state, &proteome), "", "no flagellum, no line");
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        assert!(burst_line(&state, &proteome).starts_with("[Shift] Flagellum burst: ready"));
    }

    #[test]
    fn a_droplet_fades_out_over_its_life() {
        assert_eq!(droplet_alpha(0.0), 1.0);
        assert!(droplet_alpha(TOXIN_LIFETIME * 0.5) < 1.0);
        assert_eq!(droplet_alpha(TOXIN_LIFETIME), 0.0);
        assert_eq!(droplet_alpha(TOXIN_LIFETIME * 3.0), 0.0);
    }

    #[test]
    fn induced_mutation_costs_atp_and_adds_one_load_unit() {
        let mut rig = Rig::new();
        assert_eq!(rig.use_action(ActionKind::InduceMutation), UseResult::Done);
        assert_abs_diff_eq!(rig.pools.get(Currency::ATP), 100.0 - INDUCE_ATP);
        assert_eq!(rig.load.0, INDUCE_LOAD);
        assert_eq!(rig.induced.0, 1);

        rig.pools.set(Currency::ATP, 9.0);
        assert_eq!(
            rig.use_action(ActionKind::InduceMutation),
            UseResult::Refused(Refused::NotEnoughAtp)
        );
        assert_eq!(rig.induced.0, 1);
    }

    #[test]
    fn the_aim_follows_the_cursor_and_keeps_the_old_aim_in_the_dead_zone() {
        let mut state = ActionState::default();
        state.aim_toward(Vec2::new(10.0, 10.0), Vec2::new(10.0, 60.0));
        assert_abs_diff_eq!(state.aim.x, 0.0, epsilon = 1e-6);
        assert_abs_diff_eq!(state.aim.y, 1.0, epsilon = 1e-6);
        // A cursor on the cell: the aim stays.
        state.aim_toward(Vec2::new(10.0, 10.0), Vec2::new(10.5, 10.5));
        assert_abs_diff_eq!(state.aim.y, 1.0, epsilon = 1e-6);
        // A cursor far away still gives a unit vector.
        state.aim_toward(Vec2::ZERO, Vec2::new(300.0, 400.0));
        assert_abs_diff_eq!(state.aim.length(), 1.0, epsilon = 1e-5);
        // Without a cursor, the move direction sets the aim (the fallback).
        state.steer(Vec2::new(-1.0, 0.0));
        assert_eq!(state.aim, Vec2::NEG_X);
        assert_eq!(state.heading, Vec2::NEG_X);
    }

    #[test]
    fn only_aimed_actions_show_the_aim_line() {
        assert!(ActionKind::Pilus.needs_aim());
        assert!(ActionKind::Injector.needs_aim());
        assert!(ActionKind::Toxin.needs_aim());
        assert!(!ActionKind::Adhesin.needs_aim());
        assert!(!ActionKind::InduceMutation.needs_aim());
    }

    #[test]
    fn the_injector_timers_run_down() {
        let mut state = ActionState {
            injector_cooldown: 1.0,
            injector_flash: 0.3,
            pilus_flash: 0.3,
            ..default()
        };
        state.advance(0.5);
        assert_abs_diff_eq!(state.injector_cooldown, 0.5, epsilon = 1e-6);
        state.advance(5.0);
        assert_eq!((state.injector_cooldown, state.injector_flash, state.pilus_flash), (0.0, 0.0, 0.0));
    }
}
