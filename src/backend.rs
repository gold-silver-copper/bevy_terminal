use std::{borrow::Cow, convert::Infallible, fmt, marker::PhantomData};

use bevy_terminal::bevy::{ecs::component::Mutable, prelude::*};
use bevy_terminal::prelude::{
    CellPosition, GridSize, StyleFlags, TerminalCell, TerminalColor, TerminalGeometry,
    TerminalGrid, TerminalRenderConfig, TerminalSource, TerminalSourcePlugin, TerminalStyle,
    TerminalSystems, TerminalTexture,
};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::{Cell, CellDiffOption, CellWidth},
    layout::{Position, Size},
    style::{Color, Modifier},
};

/// A [`TerminalSource`] backed by a [`RatatuiBackend`].
///
/// Register it with [`RatatuiSourcePlugin`] so the renderer draws its grid and
/// hands back each measured [`TerminalGeometry`] for [`Backend::window_size`].
pub trait RatatuiSource: TerminalSource + Component<Mutability = Mutable> {
    /// Mutable access to the inner [`RatatuiBackend`].
    fn backend_mut(&mut self) -> &mut RatatuiBackend;
}

/// Draws every [`RatatuiSource`] `S`, adding the renderer's
/// [`TerminalPlugin`](bevy_terminal::prelude::TerminalPlugin) if it is
/// missing, and hands each terminal's measured geometry to its backend in
/// [`PostUpdate`] after [`TerminalSystems::Sync`].
pub struct RatatuiSourcePlugin<S>(PhantomData<fn() -> S>);

impl<S> Default for RatatuiSourcePlugin<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S> Clone for RatatuiSourcePlugin<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for RatatuiSourcePlugin<S> {}

impl<S> fmt::Debug for RatatuiSourcePlugin<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RatatuiSourcePlugin")
            .field(&std::any::type_name::<S>())
            .finish()
    }
}

impl<S: RatatuiSource> Plugin for RatatuiSourcePlugin<S> {
    fn build(&self, app: &mut App) {
        app.add_plugins(TerminalSourcePlugin::<S>::default())
            .add_systems(PostUpdate, adopt_geometry::<S>.after(TerminalSystems::Sync));
    }
}

/// Draws every [`RatatuiTerminal`] entity, adding the renderer's
/// [`TerminalPlugin`](bevy_terminal::prelude::TerminalPlugin) if it is
/// missing, and hands each terminal's measured geometry to its backend so
/// [`Backend::window_size`] reports pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct RatatuiPlugin;

impl Plugin for RatatuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RatatuiSourcePlugin::<RatatuiTerminal>::default());
    }
}

/// Keeps each backend's geometry that of its texture: gone while the terminal
/// loads or has failed, and present on a backend that replaced another.
fn adopt_geometry<S: RatatuiSource>(mut terminals: Query<(&mut S, &TerminalTexture)>) {
    for (mut terminal, texture) in &mut terminals {
        // Pixel metrics do not change what is drawn.
        terminal.bypass_change_detection().backend_mut().geometry = texture.measured().copied();
    }
}

/// A Ratatui [`Backend`] that writes into a [`TerminalGrid`] it owns.
///
/// Wrap it in a [`RatatuiTerminal`] and spawn that, after adding
/// [`RatatuiPlugin`], to draw it.
#[derive(Debug)]
pub struct RatatuiBackend {
    grid: TerminalGrid,
    /// The texture's measurement, reported by [`Backend::window_size`] while
    /// it describes the grid's size.
    geometry: Option<TerminalGeometry>,
    /// The grid as last written and where the cursor was last put, while
    /// only resizes have followed. Ratatui still holds that frame as drawn,
    /// so a resize brings back from it what an earlier resize cut off.
    written: Option<(TerminalGrid, CellPosition)>,
}

impl RatatuiBackend {
    /// Creates a backend with a fixed initial size in terminal cells, clamped
    /// to [`TerminalGrid::MAX_CELLS`].
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self::from_grid(TerminalGrid::new((columns, rows)))
    }

    /// Creates a backend writing to an existing grid.
    #[must_use]
    pub const fn from_grid(grid: TerminalGrid) -> Self {
        Self {
            grid,
            geometry: None,
            written: None,
        }
    }

    /// The grid this backend writes.
    #[must_use]
    pub const fn grid(&self) -> &TerminalGrid {
        &self.grid
    }

    /// Resizes the terminal grid, preserving cells in the overlapping area.
    /// Fullscreen and inline Ratatui terminals adopt this size on their next
    /// draw, or immediately through `Terminal::autoresize`. Fixed viewports
    /// remain application-owned; use `Terminal::resize` to change their area.
    /// The grid is clamped to [`TerminalGrid::MAX_CELLS`].
    ///
    /// Until cells are written again, resizes are relative to the last
    /// frame: a grid that shrinks and grows back shows that frame whole
    /// again, as Ratatui, which redraws only what differs from it, takes it
    /// to, with its cursor where it was put. A fixed viewport that draws
    /// while the grid is smaller than its area loses what did not fit.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        let size = GridSize::new(columns, rows).bounded();
        if self.grid.size() == size {
            return;
        }
        let resized_before = self.written.is_some();
        let (written, cursor) = self
            .written
            .get_or_insert_with(|| (self.grid.clone(), self.grid.cursor_position()));
        self.grid.resize(size);
        if resized_before {
            for y in 0..size.height.min(written.size().height) {
                for (x, cell) in (0..size.width).zip(written.row(y)) {
                    self.grid.set_cell((x, y), cell);
                }
            }
        }
        self.grid.set_cursor_position(*cursor);
    }

    /// Notes a cursor that was moved as the one resizes return to.
    fn keep_cursor(&mut self) {
        if let Some((_, cursor)) = &mut self.written {
            *cursor = self.grid.cursor_position();
        }
    }

    /// Resizes the grid to the largest that fits `physical_size` pixels at
    /// the cell `texture` measured (see
    /// [`CellGeometry::grid_for`](bevy_terminal::prelude::CellGeometry::grid_for)), and
    /// returns whether it changed. Does nothing until the cell is measured.
    /// It needs no geometry for the current grid, so it also brings a grid
    /// too large for a texture back to one that can be drawn.
    ///
    /// Reaching the backend through a Bevy `Mut` marks the terminal changed
    /// whether or not it is resized: to leave an unchanged terminal alone,
    /// compare `cell.grid_for(..)` with the grid's size first and call
    /// [`resize`](Self::resize) only when they differ.
    pub fn fit(&mut self, texture: &TerminalTexture, physical_size: Vec2) -> bool {
        let Some(grid) = texture.cell().map(|cell| cell.grid_for(physical_size)) else {
            return false;
        };
        let changed = grid != self.grid.size();
        self.resize(grid.width, grid.height);
        changed
    }
}

/// A [`ratatui::Terminal`] driving a [`RatatuiBackend`], as a Bevy component.
///
/// Spawn it after adding [`RatatuiPlugin`] and it is drawn into the entity's
/// [`TerminalTexture`]. It dereferences to the inner [`ratatui::Terminal`];
/// the inherent [`draw`](Self::draw) shadows Ratatui's so the infallible
/// backend needs no error handling at call sites.
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy_terminal_ratatui::prelude::*;
/// use ratatui::widgets::Paragraph;
///
/// fn setup(mut commands: Commands) {
///     commands.spawn(RatatuiTerminal::new(80, 24));
/// }
///
/// fn draw(mut terminal: Single<&mut RatatuiTerminal>) {
///     terminal.draw(|frame| frame.render_widget(Paragraph::new("hi"), frame.area()));
/// }
/// ```
#[derive(Component, Debug, Deref, DerefMut)]
#[require(TerminalRenderConfig)]
pub struct RatatuiTerminal(ratatui::Terminal<RatatuiBackend>);

impl RatatuiTerminal {
    /// Creates a terminal of `columns` × `rows` cells.
    /// See [`RatatuiBackend::new`] for the grid allocation limit.
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self::from_backend(RatatuiBackend::new(columns, rows))
    }

    /// Wraps an existing backend in a fullscreen Ratatui terminal.
    #[must_use]
    pub fn from_backend(backend: RatatuiBackend) -> Self {
        let Ok(terminal) = ratatui::Terminal::new(backend);
        Self(terminal)
    }

    /// Draws one frame, returning its completed buffer and frame count.
    pub fn draw(
        &mut self,
        draw: impl FnOnce(&mut ratatui::Frame<'_>),
    ) -> ratatui::CompletedFrame<'_> {
        let Ok(frame) = self.0.draw(draw);
        frame
    }

    /// Resizes the backend grid and Ratatui's own double buffers together, so
    /// the next [`draw`](Self::draw) renders at the new size. As in Ratatui,
    /// a fixed viewport retains its explicitly configured area.
    pub fn resize_grid(&mut self, columns: u16, rows: u16) {
        self.0.backend_mut().resize(columns, rows);
        let Ok(()) = self.0.autoresize();
    }

    /// The grid this terminal draws into.
    #[must_use]
    pub fn grid(&self) -> &TerminalGrid {
        self.0.backend().grid()
    }
}

impl TerminalSource for RatatuiTerminal {
    fn grid(&self) -> &TerminalGrid {
        self.0.backend().grid()
    }
}

impl RatatuiSource for RatatuiTerminal {
    fn backend_mut(&mut self) -> &mut RatatuiBackend {
        self.0.backend_mut()
    }
}

impl From<ratatui::Terminal<RatatuiBackend>> for RatatuiTerminal {
    fn from(terminal: ratatui::Terminal<RatatuiBackend>) -> Self {
        Self(terminal)
    }
}

/// Converts a Ratatui cell into the neutral cell model.
///
/// Wide symbols keep the width Ratatui declared for them so the renderer can
/// anchor the glyph to its columns; the grid synthesizes the continuation
/// cells that Ratatui omits from its diff iterator. Only what a terminal would
/// show of the symbol is kept (see [`printable`]).
fn translate_cell(cell: &Cell) -> TerminalCell {
    let style = TerminalStyle {
        foreground: translate_color(cell.fg),
        background: translate_color(cell.bg),
        underline: translate_color(cell.underline_color),
        flags: translate_modifier(cell.modifier),
    };
    let symbol = printable(cell.symbol());
    let width = match cell.diff_option {
        CellDiffOption::ForcedWidth(width) => width.get(),
        _ => symbol.cell_width(),
    };
    TerminalCell::wide(&symbol, width).with_style(style)
}

/// What a terminal shows of `symbol`: nothing of its control characters, nor
/// of the escape sequences (such as OSC 8 hyperlinks) it would interpret.
fn printable(symbol: &str) -> Cow<'_, str> {
    if !symbol.contains(char::is_control) {
        return Cow::Borrowed(symbol);
    }
    // A C1 control is the one-character form of ESC and a letter.
    let seven_bit = |c: char| match c {
        '\u{80}'..='\u{9f}' => [Some('\x1b'), char::from_u32(u32::from(c) - 0x40)],
        _ => [Some(c), None],
    };
    let symbol: String = symbol.chars().flat_map(seven_bit).flatten().collect();
    let mut parts = symbol.split('\x1b');
    let mut shown = String::from(parts.next().unwrap_or_default());
    let mut in_string = false;
    for part in parts {
        let mut rest = part.chars();
        let first = rest.next();
        // A string (OSC, DCS, SOS, PM, APC) runs to its BEL or its ST,
        // through any other sequence inside it.
        in_string = (in_string && first != Some('\\'))
            || matches!(first, Some(']' | 'P' | 'X' | '^' | '_'));
        if in_string {
            if let Some((_, text)) = part.split_once('\x07') {
                in_string = false;
                shown.push_str(text);
            }
        } else if first == Some('[') {
            // A control sequence runs to its final byte.
            rest.find(|c| ('\x40'..='\x7e').contains(c));
            shown.push_str(rest.as_str());
        } else {
            // Any other sequence is intermediate bytes and a final one, as
            // are ST and the two-character sequences with no intermediates.
            let mut rest = part
                .trim_start_matches(|c| (' '..='/').contains(&c))
                .chars();
            rest.next();
            shown.push_str(rest.as_str());
        }
    }
    shown.retain(|c| !c.is_control());
    Cow::Owned(shown)
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

/// Ratatui's modifier bits and [`StyleFlags`] use the same bit layout, so the
/// translation is a mask; the tests check that mapping flag by flag.
const fn translate_modifier(modifier: Modifier) -> StyleFlags {
    StyleFlags::from_bits_truncate(modifier.bits())
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
        for (x, y, cell) in content {
            // A draw that writes nothing leaves the frame as it was.
            self.written = None;
            // Positions outside the grid are ignored by the grid.
            self.grid.set_cell((x, y), &translate_cell(cell));
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.grid.set_cursor_visible(false);
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.grid.set_cursor_visible(true);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        let position = self.grid.cursor_position();
        Ok(Position::new(position.x, position.y))
    }

    fn set_cursor_position<P>(&mut self, position: P) -> Result<(), Self::Error>
    where
        P: Into<Position>,
    {
        // The grid clamps the cursor into itself, as a terminal does.
        let position = position.into();
        self.grid.set_cursor_position((position.x, position.y));
        self.keep_cursor();
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.written = None;
        self.grid.clear();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.written = None;
        let cursor = self.grid.cursor_position();
        match clear_type {
            ClearType::All => self.grid.clear(),
            ClearType::AfterCursor => self.grid.clear_range(cursor, (u16::MAX, u16::MAX)),
            ClearType::BeforeCursor => self.grid.clear_range((0, 0), cursor),
            ClearType::CurrentLine => self.grid.clear_row(cursor.y),
            ClearType::UntilNewLine => self.grid.clear_range(cursor, (u16::MAX, cursor.y)),
        };
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(size_from_grid(self.grid.size()))
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let pixels = self
            .geometry
            .filter(|geometry| geometry.grid() == self.grid.size())
            .map_or(UVec2::ZERO, |geometry| geometry.size());
        Ok(WindowSize {
            columns_rows: size_from_grid(self.grid.size()),
            pixels: Size::new(
                u16::try_from(pixels.x).unwrap_or(u16::MAX),
                u16::try_from(pixels.y).unwrap_or(u16::MAX),
            ),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn append_lines(&mut self, line_count: u16) -> Result<(), Self::Error> {
        let size = self.grid.size();
        let cursor = self.grid.cursor_position();
        let last_row = size.height.saturating_sub(1);
        let rows_below = last_row.saturating_sub(cursor.y);
        if line_count > rows_below {
            self.written = None;
            self.grid.scroll_up(0..size.height, line_count - rows_below);
        }
        self.grid.set_cursor_position((
            cursor.x.saturating_add(1),
            cursor.y.saturating_add(line_count),
        ));
        self.keep_cursor();
        Ok(())
    }

    #[cfg(feature = "scrolling-regions")]
    fn scroll_region_up(
        &mut self,
        region: std::ops::Range<u16>,
        line_count: u16,
    ) -> Result<(), Self::Error> {
        if self.grid.scroll_up(region, line_count) {
            self.written = None;
        }
        Ok(())
    }

    #[cfg(feature = "scrolling-regions")]
    fn scroll_region_down(
        &mut self,
        region: std::ops::Range<u16>,
        line_count: u16,
    ) -> Result<(), Self::Error> {
        if self.grid.scroll_down(region, line_count) {
            self.written = None;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_support;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_terminal::bevy::math::Vec2;
    use ratatui::style::Style;

    use super::test_support::measure;

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

    #[test]
    fn geometry_is_reported_only_for_its_grid_size() {
        let mut backend = RatatuiBackend::new(4, 2);
        let measured = measure(backend.grid().clone(), Vec2::new(10.0, 20.0));
        backend.geometry = measured.measured().copied();
        assert_eq!(backend.window_size().unwrap().pixels, Size::new(40, 40));
        backend.draw([(0, 0, &Cell::new("A"))].into_iter()).unwrap();
        assert_eq!(backend.window_size().unwrap().pixels, Size::new(40, 40));
        // A resize makes the adopted geometry stale until a new one arrives.
        backend.resize(8, 2);
        assert_eq!(backend.window_size().unwrap().pixels, Size::default());
    }

    #[test]
    fn escape_sequences_and_control_characters_are_not_drawn() {
        use std::num::NonZeroU16;
        let mut link = Cell::new("\x1b]8;;http://example.com\x1b\\link\x1b]8;;\x1b\\");
        link.set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(4).unwrap()));
        let cell = translate_cell(&link);
        assert_eq!(cell.symbol(), "link");
        assert_eq!(cell.occupancy().columns(), 4);
        assert_eq!(printable("\x1b]0;title\x07a\x1b[1;31mb\tc"), "abc");
        // Charset selection, strings ending in ST, and the one-character C1 forms.
        assert_eq!(
            printable("\x1b(Ba\x1bPq#0~~\x1b\\b\x1b_Gf=1;AA\x1b\\c"),
            "abc"
        );
        assert_eq!(printable("\u{9d}0;title\u{9c}a\u{9b}31mb\x1b7c"), "abc");
        // A string hides the sequences inside it.
        assert_eq!(
            printable("\x1bXa\x1b[31mb\x1b\\c\x1b]0;d\u{84}e\x07f"),
            "cf"
        );
        assert_eq!(translate_cell(&Cell::new("\x07")).symbol(), "");
        assert!(matches!(printable("e\u{301}"), Cow::Borrowed(_)));
    }

    #[test]
    fn plain_terminal_resizing_respects_viewport_policy() {
        use ratatui::{TerminalOptions, Viewport, layout::Rect};
        for viewport in [
            Viewport::Fullscreen,
            Viewport::Inline(2),
            Viewport::Fixed(Rect::new(1, 1, 3, 2)),
        ] {
            let mut terminal = ratatui::Terminal::with_options(
                RatatuiBackend::new(8, 4),
                TerminalOptions {
                    viewport: viewport.clone(),
                },
            )
            .unwrap();
            terminal.backend_mut().resize(10, 6);
            let frame = terminal
                .draw(|frame| {
                    frame.render_widget("abc", frame.area());
                })
                .unwrap();
            match viewport {
                Viewport::Fullscreen => assert_eq!(frame.buffer.area, Rect::new(0, 0, 10, 6)),
                Viewport::Inline(_) => assert_eq!(frame.buffer.area.as_size(), Size::new(10, 2)),
                Viewport::Fixed(area) => assert_eq!(frame.buffer.area, area),
            }
            assert_eq!(terminal.backend().grid().size(), GridSize::new(10, 6));
            if matches!(viewport, Viewport::Fixed(_)) {
                terminal.resize(Rect::new(0, 0, 5, 3)).unwrap();
                assert_eq!(terminal.draw(|_| {}).unwrap().area, Rect::new(0, 0, 5, 3));
            }
        }
    }

    #[test]
    fn wrapper_preserves_completed_frame_and_accepts_configured_terminal() {
        let raw = ratatui::Terminal::new(RatatuiBackend::new(4, 2)).unwrap();
        let mut terminal = RatatuiTerminal::from(raw);
        let first = terminal.draw(|frame| frame.render_widget("hi", frame.area()));
        assert_eq!(first.count, 0);
        assert_eq!(first.buffer[(0, 0)].symbol(), "h");
        assert_eq!(terminal.draw(|_| {}).count, 1);
    }

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
        assert_eq!(
            translated.occupancy(),
            bevy_terminal::prelude::CellOccupancy::Single
        );
        for (modifier, flag) in MODIFIER_FLAGS {
            assert!(translated.style.flags.contains(flag));
            assert_eq!(translate_modifier(modifier), flag);
        }
        assert_eq!(translate_modifier(Modifier::empty()), StyleFlags::empty());
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

        let snapshot = backend.grid();
        assert_eq!(snapshot.cell((1, 0)).unwrap(), &translate_cell(&cell));
        assert_eq!(
            snapshot.cell((1, 0)).unwrap().style.foreground,
            TerminalColor::Indexed(1)
        );
        assert!(snapshot.cell((1, 0)).unwrap().style.has(StyleFlags::BOLD));
        assert_eq!(snapshot.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn wide_cells_get_an_explicit_continuation() {
        let mut backend = RatatuiBackend::new(4, 1);
        let cell = Cell::new("界");
        backend.draw([(1, 0, &cell)].into_iter()).unwrap();

        let snapshot = backend.grid();
        assert_eq!(snapshot.cell((1, 0)).unwrap().symbol(), "界");
        assert_eq!(snapshot.cell((1, 0)).unwrap().occupancy().columns(), 2);
        assert!(snapshot.cell((2, 0)).unwrap().is_continuation());
        assert_eq!(snapshot.cell((3, 0)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn terminal_diff_replaces_wide_cells_without_stale_continuations() {
        let backend = RatatuiBackend::new(4, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| frame.render_widget("界A", frame.area()))
            .unwrap();
        let snapshot = terminal.backend().grid();
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "界");
        assert!(snapshot.cell((1, 0)).unwrap().is_continuation());
        assert_eq!(snapshot.cell((2, 0)).unwrap().symbol(), "A");

        terminal
            .draw(|frame| frame.render_widget("abc", frame.area()))
            .unwrap();
        let snapshot = terminal.backend().grid();
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "a");
        assert_eq!(snapshot.cell((1, 0)).unwrap().symbol(), "b");
        assert!(!snapshot.cell((1, 0)).unwrap().is_continuation());
        assert_eq!(snapshot.cell((2, 0)).unwrap().symbol(), "c");

        // A narrow replacement of only the anchor clears the orphaned continuation.
        terminal
            .draw(|frame| frame.render_widget("界 ", frame.area()))
            .unwrap();
        let mut backend = RatatuiBackend::from_grid(terminal.backend().grid().clone());
        backend.draw([(0, 0, &Cell::new("x"))].into_iter()).unwrap();
        let snapshot = backend.grid();
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "x");
        assert_eq!(snapshot.cell((1, 0)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn cursor_clear_and_resize_semantics_are_retained() {
        let mut backend = RatatuiBackend::new(4, 2);
        draw_text(&mut backend, "ABCDEFGH", 4);
        backend.set_cursor_position((1, 0)).unwrap();
        backend.show_cursor().unwrap();
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(1, 0));
        backend.clear_region(ClearType::UntilNewLine).unwrap();

        let snapshot = backend.grid();
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "A");
        assert_eq!(snapshot.cell((1, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(snapshot.cell((3, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(snapshot.cell((0, 1)).unwrap().symbol(), "E");
        assert!(snapshot.cursor_visible());
        assert_eq!(snapshot.cursor_position().x, 1);

        backend.clear_region(ClearType::CurrentLine).unwrap();
        assert_eq!(backend.grid().cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        backend.clear_region(ClearType::All).unwrap();
        assert_eq!(backend.grid().cell((0, 1)).unwrap(), &TerminalCell::EMPTY);

        draw_text(&mut backend, "ABCDEFGH", 4);
        backend.resize(2, 1);
        let resized = backend.grid();
        assert_eq!(resized.size(), GridSize::new(2, 1));
        assert_eq!(backend.size().unwrap(), Size::new(2, 1));
        assert_eq!(resized.cell((0, 0)).unwrap().symbol(), "A");
    }

    #[test]
    fn before_and_after_cursor_include_the_cursor_cell() {
        let mut after = RatatuiBackend::new(3, 2);
        draw_text(&mut after, "ABCDEF", 3);
        after.set_cursor_position((1, 0)).unwrap();
        after.clear_region(ClearType::AfterCursor).unwrap();
        assert_eq!(after.grid().cell((0, 0)).unwrap().symbol(), "A");
        assert_eq!(after.grid().cell((1, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(after.grid().cell((2, 1)).unwrap(), &TerminalCell::EMPTY);

        let mut before = RatatuiBackend::new(3, 2);
        draw_text(&mut before, "ABCDEF", 3);
        before.set_cursor_position((1, 1)).unwrap();
        before.clear_region(ClearType::BeforeCursor).unwrap();
        assert_eq!(before.grid().cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(before.grid().cell((1, 1)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(before.grid().cell((2, 1)).unwrap().symbol(), "F");
    }

    #[test]
    fn clearing_with_the_cursor_below_the_grid_clears_from_the_last_row() {
        let mut backend = RatatuiBackend::new(2, 2);
        let mut clear_at = |text, cursor, clear_type| {
            draw_text(&mut backend, text, 2);
            backend.set_cursor_position(cursor).unwrap();
            backend.clear_region(clear_type).unwrap();
            backend.grid().to_string()
        };
        assert_eq!(clear_at("ABCD", (1, 5), ClearType::UntilNewLine), "AB\nC ");
        assert_eq!(clear_at("ABCD", (1, 5), ClearType::CurrentLine), "AB\n  ");
        assert_eq!(clear_at("ABCD", (0, 5), ClearType::BeforeCursor), "  \n D");
    }

    #[test]
    fn append_lines_moves_the_cursor_on_an_empty_grid() {
        let mut backend = RatatuiBackend::new(15, 0);
        backend.append_lines(1).unwrap();
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(1, 0));
    }

    #[test]
    fn a_wide_glyph_moving_left_leaves_no_stale_cells() {
        use ratatui::text::{Line, Span};
        let mut terminal = RatatuiTerminal::new(4, 1);
        let red = Style::new().bg(Color::Red);
        terminal.draw(|frame| {
            let line = Line::from(vec![Span::raw("a"), Span::styled("界", red)]);
            frame.render_widget(line, frame.area());
        });
        terminal.draw(|frame| frame.render_widget(Span::styled("界", red), frame.area()));
        let snapshot = terminal.grid();
        assert_eq!(snapshot.row_text(0), "界  ");
        assert_eq!(snapshot.cell((2, 0)).unwrap(), &TerminalCell::EMPTY);
    }

    #[cfg(feature = "scrolling-regions")]
    #[test]
    fn scroll_regions_move_and_clear_rows() {
        let mut backend = RatatuiBackend::new(2, 3);
        draw_text(&mut backend, "AABBCC", 2);

        backend.scroll_region_up(0..3, 1).unwrap();
        let up = backend.grid();
        assert_eq!(up.cell((0, 0)).unwrap().symbol(), "B");
        assert_eq!(up.cell((0, 1)).unwrap().symbol(), "C");
        assert_eq!(up.cell((0, 2)).unwrap(), &TerminalCell::EMPTY);

        backend.scroll_region_down(0..3, 1).unwrap();
        let down = backend.grid();
        assert_eq!(down.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(down.cell((0, 1)).unwrap().symbol(), "B");
        assert_eq!(down.cell((0, 2)).unwrap().symbol(), "C");
    }

    #[test]
    fn resize_preserves_the_overlap_and_window_size_reports_pixels() {
        let mut backend = RatatuiBackend::new(4, 3);
        draw_text(&mut backend, "AAAABBBBCCCC", 4);

        backend.resize(2, 3);
        let snapshot = backend.grid();
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "A");
        assert_eq!(snapshot.cell((0, 1)).unwrap().symbol(), "B");
        assert_eq!(snapshot.cell((0, 2)).unwrap().symbol(), "C");

        // Pixel metrics stay zero until a renderer configures the cell size.
        let window_size = backend.window_size().unwrap();
        assert_eq!(window_size.columns_rows, Size::new(2, 3));
        assert_eq!(window_size.pixels, Size::new(0, 0));

        backend.geometry = measure(backend.grid().clone(), Vec2::new(11.0, 20.0))
            .measured()
            .copied();
        assert_eq!(backend.window_size().unwrap().pixels, Size::new(22, 60));
        let mut other = RatatuiBackend::from_grid(backend.grid().clone());
        other.geometry = measure(other.grid().clone(), Vec2::new(22.0, 40.0))
            .measured()
            .copied();
        assert_eq!(backend.window_size().unwrap().pixels, Size::new(22, 60));
        assert_eq!(other.window_size().unwrap().pixels, Size::new(44, 120));
        // Geometry measured for another grid size is not reported...
        backend.resize(3, 3);
        assert_eq!(backend.window_size().unwrap().pixels, Size::default());
        // ...until the grid has that size again.
        backend.resize(2, 3);
        assert_eq!(backend.window_size().unwrap().pixels, Size::new(22, 60));
    }

    #[test]
    fn resize_grid_resizes_the_backend_and_ratatui_buffers_together() {
        let mut terminal = RatatuiTerminal::new(4, 2);
        terminal.draw(|frame| frame.render_widget("ABCDEFGH", frame.area()));
        terminal.resize_grid(6, 3);
        assert_eq!(terminal.size().unwrap(), Size::new(6, 3));
        assert_eq!(terminal.grid().size(), GridSize::new(6, 3));
        terminal.draw(|frame| frame.render_widget("wide now", frame.area()));
        assert_eq!(terminal.grid().cell((5, 0)).unwrap().symbol(), "n");
        assert_eq!(terminal.grid().row_text(0), "wide n");
        assert_eq!(terminal.grid().to_string(), "wide n\n      \n      ");
    }

    /// However a grid is resized between draws, what a draw leaves is what
    /// drawing the same frame on a new terminal of that size shows: Ratatui
    /// writes only what differs from its last frame, so the grid must still
    /// hold that frame wherever the sizes overlap.
    #[test]
    fn resizes_between_draws_never_leave_a_stale_frame() {
        // Narrow text only: Ratatui's own diff skips the cell after a wide
        // glyph at a row's end, on any backend.
        const FRAMES: [&str; 3] = ["ABCDEFGHIJKLMNOPQRSTUVWX", "abcdefgh ijkl mn", ""];
        let draw = |terminal: &mut RatatuiTerminal, frame: usize| {
            let text = ratatui::widgets::Paragraph::new(FRAMES[frame])
                .wrap(ratatui::widgets::Wrap { trim: false });
            terminal.draw(|frame| frame.render_widget(text, frame.area()));
        };
        // A fixed sequence of pseudo-random numbers.
        let mut state = 0x2545_f491_u32;
        let mut next = |below: u32| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state % below) as u16
        };
        let mut terminal = RatatuiTerminal::new(6, 3);
        let mut frame = 0;
        draw(&mut terminal, frame);
        for step in 0..2000 {
            // Mostly resizes, as a dragged window makes, and mostly the same
            // frame again, as an application that draws on change makes.
            if next(4) > 0 {
                let (columns, rows) = (1 + next(8), 1 + next(4));
                terminal.backend_mut().resize(columns, rows);
                continue;
            }
            if next(4) == 0 {
                frame = usize::from(next(3));
            }
            draw(&mut terminal, frame);
            let size = terminal.grid().size();
            let mut fresh = RatatuiTerminal::new(size.width, size.height);
            draw(&mut fresh, frame);
            assert_eq!(
                terminal.grid().to_string(),
                fresh.grid().to_string(),
                "step {step}, frame {frame}, {size:?}"
            );
        }
    }

    /// Any run of resizes leaves the last frame as one resize to the final
    /// size would, wide glyphs cut and brought back whole.
    #[test]
    fn a_run_of_resizes_is_one_resize_of_the_last_frame() {
        let mut backend = RatatuiBackend::new(7, 3);
        let cells = [(0, 0, "a"), (1, 0, "界"), (3, 0, "b"), (5, 0, "語")];
        let cells = cells
            .into_iter()
            .chain([(0, 1, "日"), (2, 1, "本"), (6, 2, "c")]);
        let cells: Vec<_> = cells
            .map(|(x, y, symbol)| (x, y, Cell::new(symbol)))
            .collect();
        let draw = cells.iter().map(|(x, y, cell)| (*x, *y, cell));
        backend.draw(draw).unwrap();
        let frame = backend.grid().clone();
        assert!(frame.cell((2, 0)).unwrap().is_continuation());
        assert!(frame.cell((6, 0)).unwrap().is_continuation());
        let mut state = 0x9e37_79b9_u32;
        let mut next = |below: u32| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state % below) as u16
        };
        for step in 0..500 {
            let (columns, rows) = (1 + next(9), 1 + next(5));
            backend.resize(columns, rows);
            let mut once = frame.clone();
            once.resize((columns, rows));
            assert_eq!(backend.grid().cells(), once.cells(), "step {step}");
        }
        backend.resize(7, 3);
        assert_eq!(backend.grid().cells(), frame.cells());
    }

    #[test]
    fn a_draw_that_writes_nothing_keeps_the_frame_resizes_restore() {
        let mut backend = RatatuiBackend::new(4, 1);
        draw_text(&mut backend, "ABCD", 4);
        backend.resize(2, 1);
        backend.draw(std::iter::empty()).unwrap();
        backend.append_lines(0).unwrap();
        backend.resize(4, 1);
        assert_eq!(backend.grid().row_text(0), "ABCD");
        // The cursor comes back too, to where it was last put.
        let mut backend = RatatuiBackend::new(6, 3);
        backend.set_cursor_position((5, 2)).unwrap();
        backend.resize(3, 1);
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(2, 0));
        backend.resize(6, 4);
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(5, 2));
        backend.resize(2, 2);
        backend.set_cursor_position((0, 1)).unwrap();
        backend.resize(6, 3);
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(0, 1));
        // Asking for more than the grid can hold changes nothing and keeps no copy.
        let mut backend = RatatuiBackend::new(1024, 1024);
        backend.resize(1024, 1025);
        assert!(backend.written.is_none());
    }

    #[test]
    fn append_lines_moves_right_scrolls_and_clamps_the_cursor() {
        let mut backend = RatatuiBackend::new(2, 3);
        draw_text(&mut backend, "AABBCC", 2);

        backend.set_cursor_position((0, 2)).unwrap();
        backend.append_lines(1).unwrap();
        let scrolled = backend.grid();
        assert_eq!(scrolled.cursor_position().x, 1);
        assert_eq!(scrolled.cursor_position().y, 2);
        assert_eq!(scrolled.cell((0, 0)).unwrap().symbol(), "B");
        assert_eq!(scrolled.cell((0, 2)).unwrap(), &TerminalCell::EMPTY);

        backend.set_cursor_position((0, u16::MAX)).unwrap();
        backend.append_lines(0).unwrap();
        assert_eq!(backend.grid().cursor_position().x, 1);
        assert_eq!(backend.grid().cursor_position().y, 2);
    }

    #[test]
    fn the_plugin_draws_spawned_terminals_and_hands_them_their_geometry() {
        use bevy_terminal::prelude::{FontFaces, TerminalSizing};
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy_terminal::bevy::asset::AssetPlugin::default(),
            bevy_terminal::bevy::text::TextPlugin,
        ))
        .init_asset::<Image>()
        .add_plugins(RatatuiPlugin);
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                include_bytes!(
                    "../crates/bevy_terminal/assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
                )
                .to_vec(),
            ));
        let mut terminal = RatatuiTerminal::new(5, 2);
        terminal.draw(|frame| frame.render_widget("Hello", frame.area()));
        let entity = app
            .world_mut()
            .spawn((
                terminal,
                TerminalRenderConfig {
                    font: FontFaces::regular(font),
                    sizing: TerminalSizing::Fixed {
                        cell_size: Vec2::new(10.0, 20.0),
                        font_size: 12.0,
                    },
                    ..default()
                },
            ))
            .id();
        for _ in 0..4 {
            app.update();
        }
        let texture = app.world().get::<TerminalTexture>(entity).unwrap();
        assert_eq!(texture.measured().unwrap().grid(), GridSize::new(5, 2));
        let mut terminal = app.world_mut().get_mut::<RatatuiTerminal>(entity).unwrap();
        assert_eq!(terminal.grid().row_text(0), "Hello");
        let pixels = terminal.backend_mut().window_size().unwrap().pixels;
        assert_eq!(pixels, Size::new(50, 40));

        // A backend that replaces another is handed the geometry too...
        app.world_mut()
            .entity_mut(entity)
            .insert(RatatuiTerminal::new(5, 2));
        app.update();
        let mut terminal = app.world_mut().get_mut::<RatatuiTerminal>(entity).unwrap();
        let pixels = terminal.backend_mut().window_size().unwrap().pixels;
        assert_eq!(pixels, Size::new(50, 40));
        // ...and loses it when the terminal can no longer be drawn.
        let config = app.world_mut().get_mut::<TerminalRenderConfig>(entity);
        config.unwrap().sizing = TerminalSizing::font(f32::NAN);
        app.update();
        let mut terminal = app.world_mut().get_mut::<RatatuiTerminal>(entity).unwrap();
        let pixels = terminal.backend_mut().window_size().unwrap().pixels;
        assert_eq!(pixels, Size::default());
    }

    #[test]
    fn a_real_ratatui_terminal_drives_the_grid() {
        use ratatui::widgets::{Block, Borders, Paragraph};

        let mut terminal = ratatui::Terminal::new(RatatuiBackend::new(10, 3)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("hi").block(Block::new().borders(Borders::ALL)),
                    frame.area(),
                );
                frame.set_cursor_position((3, 1));
            })
            .unwrap();
        let grid = terminal.backend().grid();
        assert_eq!(grid.cell((0, 0)).unwrap().symbol(), "┌");
        assert_eq!(grid.cell((1, 1)).unwrap().symbol(), "h");
        assert!(grid.cursor_visible());
        assert_eq!(grid.cursor_position().x, 3);
        assert_eq!(grid.cursor_position().y, 1);
    }
}
