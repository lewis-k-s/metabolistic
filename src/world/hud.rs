//! # HUD
//!
//! Text bars for ATP, membrane integrity, genome integrity and the main pools,
//! in a panel at the top left of the Run screen. Minerals show the cell's stock. Heat is the
//! condition at the cell: the panel warns when it is above the damage
//! threshold. Light shows as UV at the cell, and as the pigment shield.

use bevy::prelude::*;

use crate::cell::Cell;
use crate::environment::{Channel, LocalEnvironment};
use crate::exchange::{HEAT_DAMAGE_THRESHOLD, MINERAL_STOCK_CAP};
use crate::genome_damage::{GenomeIntegrity, UvFraction, is_fragmented, shield, uv_level};
use crate::membrane::Membrane;
use crate::metabolism::CurrencyPools;
use crate::molecules::Currency;
use crate::player::{ControlledCell, LocalPlayer};
use crate::screens::label;
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
    Atp,
    Membrane,
    Genome,
    Minerals,
    Lipids,
    AminoAcids,
    Heat,
    Uv,
}

const LINES: [HudLine; 8] = [
    HudLine::Atp,
    HudLine::Membrane,
    HudLine::Genome,
    HudLine::Minerals,
    HudLine::Lipids,
    HudLine::AminoAcids,
    HudLine::Heat,
    HudLine::Uv,
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
        (&CurrencyPools, &Membrane, &LocalEnvironment, &GenomeIntegrity),
        With<Cell>,
    >,
    fraction: Res<UvFraction>,
    mut lines: Query<(&HudLine, &mut Text)>,
) {
    let Some((pools, membrane, env, genome)) = players
        .iter()
        .find_map(|controlled| cells.get(controlled.0).ok())
    else {
        return;
    };
    for (line, mut text) in &mut lines {
        text.0 = match line {
            HudLine::Atp => bar_line("ATP", pools.get(Currency::ATP), ATP_BAR_MAX),
            HudLine::Membrane => bar_line("Membrane", membrane.integrity, membrane.max),
            HudLine::Genome => {
                let note = if is_fragmented(genome.0) { "  FRAGMENTED" } else { "" };
                format!("{}{note}", bar_line("Genome", genome.0, 1.0))
            }
            HudLine::Uv => {
                let uv = uv_level(env.get(Channel::Light), fraction.0);
                let shield = shield(pools.get(Currency::Pigments));
                let warning = if uv > 1.0 && shield < 0.5 { "  UV!" } else { "" };
                format!("UV {uv:6.1}  shield {:3.0}%{warning}", shield * 100.0)
            }
            HudLine::Minerals => bar_line("Minerals", pools.get(Currency::Minerals), MINERAL_STOCK_CAP),
            HudLine::Lipids => bar_line("Lipids", pools.get(Currency::Lipids), POOL_BAR_MAX),
            HudLine::AminoAcids => {
                bar_line("AminoAcids", pools.get(Currency::AminoAcids), POOL_BAR_MAX)
            }
            HudLine::Heat => {
                let heat = env.get(Channel::Heat);
                let warning = if heat > HEAT_DAMAGE_THRESHOLD { "  TOO HOT!" } else { "" };
                format!("Heat {heat:6.1}{warning}")
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
