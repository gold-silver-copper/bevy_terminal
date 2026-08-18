//! Shared spawning helpers for the executable examples.

#![allow(dead_code)]

use bevy::prelude::*;
use bevy_terminal_ratatui::{TerminalRenderConfig, TerminalRenderer};

/// A terminal presented through a Bevy UI image node positioned absolutely at
/// `origin` (logical pixels). The renderer sizes the node; place it however
/// you like — this helper just uses absolute positioning.
pub fn ui_terminal(
    renderer: TerminalRenderer,
    config: TerminalRenderConfig,
    origin: Vec2,
) -> impl Bundle {
    (
        renderer,
        config,
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: px(origin.x),
            top: px(origin.y),
            ..default()
        },
    )
}

/// A headless terminal: only the texture is produced.
pub fn headless_terminal(renderer: TerminalRenderer, config: TerminalRenderConfig) -> impl Bundle {
    (renderer, config)
}
