use std::{
    convert::Infallible,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard},
};

use bevy::prelude::Resource;
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::{Buffer, Cell, CellDiffOption, CellWidth},
    layout::{Position, Rect, Size},
};

/// A cheap, thread-safe handle to a terminal surface.
///
/// The Ratatui backend writes through this handle. [`crate::BevyGridPlugin`]
/// reads snapshots from it in a Bevy system, so drawing a Ratatui frame never
/// requires access to the Bevy [`bevy::prelude::World`].
#[derive(Clone, Resource)]
pub struct TerminalSurface {
    shared: Arc<Mutex<SurfaceState>>,
}

impl TerminalSurface {
    /// Creates an empty terminal surface with the given size in cells.
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            shared: Arc::new(Mutex::new(SurfaceState {
                buffer: Buffer::empty(Rect::new(0, 0, columns, rows)),
                cursor_position: Position::ORIGIN,
                cursor_visible: false,
                cell_size: None,
                pixel_size: Size::ZERO,
                revision: 0,
                dirty_cells: vec![false; usize::from(columns) * usize::from(rows)],
            })),
        }
    }

    /// Returns an owned snapshot of the most recently submitted terminal state.
    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        let state = self.lock();
        TerminalSnapshot {
            buffer: state.buffer.clone(),
            cursor_position: state.cursor_position,
            cursor_visible: state.cursor_visible,
            revision: state.revision,
        }
    }

    /// Returns the current monotonically increasing change revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    pub(crate) fn update_snapshot(&self, snapshot: &mut TerminalSnapshot) -> SnapshotUpdate {
        let mut state = self.lock();
        if snapshot.size() != state.size() {
            let changed_cells = state.buffer.content.len();
            let changed_rows = (0..state.buffer.area.height).collect();
            *snapshot = TerminalSnapshot {
                buffer: state.buffer.clone(),
                cursor_position: state.cursor_position,
                cursor_visible: state.cursor_visible,
                revision: state.revision,
            };
            state.dirty_cells.fill(false);
            return SnapshotUpdate {
                changed_rows,
                changed_cells,
                resized: true,
                cursor_position_changed: true,
                cursor_visibility_changed: true,
            };
        }

        let width = usize::from(state.buffer.area.width);
        let mut changed_rows = Vec::new();
        let mut changed_cells = 0;
        for row in 0..state.buffer.area.height {
            let start = usize::from(row) * width;
            let end = start + width;
            let mut row_changed = false;
            for index in start..end {
                if state.dirty_cells[index] {
                    snapshot.buffer.content[index] = state.buffer.content[index].clone();
                    state.dirty_cells[index] = false;
                    changed_cells += 1;
                    row_changed = true;
                }
            }
            if row_changed {
                changed_rows.push(row);
            }
        }

        let cursor_position_changed = snapshot.cursor_position != state.cursor_position;
        let cursor_visibility_changed = snapshot.cursor_visible != state.cursor_visible;
        snapshot.cursor_position = state.cursor_position;
        snapshot.cursor_visible = state.cursor_visible;
        snapshot.revision = state.revision;
        SnapshotUpdate {
            changed_rows,
            changed_cells,
            resized: false,
            cursor_position_changed,
            cursor_visibility_changed,
        }
    }

    /// Sets the logical-pixel dimensions of one cell for [`Backend::window_size`].
    ///
    /// [`crate::BevyGridPlugin`] calls this from its render configuration.
    pub fn set_cell_size(&self, width: f32, height: f32) {
        let mut state = self.lock();
        let cell_size = Some((width.max(0.0), height.max(0.0)));
        let pixel_size = surface_pixel_size(state.size(), cell_size);
        if state.cell_size != cell_size || state.pixel_size != pixel_size {
            state.cell_size = cell_size;
            state.pixel_size = pixel_size;
            state.touch();
        }
    }

    fn lock(&self) -> MutexGuard<'_, SurfaceState> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) struct SnapshotUpdate {
    pub(crate) changed_rows: Vec<u16>,
    pub(crate) changed_cells: usize,
    pub(crate) resized: bool,
    pub(crate) cursor_position_changed: bool,
    pub(crate) cursor_visibility_changed: bool,
}

/// An owned view of all state needed to render a terminal surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalSnapshot {
    buffer: Buffer,
    cursor_position: Position,
    cursor_visible: bool,
    revision: u64,
}

impl TerminalSnapshot {
    /// Returns the terminal cell buffer.
    #[must_use]
    pub const fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Returns the terminal size in columns and rows.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.buffer.area.as_size()
    }

    /// Returns the cursor position in terminal cells.
    #[must_use]
    pub const fn cursor_position(&self) -> Position {
        self.cursor_position
    }

    /// Returns whether Ratatui requested a visible cursor.
    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Returns the surface change revision captured by this snapshot.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

struct SurfaceState {
    buffer: Buffer,
    cursor_position: Position,
    cursor_visible: bool,
    cell_size: Option<(f32, f32)>,
    pixel_size: Size,
    revision: u64,
    dirty_cells: Vec<bool>,
}

impl SurfaceState {
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn size(&self) -> Size {
        self.buffer.area.as_size()
    }

    fn contains(&self, x: u16, y: u16) -> bool {
        x < self.buffer.area.width && y < self.buffer.area.height
    }

    fn reset_row(&mut self, row: u16) {
        if row >= self.buffer.area.height {
            return;
        }
        for column in 0..self.buffer.area.width {
            let index =
                usize::from(row) * usize::from(self.buffer.area.width) + usize::from(column);
            if self.buffer.content[index] != Cell::EMPTY {
                self.buffer.content[index].reset();
                self.dirty_cells[index] = true;
            }
        }
    }

    fn mark_cell_dirty(&mut self, x: u16, y: u16) {
        let index = usize::from(y) * usize::from(self.buffer.area.width) + usize::from(x);
        if let Some(dirty) = self.dirty_cells.get_mut(index) {
            *dirty = true;
        }
    }

    fn mark_rows_dirty(&mut self, rows: Range<u16>) {
        let width = usize::from(self.buffer.area.width);
        for row in rows.start.min(self.buffer.area.height)..rows.end.min(self.buffer.area.height) {
            let start = usize::from(row) * width;
            self.dirty_cells[start..start + width].fill(true);
        }
    }
}

/// A Ratatui [`Backend`] whose output is consumed by the Bevy renderer.
///
/// Clone [`Self::surface`] before moving the backend into a
/// [`ratatui::Terminal`], then pass that handle to [`crate::BevyGridPlugin`].
pub struct BevyBackend {
    surface: TerminalSurface,
}

impl BevyBackend {
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

    /// Resizes the backend buffer, preserving cells in the overlapping area.
    ///
    /// When this backend is owned by a [`ratatui::Terminal`], call
    /// [`ratatui::Terminal::autoresize`] after this method so Ratatui's own
    /// double buffers adopt the new size.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        let mut state = self.surface.lock();
        if state.buffer.area.width == columns && state.buffer.area.height == rows {
            return;
        }
        let old_buffer = std::mem::replace(
            &mut state.buffer,
            Buffer::empty(Rect::new(0, 0, columns, rows)),
        );
        let copied_columns = columns.min(old_buffer.area.width);
        let copied_rows = rows.min(old_buffer.area.height);
        for y in 0..copied_rows {
            for x in 0..copied_columns {
                state.buffer[(x, y)] = old_buffer[(x, y)].clone();
            }
        }
        state.pixel_size = surface_pixel_size(state.size(), state.cell_size);
        state.dirty_cells = vec![true; usize::from(columns) * usize::from(rows)];
        state.cursor_position.x = state.cursor_position.x.min(columns.saturating_sub(1));
        state.cursor_position.y = state.cursor_position.y.min(rows.saturating_sub(1));
        state.touch();
    }

    /// Returns a snapshot without requiring a second surface handle.
    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        self.surface.snapshot()
    }

    fn scroll_up(state: &mut SurfaceState, region: Range<u16>, line_count: u16) {
        let start = region.start.min(state.buffer.area.height);
        let end = region.end.min(state.buffer.area.height).max(start);
        let height = end - start;
        let count = line_count.min(height);
        if count == 0 {
            return;
        }

        let width = state.buffer.area.width;
        for destination_y in start..end - count {
            let source_y = destination_y + count;
            for x in 0..width {
                state.buffer[(x, destination_y)] = state.buffer[(x, source_y)].clone();
            }
        }
        for y in end - count..end {
            state.reset_row(y);
        }
        state.mark_rows_dirty(start..end);
    }

    fn scroll_down(state: &mut SurfaceState, region: Range<u16>, line_count: u16) {
        let start = region.start.min(state.buffer.area.height);
        let end = region.end.min(state.buffer.area.height).max(start);
        let height = end - start;
        let count = line_count.min(height);
        if count == 0 {
            return;
        }

        let width = state.buffer.area.width;
        for destination_y in (start + count..end).rev() {
            let source_y = destination_y - count;
            for x in 0..width {
                state.buffer[(x, destination_y)] = state.buffer[(x, source_y)].clone();
            }
        }
        for y in start..start + count {
            state.reset_row(y);
        }
        state.mark_rows_dirty(start..end);
    }
}

fn surface_pixel_size(size: Size, cell_size: Option<(f32, f32)>) -> Size {
    let Some((cell_width, cell_height)) = cell_size else {
        return Size::ZERO;
    };
    Size::new(
        pixel_dimension(size.width, cell_width),
        pixel_dimension(size.height, cell_height),
    )
}

fn pixel_dimension(cells: u16, cell_size: f32) -> u16 {
    let value = f32::from(cells) * cell_size;
    if value.is_finite() {
        value.round().clamp(0.0, f32::from(u16::MAX)) as u16
    } else if value.is_sign_positive() {
        u16::MAX
    } else {
        0
    }
}

impl Backend for BevyBackend {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut state = self.surface.lock();
        let mut changed = false;
        for (x, y, cell) in content {
            if !state.contains(x, y) {
                continue;
            }

            if state.buffer[(x, y)] != *cell {
                state.buffer[(x, y)] = cell.clone();
                state.mark_cell_dirty(x, y);
                changed = true;
            }

            // Ratatui omits Skip continuation cells from its diff iterator. Keep
            // explicit markers in our complete retained buffer so wide symbols
            // do not gain an extra blank column when converted to Bevy text.
            let width = cell.cell_width().max(1);
            for continuation_x in x.saturating_add(1)..x.saturating_add(width) {
                if !state.contains(continuation_x, y) {
                    break;
                }
                let mut continuation = cell.clone();
                continuation
                    .set_symbol(" ")
                    .set_diff_option(CellDiffOption::Skip);
                if state.buffer[(continuation_x, y)] != continuation {
                    state.buffer[(continuation_x, y)] = continuation;
                    state.mark_cell_dirty(continuation_x, y);
                    changed = true;
                }
            }
        }
        if changed {
            state.touch();
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        if state.cursor_visible {
            state.cursor_visible = false;
            state.touch();
        }
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        if !state.cursor_visible {
            state.cursor_visible = true;
            state.touch();
        }
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.surface.lock().cursor_position)
    }

    fn set_cursor_position<P>(&mut self, position: P) -> Result<(), Self::Error>
    where
        P: Into<Position>,
    {
        let mut state = self.surface.lock();
        let position = position.into();
        if state.cursor_position != position {
            state.cursor_position = position;
            state.touch();
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        let changed = state.buffer.content.iter().any(|cell| cell != &Cell::EMPTY);
        if changed {
            state.buffer.reset();
            state.dirty_cells.fill(true);
            state.touch();
        }
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        let width = state.buffer.area.width;
        let height = state.buffer.area.height;
        if width == 0 || height == 0 {
            return Ok(());
        }

        let cursor_x = state.cursor_position.x.min(width - 1);
        let cursor_y = state.cursor_position.y.min(height - 1);
        let cursor_index = usize::from(cursor_y) * usize::from(width) + usize::from(cursor_x);
        let line_start = usize::from(cursor_y) * usize::from(width);
        let line_end = line_start + usize::from(width);

        let range = match clear_type {
            ClearType::All => 0..state.buffer.content.len(),
            ClearType::AfterCursor => cursor_index..state.buffer.content.len(),
            ClearType::BeforeCursor => 0..cursor_index.saturating_add(1),
            ClearType::CurrentLine => line_start..line_end,
            ClearType::UntilNewLine => cursor_index..line_end,
        };
        let mut changed = false;
        for index in range {
            if state.buffer.content[index] != Cell::EMPTY {
                state.buffer.content[index].reset();
                state.dirty_cells[index] = true;
                changed = true;
            }
        }
        if changed {
            state.touch();
        }
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.surface.lock().size())
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let state = self.surface.lock();
        Ok(WindowSize {
            columns_rows: state.size(),
            pixels: state.pixel_size,
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn append_lines(&mut self, line_count: u16) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        let size = state.size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }

        state.cursor_position.x = state
            .cursor_position
            .x
            .saturating_add(1)
            .min(size.width - 1);
        let rows_below = size.height - 1 - state.cursor_position.y.min(size.height - 1);
        if line_count <= rows_below {
            state.cursor_position.y = state
                .cursor_position
                .y
                .saturating_add(line_count)
                .min(size.height - 1);
        } else {
            let scroll_count = line_count - rows_below;
            Self::scroll_up(&mut state, 0..size.height, scroll_count);
            state.cursor_position.y = size.height - 1;
        }
        state.touch();
        Ok(())
    }

    fn scroll_region_up(&mut self, region: Range<u16>, line_count: u16) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        Self::scroll_up(&mut state, region, line_count);
        state.touch();
        Ok(())
    }

    fn scroll_region_down(
        &mut self,
        region: Range<u16>,
        line_count: u16,
    ) -> Result<(), Self::Error> {
        let mut state = self.surface.lock();
        Self::scroll_down(&mut state, region, line_count);
        state.touch();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};

    #[test]
    fn partial_draw_preserves_other_cells_and_clips_invalid_coordinates() {
        let mut backend = BevyBackend::new(3, 2);
        let mut cell = Cell::new("X");
        cell.set_style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD));
        backend
            .draw([(1, 0, &cell), (99, 99, &cell)].into_iter())
            .unwrap();

        let snapshot = backend.snapshot();
        assert_eq!(snapshot.buffer()[(1, 0)], cell);
        assert_eq!(snapshot.buffer()[(0, 0)], Cell::EMPTY);
    }

    #[test]
    fn no_op_backend_calls_do_not_advance_the_surface_revision() {
        let mut backend = BevyBackend::new(2, 1);
        let empty = Cell::EMPTY;
        let initial = backend.surface().revision();

        backend.draw([(0, 0, &empty)].into_iter()).unwrap();
        backend.flush().unwrap();
        backend.hide_cursor().unwrap();
        backend.set_cursor_position(Position::ORIGIN).unwrap();
        backend.clear().unwrap();
        backend.resize(2, 1);

        assert_eq!(backend.surface().revision(), initial);

        let changed = Cell::new("X");
        backend.draw([(0, 0, &changed)].into_iter()).unwrap();
        let changed_revision = backend.surface().revision();
        assert_ne!(changed_revision, initial);
        backend.draw([(0, 0, &changed)].into_iter()).unwrap();
        backend.flush().unwrap();
        assert_eq!(backend.surface().revision(), changed_revision);
    }

    #[test]
    fn incremental_snapshots_clone_only_changed_cells_and_report_rows() {
        let mut backend = BevyBackend::new(4, 3);
        let mut snapshot = backend.snapshot();
        let changed = Cell::new("X");
        backend.draw([(2, 1, &changed)].into_iter()).unwrap();

        let update = backend.surface().update_snapshot(&mut snapshot);
        assert_eq!(update.changed_cells, 1);
        assert_eq!(update.changed_rows, [1]);
        assert!(!update.resized);
        assert_eq!(snapshot.buffer()[(2, 1)], changed);

        backend.set_cursor_position((3, 2)).unwrap();
        let cursor_update = backend.surface().update_snapshot(&mut snapshot);
        assert_eq!(cursor_update.changed_cells, 0);
        assert!(cursor_update.changed_rows.is_empty());
        assert!(cursor_update.cursor_position_changed);
        assert!(!cursor_update.cursor_visibility_changed);

        backend.resize(2, 2);
        let resized = backend.surface().update_snapshot(&mut snapshot);
        assert!(resized.resized);
        assert_eq!(resized.changed_cells, 4);
        assert_eq!(resized.changed_rows, [0, 1]);
    }

    #[test]
    fn wide_cells_get_an_explicit_skip_continuation() {
        let mut backend = BevyBackend::new(4, 1);
        let cell = Cell::new("界");
        backend.draw([(1, 0, &cell)].into_iter()).unwrap();

        let snapshot = backend.snapshot();
        assert_eq!(snapshot.buffer()[(1, 0)].symbol(), "界");
        assert_eq!(snapshot.buffer()[(2, 0)].diff_option, CellDiffOption::Skip);
    }

    #[test]
    fn terminal_diff_replaces_wide_cells_without_stale_continuations() {
        let backend = BevyBackend::new(4, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| frame.render_widget("界A", frame.area()))
            .unwrap();
        assert_eq!(
            terminal.backend().snapshot().buffer()[(0, 0)].symbol(),
            "界"
        );
        assert_eq!(
            terminal.backend().snapshot().buffer()[(1, 0)].diff_option,
            CellDiffOption::Skip
        );
        assert_eq!(terminal.backend().snapshot().buffer()[(2, 0)].symbol(), "A");

        terminal
            .draw(|frame| frame.render_widget("abc", frame.area()))
            .unwrap();
        let snapshot = terminal.backend().snapshot();
        assert_eq!(snapshot.buffer()[(0, 0)].symbol(), "a");
        assert_eq!(snapshot.buffer()[(1, 0)].symbol(), "b");
        assert_ne!(snapshot.buffer()[(1, 0)].diff_option, CellDiffOption::Skip);
        assert_eq!(snapshot.buffer()[(2, 0)].symbol(), "c");
    }

    #[test]
    fn cursor_clear_and_resize_semantics_are_retained() {
        let mut backend = BevyBackend::new(4, 2);
        let cells: Vec<Cell> = "ABCDEFGH".chars().map(Cell::from).collect();
        backend
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index % 4) as u16, (index / 4) as u16, cell)),
            )
            .unwrap();
        backend.set_cursor_position((1, 0)).unwrap();
        backend.show_cursor().unwrap();
        backend.clear_region(ClearType::UntilNewLine).unwrap();

        let snapshot = backend.snapshot();
        assert_eq!(snapshot.buffer()[(0, 0)].symbol(), "A");
        assert_eq!(snapshot.buffer()[(1, 0)], Cell::EMPTY);
        assert_eq!(snapshot.buffer()[(3, 0)], Cell::EMPTY);
        assert_eq!(snapshot.buffer()[(0, 1)].symbol(), "E");
        assert!(snapshot.cursor_visible());
        assert_eq!(snapshot.cursor_position(), Position::new(1, 0));

        backend.resize(2, 1);
        let resized = backend.snapshot();
        assert_eq!(resized.size(), Size::new(2, 1));
        assert_eq!(resized.buffer()[(0, 0)].symbol(), "A");
    }

    #[test]
    fn before_and_after_cursor_include_the_cursor_cell() {
        let cells: Vec<Cell> = "ABCDEF".chars().map(Cell::from).collect();

        let mut after = BevyBackend::new(3, 2);
        after
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index % 3) as u16, (index / 3) as u16, cell)),
            )
            .unwrap();
        after.set_cursor_position((1, 0)).unwrap();
        after.clear_region(ClearType::AfterCursor).unwrap();
        assert_eq!(after.snapshot().buffer()[(0, 0)].symbol(), "A");
        assert_eq!(after.snapshot().buffer()[(1, 0)], Cell::EMPTY);

        let mut before = BevyBackend::new(3, 2);
        before
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index % 3) as u16, (index / 3) as u16, cell)),
            )
            .unwrap();
        before.set_cursor_position((1, 1)).unwrap();
        before.clear_region(ClearType::BeforeCursor).unwrap();
        assert_eq!(before.snapshot().buffer()[(1, 1)], Cell::EMPTY);
        assert_eq!(before.snapshot().buffer()[(2, 1)].symbol(), "F");
    }

    #[test]
    fn scroll_regions_move_and_clear_rows() {
        let mut backend = BevyBackend::new(2, 3);
        let cells: Vec<Cell> = "AABBCC".chars().map(Cell::from).collect();
        backend
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index % 2) as u16, (index / 2) as u16, cell)),
            )
            .unwrap();

        backend.scroll_region_up(0..3, 1).unwrap();
        let up = backend.snapshot();
        assert_eq!(up.buffer()[(0, 0)].symbol(), "B");
        assert_eq!(up.buffer()[(0, 1)].symbol(), "C");
        assert_eq!(up.buffer()[(0, 2)], Cell::EMPTY);

        backend.scroll_region_down(0..3, 1).unwrap();
        let down = backend.snapshot();
        assert_eq!(down.buffer()[(0, 0)], Cell::EMPTY);
        assert_eq!(down.buffer()[(0, 1)].symbol(), "B");
        assert_eq!(down.buffer()[(0, 2)].symbol(), "C");
    }

    #[test]
    fn resize_preserves_the_two_dimensional_overlap() {
        let mut backend = BevyBackend::new(4, 3);
        let cells: Vec<Cell> = "AAAABBBBCCCC".chars().map(Cell::from).collect();
        backend
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index % 4) as u16, (index / 4) as u16, cell)),
            )
            .unwrap();

        backend.resize(2, 3);
        let snapshot = backend.snapshot();
        assert_eq!(snapshot.buffer()[(0, 0)].symbol(), "A");
        assert_eq!(snapshot.buffer()[(0, 1)].symbol(), "B");
        assert_eq!(snapshot.buffer()[(0, 2)].symbol(), "C");

        backend.surface().set_cell_size(10.8, 20.0);
        let window_size = backend.window_size().unwrap();
        assert_eq!(window_size.columns_rows, Size::new(2, 3));
        assert_eq!(window_size.pixels, Size::new(22, 60));
    }

    #[test]
    fn append_lines_moves_right_scrolls_and_clamps_the_cursor() {
        let mut backend = BevyBackend::new(2, 3);
        let cells: Vec<Cell> = "AABBCC".chars().map(Cell::from).collect();
        backend
            .draw(
                cells
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| ((index % 2) as u16, (index / 2) as u16, cell)),
            )
            .unwrap();

        backend.set_cursor_position((0, 2)).unwrap();
        backend.append_lines(1).unwrap();
        let scrolled = backend.snapshot();
        assert_eq!(scrolled.cursor_position(), Position::new(1, 2));
        assert_eq!(scrolled.buffer()[(0, 0)].symbol(), "B");
        assert_eq!(scrolled.buffer()[(0, 2)], Cell::EMPTY);

        backend.set_cursor_position((0, u16::MAX)).unwrap();
        backend.append_lines(0).unwrap();
        assert_eq!(backend.snapshot().cursor_position(), Position::new(1, 2));
    }
}
