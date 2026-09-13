//! Shared spawning helpers for the executable examples.

#![allow(dead_code)]

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_terminal_ratatui::prelude::{TerminalRenderConfig, TerminalSystems, TerminalTexture};
use bevy_terminal_ratatui::{RatatuiTerminal, TerminalRenderer};

/// A terminal presented through a Bevy UI image node positioned absolutely at
/// `origin` (logical pixels). The example presentation system sizes the node; place it however
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

/// Adopts current pixel geometry and fits the example's grid to logical space.
/// Returns whether the grid changed, so callers can redraw only when needed.
pub fn fit_grid(
    terminal: &mut RatatuiTerminal,
    texture: &TerminalTexture,
    available: Vec2,
) -> bool {
    let Some(geometry) = texture.measured() else {
        return false;
    };
    if !terminal.backend_mut().set_geometry(geometry) {
        return false;
    }
    let grid = geometry.grid_for(available);
    if terminal.surface().size() == grid {
        return false;
    }
    terminal.resize_grid(grid.width, grid.height);
    true
}

/// Fits `terminal`'s grid to the primary window (minus `margin` on every
/// side) at the renderer's measured cell size. Returns whether the grid
/// changed, in which case the caller should redraw. Does nothing until the
/// terminal has been measured (its `TerminalTexture` exists).
pub fn fit_grid_to_window(
    terminal: &mut RatatuiTerminal,
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
    fit_grid(terminal, texture, available)
}

/// Installs application-owned UI layout without changing the requested raster scale.
pub fn presentation(app: &mut App) {
    app.add_systems(Update, present_ui.after(TerminalSystems::Sync));
}

/// Opts these single-window examples into following window DPI and UI scale.
pub fn window_scale(app: &mut App) {
    app.add_systems(Update, set_ui_scale.before(TerminalSystems::Sync));
}

fn set_ui_scale(
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    mut terminals: Query<&mut TerminalRenderConfig, With<ImageNode>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let scale = window.scale_factor() * ui_scale.as_ref().map_or(1.0, |scale| scale.0);
    for mut config in &mut terminals {
        if config.raster.scale != scale {
            config.raster.scale = scale;
        }
    }
}

fn present_ui(mut terminals: Query<(&TerminalTexture, &mut Node, &mut ImageNode)>) {
    for (texture, mut node, mut image) in &mut terminals {
        let Some(geometry) = texture.measured() else {
            continue;
        };
        let size = geometry.logical_size();
        if node.width != px(size.x) {
            node.width = px(size.x);
        }
        if node.height != px(size.y) {
            node.height = px(size.y);
        }
        if image.image != texture.image {
            image.image = texture.image.clone();
        }
    }
}
