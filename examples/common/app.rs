//! Shared spawning helpers for the executable examples.

#![allow(dead_code, reason = "shared by examples that each use part of it")]

use bevy::{prelude::*, ui::UiSystems, window::PrimaryWindow};
use bevy_terminal_ratatui::prelude::{
    RatatuiTerminal, TerminalRenderConfig, TerminalSystems, TerminalTexture,
};

/// A terminal presented through a Bevy UI image node positioned absolutely at
/// `origin` (logical pixels). The example presentation system sizes the node; place it however
/// you like — this helper just uses absolute positioning.
pub fn ui_terminal(
    terminal: impl Bundle,
    config: TerminalRenderConfig,
    origin: Vec2,
) -> impl Bundle {
    (
        terminal,
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
pub fn headless_terminal(terminal: impl Bundle, config: TerminalRenderConfig) -> impl Bundle {
    (terminal, config)
}

/// Fits the example's grid to logical space at the measured cell size.
/// Returns whether the grid changed, so callers can redraw only when needed.
pub fn fit_grid(
    terminal: &mut RatatuiTerminal,
    texture: &TerminalTexture,
    available: Vec2,
) -> bool {
    let scale = texture.cell().map_or(1.0, |cell| cell.raster_scale());
    terminal.backend_mut().fit(texture, available * scale)
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
    let Some(cell) = texture.cell() else {
        return false;
    };
    // The window in the pixels the terminal is rasterized in, which are its
    // own physical pixels while the terminal follows its scale: whole pixels
    // divide into whole cells exactly.
    let scale = cell.raster_scale();
    let available = window.resolution.physical_size().as_vec2() * (scale / window.scale_factor())
        - Vec2::splat(margin * 2.0 * scale);
    if available.x <= 0.0 || available.y <= 0.0 {
        return false;
    }
    terminal.backend_mut().fit(texture, available)
}

/// Installs application-owned UI layout without changing the requested raster scale.
pub fn presentation(app: &mut App) {
    app.add_systems(
        PostUpdate,
        present_ui
            .after(TerminalSystems::Sync)
            .before(UiSystems::Layout),
    );
}

/// Opts these single-window examples into following window DPI and UI scale.
pub fn window_scale(app: &mut App) {
    app.add_systems(Update, set_ui_scale);
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

fn present_ui(
    mut terminals: Query<(&TerminalTexture, &mut Node, &mut ImageNode), Changed<TerminalTexture>>,
) {
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
        if image.image != *texture.image() {
            image.image = texture.image().clone();
        }
    }
}
