//! Shared spawning helpers for the executable examples.

#![allow(dead_code)]

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_terminal_ratatui::prelude::{TerminalRenderConfig, TerminalTexture};
use bevy_terminal_ratatui::{RatatuiBackend, RatatuiTerminalExt, TerminalRenderer};
use ratatui::Terminal;

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

/// Fits `terminal`'s grid to the primary window (minus `margin` on every
/// side) at the renderer's measured cell size. Returns whether the grid
/// changed, in which case the caller should redraw. Does nothing until the
/// terminal has been measured (its `TerminalTexture` exists).
pub fn fit_grid_to_window(
    terminal: &mut Terminal<RatatuiBackend>,
    textures: &Query<&TerminalTexture>,
    windows: &Query<&Window, With<PrimaryWindow>>,
    margin: f32,
) -> bool {
    let (Ok(texture), Ok(window)) = (textures.single(), windows.single()) else {
        return false;
    };
    let available = window.resolution.size() - Vec2::splat(margin * 2.0);
    if available.x <= 0.0 || available.y <= 0.0 {
        return false;
    }
    terminal.fit_to(texture, available)
}
