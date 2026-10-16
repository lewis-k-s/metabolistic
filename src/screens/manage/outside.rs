//! The outside, above the membrane band: what the grid holds at the cell. It
//! uses the colours of the Run grid (`grid_view::heat_glow` and friends):
//! heat glows up from the membrane, light comes down from the top, and
//! minerals drift down to the membrane as specks. The levels come from the
//! cell's `LocalEnvironment`, the same sample that the pathways use.

use bevy::prelude::*;

use super::diagram::text_at;
use super::layout;
use super::{MapEntity, MapLayout, map_layer};
use crate::GameState;
use crate::cell::Cell;
use crate::environment::{Channel, LocalEnvironment};
use crate::player::{ControlledCell, LocalPlayer};
use crate::world::grid_view::{heat_glow, light_glow, mineral_glow};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (spawn_outside.run_if(resource_exists_and_changed::<MapLayout>), update_outside, drift_specks)
            .chain()
            .after(super::fit_map)
            .run_if(in_state(GameState::Manage)),
    );
}

const Z_BACKDROP: f32 = -2.0;
const Z_GLOW: f32 = -1.0;
const LAYER_ALPHA: f32 = 0.5;

/// One layer of a glow: `index` is the band, `channel` heat or light.
#[derive(Component)]
struct GlowLayer {
    channel: Channel,
    index: usize,
}

#[derive(Component)]
struct OutsideText;

#[derive(Component)]
struct OutsideSpeck {
    speed: f32,
}

/// Seconds between mineral specks at a level; `None` below the threshold.
pub fn speck_interval(level: f32) -> Option<f32> {
    (level >= 0.3).then(|| (1.2 / (1.0 + level)).clamp(0.08, 1.2))
}

fn spawn_outside(mut commands: Commands, layout: Res<MapLayout>, clear: Res<ClearColor>) {
    let top = layout.size.y / 2.0;
    let height = top - layout.outside_bottom;
    let centre = Vec2::new(0.0, layout.outside_bottom + height / 2.0);
    commands.spawn((
        Name::new("Outside"),
        MapEntity,
        Sprite::from_color(clear.0, Vec2::new(layout.size.x + layout::BAND_TILE, height)),
        Transform::from_translation(centre.extend(Z_BACKDROP)),
        map_layer(),
        DespawnOnExit(GameState::Manage),
    ));
    // Heat: layers that grow from the membrane up, the hot band the smallest.
    for index in 0..4 {
        let h = height * (1.0 - 0.2 * index as f32);
        commands.spawn((
            GlowLayer { channel: Channel::Heat, index },
            MapEntity,
            Sprite::from_color(Color::NONE, Vec2::new(layout.size.x, h)),
            Transform::from_xyz(0.0, layout.outside_bottom + h / 2.0, Z_GLOW + index as f32 * 0.01),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
    }
    // Light: layers that come down from the top.
    for index in 0..2 {
        let h = height * (1.0 - 0.45 * index as f32);
        commands.spawn((
            GlowLayer { channel: Channel::Light, index },
            MapEntity,
            Sprite::from_color(Color::NONE, Vec2::new(layout.size.x, h)),
            Transform::from_xyz(0.0, top - h / 2.0, Z_GLOW + 0.1 + index as f32 * 0.01),
            map_layer(),
            DespawnOnExit(GameState::Manage),
        ));
    }
    commands.spawn((
        OutsideText,
        MapEntity,
        Text2d::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.88, 0.95)),
        bevy::sprite::Anchor::TOP_LEFT,
        text_at(layout.left() + 6.0, top - 4.0, Z_GLOW + 1.0),
        map_layer(),
        DespawnOnExit(GameState::Manage),
    ));
}

#[allow(clippy::type_complexity)]
fn update_outside(
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&LocalEnvironment, With<Cell>>,
    mut layers: Query<(&GlowLayer, &mut Sprite)>,
    mut text: Query<&mut Text2d, With<OutsideText>>,
) {
    let Some(env) = super::local_cell(&players).and_then(|c| cells.get(c).ok()) else {
        return;
    };
    let heat = heat_glow(env.get(Channel::Heat));
    let light = light_glow(env.get(Channel::Light));
    for (layer, mut sprite) in &mut layers {
        let glow = match layer.channel {
            Channel::Heat => &heat,
            _ => &light,
        };
        let Some((color, strength)) = glow.get(layer.index) else { continue };
        let want = color.with_alpha(strength * LAYER_ALPHA);
        if sprite.color != want {
            sprite.color = want;
        }
    }
    if let Ok(mut text) = text.single_mut() {
        let line = format!(
            "Outside   heat {:.1}   minerals {:.1}   light {:.1}",
            env.get(Channel::Heat),
            env.get(Channel::Minerals),
            env.get(Channel::Light)
        );
        if text.0 != line {
            text.0 = line;
        }
    }
}

/// Mineral specks fall from the top of the strip to the membrane.
#[allow(clippy::too_many_arguments)]
fn drift_specks(
    mut commands: Commands,
    time: Res<Time>,
    layout: Res<MapLayout>,
    players: Query<&ControlledCell, With<LocalPlayer>>,
    cells: Query<&LocalEnvironment, With<Cell>>,
    mut specks: Query<(Entity, &OutsideSpeck, &mut Transform)>,
    mut since: Local<f32>,
    mut seed: Local<u32>,
) {
    let level = super::local_cell(&players)
        .and_then(|c| cells.get(c).ok())
        .map_or(0.0, |e| e.get(Channel::Minerals));
    for (entity, speck, mut transform) in &mut specks {
        transform.translation.y -= speck.speed * time.delta_secs();
        if transform.translation.y <= layout.outside_bottom {
            commands.entity(entity).despawn();
        }
    }
    let Some(interval) = speck_interval(level) else { return };
    *since += time.delta_secs();
    if *since < interval {
        return;
    }
    *since = 0.0;
    // A small generator: the specks only need to look scattered.
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    let unit = (*seed >> 8) as f32 / (1u32 << 24) as f32;
    let colors = mineral_glow(level.max(8.0));
    let color = colors.get(1).map_or(Color::WHITE, |c| c.0);
    commands.spawn((
        OutsideSpeck { speed: 14.0 + 10.0 * unit },
        MapEntity,
        Sprite::from_color(color, Vec2::splat(2.0)),
        Transform::from_xyz(
            (layout.left() + unit * layout.size.x).round(),
            layout.size.y / 2.0,
            Z_GLOW + 0.5,
        ),
        map_layer(),
        DespawnOnExit(GameState::Manage),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minerals_make_specks_above_a_threshold_and_faster_when_rich() {
        assert_eq!(speck_interval(0.0), None);
        let low = speck_interval(0.5).unwrap();
        let high = speck_interval(10.0).unwrap();
        assert!(high < low);
        assert!(high >= 0.08);
    }
}
