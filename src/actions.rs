//! # Live control: the action bar
//!
//! Regulation is fast, like gene expression. A cell has `SLOT_COUNT` action
//! slots (`ActionBar`). A slot holds one `ActionKind`:
//!
//! - **Pathway**: an expressed block. Using the slot cycles its level
//!   Off -> Low -> High -> Off (`Level`). The level is the target of the block's
//!   `Throttle`, which ramps to it at `RAMP_PER_SEC` (capped at the gap, so it
//!   never overshoots and does not depend on the tick length). Each level
//!   change costs `LEVEL_CHANGE_ATP`; with too little ATP it does not happen.
//!   The `Throttle` multiplies the solver's drive, so regulated blocks (fat
//!   storage, protein synthesis, refill blocks) keep setting `Drive` on their own.
//! - **Flagellum burst**: thrust x`BURST_THRUST_MULTIPLIER` for `BURST_SECONDS`,
//!   then a cooldown. Needs a flagellum.
//! - **Adhesin**: a toggle. While held, the body does not move (thrust is
//!   ignored and damping is high) and the cell pays ATP each second. Needs an
//!   adhesin.
//! - **Toxin**: fires a droplet in the aim direction (`FireToxin`). Costs ATP and
//!   Metals. Needs a toxin. There are no targets until M5.
//! - **Lyse**: cuts the wall tile in contact (`walls`). 5 ATP. Needs a lytic protein.
//! - **Induce mutation**: costs ATP, adds 1.0 to the `MutationLoad`, so a gene
//!   mutates. It has a chance to give an `Enhanced` gene (see `genome_damage`).
//!
//! Passive pathway effects stay: only the drive level uses a slot.
//!
//! The rules are pure functions and methods. The systems fetch data and call
//! them. The hotkeys and the droplet sprites are in `world` (full builds).

use std::collections::HashMap;

use bevy::prelude::*;

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

/// The level of a pathway slot.
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

    /// The level after a use of the slot: Off -> Low -> High -> Off.
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
/// at High (normal). A pathway that is not in a slot always runs at High (see
/// `effective_level`), so it cannot be stuck at Off.
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct PathwayLevels(pub HashMap<BlockKind, Level>);

impl PathwayLevels {
    pub fn level(&self, kind: BlockKind) -> Level {
        self.0.get(&kind).copied().unwrap_or(Level::High)
    }
}

/// The level a pathway runs at: the level that the player set, if the pathway
/// is in a slot of the bar. Otherwise High.
pub fn effective_level(levels: &PathwayLevels, bar: &ActionBar, kind: BlockKind) -> Level {
    if bar.slots.contains(&Some(ActionKind::Pathway(kind))) {
        levels.level(kind)
    } else {
        Level::High
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

/// A level change if the cell can pay for it: the new level and the ATP cost.
/// `None` if there is too little ATP (the level stays).
pub fn change_level(level: Level, atp: f32) -> Option<(Level, f32)> {
    (atp >= LEVEL_CHANGE_ATP).then(|| (level.next(), LEVEL_CHANGE_ATP))
}

// --- The bar ---

/// What a slot can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// An expressed block: Off / Low / High.
    Pathway(BlockKind),
    FlagellumBurst,
    Adhesin,
    Toxin,
    Lyse,
    InduceMutation,
}

impl ActionKind {
    /// The membrane protein that the action needs, if any.
    pub fn required_protein(self) -> Option<MembraneProtein> {
        match self {
            ActionKind::FlagellumBurst => Some(MembraneProtein::Flagellum),
            ActionKind::Adhesin => Some(MembraneProtein::Adhesin),
            ActionKind::Toxin => Some(MembraneProtein::Toxin),
            ActionKind::Lyse => Some(MembraneProtein::Lytic),
            _ => None,
        }
    }

    pub fn name(self) -> String {
        match self {
            ActionKind::Pathway(kind) => format!("{kind:?}"),
            ActionKind::FlagellumBurst => "Flagellum burst".into(),
            ActionKind::Adhesin => "Adhesin".into(),
            ActionKind::Toxin => "Toxin".into(),
            ActionKind::Lyse => "Lyse".into(),
            ActionKind::InduceMutation => "Induce mutation".into(),
        }
    }
}

/// The action slots of a cell.
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct ActionBar {
    pub slots: [Option<ActionKind>; SLOT_COUNT],
}

/// The bar of a new vent cell: chemosynthesis in slot 1, induced mutation in
/// slot 2.
pub fn starter_action_bar() -> ActionBar {
    ActionBar {
        slots: [
            Some(ActionKind::Pathway(BlockKind::Chemosynthesis)),
            Some(ActionKind::InduceMutation),
            None,
            None,
        ],
    }
}

/// Everything the cell can put in a slot now: its expressed blocks (in block
/// order), then the actions of its membrane proteins, then induced mutation.
pub fn available_actions(genome: &Genome, proteome: &Proteome) -> Vec<ActionKind> {
    let mut kinds: Vec<BlockKind> = genome
        .table
        .iter()
        .filter(|(kind, state)| {
            crate::blocks::block_spec(**kind).is_some()
                && matches!(state, GeneState::Expressed | GeneState::Enhanced)
        })
        .map(|(kind, _)| *kind)
        .collect();
    kinds.sort();
    let mut out: Vec<ActionKind> = kinds.into_iter().map(ActionKind::Pathway).collect();
    for action in [
        ActionKind::FlagellumBurst,
        ActionKind::Adhesin,
        ActionKind::Toxin,
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
/// (for example a pathway that mutated) goes to the first option.
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
    /// The last move direction (a unit vector). Bursts and shots use it.
    pub aim: Vec2,
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
            aim: Vec2::X,
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
        if self.adhesin_held {
            self.held_for += dt;
        }
    }

    /// Sets the aim from a move input. A zero input keeps the old aim.
    pub fn aim_at(&mut self, input: Vec2) {
        if input.length_squared() > 1e-6 {
            self.aim = input.normalize();
        }
    }
}

/// The thrust force of a body. A held adhesin ignores the input. During a burst
/// the thrust is multiplied, and without input the burst goes along the aim.
pub fn action_thrust(input: Vec2, capacity: f32, state: &ActionState) -> Vec2 {
    if state.adhesin_held {
        return Vec2::ZERO;
    }
    let direction = if state.bursting() && input.length_squared() < 1e-6 {
        state.aim
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

/// One line of text for a slot: what it holds and its level or cooldown. For the
/// HUD and the editor.
pub fn slot_status(
    kind: Option<ActionKind>,
    levels: &PathwayLevels,
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
        ActionKind::Pathway(block) => levels.level(block).name().to_string(),
        ActionKind::FlagellumBurst if state.bursting() => format!("BURST {:.1}s", state.burst_left),
        ActionKind::FlagellumBurst if state.burst_cooldown > 0.0 => {
            format!("cooldown {:.1}s", state.burst_cooldown)
        }
        ActionKind::FlagellumBurst => format!("ready ({BURST_ATP} ATP)"),
        ActionKind::Adhesin if state.adhesin_held => "HOLDING".into(),
        ActionKind::Adhesin => "ready".into(),
        ActionKind::Toxin => format!("{TOXIN_ATP} ATP + {TOXIN_METALS} Metals"),
        ActionKind::Lyse => format!("{} ATP", crate::walls::LYSE_ATP),
        ActionKind::InduceMutation => format!("{INDUCE_ATP} ATP, risky"),
    };
    format!("{}: {detail}", kind.name())
}

// --- Using a slot ---

/// Why a use was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    EmptySlot,
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
    pub levels: &'a mut PathwayLevels,
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
        ActionKind::Pathway(block) => {
            if !matches!(
                target.genome.get_gene_state(&block),
                Some(GeneState::Expressed | GeneState::Enhanced)
            ) {
                return UseResult::Refused(Refused::NoGene);
            }
            let level = target.levels.level(block);
            let Some((next, cost)) = change_level(level, atp) else {
                return UseResult::Refused(Refused::NotEnoughAtp);
            };
            target.levels.0.insert(block, next);
            target.pools.modify(Currency::ATP, -cost);
            UseResult::Done
        }
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

/// Asks a cell to use an action slot (0-based).
#[derive(Message, Debug, Clone, Copy)]
pub struct UseSlot {
    pub cell: Entity,
    pub slot: usize,
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

/// Applies `UseSlot` messages.
#[allow(clippy::type_complexity)]
pub fn use_slot_system(
    mut orders: MessageReader<UseSlot>,
    mut fired: MessageWriter<FireToxin>,
    mut lyse: MessageWriter<LyseOrder>,
    mut cells: Query<
        (
            &ActionBar,
            &mut CurrencyPools,
            &mut FlowLedger,
            &mut ActionState,
            &mut PathwayLevels,
            &mut MutationLoad,
            &mut InducedMutations,
            &Genome,
            &Proteome,
        ),
        With<Cell>,
    >,
) {
    for order in orders.read() {
        let Ok((
            bar,
            mut pools,
            mut ledger,
            mut state,
            mut levels,
            mut load,
            mut induced,
            genome,
            proteome,
        )) = cells.get_mut(order.cell)
        else {
            continue;
        };
        let Some(kind) = bar.slots.get(order.slot).copied().flatten() else {
            continue;
        };
        let before = pools.clone();
        let mut target = ActionTarget {
            pools: &mut pools,
            state: &mut state,
            levels: &mut levels,
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
                    cell: order.cell,
                    direction,
                });
            }
            UseResult::Lyse(direction) => {
                lyse.write(LyseOrder {
                    cell: order.cell,
                    direction,
                });
            }
            UseResult::Refused(why) => debug!("Slot {} refused: {why:?}", order.slot + 1),
            UseResult::Done => {}
        }
    }
}

/// Moves the action timers on, in fixed steps (the same clock as the physics).
pub fn advance_action_state_system(time: Res<Time>, mut cells: Query<&mut ActionState, With<Cell>>) {
    for mut state in &mut cells {
        state.advance(time.delta_secs());
    }
}

/// Moves each block's throttle towards the level of its slot's pathway.
pub fn throttle_ramp_system(
    timing: Res<FlowmapTimingConfig>,
    cells: Query<(&PathwayLevels, &ActionBar, &CellTick), With<Cell>>,
    mut blocks: Query<(&OwnedBy, &MetabolicNode, &mut Throttle)>,
) {
    for (owner, node, mut throttle) in &mut blocks {
        let Ok((levels, bar, tick)) = cells.get(owner.0) else {
            continue;
        };
        if !tick.due {
            continue;
        }
        let dt = tick.dt(timing.target_delta());
        let level = effective_level(levels, bar, node.kind);
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
            .add_message::<FireToxin>()
            .add_message::<LyseOrder>()
            .add_systems(Update, use_slot_system)
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

    fn pools(atp: f32, metals: f32) -> CurrencyPools {
        let mut p = CurrencyPools::default();
        p.set(Currency::ATP, atp);
        p.set(Currency::Metals, metals);
        p
    }

    struct Rig {
        pools: CurrencyPools,
        state: ActionState,
        levels: PathwayLevels,
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
                levels: PathwayLevels::default(),
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
                    levels: &mut self.levels,
                    load: &mut self.load,
                    induced: &mut self.induced,
                    genome: &self.genome,
                    proteome: &self.proteome,
                },
            )
        }
    }

    #[test]
    fn a_pathway_outside_the_bar_runs_at_high() {
        let mut levels = PathwayLevels::default();
        levels.0.insert(BlockKind::Chemosynthesis, Level::Off);
        let mut bar = starter_action_bar();
        assert_eq!(effective_level(&levels, &bar, BlockKind::Chemosynthesis), Level::Off);
        bar.slots[0] = Some(ActionKind::Toxin);
        assert_eq!(effective_level(&levels, &bar, BlockKind::Chemosynthesis), Level::High);
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
        let chemo = ActionKind::Pathway(BlockKind::Chemosynthesis);
        assert_eq!(rig.use_action(chemo), UseResult::Done);
        assert_eq!(rig.levels.level(BlockKind::Chemosynthesis), Level::Off);
        assert_abs_diff_eq!(rig.pools.get(Currency::ATP), 100.0 - LEVEL_CHANGE_ATP);

        rig.pools.set(Currency::ATP, 0.5);
        assert_eq!(rig.use_action(chemo), UseResult::Refused(Refused::NotEnoughAtp));
        assert_eq!(rig.levels.level(BlockKind::Chemosynthesis), Level::Off);
        assert_eq!(rig.pools.get(Currency::ATP), 0.5, "nothing is paid");
    }

    #[test]
    fn a_silent_gene_cannot_take_a_slot_use() {
        let mut rig = Rig::new();
        // Fermentation is in the starter genome but silent.
        assert_eq!(
            rig.use_action(ActionKind::Pathway(BlockKind::Fermentation)),
            UseResult::Refused(Refused::NoGene)
        );
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
    fn available_actions_follow_the_genome_and_the_proteome() {
        let genome = crate::blocks::genome::create_starter_genome();
        let mut proteome = Proteome::default();
        let options = available_actions(&genome, &proteome);
        assert!(options.contains(&ActionKind::Pathway(BlockKind::Chemosynthesis)));
        assert!(!options.contains(&ActionKind::Pathway(BlockKind::Fermentation)));
        assert!(!options.contains(&ActionKind::Toxin));
        assert_eq!(options.last(), Some(&ActionKind::InduceMutation));
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Toxin));
        assert!(available_actions(&genome, &proteome).contains(&ActionKind::Toxin));
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
    fn a_burst_without_input_goes_along_the_aim() {
        let mut state = ActionState::default();
        state.aim_at(Vec2::new(0.0, 5.0));
        state.burst_left = 1.0;
        assert_eq!(action_thrust(Vec2::ZERO, 300.0, &state), Vec2::new(0.0, 900.0));
        state.burst_left = 0.0;
        assert_eq!(action_thrust(Vec2::ZERO, 300.0, &state), Vec2::ZERO);
        state.aim_at(Vec2::ZERO);
        assert_eq!(state.aim, Vec2::Y, "zero input keeps the aim");
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
        rig.state.aim_at(Vec2::NEG_X);
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
    fn slot_text_names_the_level_cooldown_or_missing_protein() {
        let levels = PathwayLevels::default();
        let mut state = ActionState::default();
        let mut proteome = Proteome::default();
        let chemo = Some(ActionKind::Pathway(BlockKind::Chemosynthesis));
        assert_eq!(slot_status(None, &levels, &state, &proteome), "empty");
        assert!(slot_status(chemo, &levels, &state, &proteome).ends_with("High"));
        let burst = Some(ActionKind::FlagellumBurst);
        assert!(slot_status(burst, &levels, &state, &proteome).contains("no protein"));
        proteome.add(ProteinTarget::Membrane(MembraneProtein::Flagellum));
        assert!(slot_status(burst, &levels, &state, &proteome).contains("ready"));
        state.burst_left = 1.0;
        assert!(slot_status(burst, &levels, &state, &proteome).contains("BURST"));
        state.burst_left = 0.0;
        state.burst_cooldown = 2.5;
        assert!(slot_status(burst, &levels, &state, &proteome).contains("cooldown 2.5"));
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
}
