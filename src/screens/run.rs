//! Run screen: the main loop. The player's cell moves through the world and
//! interacts with it. For now it is a disc that moves with WASD.

use bevy::prelude::*;

use super::label;
use crate::GameState;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Run), spawn_run_screen)
        .add_systems(Update, move_run_player.run_if(in_state(GameState::Run)));
}

/// The player's cell on the Run screen.
#[derive(Component)]
pub struct RunPlayer {
    /// Pixels per second.
    pub speed: f32,
}

fn spawn_run_screen(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((
        Name::new("Run Player"),
        RunPlayer { speed: 300.0 },
        Mesh2d(meshes.add(Circle::new(30.0))),
        MeshMaterial2d(materials.add(Color::srgb(0.8, 0.5, 0.3))),
        Transform::default(),
        DespawnOnExit(GameState::Run),
    ));
    commands.spawn((
        Name::new("Run HUD"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        DespawnOnExit(GameState::Run),
        children![label("Run    WASD: move    Tab: manage    Esc: menu", 18.0)],
    ));
}

fn move_run_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut players: Query<(&mut Transform, &RunPlayer)>,
) {
    let direction = wasd_direction(
        keys.pressed(KeyCode::KeyW),
        keys.pressed(KeyCode::KeyA),
        keys.pressed(KeyCode::KeyS),
        keys.pressed(KeyCode::KeyD),
    );
    for (mut transform, player) in &mut players {
        transform.translation += (direction * player.speed * time.delta_secs()).extend(0.0);
    }
}

/// Unit direction from the held keys, or zero. +Y is up on screen.
fn wasd_direction(w: bool, a: bool, s: bool, d: bool) -> Vec2 {
    let axis = |neg: bool, pos: bool| pos as i8 as f32 - neg as i8 as f32;
    Vec2::new(axis(a, d), axis(s, w)).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::wasd_direction;
    use bevy::math::Vec2;

    #[test]
    fn wasd_direction_is_unit_or_zero() {
        assert_eq!(wasd_direction(false, false, false, false), Vec2::ZERO);
        assert_eq!(wasd_direction(true, false, false, false), Vec2::Y);
        assert_eq!(wasd_direction(true, false, true, false), Vec2::ZERO);
        let diagonal = wasd_direction(true, false, false, true);
        assert!((diagonal.length() - 1.0).abs() < 1e-6);
    }
}
