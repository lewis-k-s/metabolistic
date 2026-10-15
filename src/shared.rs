use crate::GameState;
use crate::blocks::genome::{self, BlockKind};
use crate::environment::LocalEnvironment;
use crate::level;
use crate::metabolism::CurrencyPools;
use crate::player::spawn_local_player_cell;
use crate::proteins::Proteome;
use bevy::prelude::*;

/// Outside channel level of a constant vent supply, for tests without a grid
/// (`LocalEnvironment::vent`).
pub const STARTER_VENT_LEVEL: f32 = 20.0;

/// The enzymes that a new vent cell starts with.
pub fn starter_proteome() -> Proteome {
    Proteome::with_enzymes(&[
        (BlockKind::Chemosynthesis, 2),
        (BlockKind::AminoAcidBiosynthesis, 1),
        (BlockKind::LipidMetabolism, 1),
        (BlockKind::ProteinSynthesis, 1),
        (BlockKind::NucleotideCofactorSynthesis, 1),
    ])
}

/// Spawns the local player's cell at `position`: starter genome and proteome,
/// default pools, and a mineral stock. Returns `(player, cell)`.
pub fn spawn_starter_cell(commands: &mut Commands, position: Vec2) -> (Entity, Entity) {
    let (player, cell) = spawn_local_player_cell(commands, genome::create_starter_genome());
    commands.entity(cell).insert((
        CurrencyPools::with_defaults(),
        starter_proteome(),
        LocalEnvironment::default(),
        Transform::from_translation(position.extend(0.0)),
    ));
    (player, cell)
}

/// Spawns the starter cell at the start of the vent field.
pub fn spawn_starter_cell_at_start(commands: &mut Commands) -> (Entity, Entity) {
    spawn_starter_cell(commands, level::START_POSITION)
}

/// Screen-level navigation commands, independent of the keys bound to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    /// Return to the menu.
    Back,
    /// Switch between the Run and Manage screens.
    ToggleManage,
}

/// The screen that `nav` leads to from `current`, if any.
pub fn navigate(current: &GameState, nav: Navigation) -> Option<GameState> {
    match (current, nav) {
        (GameState::Run | GameState::Manage | GameState::RunOver, Navigation::Back) => {
            Some(GameState::Menu)
        }
        (GameState::Run, Navigation::ToggleManage) => Some(GameState::Manage),
        (GameState::Manage, Navigation::ToggleManage) => Some(GameState::Run),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_returns_to_menu_from_play_screens() {
        assert_eq!(
            navigate(&GameState::Run, Navigation::Back),
            Some(GameState::Menu)
        );
        assert_eq!(
            navigate(&GameState::Manage, Navigation::Back),
            Some(GameState::Menu)
        );
        assert_eq!(
            navigate(&GameState::RunOver, Navigation::Back),
            Some(GameState::Menu)
        );
        assert_eq!(navigate(&GameState::Menu, Navigation::Back), None);
        assert_eq!(navigate(&GameState::Loading, Navigation::Back), None);
    }

    #[test]
    fn toggle_switches_run_and_manage_only() {
        let toggle = Navigation::ToggleManage;
        assert_eq!(navigate(&GameState::Run, toggle), Some(GameState::Manage));
        assert_eq!(navigate(&GameState::Manage, toggle), Some(GameState::Run));
        assert_eq!(navigate(&GameState::Menu, toggle), None);
        assert_eq!(navigate(&GameState::Loading, toggle), None);
        assert_eq!(navigate(&GameState::RunOver, toggle), None);
    }
}
