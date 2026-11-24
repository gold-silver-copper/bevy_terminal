//! One terminal filling the primary window, shown through Bevy UI.

use std::{fmt, marker::PhantomData};

use bevy_terminal::bevy::{
    prelude::*,
    ui::{DefaultUiCamera, UiSystems},
    window::PrimaryWindow,
};
use bevy_terminal::prelude::{TerminalRenderConfig, TerminalSystems, TerminalTexture};

use crate::{RatatuiSource, RatatuiSourcePlugin};

/// Shows the terminal with the source `S` filling the primary window, adding
/// [`RatatuiSourcePlugin<S>`] if it is missing.
///
/// Every frame the grid is resized to the largest that fits the window, the
/// terminal is rasterized at the window's scale factor times [`UiScale`], and
/// a UI node centered in the window is sized to the texture. A 2D camera is
/// spawned unless the primary window already has an active one.
///
/// The plugin owns what an application would otherwise set: the grid's size
/// and [`RasterConfig::scale`](bevy_terminal::prelude::RasterConfig::scale),
/// which it overwrites every frame. Everything else in
/// [`TerminalRenderConfig`] is the application's.
///
/// Only the backend's grid is resized. Ratatui adopts the new size, and
/// repaints, on its next draw; until then the last frame stays on screen
/// through any number of resizes, so an application may draw only when its
/// state changes. Draw in `Update` to draw at the size fitted this frame.
///
/// There must be exactly one entity with `S`: with none, or several, the
/// plugin does nothing. Lay several terminals out yourself, as the
/// `multiple_terminals` example does.
pub struct RatatuiWindowPlugin<S>(PhantomData<fn() -> S>);

impl<S> Default for RatatuiWindowPlugin<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S> Clone for RatatuiWindowPlugin<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for RatatuiWindowPlugin<S> {}

impl<S> fmt::Debug for RatatuiWindowPlugin<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RatatuiWindowPlugin")
            .field(&std::any::type_name::<S>())
            .finish()
    }
}

/// When [`RatatuiWindowPlugin`] works, for ordering other systems against it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RatatuiWindowSystems {
    /// In `PreUpdate`: the grid is resized to the window and the raster scale
    /// set. Before any `Update` system draws, so that a resize never clears a
    /// frame already drawn.
    Fit,
    /// In `PostUpdate`, after [`TerminalSystems::Sync`] and before UI layout:
    /// the UI node is sized to the texture measured this frame.
    Present,
}

impl<S: RatatuiSource> Plugin for RatatuiWindowPlugin<S> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<RatatuiSourcePlugin<S>>() {
            app.add_plugins(RatatuiSourcePlugin::<S>::default());
        }
        app.configure_sets(
            PostUpdate,
            RatatuiWindowSystems::Present
                .after(TerminalSystems::Sync)
                .before(UiSystems::Layout),
        )
        // The first `PreUpdate` is after every startup schedule, so a camera
        // spawned in any of them is seen.
        .add_systems(PreUpdate, setup::<S>.run_if(run_once))
        .add_systems(
            PreUpdate,
            (follow_window_scale::<S>, fit_to_window::<S>).in_set(RatatuiWindowSystems::Fit),
        )
        .add_systems(
            PostUpdate,
            present::<S>.in_set(RatatuiWindowSystems::Present),
        );
    }
}

/// The UI node that shows the terminal with the source `S`.
#[derive(Component)]
struct TerminalView<S>(PhantomData<fn() -> S>);

/// Spawns the UI node showing the terminal, and a 2D camera unless the
/// application spawned an active one that Bevy UI draws to by default: one on
/// the primary window, or marked `IsDefaultUiCamera`. A camera on another
/// window or an image, or an inactive one, would show nothing; the node is
/// then shown by the camera spawned here, whichever Bevy UI would pick.
fn setup<S: RatatuiSource>(
    mut commands: Commands,
    ui_camera: DefaultUiCamera,
    cameras: Query<&Camera>,
) {
    let mut view = commands.spawn((
        TerminalView::<S>(PhantomData),
        ImageNode::default(),
        Node {
            justify_self: JustifySelf::Center,
            align_self: AlignSelf::Center,
            ..default()
        },
    ));
    let usable = ui_camera.get().and_then(|camera| cameras.get(camera).ok());
    if !usable.is_some_and(|camera| camera.is_active) {
        let camera = view.commands().spawn(Camera2d).id();
        view.insert(UiTargetCamera(camera));
    }
}

/// The scale Bevy UI applies to this window: one logical UI pixel is this
/// many physical pixels. A scale that is not a positive number counts as
/// one, as the renderer rasterizes it.
fn ui_scale_factor(window: &Window, ui_scale: Option<&UiScale>) -> f32 {
    let scale = window.scale_factor() * ui_scale.map_or(1.0, |scale| scale.0);
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Rasterizes the terminal at the scale the UI displays it, so text stays
/// sharp on HiDPI displays.
fn follow_window_scale<S: RatatuiSource>(
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    mut config: Single<&mut TerminalRenderConfig, With<S>>,
) {
    let scale = ui_scale_factor(&window, ui_scale.as_deref());
    if config.raster.scale != scale {
        config.raster.scale = scale;
    }
}

/// Resizes the grid to fill the window, at the cell the terminal measured.
///
/// The cell is measured whatever the grid's size, so a grid too large for a
/// texture (on the first frame, or after a jump in font size) is fitted like
/// any other.
fn fit_to_window<S: RatatuiSource>(
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    terminal: Single<(&mut S, &TerminalTexture)>,
) {
    let window_size = window.resolution.physical_size().as_vec2();
    // A minimized window has no size; keep the grid, and what is drawn in
    // it, until it returns.
    if window_size.min_element() <= 0.0 {
        return;
    }
    let (mut terminal, texture) = terminal.into_inner();
    let Some(cell) = texture.cell() else {
        return;
    };
    // The window in the pixels the terminal is rasterized in: its own
    // physical pixels, unless the renderer clamped the scale. Whole pixels
    // divide into whole cells exactly, which sizes in UI pixels do not at
    // fractional scales.
    let raster_per_window = cell.raster_scale() / ui_scale_factor(&window, ui_scale.as_deref());
    // A window larger than a texture can be is filled as far as one reaches.
    let grid = cell.grid_for(window_size * raster_per_window);
    // Compared first, so that an unchanged terminal is not marked changed.
    if grid != terminal.grid().size() {
        terminal.backend_mut().resize(grid.width, grid.height);
    }
}

/// Shows the rendered texture at its measured size, and nothing once the
/// terminal is gone (which also lets go of its image).
fn present<S: RatatuiSource>(
    texture: Option<Single<&TerminalTexture, With<S>>>,
    view: Single<(&mut Node, &mut ImageNode), With<TerminalView<S>>>,
) {
    let (width, height, shown) = match &texture {
        Some(texture) => match texture.measured() {
            Some(geometry) => {
                let size = geometry.logical_size();
                (px(size.x), px(size.y), texture.image().clone())
            }
            // Not drawn yet, or failed: keep what is shown.
            None => return,
        },
        None => (Val::Auto, Val::Auto, ImageNode::default().image),
    };
    let (mut node, mut image) = view.into_inner();
    // Compared first, so that an unchanged node is not laid out again.
    if node.width != width {
        node.width = width;
    }
    if node.height != height {
        node.height = height;
    }
    if image.image != shown {
        image.image = shown;
    }
}
