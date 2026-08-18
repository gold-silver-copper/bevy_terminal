//! Thread-safe retained terminal surface with transactional updates.

use std::{
    ops::Range,
    sync::{Arc, Mutex, MutexGuard},
};

use bevy::math::{UVec2, Vec2};

use crate::scene::{CellOccupancy, CellPosition, GridSize, TerminalCell, TerminalSnapshot};

/// A cheap, thread-safe handle to a retained terminal surface.
///
/// Producers write through [`TerminalSurface::begin_update`]. The renderer
/// plugin reads incremental snapshots from the same handle in a Bevy system, so
/// producing a frame never requires access to the Bevy world.
#[derive(Clone)]
pub struct TerminalSurface {
    shared: Arc<Mutex<SurfaceState>>,
}

impl TerminalSurface {
    /// Creates an empty terminal surface with the given size in cells
    /// (`(columns, rows)` or a [`GridSize`]).
    #[must_use]
    pub fn new(size: impl Into<GridSize>) -> Self {
        let size = size.into();
        Self {
            shared: Arc::new(Mutex::new(SurfaceState {
                size,
                cells: vec![TerminalCell::EMPTY; size.area()],
                cursor_position: CellPosition::ORIGIN,
                cursor_visible: false,
                cell_size: None,
                pixel_size: UVec2::ZERO,
                revision: 0,
                dirty_cells: vec![false; size.area()],
            })),
        }
    }

    /// Returns the current grid size.
    #[must_use]
    pub fn size(&self) -> GridSize {
        self.lock().size
    }

    /// Returns an owned snapshot of the most recently published terminal state.
    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        let state = self.lock();
        state.snapshot()
    }

    /// Returns the current monotonically increasing change revision.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    /// Returns whether both handles refer to the same terminal surface.
    ///
    /// This is useful when associating a [`crate::Terminal`] query result with one of several
    /// surface handles owned by the application.
    #[must_use]
    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// Applies a batched update and returns whether a new revision was
    /// published.
    ///
    /// This is the recommended way to write to a surface: the lock is taken
    /// once, every change made through the [`SurfaceUpdate`] is published
    /// together as at most one new revision, and nothing is published if no
    /// cell, cursor or size actually changed.
    ///
    /// ```
    /// # use bevy_terminal::{TerminalSurface, TerminalCell};
    /// let surface = TerminalSurface::new((4, 1));
    /// let changed = surface.update(|update| {
    ///     update.set_cell((0, 0), &TerminalCell::new("A"));
    ///     update.set_cursor_position((1, 0));
    ///     update.set_cursor_visible(true);
    /// });
    /// assert!(changed);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut SurfaceUpdate<'_>)) -> bool {
        let mut update = self.begin_update();
        f(&mut update);
        update.commit()
    }

    /// Starts a batched update and returns its guard.
    ///
    /// The surface lock is held for the lifetime of the guard, so all changes
    /// made through it are published together as at most one new revision when
    /// the guard is committed or dropped. Nothing is published if no cell,
    /// cursor or size actually changed. Keep the guard short-lived: holding it
    /// across frames blocks the renderer. Prefer [`TerminalSurface::update`]
    /// unless a producer needs to interleave other work with the update.
    pub fn begin_update(&self) -> SurfaceUpdate<'_> {
        SurfaceUpdate {
            state: self.lock(),
            changed: false,
        }
    }

    /// Brings `snapshot` up to date by copying only the cells changed since it
    /// was last synchronized, and reports what changed.
    ///
    /// The renderer calls this once per frame whose revision differs from the
    /// snapshot's; the lock is held only while dirty cells are copied.
    pub(crate) fn update_snapshot(&self, snapshot: &mut TerminalSnapshot) -> SnapshotDelta {
        let mut state = self.lock();
        if snapshot.size != state.size {
            let changed_cells = state.cells.len();
            let changed_rows = (0..state.size.height).collect();
            *snapshot = state.snapshot();
            state.dirty_cells.fill(false);
            return SnapshotDelta {
                changed_rows,
                changed_cells,
                resized: true,
                cursor_position_changed: true,
                cursor_visibility_changed: true,
            };
        }

        let width = usize::from(state.size.width);
        let height = state.size.height;
        let mut changed_rows = Vec::new();
        let mut changed_cells = 0;
        let SurfaceState {
            cells, dirty_cells, ..
        } = &mut *state;
        for row in 0..height {
            let start = usize::from(row) * width;
            let end = start + width;
            let mut row_changed = false;
            for index in start..end {
                if dirty_cells[index] {
                    snapshot.cells[index].clone_from(&cells[index]);
                    dirty_cells[index] = false;
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
        SnapshotDelta {
            changed_rows,
            changed_cells,
            resized: false,
            cursor_position_changed,
            cursor_visibility_changed,
        }
    }

    /// Sets the logical-pixel dimensions of one cell.
    ///
    /// The renderer calls this from its render configuration so producers can
    /// report pixel metrics through [`TerminalSurface::metrics`].
    pub(crate) fn set_cell_size(&self, width: f32, height: f32) {
        let mut state = self.lock();
        let cell_size = Some(Vec2::new(width.max(0.0), height.max(0.0)));
        let pixel_size = surface_pixel_size(state.size, cell_size);
        if state.cell_size != cell_size || state.pixel_size != pixel_size {
            state.cell_size = cell_size;
            state.pixel_size = pixel_size;
            state.touch();
        }
    }

    /// Returns the grid size together with the renderer-derived pixel metrics.
    #[must_use]
    pub fn metrics(&self) -> SurfaceMetrics {
        let state = self.lock();
        SurfaceMetrics {
            size: state.size,
            cell_size: state.cell_size,
            pixel_size: state.pixel_size,
        }
    }

    fn lock(&self) -> MutexGuard<'_, SurfaceState> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Logical grid and pixel metrics of a surface.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceMetrics {
    /// Grid size in cells.
    pub size: GridSize,
    /// Logical pixel size of one cell, once a renderer has configured it.
    pub cell_size: Option<Vec2>,
    /// Logical pixel size of the whole surface, zero until a renderer has
    /// configured the cell size.
    pub pixel_size: UVec2,
}

/// The result of `TerminalSurface::update_snapshot`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SnapshotDelta {
    /// Rows containing at least one changed cell, in ascending order.
    pub(crate) changed_rows: Vec<u16>,
    /// Number of cells copied into the snapshot.
    pub(crate) changed_cells: usize,
    /// Whether the grid size changed; every cell was copied if so.
    pub(crate) resized: bool,
    /// Whether the cursor position changed.
    pub(crate) cursor_position_changed: bool,
    /// Whether the cursor visibility changed.
    pub(crate) cursor_visibility_changed: bool,
}

struct SurfaceState {
    size: GridSize,
    cells: Vec<TerminalCell>,
    cursor_position: CellPosition,
    cursor_visible: bool,
    cell_size: Option<Vec2>,
    pixel_size: UVec2,
    revision: u64,
    dirty_cells: Vec<bool>,
}

impl SurfaceState {
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            size: self.size,
            cells: self.cells.clone(),
            cursor_position: self.cursor_position,
            cursor_visible: self.cursor_visible,
            revision: self.revision,
        }
    }

    fn index(&self, x: u16, y: u16) -> usize {
        usize::from(y) * usize::from(self.size.width) + usize::from(x)
    }

    /// Writes `cell` at `index`, marking it dirty when it differs.
    fn write(&mut self, index: usize, cell: &TerminalCell) -> bool {
        if self.cells[index] == *cell {
            return false;
        }
        self.cells[index].clone_from(cell);
        self.dirty_cells[index] = true;
        true
    }

    fn reset(&mut self, index: usize) -> bool {
        self.write(index, &TerminalCell::EMPTY)
    }

    fn reset_range(&mut self, range: Range<usize>) -> bool {
        let mut changed = false;
        for index in range {
            changed |= self.reset(index);
        }
        changed
    }

    fn row_range(&self, row: u16) -> Range<usize> {
        let width = usize::from(self.size.width);
        let start = usize::from(row) * width;
        start..start + width
    }

    fn mark_rows_dirty(&mut self, rows: Range<u16>) {
        for row in rows.start.min(self.size.height)..rows.end.min(self.size.height) {
            let range = self.row_range(row);
            self.dirty_cells[range].fill(true);
        }
    }

    fn scroll_up(&mut self, region: Range<u16>, line_count: u16) -> bool {
        let start = region.start.min(self.size.height);
        let end = region.end.min(self.size.height).max(start);
        let count = line_count.min(end - start);
        if count == 0 {
            return false;
        }
        let width = usize::from(self.size.width);
        let first = usize::from(start) * width;
        let last = usize::from(end) * width;
        self.cells[first..last].rotate_left(usize::from(count) * width);
        for row in end - count..end {
            let range = self.row_range(row);
            self.cells[range].fill(TerminalCell::EMPTY);
        }
        self.mark_rows_dirty(start..end);
        true
    }

    fn scroll_down(&mut self, region: Range<u16>, line_count: u16) -> bool {
        let start = region.start.min(self.size.height);
        let end = region.end.min(self.size.height).max(start);
        let count = line_count.min(end - start);
        if count == 0 {
            return false;
        }
        let width = usize::from(self.size.width);
        let first = usize::from(start) * width;
        let last = usize::from(end) * width;
        self.cells[first..last].rotate_right(usize::from(count) * width);
        for row in start..start + count {
            let range = self.row_range(row);
            self.cells[range].fill(TerminalCell::EMPTY);
        }
        self.mark_rows_dirty(start..end);
        true
    }
}

fn surface_pixel_size(size: GridSize, cell_size: Option<Vec2>) -> UVec2 {
    let Some(cell) = cell_size else {
        return UVec2::ZERO;
    };
    UVec2::new(
        pixel_dimension(size.width, cell.x),
        pixel_dimension(size.height, cell.y),
    )
}

fn pixel_dimension(cells: u16, cell_size: f32) -> u32 {
    let value = f32::from(cells) * cell_size;
    if value.is_finite() {
        value.round().clamp(0.0, u32::MAX as f32) as u32
    } else if value.is_sign_positive() {
        u32::MAX
    } else {
        0
    }
}

/// A batched surface update holding the surface lock.
///
/// Every mutation returns whether it changed the retained state. Dropping or
/// [committing](SurfaceUpdate::commit) the guard publishes one new revision if
/// anything changed and none otherwise.
#[must_use = "an update publishes its changes when it is committed or dropped"]
pub struct SurfaceUpdate<'a> {
    state: MutexGuard<'a, SurfaceState>,
    changed: bool,
}

impl SurfaceUpdate<'_> {
    /// Returns the grid size.
    #[must_use]
    pub fn size(&self) -> GridSize {
        self.state.size
    }

    /// Returns whether `position` lies inside the grid.
    #[must_use]
    pub fn contains(&self, position: impl Into<CellPosition>) -> bool {
        self.state.size.contains(position.into())
    }

    /// Returns the retained cell at `position`, or `None` outside the grid.
    #[must_use]
    pub fn cell(&self, position: impl Into<CellPosition>) -> Option<&TerminalCell> {
        let position = position.into();
        self.contains(position)
            .then(|| &self.state.cells[self.state.index(position.x, position.y)])
    }

    /// Returns the cursor position.
    #[must_use]
    pub fn cursor_position(&self) -> CellPosition {
        self.state.cursor_position
    }

    /// Returns whether the cursor is visible.
    #[must_use]
    pub fn cursor_visible(&self) -> bool {
        self.state.cursor_visible
    }

    /// Returns whether this update has changed anything so far.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        self.changed
    }

    /// Writes `cell` at `position`; positions outside the grid are ignored.
    ///
    /// A [`CellOccupancy::Wide`] anchor also writes explicit continuation
    /// cells for the columns it spans, clipped to the row. Replacing a wide
    /// anchor with a narrower cell resets the continuation cells that no
    /// longer belong to an anchor, so stale continuations cannot survive.
    pub fn set_cell(&mut self, position: impl Into<CellPosition>, cell: &TerminalCell) -> bool {
        let CellPosition { x, y } = position.into();
        if !self.contains((x, y)) {
            return false;
        }
        let state = &mut *self.state;
        let index = state.index(x, y);
        let previous_columns = match state.cells[index].occupancy() {
            CellOccupancy::Wide { columns } => columns,
            CellOccupancy::Single | CellOccupancy::Continuation => 1,
        };
        let mut changed = state.write(index, cell);

        let width = state.size.width;
        let new_columns = cell.columns().max(1);
        let last_column = x.saturating_add(new_columns).min(width);
        if new_columns > 1 {
            let continuation = TerminalCell::continuation_of(cell);
            for column in x + 1..last_column {
                changed |= state.write(state.index(column, y), &continuation);
            }
        }
        // Continuations orphaned by a narrower replacement are reset.
        let previous_last = x.saturating_add(previous_columns).min(width);
        for column in last_column.max(x + 1)..previous_last {
            let orphan = state.index(column, y);
            if state.cells[orphan].is_continuation() {
                changed |= state.reset(orphan);
            }
        }
        self.changed |= changed;
        changed
    }

    /// Writes an iterator of `(x, y, cell)` triples through [`Self::set_cell`].
    pub fn set_cells<'c, I>(&mut self, cells: I) -> bool
    where
        I: IntoIterator<Item = (u16, u16, &'c TerminalCell)>,
    {
        let mut changed = false;
        for (x, y, cell) in cells {
            changed |= self.set_cell((x, y), cell);
        }
        changed
    }

    /// Moves the cursor. Positions outside the grid are retained as given and
    /// simply hide the cursor while out of range.
    pub fn set_cursor_position(&mut self, position: impl Into<CellPosition>) -> bool {
        let position = position.into();
        if self.state.cursor_position == position {
            return false;
        }
        self.state.cursor_position = position;
        self.changed = true;
        true
    }

    /// Shows or hides the cursor.
    pub fn set_cursor_visible(&mut self, visible: bool) -> bool {
        if self.state.cursor_visible == visible {
            return false;
        }
        self.state.cursor_visible = visible;
        self.changed = true;
        true
    }

    /// Resets every cell to [`TerminalCell::EMPTY`].
    pub fn clear(&mut self) -> bool {
        let range = 0..self.state.cells.len();
        self.reset_cells(range)
    }

    /// Resets one row.
    pub fn clear_row(&mut self, row: u16) -> bool {
        if row >= self.state.size.height {
            return false;
        }
        let range = self.state.row_range(row);
        self.reset_cells(range)
    }

    /// Resets the cells from `start` through `end` (both inclusive) in
    /// row-major order. Positions are clamped into the grid; nothing happens
    /// when `end` precedes `start`.
    pub fn clear_range(
        &mut self,
        start: impl Into<CellPosition>,
        end: impl Into<CellPosition>,
    ) -> bool {
        let (Some(start), Some(end)) = (
            self.clamped_index(start.into()),
            self.clamped_index(end.into()),
        ) else {
            return false;
        };
        if end < start {
            return false;
        }
        self.reset_cells(start..end + 1)
    }

    /// Resizes the grid, preserving the overlapping cells and clamping the
    /// cursor into the new bounds. Every cell is marked dirty.
    pub fn resize(&mut self, size: impl Into<GridSize>) -> bool {
        let state = &mut *self.state;
        let new_size = size.into();
        let GridSize {
            width: columns,
            height: rows,
        } = new_size;
        if state.size == new_size {
            return false;
        }
        let old_size = state.size;
        let old_cells =
            std::mem::replace(&mut state.cells, vec![TerminalCell::EMPTY; new_size.area()]);
        let copied_columns = usize::from(columns.min(old_size.width));
        for y in 0..rows.min(old_size.height) {
            let old_start = usize::from(y) * usize::from(old_size.width);
            let new_start = usize::from(y) * usize::from(columns);
            state.cells[new_start..new_start + copied_columns]
                .clone_from_slice(&old_cells[old_start..old_start + copied_columns]);
        }
        state.size = new_size;
        state.pixel_size = surface_pixel_size(new_size, state.cell_size);
        state.dirty_cells = vec![true; new_size.area()];
        state.cursor_position.x = state.cursor_position.x.min(columns.saturating_sub(1));
        state.cursor_position.y = state.cursor_position.y.min(rows.saturating_sub(1));
        self.changed = true;
        true
    }

    /// Scrolls the rows in `region` up by `line_count`, clearing the rows that
    /// enter at the bottom.
    pub fn scroll_up(&mut self, region: Range<u16>, line_count: u16) -> bool {
        let changed = self.state.scroll_up(region, line_count);
        self.changed |= changed;
        changed
    }

    /// Scrolls the rows in `region` down by `line_count`, clearing the rows
    /// that enter at the top.
    pub fn scroll_down(&mut self, region: Range<u16>, line_count: u16) -> bool {
        let changed = self.state.scroll_down(region, line_count);
        self.changed |= changed;
        changed
    }

    /// Publishes the update, returning whether a new revision was created.
    ///
    /// Dropping the guard publishes too; [`TerminalSurface::update`] returns
    /// this same value for its closure.
    pub fn commit(mut self) -> bool {
        self.finish()
    }

    fn finish(&mut self) -> bool {
        if self.changed {
            self.state.touch();
            self.changed = false;
            true
        } else {
            false
        }
    }

    fn clamped_index(&self, position: CellPosition) -> Option<usize> {
        let size = self.state.size;
        if size.width == 0 || size.height == 0 {
            return None;
        }
        Some(self.state.index(
            position.x.min(size.width - 1),
            position.y.min(size.height - 1),
        ))
    }

    fn reset_cells(&mut self, range: Range<usize>) -> bool {
        let changed = self.state.reset_range(range);
        self.changed |= changed;
        changed
    }
}

impl Drop for SurfaceUpdate<'_> {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{StyleFlags, TerminalColor, TerminalStyle};

    fn fill(surface: &TerminalSurface, text: &str, width: u16) {
        let mut update = surface.begin_update();
        for (index, symbol) in text.chars().enumerate() {
            let cell = TerminalCell::new(&symbol.to_string());
            update.set_cell(((index as u16) % width, (index as u16) / width), &cell);
        }
        update.commit();
    }

    #[test]
    fn transactions_publish_at_most_one_revision_and_none_when_unchanged() {
        let surface = TerminalSurface::new((3, 2));
        let initial = surface.revision();
        {
            let mut update = surface.begin_update();
            update.set_cell((0, 0), &TerminalCell::EMPTY);
            update.set_cursor_position((0, 0));
            update.set_cursor_visible(false);
            update.clear();
            update.resize((3, 2));
            assert!(!update.has_changes());
        }
        assert_eq!(surface.revision(), initial);

        let mut update = surface.begin_update();
        assert!(update.set_cell((0, 0), &TerminalCell::new("A")));
        assert!(update.set_cell((1, 0), &TerminalCell::new("B")));
        assert!(update.set_cursor_position((1, 1)));
        assert!(update.set_cursor_visible(true));
        assert!(update.commit());
        assert_eq!(surface.revision(), initial + 1);

        let mut update = surface.begin_update();
        assert!(!update.set_cell((0, 0), &TerminalCell::new("A")));
        assert!(!update.commit());
        assert_eq!(surface.revision(), initial + 1);
    }

    #[test]
    fn partial_updates_preserve_other_cells_and_clip_invalid_coordinates() {
        let surface = TerminalSurface::new((3, 2));
        let cell = TerminalCell::new("X").with_style(
            TerminalStyle::new()
                .fg(TerminalColor::RED)
                .with(StyleFlags::BOLD),
        );
        let mut update = surface.begin_update();
        update.set_cell((1, 0), &cell);
        assert!(!update.set_cell((99, 99), &cell));
        drop(update);

        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(1, 0)], cell);
        assert_eq!(snapshot[(0, 0)], TerminalCell::EMPTY);
    }

    #[test]
    fn incremental_snapshots_copy_only_changed_cells_and_report_rows() {
        let surface = TerminalSurface::new((4, 3));
        let mut snapshot = surface.snapshot();
        let changed = TerminalCell::new("X");
        surface.begin_update().set_cell((2, 1), &changed);

        let delta = surface.update_snapshot(&mut snapshot);
        assert_eq!(delta.changed_cells, 1);
        assert_eq!(delta.changed_rows, [1]);
        assert!(!delta.resized);
        assert_eq!(snapshot[(2, 1)], changed);
        assert_eq!(snapshot.revision(), surface.revision());

        surface.begin_update().set_cursor_position((3, 2));
        let cursor_delta = surface.update_snapshot(&mut snapshot);
        assert_eq!(cursor_delta.changed_cells, 0);
        assert!(cursor_delta.changed_rows.is_empty());
        assert!(cursor_delta.cursor_position_changed);
        assert!(!cursor_delta.cursor_visibility_changed);

        surface.begin_update().resize((2, 2));
        let resized = surface.update_snapshot(&mut snapshot);
        assert!(resized.resized);
        assert_eq!(resized.changed_cells, 4);
        assert_eq!(resized.changed_rows, [0, 1]);
        assert_eq!(snapshot.size(), GridSize::new(2, 2));
    }

    #[test]
    fn wide_anchors_write_continuations_and_narrow_replacements_clear_them() {
        let surface = TerminalSurface::new((4, 1));
        let wide =
            TerminalCell::wide("界", 2).with_style(TerminalStyle::new().bg(TerminalColor::BLUE));
        surface.begin_update().set_cell((1, 0), &wide);
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(1, 0)].symbol(), "界");
        assert_eq!(snapshot[(1, 0)].columns(), 2);
        assert!(snapshot[(2, 0)].is_continuation());
        assert_eq!(snapshot[(2, 0)].style, wide.style);
        assert_eq!(snapshot[(3, 0)], TerminalCell::EMPTY);

        surface
            .begin_update()
            .set_cell((1, 0), &TerminalCell::new("a"));
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(1, 0)].symbol(), "a");
        assert_eq!(snapshot[(2, 0)], TerminalCell::EMPTY);

        // A wide glyph at the last column is clipped to the row.
        surface.begin_update().set_cell((3, 0), &wide);
        assert_eq!(surface.snapshot()[(3, 0)].columns(), 2);
    }

    #[test]
    fn clear_range_clamps_and_rejects_reversed_ranges() {
        let surface = TerminalSurface::new((4, 2));
        fill(&surface, "ABCDEFGH", 4);
        assert!(!surface.begin_update().clear_range((2, 0), (1, 0)));
        assert!(surface.begin_update().clear_range((3, 1), (99, 99)));
        assert_eq!(surface.snapshot()[(3, 1)], TerminalCell::EMPTY);
        assert_eq!(surface.snapshot()[(2, 1)].symbol(), "G");
        assert!(surface.update(|update| {
            update.clear_range((0, 0), (0, 0));
        }));
        assert_eq!(surface.snapshot()[(0, 0)], TerminalCell::EMPTY);
        assert!(!surface.update(|update| {
            update.clear_range((0, 0), (0, 0));
        }));
    }

    #[test]
    fn clear_operations_follow_row_major_semantics() {
        let surface = TerminalSurface::new((4, 2));
        fill(&surface, "ABCDEFGH", 4);
        surface.begin_update().clear_range((1, 0), (3, 0));
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "A");
        assert_eq!(snapshot[(1, 0)], TerminalCell::EMPTY);
        assert_eq!(snapshot[(3, 0)], TerminalCell::EMPTY);
        assert_eq!(snapshot[(0, 1)].symbol(), "E");

        fill(&surface, "ABCDEFGH", 4);
        surface
            .begin_update()
            .clear_range((1, 0), (u16::MAX, u16::MAX));
        assert_eq!(surface.snapshot()[(0, 0)].symbol(), "A");
        assert_eq!(surface.snapshot()[(0, 1)], TerminalCell::EMPTY);

        fill(&surface, "ABCDEFGH", 4);
        surface.begin_update().clear_range((0, 0), (1, 1));
        assert_eq!(surface.snapshot()[(1, 1)], TerminalCell::EMPTY);
        assert_eq!(surface.snapshot()[(2, 1)].symbol(), "G");

        surface.begin_update().clear_row(1);
        assert_eq!(surface.snapshot()[(2, 1)], TerminalCell::EMPTY);
        assert!(!surface.begin_update().clear_row(5));
        assert!(!surface.begin_update().clear());
        fill(&surface, "ABCDEFGH", 4);
        assert!(surface.begin_update().clear());
        assert_eq!(surface.snapshot().cells(), vec![TerminalCell::EMPTY; 8]);
    }

    #[test]
    fn scroll_regions_move_and_clear_rows() {
        let surface = TerminalSurface::new((2, 3));
        fill(&surface, "AABBCC", 2);
        surface.begin_update().scroll_up(0..3, 1);
        let up = surface.snapshot();
        assert_eq!(up[(0, 0)].symbol(), "B");
        assert_eq!(up[(0, 1)].symbol(), "C");
        assert_eq!(up[(0, 2)], TerminalCell::EMPTY);

        surface.begin_update().scroll_down(0..3, 1);
        let down = surface.snapshot();
        assert_eq!(down[(0, 0)], TerminalCell::EMPTY);
        assert_eq!(down[(0, 1)].symbol(), "B");
        assert_eq!(down[(0, 2)].symbol(), "C");

        assert!(!surface.begin_update().scroll_up(0..3, 0));
        assert!(surface.begin_update().scroll_up(1..3, 5));
        assert_eq!(surface.snapshot()[(0, 1)], TerminalCell::EMPTY);
    }

    #[test]
    fn resize_preserves_the_two_dimensional_overlap_and_reports_metrics() {
        let surface = TerminalSurface::new((4, 3));
        fill(&surface, "AAAABBBBCCCC", 4);
        surface.begin_update().set_cursor_position((3, 2));
        surface.begin_update().resize((2, 3));
        let snapshot = surface.snapshot();
        assert_eq!(snapshot.size(), GridSize::new(2, 3));
        assert_eq!(snapshot[(0, 0)].symbol(), "A");
        assert_eq!(snapshot[(0, 1)].symbol(), "B");
        assert_eq!(snapshot[(0, 2)].symbol(), "C");
        assert_eq!(snapshot.cursor_position(), CellPosition::new(1, 2));

        surface.set_cell_size(10.8, 20.0);
        let metrics = surface.metrics();
        assert_eq!(metrics.size, GridSize::new(2, 3));
        assert_eq!(metrics.pixel_size, UVec2::new(22, 60));
        assert_eq!(metrics.cell_size, Some(Vec2::new(10.8, 20.0)));
    }

    #[test]
    fn cloned_surface_handles_have_stable_identity() {
        let first = TerminalSurface::new((2, 1));
        let first_clone = first.clone();
        let second = TerminalSurface::new((2, 1));
        assert!(first.shares_state_with(&first_clone));
        assert!(!first.shares_state_with(&second));
    }
}
