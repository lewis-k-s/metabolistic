//! # HUD
//!
//! Text bars for ATP, membrane integrity, genome integrity and the main pools,
//! in a panel at the top left of the Run screen. Minerals show the cell's stock. Heat is the
//! condition at the cell: the panel warns when it is above the damage
//! threshold. Light shows as UV at the cell, and as the pigment shield. On land,
//! the panel also shows the hydration bar and the cuticle shield.

use bevy::prelude::*;

use crate::actions::{ActionBar, ActionState, PathwayLevels, SLOT_COUNT, slot_status};
use crate::cell::Cell;
use crate::desiccation::{CuticleShield, Hydration};
use crate::level::CurrentLevel;
use crate::environment::{Channel, LocalEnvironment};
use crate::exchange::{HEAT_DAMAGE_THRESHOLD, MINERAL_STOCK_CAP};
use crate::gate::{GATE_SECONDS, GateProgress};
use crate::genome_damage::{GenomeIntegrity, UvFraction, is_fragmented, shield, uv_level};
use crate::membrane::Membrane;
use crate::metabolism::CurrencyPools;
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::proteins::Proteome;
use crate::shelter::InShelter;
use crate::run::RunState;
use crate::screens::label;
use crate::stage::STAGES;
use crate::GameState;

/// Width of a bar in characters.
const BAR_WIDTH: usize = 20;
/// The ATP bar is full at this amount (the starting amount).
const ATP_BAR_MAX: f32 = 100.0;
/// The lipid and amino acid bars are full at this amount.
const POOL_BAR_MAX: f32 = 50.0;

/// One line of the HUD.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum HudLine {
    Stage,
    Atp,
    Membrane,
    Genome,
    Minerals,
    Metals,
    Gate,
    Lipids,
    AminoAcids,
    Heat,
    Uv,
    O2,
    Ros,
    Hydration,
    Cuticle,
    Shelter,
}

/// The ROS bar is full at this amount: the tint is full here too.
const ROS_BAR_MAX: f32 = crate::oxygen::ROS_TINT_FULL;

/// A warning after the ROS bar when the level is high.
pub fn ros_warning(ros: f32) -> &'static str {
    if ros >= ROS_BAR_MAX * 0.5 {
        "  OXIDATIVE DAMAGE!"
    } else if ros >= ROS_BAR_MAX * 0.2 {
        "  ROS rising"
    } else {
        ""
    }
}

/// A warning after the hydration bar when the cell is dry.
pub fn hydration_warning(hydration: f32) -> &'static str {
    if hydration < crate::desiccation::DAMAGE_THRESHOLD {
        "  DRYING OUT! Find water"
    } else if hydration < 0.6 {
        "  getting dry"
    } else {
        ""
    }
}

/// The cuticle line: the shield as a percentage of the water loss it removes.
pub fn cuticle_line(shield: f32) -> String {
    format!("Cuticle    shield {:3.0}%", shield * 100.0)
}

/// One slot of the action bar (0-based).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct HudSlot(usize);

const LINES: [HudLine; 16] = [
    HudLine::Stage,
    HudLine::Atp,
    HudLine::Membrane,
    HudLine::Genome,
    HudLine::Minerals,
    HudLine::Metals,
    HudLine::Gate,
    HudLine::Lipids,
    HudLine::AminoAcids,
    HudLine::Heat,
    HudLine::Uv,
    HudLine::O2,
    HudLine::Ros,
    HudLine::Hydration,
    HudLine::Cuticle,
    HudLine::Shelter,
];

pub fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Run), spawn_hud)
        .add_systems(Update, update_hud.run_if(in_state(GameState::Run)));
}

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Run HUD"),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            },
            DespawnOnExit(GameState::Run),
        ))
        .with_children(|parent| {
            for line in LINES {
                parent.spawn((line, label("", 18.0)));
            }
        });
    commands
        .spawn((
            Name::new("Action Bar"),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(44.0),
                left: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            },
            DespawnOnExit(GameState::Run),
        ))
        .with_children(|parent| {
            for slot in 0..SLOT_COUNT {
                parent.spawn((HudSlot(slot), label("", 18.0)));
            }
        });
}

/// The stage name and the run seed.
pub fn stage_line(run: &RunState) -> String {
    format!(
        "Stage {}/{}: {}    seed {}",
        run.stage_index + 1,
        STAGES.len(),
        run.stage().name(),
        run.seed
    )
}

/// A text bar: `[#####---]` with `value` out of `max`, `width` characters wide.
/// The value is clamped to the range.
pub fn text_bar(value: f32, max: f32, width: usize) -> String {
    let fraction = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (fraction * width as f32).round() as usize;
    format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled))
}

/// One HUD line of text: a name, a bar and the number.
fn bar_line(name: &str, value: f32, max: f32) -> String {
    format!("{name:<10} {} {value:6.1}", text_bar(value, max, BAR_WIDTH))
}

fn update_hud(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<
        (
            &CurrencyPools,
            &Membrane,
            &LocalEnvironment,
            &GenomeIntegrity,
            &InShelter,
            &ActionBar,
            &PathwayLevels,
            &ActionState,
            &Proteome,
            &Hydration,
            &CuticleShield,
        ),
        With<Cell>,
    >,
    level: Option<Res<CurrentLevel>>,
    fraction: Res<UvFraction>,
    run: Res<RunState>,
    gate: Query<&GateProgress>,
    mut lines: Query<(&HudLine, &mut Text), Without<HudSlot>>,
    mut slots: Query<(&HudSlot, &mut Text), Without<HudLine>>,
) {
    let Some((pools, membrane, env, genome, shelter, bar, levels, actions, proteome, hydration, cuticle)) = players
        .iter()
        .find_map(|controlled| cells.get(controlled.0).ok())
    else {
        return;
    };
    for (slot, mut text) in &mut slots {
        let kind = bar.slots.get(slot.0).copied().flatten();
        text.0 = format!("[{}] {}", slot.0 + 1, slot_status(kind, levels, actions, proteome));
    }
    for (line, mut text) in &mut lines {
        text.0 = match line {
            HudLine::Shelter => {
                if shelter.0 {
                    "SHELTER: heat and UV are calm".to_string()
                } else {
                    String::new()
                }
            }
            HudLine::Stage => stage_line(&run),
            HudLine::Atp => bar_line("ATP", pools.get(Currency::ATP), ATP_BAR_MAX),
            HudLine::Membrane => bar_line("Membrane", membrane.integrity, membrane.max),
            HudLine::Genome => {
                let note = if is_fragmented(genome.0) { "  FRAGMENTED" } else { "" };
                format!("{}{note}", bar_line("Genome", genome.0, 1.0))
            }
            HudLine::Uv => {
                let uv = uv_level(env.get(Channel::Light), fraction.0);
                let shield = shield(pools.get(Currency::Pigments));
                let warning = if uv > 1.0 && shield < 0.5 && !shelter.0 { "  UV!" } else { "" };
                format!("UV {uv:6.1}  shield {:3.0}%{warning}", shield * 100.0)
            }
            HudLine::O2 => {
                // Only a cell that has oxygen shows it, so the vents stay plain.
                let o2 = pools.get(Currency::O2);
                let grid = env.get(Channel::O2);
                if o2 > 0.05 || grid > 0.5 {
                    format!("O2 {o2:6.1}  water {grid:4.1}")
                } else {
                    String::new()
                }
            }
            HudLine::Ros => {
                let ros = pools.get(Currency::ROS);
                if ros > 0.05 {
                    format!("{}{}", bar_line("ROS", ros, ROS_BAR_MAX), ros_warning(ros))
                } else {
                    String::new()
                }
            }
            HudLine::Hydration => {
                if level.as_ref().is_some_and(|l| l.0.field.dries()) {
                    format!(
                        "{}{}",
                        bar_line("Hydration", hydration.0, 1.0),
                        hydration_warning(hydration.0)
                    )
                } else {
                    String::new()
                }
            }
            HudLine::Cuticle => {
                if level.as_ref().is_some_and(|l| l.0.field.dries()) {
                    cuticle_line(cuticle.0)
                } else {
                    String::new()
                }
            }
            HudLine::Metals => format!("Metals {:6.1}", pools.get(Currency::Metals)),
            HudLine::Gate => match gate.iter().find(|p| p.held > 0.0) {
                Some(progress) => format!(
                    "GATE {} {:3.1}/{} s",
                    text_bar(progress.held, GATE_SECONDS, BAR_WIDTH),
                    progress.held,
                    GATE_SECONDS
                ),
                None => String::new(),
            },
            HudLine::Minerals => bar_line("Minerals", pools.get(Currency::Minerals), MINERAL_STOCK_CAP),
            HudLine::Lipids => bar_line("Lipids", pools.get(Currency::Lipids), POOL_BAR_MAX),
            HudLine::AminoAcids => {
                bar_line("AminoAcids", pools.get(Currency::AminoAcids), POOL_BAR_MAX)
            }
            HudLine::Heat => {
                let heat = env.get(Channel::Heat);
                let warning = if heat > HEAT_DAMAGE_THRESHOLD && !shelter.0 {
                    "  TOO HOT!"
                } else {
                    ""
                };
                format!("Heat {heat:6.1}{warning}")
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_line_names_the_stage_and_the_seed() {
        let run = RunState::begin(1234, Default::default());
        let line = stage_line(&run);
        assert!(line.contains("Deep-sea vents") && line.contains("1234") && line.contains("1/3"), "{line}");
    }

    #[test]
    fn the_hydration_warning_and_the_cuticle_line() {
        assert_eq!(hydration_warning(1.0), "");
        assert!(hydration_warning(0.5).contains("dry"));
        assert!(hydration_warning(0.1).contains("DRYING"));
        assert!(cuticle_line(0.0).contains("0%"));
        assert!(cuticle_line(0.5).contains("50%"));
    }

    #[test]
    fn the_ros_warning_grows_with_the_level() {
        assert_eq!(ros_warning(0.0), "");
        assert!(ros_warning(ROS_BAR_MAX * 0.3).contains("rising"));
        assert!(ros_warning(ROS_BAR_MAX).contains("DAMAGE"));
    }

    #[test]
    fn text_bar_fills_in_proportion_and_clamps() {
        assert_eq!(text_bar(0.0, 10.0, 10), "[----------]");
        assert_eq!(text_bar(5.0, 10.0, 10), "[#####-----]");
        assert_eq!(text_bar(10.0, 10.0, 10), "[##########]");
        assert_eq!(text_bar(99.0, 10.0, 10), "[##########]");
        assert_eq!(text_bar(-3.0, 10.0, 10), "[----------]");
        assert_eq!(text_bar(5.0, 0.0, 4), "[----]");
    }
}
