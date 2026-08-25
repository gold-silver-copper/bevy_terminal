//! Ratatui's `colors_rgb` example, rendered by `bevy_terminal_ratatui`.
//!
//! A port of `examples/apps/colors-rgb` from Ratatui 0.30.2 (upstream commit
//! `e665c36cb14752a61cd777fbd06dbef8474f2add`). Every cell in the color field
//! is a half block (`▀`) carrying a distinct 24-bit foreground and background,
//! and the whole field is re-colored every frame, so the scene is a worst case
//! for the renderer: no cell is ever unchanged and no row can be skipped.
//!
//! The window is opened with [`PresentMode::AutoNoVsync`] and winit is held in
//! [`UpdateMode::Continuous`], so Bevy runs the schedule as fast as it can
//! instead of pacing to the display's refresh rate. The `fps` readout in the
//! top-right corner is upstream's own widget, counting `Terminal::draw` calls;
//! because this example draws exactly once per Bevy frame it is the frame rate.
//! The window title carries the grid size and the renderer's per-frame
//! statistics, refreshed once a second so the title bar does not become part of
//! what is being measured.
//!
//! The window is resizable and the grid refits to it; a larger window means
//! more cells and a lower frame rate. Press `q` or `Escape` to quit.
//!
//! Note that the `colors-rgb` entry in the `ratatui_examples` gallery is a
//! deliberately frozen, deterministic still of this scene, so that gallery's
//! exports stay reproducible. This example is the live animation.

#[path = "common/app.rs"]
mod app;
#[path = "common/fonts.rs"]
mod fonts;

use std::time::{Duration, Instant};

use bevy::{
    prelude::*,
    window::{PresentMode, PrimaryWindow},
    winit::{UpdateMode, WinitSettings},
};
use bevy_terminal_ratatui::RatatuiTerminal;
use bevy_terminal_ratatui::prelude::{
    CellSizing, FontSizing, TerminalPlugin, TerminalRenderConfig, TerminalStats, TerminalSystems,
    TerminalTexture,
};
use palette::{Okhsv, Srgb, convert::FromColorUnclamped};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::Color,
    text::Text,
    widgets::Widget,
};

/// Font size in logical pixels. The cell is derived from it, so this is the
/// example's zoom control: a smaller font means more cells and a lower frame
/// rate.
const FONT_SIZE: f32 = 14.0;

/// Logical pixels left free around the terminal. The color field fills the
/// window so the measurement covers as many cells as the window can hold.
const MARGIN: f32 = 0.0;

/// Columns the terminal starts at, before the first refit to the window.
const INITIAL_COLUMNS: u16 = 80;

/// Rows the terminal starts at, before the first refit to the window.
const INITIAL_ROWS: u16 = 24;

fn main() {
    let (terminal, renderer) = RatatuiTerminal::new(INITIAL_COLUMNS, INITIAL_ROWS);
    let config = TerminalRenderConfig {
        cell_size: CellSizing::FROM_FONT,
        font_size: FontSizing::Px(FONT_SIZE),
        ..default()
    };

    let mut bevy = App::new();
    bevy.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "colors_rgb · bevy_terminal_ratatui".into(),
            // Present frames as soon as they are ready instead of pacing to the
            // display, so the readout shows the renderer's ceiling.
            present_mode: PresentMode::AutoNoVsync,
            ..default()
        }),
        ..default()
    }));
    let fonts = fonts::load(&mut bevy);
    let config = fonts.configure(config);
    bevy.add_plugins(TerminalPlugin)
        // Winit's default reactive pacing would idle the app between events.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .insert_resource(ColorsRgb {
            terminal,
            example: ColorsApp::default(),
            title: TitleTimer::default(),
        })
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(app::ui_terminal(
                renderer.clone(),
                config.clone(),
                Vec2::splat(MARGIN),
            ));
        })
        .add_systems(Update, (draw.before(TerminalSystems::Sync), report, quit))
        .run();
}

/// The Ratatui terminal and the ported upstream example state.
#[derive(Resource)]
struct ColorsRgb {
    /// The Ratatui terminal writing into the renderer's surface.
    terminal: RatatuiTerminal,
    /// The ported upstream application.
    example: ColorsApp,
    /// Throttle for the window-title statistics.
    title: TitleTimer,
}

/// Refits the grid to the window and draws one animated frame.
fn draw(
    mut colors: ResMut<ColorsRgb>,
    textures: Query<&TerminalTexture>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let ColorsRgb {
        terminal, example, ..
    } = &mut *colors;
    app::fit_grid_to_window(terminal, &textures, &windows, MARGIN);
    terminal.draw(|frame| frame.render_widget(&mut *example, frame.area()));
}

/// Shows the grid size and the renderer's per-frame statistics in the window
/// title, at most once a second. Writing the title every frame would make winit
/// part of the measurement.
fn report(
    mut colors: ResMut<ColorsRgb>,
    stats: Query<&TerminalStats>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !colors.title.due() {
        return;
    }
    let (Ok(stats), Ok(mut window)) = (stats.single(), windows.single_mut()) else {
        return;
    };
    let size = colors.terminal.size().unwrap_or_default();
    window.title = format!(
        "colors_rgb · {}x{} cells · vsync off · {stats}",
        size.width, size.height
    );
}

/// Quits on `q` or `Escape`. Upstream quits on any key press; the arrow and
/// modifier keys are left alone here so the window stays usable.
fn quit(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

/// A one-second throttle for the window title.
struct TitleTimer {
    /// When the title was last written.
    last: Instant,
}

impl Default for TitleTimer {
    fn default() -> Self {
        Self {
            last: Instant::now(),
        }
    }
}

impl TitleTimer {
    /// Returns whether a second has passed since the last write, resetting the
    /// timer when it has.
    fn due(&mut self) -> bool {
        if self.last.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.last = Instant::now();
        true
    }
}

// ---------------------------------------------------------------------------
// The upstream example, unchanged apart from its event loop.
//
// Upstream implements `Widget` for `&mut App` so that the widgets can update
// their own state while rendering: the fps widget recalculates the frame rate
// and the colors widget caches the gradient instead of recomputing it every
// frame. That structure is preserved here; only the crossterm terminal, event
// poll and `AppState` quit flag are replaced by Bevy systems.
// ---------------------------------------------------------------------------

/// The ported upstream application: an fps readout above the color field.
#[derive(Debug, Default)]
struct ColorsApp {
    /// A widget that displays the current frames per second.
    fps_widget: FpsWidget,

    /// A widget that displays the full range of RGB colors that can be
    /// displayed in the terminal.
    colors_widget: ColorsWidget,
}

/// A widget that displays the current frames per second.
#[derive(Debug)]
struct FpsWidget {
    /// The number of elapsed frames that have passed - used to calculate the fps.
    frame_count: usize,

    /// The last instant that the fps was calculated.
    last_instant: Instant,

    /// The current frames per second.
    fps: Option<f32>,
}

/// A widget that displays the full range of RGB colors that can be displayed in
/// the terminal.
///
/// This widget is animated and will change colors over time.
#[derive(Debug, Default)]
struct ColorsWidget {
    /// The colors to render - should be double the height of the area as we
    /// render two rows of pixels for each row of the widget using the half block
    /// character. This is computed any time the size of the widget changes.
    colors: Vec<Vec<Color>>,

    /// The number of elapsed frames that have passed - used to animate the
    /// colors by shifting the x index by the frame number.
    frame_count: usize,
}

impl Widget for &mut ColorsApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let [top, colors] = Layout::vertical([Length(1), Min(0)]).areas(area);
        let [title, fps] = Layout::horizontal([Min(0), Length(8)]).areas(top);
        Text::from("colors_rgb example. Press q to quit")
            .centered()
            .render(title, buf);
        self.fps_widget.render(fps, buf);
        self.colors_widget.render(colors, buf);
    }
}

/// Manual impl is required because we need to initialize the `last_instant`
/// field to the current instant.
impl Default for FpsWidget {
    fn default() -> Self {
        Self {
            frame_count: 0,
            last_instant: Instant::now(),
            fps: None,
        }
    }
}

impl Widget for &mut FpsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.calculate_fps();
        if let Some(fps) = self.fps {
            let text = format!("{fps:.1} fps");
            Text::from(text).render(area, buf);
        }
    }
}

impl FpsWidget {
    /// Update the fps calculation.
    ///
    /// This updates the fps once a second, but only if the widget has rendered
    /// at least 2 frames since the last calculation. This avoids noise in the
    /// fps calculation when rendering on slow machines that can't render at
    /// least 2 frames per second.
    #[expect(clippy::cast_precision_loss)]
    fn calculate_fps(&mut self) {
        self.frame_count += 1;
        let elapsed = self.last_instant.elapsed();
        if elapsed > Duration::from_secs(1) && self.frame_count > 2 {
            self.fps = Some(self.frame_count as f32 / elapsed.as_secs_f32());
            self.frame_count = 0;
            self.last_instant = Instant::now();
        }
    }
}

impl Widget for &mut ColorsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.setup_colors(area);
        let colors = &self.colors;
        if colors.is_empty() {
            return;
        }
        for (xi, x) in (area.left()..area.right()).enumerate() {
            // animate the colors by shifting the x index by the frame number
            let xi = (xi + self.frame_count) % (area.width as usize);
            for (yi, y) in (area.top()..area.bottom()).enumerate() {
                // render a half block character for each row of pixels with the
                // foreground color set to the color of the pixel and the
                // background color set to the color of the pixel below it
                let fg = colors[yi * 2][xi];
                let bg = colors[yi * 2 + 1][xi];
                buf[Position::new(x, y)].set_char('▀').set_fg(fg).set_bg(bg);
            }
        }
        self.frame_count += 1;
    }
}

impl ColorsWidget {
    /// Setup the colors to render.
    ///
    /// This is called once per frame to setup the colors to render. It caches
    /// the colors so that they don't need to be recalculated every frame.
    #[expect(clippy::cast_precision_loss)]
    fn setup_colors(&mut self, size: Rect) {
        let Rect { width, height, .. } = size;
        // double the height because each screen row has two rows of half block pixels
        let height = height as usize * 2;
        let width = width as usize;
        // a window can be dragged down to nothing; upstream never sees a zero area
        if height == 0 || width == 0 {
            self.colors.clear();
            return;
        }
        // only update the colors if the size has changed since the last time we rendered
        if self.colors.len() == height && self.colors[0].len() == width {
            return;
        }
        self.colors = Vec::with_capacity(height);
        for y in 0..height {
            let mut row = Vec::with_capacity(width);
            for x in 0..width {
                let hue = x as f32 * 360.0 / width as f32;
                let value = (height - y) as f32 / height as f32;
                let saturation = Okhsv::max_saturation();
                let color = Okhsv::new(hue, saturation, value);
                let color = Srgb::<f32>::from_color_unclamped(color);
                let color: Srgb<u8> = color.into_format();
                let color = Color::Rgb(color.red, color.green, color.blue);
                row.push(color);
            }
            self.colors.push(row);
        }
    }
}
