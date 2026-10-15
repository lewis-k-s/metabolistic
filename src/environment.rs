//! # Outside conditions at a cell
//!
//! Some blocks take inputs from outside the cell, for example heat and
//! minerals for chemosynthesis. These inputs are channels, not internal pools.
//!
//! `LocalEnvironment` holds the channel levels that the blocks of one cell can
//! use. Without a grid (the M1 tests), it is a constant supply. In the game,
//! the spatial grid (`envgrid::EnvGrid`) feeds it, and `exchange` moves
//! substances between the grid and the cell:
//!
//! - Heat is a condition. It is the grid level under the body, and nothing
//!   uses it up.
//! - Minerals are a stock in the cell. Transporters take them from the grid,
//!   and chemosynthesis uses them up.

use bevy::prelude::*;
use std::collections::HashMap;

/// An outside channel. M3 adds Light/UV, and later stages add OrganicCarbon,
/// O2 and Water.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    /// Heat from a vent.
    Heat,
    /// Reduced minerals (for example H2S) from a vent.
    Minerals,
}

impl Channel {
    /// All channels, in a fixed order. `index` is the position in this list.
    pub const ALL: [Channel; 2] = [Channel::Heat, Channel::Minerals];

    /// Position of the channel in `ALL`. Layers of the grid use it.
    pub const fn index(self) -> usize {
        match self {
            Channel::Heat => 0,
            Channel::Minerals => 1,
        }
    }
}

/// The channel levels at one cell. A missing channel has level 0.
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct LocalEnvironment(pub HashMap<Channel, f32>);

impl LocalEnvironment {
    /// A constant vent supply: heat and minerals at `level`.
    pub fn vent(level: f32) -> Self {
        Self(HashMap::from([
            (Channel::Heat, level),
            (Channel::Minerals, level),
        ]))
    }

    pub fn get(&self, channel: Channel) -> f32 {
        self.0.get(&channel).copied().unwrap_or(0.0)
    }

    pub fn set(&mut self, channel: Channel, level: f32) {
        self.0.insert(channel, level.max(0.0));
    }
}

/// Saturation `S / (K + S)` of a substrate level `s` with half-saturation `k`.
/// With `k = 0`, any substrate gives full saturation.
pub fn saturation(s: f32, k: f32) -> f32 {
    if s <= 0.0 { 0.0 } else { s / (k + s) }
}

/// Saturation of a block from outside channels: the lowest `S / (K + S)` of
/// its channels. A block with no outside channels has saturation 1.
pub fn external_saturation(env: &LocalEnvironment, channels: &[Channel], k: f32) -> f32 {
    channels
        .iter()
        .map(|&c| saturation(env.get(c), k))
        .fold(1.0, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturation_is_half_at_k() {
        assert_eq!(saturation(5.0, 5.0), 0.5);
        assert_eq!(saturation(0.0, 5.0), 0.0);
        assert_eq!(saturation(3.0, 0.0), 1.0);
    }

    #[test]
    fn external_saturation_uses_lowest_channel() {
        let mut env = LocalEnvironment::vent(20.0);
        env.set(Channel::Heat, 5.0);
        let s = external_saturation(&env, &[Channel::Heat, Channel::Minerals], 5.0);
        assert_eq!(s, 0.5);
        assert_eq!(external_saturation(&env, &[], 5.0), 1.0);
        assert_eq!(
            external_saturation(&LocalEnvironment::default(), &[Channel::Heat], 5.0),
            0.0
        );
    }
}
