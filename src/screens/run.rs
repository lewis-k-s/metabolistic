//! Run screen: the main loop. The player's cell moves through the world and
//! interacts with it. For now it is an animated cell sprite that moves with WASD.

use bevy::prelude::*;

use super::label;
use crate::GameState;
use crate::sprites::{
    Animator, PIXEL_SCALE, Sheets, SpriteSystems, clip, direction_row, opposite_row,
    row_direction,
};

/// Where a membrane protein sits: this fraction of the cell radius from the
/// centre (see `docs/art-direction.md`).
const MOUNT_RADIUS: f32 = 0.95;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Run), spawn_run_screen)
        .add_systems(
            Update,
            (move_run_player, drive_player_sprites)
                .chain()
                .before(SpriteSystems::Animate)
                .run_if(in_state(GameState::Run)),
        );
}

/// The player's cell on the Run screen.
#[derive(Component)]
pub struct RunPlayer {
    /// Pixels per second.
    pub speed: f32,
    /// Velocity in pixels per second (+Y is up). Zero when no key is held.
    pub velocity: Vec2,
    /// The sprite row of the last heading (see `sprites::direction_row`).
    pub facing: usize,
}

/// The flagellum on the player's membrane. A placeholder: it shows the
/// membrane protein overlays until the player's proteins set which ones to draw.
#[derive(Component)]
struct PlayerFlagellum;

fn spawn_run_screen(mut commands: Commands, sheets: Res<Sheets>) {
    let body = sheets
        .bundle("player_cell", clip::SPAWN)
        .expect("the manifest has player_cell/spawn");
    let flagellum = sheets
        .bundle("overlay_flagellum", clip::IDLE)
        .expect("the manifest has overlay_flagellum/idle");
    let player = RunPlayer {
        speed: 300.0,
        velocity: Vec2::ZERO,
        facing: 0,
    };
    let mount = flagellum_offset(&sheets, opposite_row(player.facing));
    commands
        .spawn((
            Name::new("Run Player"),
            player,
            body,
            Transform::from_scale(Vec3::splat(PIXEL_SCALE)),
            DespawnOnExit(GameState::Run),
        ))
        .with_children(|parent| {
            parent.spawn((
                Name::new("Flagellum"),
                PlayerFlagellum,
                flagellum,
                // Above the body.
                Transform::from_translation(mount.extend(0.1)),
            ));
        });
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
    mut players: Query<(&mut Transform, &mut RunPlayer)>,
) {
    let direction = wasd_direction(
        keys.pressed(KeyCode::KeyW),
        keys.pressed(KeyCode::KeyA),
        keys.pressed(KeyCode::KeyS),
        keys.pressed(KeyCode::KeyD),
    );
    for (mut transform, mut player) in &mut players {
        player.velocity = direction * player.speed;
        if direction != Vec2::ZERO {
            player.facing = direction_row(direction);
        }
        transform.translation += (player.velocity * time.delta_secs()).extend(0.0);
    }
}

/// Where the flagellum sits, in sprite pixels from the cell centre, when it
/// points along `row`.
fn flagellum_offset(sheets: &Sheets, row: usize) -> Vec2 {
    let radius = sheets
        .sprite("player_cell")
        .and_then(|s| s.info.radius)
        .unwrap_or(20.0);
    row_direction(row) * MOUNT_RADIUS * radius
}

/// Picks the clips and rows of the player's sprites. The body plays `move`
/// while a key is held, and `idle` otherwise, after the one-shot `spawn`. The
/// flagellum points away from the heading and thrusts while the cell moves.
fn drive_player_sprites(
    sheets: Res<Sheets>,
    mut players: Query<(&RunPlayer, &mut Animator, &Children), Without<PlayerFlagellum>>,
    mut flagella: Query<(&mut Animator, &mut Transform), With<PlayerFlagellum>>,
) {
    for (player, mut body, children) in &mut players {
        let moving = player.velocity != Vec2::ZERO;
        body.row = player.facing;
        if body.clip != clip::SPAWN || body.finished() {
            body.play(if moving { clip::MOVE } else { clip::IDLE }, &sheets);
        }
        let row = opposite_row(player.facing);
        for &child in children {
            if let Ok((mut flagellum, mut transform)) = flagella.get_mut(child) {
                flagellum.row = row;
                flagellum.play(if moving { clip::USE } else { clip::IDLE }, &sheets);
                transform.translation = flagellum_offset(&sheets, row).extend(0.1);
            }
        }
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
