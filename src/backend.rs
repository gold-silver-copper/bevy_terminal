use std::{convert::Infallible, ops::Range};

use bevy_terminal::{
    GridSize, StyleFlags, TerminalCell, TerminalColor, TerminalStyle, TerminalSurface,
};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::{Cell, CellWidth},
    layout::{Position, Size},
    style::{Color, Modifier},
};

/// A Ratatui [`Backend`] that writes into a [`bevy_terminal::TerminalSurface`].
///
/// Clone [`Self::surface`] before moving the backend into a
/// [`ratatui::Terminal`], then spawn a [`bevy_terminal::Terminal`] entity for
/// that handle (after adding [`bevy_terminal::TerminalPlugin`]).
pub struct RatatuiBackend {
    surface: TerminalSurface,
}

impl RatatuiBackend {
    /// Creates a backend with a fixed initial size in terminal cells.
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            surface: TerminalSurface::new(columns, rows),
        }
    }

    /// Creates a backend writing to an existing surface.
    #[must_use]
    pub const fn from_surface(surface: TerminalSurface) -> Self {
        Self { surface }
    }

    /// Returns a handle that can be passed to the Bevy renderer plugin.
    #[must_use]
    pub fn surface(&self) -> TerminalSurface {
        self.surface.clone()
    }

    /// Resizes the terminal grid, preserving cells in the overlapping area.
    ///
    /// When this backend is owned by a [`ratatui::Terminal`], prefer
    /// [`RatatuiTerminalExt::resize_grid`], which also makes Ratatui's own
    /// double buffers adopt the new size.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.surface.begin_update().resize(columns, rows);
    }
}

/// Convenience methods for a [`ratatui::Terminal`] driving a [`RatatuiBackend`].
pub trait RatatuiTerminalExt {
    /// Resizes the backend grid and Ratatui's own double buffers together, so
    /// the next `draw` renders at the new size.
    fn resize_grid(&mut self, columns: u16, rows: u16);
}

impl RatatuiTerminalExt for ratatui::Terminal<RatatuiBackend> {
    fn resize_grid(&mut self, columns: u16, rows: u16) {
        self.backend_mut().resize(columns, rows);
        let Ok(()) = self.autoresize();
    }
}

/// Converts a Ratatui cell into the neutral cell model.
///
/// Wide symbols keep the width Ratatui declared for them so the renderer can
/// anchor the glyph to its columns; the surface synthesizes the continuation
/// cells that Ratatui omits from its diff iterator.
fn translate_cell(cell: &Cell) -> TerminalCell {
    let style = TerminalStyle {
        foreground: translate_color(cell.fg),
        background: translate_color(cell.bg),
        underline: translate_color(cell.underline_color),
        flags: translate_modifier(cell.modifier),
    };
    TerminalCell::wide(cell.symbol(), cell.cell_width()).with_style(style)
}

/// Maps Ratatui colors: named colors become their ANSI palette index, indexed
/// and RGB colors are retained exactly, and `Reset` becomes the contextual
/// default.
const fn translate_color(color: Color) -> TerminalColor {
    match color {
        Color::Reset => TerminalColor::Default,
        Color::Black => TerminalColor::Indexed(0),
        Color::Red => TerminalColor::Indexed(1),
        Color::Green => TerminalColor::Indexed(2),
        Color::Yellow => TerminalColor::Indexed(3),
        Color::Blue => TerminalColor::Indexed(4),
        Color::Magenta => TerminalColor::Indexed(5),
        Color::Cyan => TerminalColor::Indexed(6),
        Color::Gray => TerminalColor::Indexed(7),
        Color::DarkGray => TerminalColor::Indexed(8),
        Color::LightRed => TerminalColor::Indexed(9),
        Color::LightGreen => TerminalColor::Indexed(10),
        Color::LightYellow => TerminalColor::Indexed(11),
        Color::LightBlue => TerminalColor::Indexed(12),
        Color::LightMagenta => TerminalColor::Indexed(13),
        Color::LightCyan => TerminalColor::Indexed(14),
        Color::White => TerminalColor::Indexed(15),
        Color::Rgb(red, green, blue) => TerminalColor::Rgb(red, green, blue),
        Color::Indexed(index) => TerminalColor::Indexed(index),
    }
}

#[cfg(test)]
const MODIFIER_FLAGS: [(Modifier, StyleFlags); 9] = [
    (Modifier::BOLD, StyleFlags::BOLD),
    (Modifier::DIM, StyleFlags::DIM),
    (Modifier::ITALIC, StyleFlags::ITALIC),
    (Modifier::UNDERLINED, StyleFlags::UNDERLINED),
    (Modifier::SLOW_BLINK, StyleFlags::SLOW_BLINK),
    (Modifier::RAPID_BLINK, StyleFlags::RAPID_BLINK),
    (Modifier::REVERSED, StyleFlags::REVERSED),
    (Modifier::HIDDEN, StyleFlags::HIDDEN),
    (Modifier::CROSSED_OUT, StyleFlags::CROSSED_OUT),
];

/// Ratatui's modifier bits and [`StyleFlags`] use the same bit layout, so the
/// translation is a mask; `MODIFIER_FLAGS` documents and tests that mapping.
const fn translate_modifier(modifier: Modifier) -> StyleFlags {
    StyleFlags::from_bits(modifier.bits())
}

const fn size_from_grid(size: GridSize) -> Size {
    Size::new(size.width, size.height)
}

impl Backend for RatatuiBackend {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut update = self.surface.begin_update();
        for (x, y, cell) in content {
            // Positions outside the grid are ignored by the surface.
            update.set_cell((x, y), &translate_cell(cell));
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.surface.begin_update().set_cursor_visible(false);
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.surface.begin_update().set_cursor_visible(true);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        let position = self.surface.begin_update().cursor_position();
        Ok(Position::new(position.x, position.y))
    }

    fn set_cursor_position<P>(&mut self, position: P) -> Result<(), Self::Error>
    where
        P: Into<Position>,
    {
        let position = position.into();
        self.surface
            .begin_update()
            .set_cursor_position((position.x, position.y));
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.surface.begin_update().clear();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        let mut update = self.surface.begin_update();
        let size = update.size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let cursor = update.cursor_position();
        let cursor = (cursor.x.min(size.width - 1), cursor.y.min(size.height - 1));
        let last = (size.width - 1, size.height - 1);
        match clear_type {
            ClearType::All => update.clear(),
            ClearType::AfterCursor => update.clear_range(cursor, last),
            ClearType::BeforeCursor => update.clear_range((0, 0), cursor),
            ClearType::CurrentLine => update.clear_row(cursor.1),
            ClearType::UntilNewLine => update.clear_range(cursor, (size.width - 1, cursor.1)),
        };
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(size_from_grid(self.surface.size()))
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let metrics = self.surface.metrics();
        Ok(WindowSize {
            columns_rows: size_from_grid(metrics.size),
            pixels: Size::new(
                u16::try_from(metrics.pixel_size.x).unwrap_or(u16::MAX),
                u16::try_from(metrics.pixel_size.y).unwrap_or(u16::MAX),
            ),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn append_lines(&mut self, line_count: u16) -> Result<(), Self::Error> {
        let mut update = self.surface.begin_update();
        let size = update.size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }

        let cursor = update.cursor_position();
        let x = cursor.x.saturating_add(1).min(size.width - 1);
        let y = cursor.y.min(size.height - 1);
        let rows_below = size.height - 1 - y;
        let y = if line_count <= rows_below {
            y.saturating_add(line_count).min(size.height - 1)
        } else {
            update.scroll_up(0..size.height, line_count - rows_below);
            size.height - 1
        };
        update.set_cursor_position((x, y));
        Ok(())
    }

    fn scroll_region_up(&mut self, region: Range<u16>, line_count: u16) -> Result<(), Self::Error> {
        self.surface.begin_update().scroll_up(region, line_count);
        Ok(())
    }

    fn scroll_region_down(
        &mut self,
        region: Range<u16>,
        line_count: u16,
    ) -> Result<(), Self::Error> {
        self.surface.begin_update().scroll_down(region, line_count);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn draw_text(backend: &mut RatatuiBackend, text: &str, width: u16) {
        let cells: Vec<Cell> = text.chars().map(Cell::from).collect();
        backend
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index as u16) % width, (index as u16) / width, cell)),
            )
            .unwrap();
    }

    #[test]
    fn cells_colors_and_modifiers_translate_completely() {
        let mut cell = Cell::new("X");
        cell.set_style(
            Style::new()
                .fg(Color::LightBlue)
                .bg(Color::Rgb(1, 2, 3))
                .underline_color(Color::Indexed(200))
                .add_modifier(Modifier::all()),
        );
        let translated = translate_cell(&cell);
        assert_eq!(translated.symbol(), "X");
        assert_eq!(translated.style.foreground, TerminalColor::Indexed(12));
        assert_eq!(translated.style.background, TerminalColor::Rgb(1, 2, 3));
        assert_eq!(translated.style.underline, TerminalColor::Indexed(200));
        assert_eq!(translated.occupancy(), bevy_terminal::CellOccupancy::Single);
        for (modifier, flag) in MODIFIER_FLAGS {
            assert!(translated.style.flags.contains(flag));
            assert_eq!(translate_modifier(modifier), flag);
        }
        assert_eq!(translate_modifier(Modifier::empty()), StyleFlags::NONE);
        assert_eq!(
            translate_modifier(Modifier::BOLD | Modifier::ITALIC),
            StyleFlags::BOLD | StyleFlags::ITALIC
        );

        let named = [
            (Color::Black, 0),
            (Color::Red, 1),
            (Color::Green, 2),
            (Color::Yellow, 3),
            (Color::Blue, 4),
            (Color::Magenta, 5),
            (Color::Cyan, 6),
            (Color::Gray, 7),
            (Color::DarkGray, 8),
            (Color::LightRed, 9),
            (Color::LightGreen, 10),
            (Color::LightYellow, 11),
            (Color::LightBlue, 12),
            (Color::LightMagenta, 13),
            (Color::LightCyan, 14),
            (Color::White, 15),
        ];
        for (color, index) in named {
            assert_eq!(translate_color(color), TerminalColor::Indexed(index));
        }
        assert_eq!(translate_color(Color::Reset), TerminalColor::Default);
        assert_eq!(
            translate_color(Color::Indexed(37)),
            TerminalColor::Indexed(37)
        );

        assert_eq!(translate_cell(&Cell::EMPTY), TerminalCell::EMPTY);
    }

    #[test]
    fn partial_draw_preserves_other_cells_and_clips_invalid_coordinates() {
        let mut backend = RatatuiBackend::new(3, 2);
        let mut cell = Cell::new("X");
        cell.set_style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD));
        backend
            .draw([(1, 0, &cell), (99, 99, &cell)].into_iter())
            .unwrap();

        let snapshot = backend.surface().snapshot();
        assert_eq!(snapshot[(1, 0)], translate_cell(&cell));
        assert_eq!(snapshot[(1, 0)].style.foreground, TerminalColor::Indexed(1));
        assert!(snapshot[(1, 0)].style.has(StyleFlags::BOLD));
        assert_eq!(snapshot[(0, 0)], TerminalCell::EMPTY);
    }

    #[test]
    fn each_draw_publishes_at_most_one_revision_and_no_ops_publish_none() {
        let mut backend = RatatuiBackend::new(2, 1);
        let empty = Cell::EMPTY;
        let initial = backend.surface().revision();

        backend.draw([(0, 0, &empty)].into_iter()).unwrap();
        backend.flush().unwrap();
        backend.hide_cursor().unwrap();
        backend.set_cursor_position(Position::ORIGIN).unwrap();
        backend.clear().unwrap();
        backend.resize(2, 1);
        assert_eq!(backend.surface().revision(), initial);

        let a = Cell::new("A");
        let b = Cell::new("B");
        backend.draw([(0, 0, &a), (1, 0, &b)].into_iter()).unwrap();
        backend.flush().unwrap();
        assert_eq!(backend.surface().revision(), initial + 1);
        backend.draw([(0, 0, &a), (1, 0, &b)].into_iter()).unwrap();
        backend.flush().unwrap();
        assert_eq!(backend.surface().revision(), initial + 1);
    }

    #[test]
    fn wide_cells_get_an_explicit_continuation() {
        let mut backend = RatatuiBackend::new(4, 1);
        let cell = Cell::new("界");
        backend.draw([(1, 0, &cell)].into_iter()).unwrap();

        let snapshot = backend.surface().snapshot();
        assert_eq!(snapshot[(1, 0)].symbol(), "界");
        assert_eq!(snapshot[(1, 0)].columns(), 2);
        assert!(snapshot[(2, 0)].is_continuation());
        assert_eq!(snapshot[(3, 0)], TerminalCell::EMPTY);
    }

    #[test]
    fn terminal_diff_replaces_wide_cells_without_stale_continuations() {
        let backend = RatatuiBackend::new(4, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| frame.render_widget("界A", frame.area()))
            .unwrap();
        let snapshot = terminal.backend().surface().snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "界");
        assert!(snapshot[(1, 0)].is_continuation());
        assert_eq!(snapshot[(2, 0)].symbol(), "A");

        terminal
            .draw(|frame| frame.render_widget("abc", frame.area()))
            .unwrap();
        let snapshot = terminal.backend().surface().snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "a");
        assert_eq!(snapshot[(1, 0)].symbol(), "b");
        assert!(!snapshot[(1, 0)].is_continuation());
        assert_eq!(snapshot[(2, 0)].symbol(), "c");

        // A narrow replacement of only the anchor clears the orphaned continuation.
        terminal
            .draw(|frame| frame.render_widget("界 ", frame.area()))
            .unwrap();
        let mut backend = RatatuiBackend::from_surface(terminal.backend().surface());
        backend.draw([(0, 0, &Cell::new("x"))].into_iter()).unwrap();
        let snapshot = backend.surface().snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "x");
        assert_eq!(snapshot[(1, 0)], TerminalCell::EMPTY);
    }

    #[test]
    fn cursor_clear_and_resize_semantics_are_retained() {
        let mut backend = RatatuiBackend::new(4, 2);
        draw_text(&mut backend, "ABCDEFGH", 4);
        backend.set_cursor_position((1, 0)).unwrap();
        backend.show_cursor().unwrap();
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(1, 0));
        backend.clear_region(ClearType::UntilNewLine).unwrap();

        let snapshot = backend.surface().snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "A");
        assert_eq!(snapshot[(1, 0)], TerminalCell::EMPTY);
        assert_eq!(snapshot[(3, 0)], TerminalCell::EMPTY);
        assert_eq!(snapshot[(0, 1)].symbol(), "E");
        assert!(snapshot.cursor_visible());
        assert_eq!(snapshot.cursor_position().x, 1);

        backend.clear_region(ClearType::CurrentLine).unwrap();
        assert_eq!(backend.surface().snapshot()[(0, 0)], TerminalCell::EMPTY);
        backend.clear_region(ClearType::All).unwrap();
        assert_eq!(backend.surface().snapshot()[(0, 1)], TerminalCell::EMPTY);

        draw_text(&mut backend, "ABCDEFGH", 4);
        backend.resize(2, 1);
        let resized = backend.surface().snapshot();
        assert_eq!(resized.size(), GridSize::new(2, 1));
        assert_eq!(backend.size().unwrap(), Size::new(2, 1));
        assert_eq!(resized[(0, 0)].symbol(), "A");
    }

    #[test]
    fn before_and_after_cursor_include_the_cursor_cell() {
        let mut after = RatatuiBackend::new(3, 2);
        draw_text(&mut after, "ABCDEF", 3);
        after.set_cursor_position((1, 0)).unwrap();
        after.clear_region(ClearType::AfterCursor).unwrap();
        assert_eq!(after.surface().snapshot()[(0, 0)].symbol(), "A");
        assert_eq!(after.surface().snapshot()[(1, 0)], TerminalCell::EMPTY);
        assert_eq!(after.surface().snapshot()[(2, 1)], TerminalCell::EMPTY);

        let mut before = RatatuiBackend::new(3, 2);
        draw_text(&mut before, "ABCDEF", 3);
        before.set_cursor_position((1, 1)).unwrap();
        before.clear_region(ClearType::BeforeCursor).unwrap();
        assert_eq!(before.surface().snapshot()[(0, 0)], TerminalCell::EMPTY);
        assert_eq!(before.surface().snapshot()[(1, 1)], TerminalCell::EMPTY);
        assert_eq!(before.surface().snapshot()[(2, 1)].symbol(), "F");
    }

    #[test]
    fn scroll_regions_move_and_clear_rows() {
        let mut backend = RatatuiBackend::new(2, 3);
        draw_text(&mut backend, "AABBCC", 2);

        backend.scroll_region_up(0..3, 1).unwrap();
        let up = backend.surface().snapshot();
        assert_eq!(up[(0, 0)].symbol(), "B");
        assert_eq!(up[(0, 1)].symbol(), "C");
        assert_eq!(up[(0, 2)], TerminalCell::EMPTY);

        backend.scroll_region_down(0..3, 1).unwrap();
        let down = backend.surface().snapshot();
        assert_eq!(down[(0, 0)], TerminalCell::EMPTY);
        assert_eq!(down[(0, 1)].symbol(), "B");
        assert_eq!(down[(0, 2)].symbol(), "C");
    }

    #[test]
    fn resize_preserves_the_overlap_and_window_size_reports_pixels() {
        let mut backend = RatatuiBackend::new(4, 3);
        draw_text(&mut backend, "AAAABBBBCCCC", 4);

        backend.resize(2, 3);
        let snapshot = backend.surface().snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "A");
        assert_eq!(snapshot[(0, 1)].symbol(), "B");
        assert_eq!(snapshot[(0, 2)].symbol(), "C");

        // Pixel metrics stay zero until a renderer configures the cell size.
        let window_size = backend.window_size().unwrap();
        assert_eq!(window_size.columns_rows, Size::new(2, 3));
        assert_eq!(window_size.pixels, Size::new(0, 0));

        let mut app = bevy::app::App::new();
        app.init_resource::<bevy::asset::Assets<bevy::image::Image>>()
            .add_plugins(bevy_terminal::TerminalPlugin);
        app.world_mut().spawn(
            bevy_terminal::Terminal::new(backend.surface())
                .with_config(bevy_terminal::TerminalRenderConfig {
                    cell_size: bevy::math::Vec2::new(10.8, 20.0),
                    ..Default::default()
                })
                .with_presentation(bevy_terminal::Presentation::Headless),
        );
        app.update();
        let window_size = backend.window_size().unwrap();
        assert_eq!(window_size.pixels, Size::new(22, 60));
    }

    #[test]
    fn resize_grid_resizes_the_backend_and_ratatui_buffers_together() {
        let backend = RatatuiBackend::new(4, 2);
        let surface = backend.surface();
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| frame.render_widget("ABCDEFGH", frame.area()))
            .unwrap();
        terminal.resize_grid(6, 3);
        assert_eq!(terminal.size().unwrap(), Size::new(6, 3));
        assert_eq!(surface.size(), GridSize::new(6, 3));
        terminal
            .draw(|frame| frame.render_widget("wide now", frame.area()))
            .unwrap();
        assert_eq!(surface.snapshot()[(5, 0)].symbol(), "n");
    }

    #[test]
    fn append_lines_moves_right_scrolls_and_clamps_the_cursor() {
        let mut backend = RatatuiBackend::new(2, 3);
        draw_text(&mut backend, "AABBCC", 2);

        backend.set_cursor_position((0, 2)).unwrap();
        backend.append_lines(1).unwrap();
        let scrolled = backend.surface().snapshot();
        assert_eq!(scrolled.cursor_position().x, 1);
        assert_eq!(scrolled.cursor_position().y, 2);
        assert_eq!(scrolled[(0, 0)].symbol(), "B");
        assert_eq!(scrolled[(0, 2)], TerminalCell::EMPTY);

        backend.set_cursor_position((0, u16::MAX)).unwrap();
        backend.append_lines(0).unwrap();
        assert_eq!(backend.surface().snapshot().cursor_position().x, 1);
        assert_eq!(backend.surface().snapshot().cursor_position().y, 2);
    }

    #[test]
    fn a_real_ratatui_terminal_drives_the_surface() {
        use ratatui::widgets::{Block, Borders, Paragraph};

        let backend = RatatuiBackend::new(10, 3);
        let surface = backend.surface();
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("hi").block(Block::new().borders(Borders::ALL)),
                    frame.area(),
                );
                frame.set_cursor_position((3, 1));
            })
            .unwrap();
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "┌");
        assert_eq!(snapshot[(1, 1)].symbol(), "h");
        assert!(snapshot.cursor_visible());
        assert_eq!(snapshot.cursor_position().x, 3);
        assert_eq!(snapshot.cursor_position().y, 1);

        let revision = surface.revision();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("hi").block(Block::new().borders(Borders::ALL)),
                    frame.area(),
                );
                frame.set_cursor_position((3, 1));
            })
            .unwrap();
        assert_eq!(surface.revision(), revision);
    }
}
