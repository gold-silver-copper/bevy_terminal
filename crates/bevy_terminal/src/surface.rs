//! The thread-safe retained surface producers write into and renderers read
//! from. Every write goes through [`TerminalSurface::update`], which takes the
//! lock once and publishes at most one new revision per call.

use std::{
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use crate::scene::{CellOccupancy, CellPosition, GridSize, TerminalCell, TerminalSnapshot};

/// A coherent observation of surface metadata, independent of its renderers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceInfo {
    /// Current dimensions in cells.
    pub size: GridSize,
    /// Cursor position, which may lie outside the grid.
    pub cursor_position: CellPosition,
    /// Whether the producer requests a visible cursor.
    pub cursor_visible: bool,
    /// Revision of the observed state.
    pub revision: u64,
    /// Changes on every actual grid resize, including a resize back to an earlier size.
    pub resize_generation: u64,
}

/// A cheap, thread-safe handle to a retained terminal surface.
///
/// Producers write through [`TerminalSurface::update`]. The renderer
/// plugin reads incremental snapshots from the same handle in a Bevy system, so
/// producing a frame never requires access to the Bevy world.
#[derive(Clone)]
pub struct TerminalSurface {
    shared: Arc<Mutex<SurfaceState>>,
}

/// Measurement identity must not keep retained terminal content alive.
#[derive(Clone, Debug)]
pub(crate) struct WeakSurface(Weak<Mutex<SurfaceState>>);

impl PartialEq for WeakSurface {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl WeakSurface {
    pub(crate) fn matches(&self, surface: &TerminalSurface) -> bool {
        self.0.ptr_eq(&Arc::downgrade(&surface.shared))
    }

    pub(crate) fn is_current(&self, generation: u64) -> bool {
        self.0
            .upgrade()
            .is_some_and(|shared| TerminalSurface { shared }.info().resize_generation == generation)
    }
}

impl TerminalSurface {
    pub(crate) fn downgrade(&self) -> WeakSurface {
        WeakSurface(Arc::downgrade(&self.shared))
    }

    /// Maximum retained cells (about 48 MiB of cell storage per surface).
    /// This bounds producer-controlled allocation independently of GPU limits.
    pub const MAX_CELLS: usize = 1 << 20;

    /// Creates an empty terminal surface with the given size in cells
    /// (`(columns, rows)` or a [`GridSize`]).
    ///
    /// # Panics
    /// Panics if the grid exceeds [`Self::MAX_CELLS`].
    #[must_use]
    pub fn new(size: impl Into<GridSize>) -> Self {
        let size = size.into();
        assert!(
            size.area() <= Self::MAX_CELLS,
            "terminal surface exceeds MAX_CELLS"
        );
        Self {
            shared: Arc::new(Mutex::new(SurfaceState {
                size,
                cells: vec![TerminalCell::EMPTY; size.area()],
                cursor_position: CellPosition::new(0, 0),
                cursor_visible: false,
                revision: 0,
                resize_generation: 0,
                row_revisions: vec![0; usize::from(size.height)],
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

    /// Returns the change revision, incremented once per changed update.
    /// The counter wraps at `u64::MAX`; compare revisions for equality rather
    /// than ordering. Incremental readers support crossing that boundary.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.lock().revision
    }

    /// Returns whether both handles refer to the same terminal surface.
    ///
    /// This is useful when associating a [`crate::render::TerminalRenderer`] query result with one of several
    /// surface handles owned by the application.
    #[must_use]
    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// Applies a batched update and returns whether a new revision was
    /// published.
    ///
    /// This is the only way to write to a surface: the lock is taken once,
    /// every change made through the [`SurfaceUpdate`] is published together
    /// as at most one new revision, and nothing is published if no cell,
    /// cursor or size actually changed. If the closure unwinds, completed
    /// writes are retained and their revision is published before unlocking.
    /// A batch that changes a value and restores it still publishes a revision.
    ///
    /// ```
    /// # use bevy_terminal::prelude::{TerminalSurface, TerminalCell};
    /// let surface = TerminalSurface::new((4, 1));
    /// let changed = surface.update(|update| {
    ///     update.set_cell((0, 0), &TerminalCell::new("A"));
    ///     update.set_cursor_position((1, 0));
    ///     update.set_cursor_visible(true);
    /// });
    /// assert!(changed);
    /// ```
    pub fn update(&self, f: impl FnOnce(&mut SurfaceUpdate<'_>)) -> bool {
        let mut update = SurfaceUpdate {
            state: self.lock(),
            changed: false,
        };
        f(&mut update);
        update.finish()
    }

    /// Brings `snapshot` up to date by copying only the cells changed since it
    /// was last synchronized, and reports what changed.
    ///
    /// The renderer calls this once per frame whose revision differs from the
    /// snapshot's; the lock is held only while dirty cells are copied.
    pub(crate) fn update_snapshot(&self, snapshot: &mut TerminalSnapshot) -> SnapshotDelta {
        let state = self.lock();
        if snapshot.size != state.size || snapshot.resize_generation != state.resize_generation {
            let changed_cells = state.cells.len();
            let changed_rows = (0..state.size.height).collect();
            *snapshot = state.snapshot();
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
        for row in 0..height {
            // Compare ages rather than raw revisions so wrapping the counter is
            // harmless. Revisions belong to each reader; synchronizing never
            // consumes another reader's changes.
            let age = state
                .revision
                .wrapping_sub(state.row_revisions[usize::from(row)]);
            if age >= state.revision.wrapping_sub(snapshot.revision) {
                continue;
            }
            let start = usize::from(row) * width;
            let end = start + width;
            let mut row_changed = false;
            for (retained, current) in snapshot.cells[start..end]
                .iter_mut()
                .zip(&state.cells[start..end])
            {
                if retained != current {
                    retained.clone_from(current);
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

    /// Reads grid, cursor and revision together without copying cell content.
    #[must_use]
    pub fn info(&self) -> SurfaceInfo {
        let state = self.lock();
        SurfaceInfo {
            size: state.size,
            cursor_position: state.cursor_position,
            cursor_visible: state.cursor_visible,
            revision: state.revision,
            resize_generation: state.resize_generation,
        }
    }

    fn lock(&self) -> MutexGuard<'_, SurfaceState> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
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
    revision: u64,
    resize_generation: u64,
    // Eight bytes per row, rather than a flag or revision for every cell.
    // Readers compare cells only in rows changed since their own snapshot.
    row_revisions: Vec<u64>,
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
            resize_generation: self.resize_generation,
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
        self.row_revisions[index / usize::from(self.size.width)] = self.revision.wrapping_add(1);
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

    fn scroll(&mut self, region: Range<u16>, line_count: u16, down: bool) -> bool {
        let start = region.start.min(self.size.height);
        let end = region.end.min(self.size.height).max(start);
        let count = line_count.min(end - start);
        let width = usize::from(self.size.width);
        if count == 0 || width == 0 {
            return false;
        }
        let first = usize::from(start) * width;
        let last = usize::from(end) * width;
        let offset = usize::from(count) * width;
        let mut changed = false;
        // Move in the direction that preserves the source, retaining allocation
        // and marking only rows whose cells actually changed.
        for step in 0..last - first {
            let index = if down { last - 1 - step } else { first + step };
            let source = if down {
                index.checked_sub(offset).filter(|source| *source >= first)
            } else {
                index.checked_add(offset).filter(|source| *source < last)
            };
            let Some(source) = source else {
                changed |= self.reset(index);
                continue;
            };
            if self.cells[index] == self.cells[source] {
                continue;
            }
            // Source and destination cannot overlap: count is nonzero. Borrow
            // them separately to avoid allocating a temporary heap-backed symbol.
            if source < index {
                let (before, after) = self.cells.split_at_mut(index);
                after[0].clone_from(&before[source]);
            } else {
                let (before, after) = self.cells.split_at_mut(source);
                before[index].clone_from(&after[0]);
            }
            self.row_revisions[index / width] = self.revision.wrapping_add(1);
            changed = true;
        }
        changed
    }
}

/// The batched update passed to [`TerminalSurface::update`].
///
/// Every mutation returns whether it changed the retained state; the update
/// publishes one new revision when the closure returns if anything changed and
/// none otherwise. Positions outside the grid are ignored.
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

    /// Writes `cell` at `position`; positions outside the grid are ignored.
    ///
    /// A [`CellOccupancy::Wide`] anchor also writes explicit continuation
    /// cells for the columns it spans, clipped to the row. Replacing a wide
    /// anchor with a narrower cell resets the continuation cells that no
    /// longer belong to an anchor, so stale continuations cannot survive.
    pub fn set_cell(&mut self, position: impl Into<CellPosition>, cell: &TerminalCell) -> bool {
        let CellPosition { x, y } = position.into();
        if !self.state.size.contains((x, y)) {
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
    /// cursor into the new bounds. Every row is invalidated.
    ///
    /// # Panics
    /// Panics before modifying the grid if it exceeds [`TerminalSurface::MAX_CELLS`].
    pub fn resize(&mut self, size: impl Into<GridSize>) -> bool {
        let state = &mut *self.state;
        let new_size = size.into();
        assert!(
            new_size.area() <= TerminalSurface::MAX_CELLS,
            "terminal surface exceeds MAX_CELLS"
        );
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
        state.resize_generation = state.resize_generation.wrapping_add(1);
        state.row_revisions = vec![state.revision.wrapping_add(1); usize::from(rows)];
        state.cursor_position.x = state.cursor_position.x.min(columns.saturating_sub(1));
        state.cursor_position.y = state.cursor_position.y.min(rows.saturating_sub(1));
        self.changed = true;
        true
    }

    /// Scrolls the rows in `region` up by `line_count`, clearing the rows that
    /// enter at the bottom.
    pub fn scroll_up(&mut self, region: Range<u16>, line_count: u16) -> bool {
        let changed = self.state.scroll(region, line_count, false);
        self.changed |= changed;
        changed
    }

    /// Scrolls the rows in `region` down by `line_count`, clearing the rows
    /// that enter at the top.
    pub fn scroll_down(&mut self, region: Range<u16>, line_count: u16) -> bool {
        let changed = self.state.scroll(region, line_count, true);
        self.changed |= changed;
        changed
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
        // A producer panic leaves its completed writes in place. Publish those
        // writes before releasing the lock, just as on a normal return.
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{StyleFlags, TerminalColor, TerminalStyle};

    #[test]
    fn independent_readers_observe_updates_at_different_rates() {
        let surface = TerminalSurface::new((4, 2));
        let mut fast = surface.snapshot();
        let mut slow = surface.snapshot();
        surface.update(|u| {
            u.set_cell((1, 0), &TerminalCell::new("A"));
        });
        surface.update_snapshot(&mut fast);
        surface.update(|u| {
            u.set_cell((2, 1), &TerminalCell::new("B"));
        });
        surface.update_snapshot(&mut fast);
        let delta = surface.update_snapshot(&mut slow);
        assert_eq!(delta.changed_rows, [0, 1]);
        assert_eq!(slow.to_text(), fast.to_text());
        assert_eq!(slow.revision(), fast.revision());
        assert!(surface.update_snapshot(&mut slow).changed_rows.is_empty());
    }

    #[test]
    fn unwinding_publishes_partial_changes_and_recovers_the_lock() {
        let surface = TerminalSurface::new((2, 1));
        let mut snapshot = surface.snapshot();
        let result = std::panic::catch_unwind(|| {
            surface.update(|u| {
                u.set_cell((0, 0), &TerminalCell::new("A"));
                panic!("producer failure");
            })
        });
        assert!(result.is_err());
        assert_ne!(surface.revision(), snapshot.revision());
        surface.update_snapshot(&mut snapshot);
        assert_eq!(snapshot.row_text(0), "A ");
        assert!(surface.update(|u| {
            u.set_cell((1, 0), &TerminalCell::new("B"));
        }));
        assert_eq!(surface.snapshot().row_text(0), "AB");
    }

    #[test]
    fn resize_generation_tracks_resizes_inside_a_single_update() {
        let surface = TerminalSurface::new((2, 1));
        let initial = surface.info();
        surface.update(|u| {
            u.set_cell((0, 0), &TerminalCell::new("A"));
            u.resize((2, 1));
        });
        assert_eq!(surface.info().resize_generation, initial.resize_generation);
        surface.update(|u| {
            u.resize((3, 1));
            u.resize((2, 1));
        });
        assert_eq!(surface.info().size, initial.size);
        assert_eq!(
            surface.info().resize_generation,
            initial.resize_generation + 2
        );
    }

    #[test]
    fn scrolling_heap_symbols_preserves_overlap_in_both_directions() {
        let surface = TerminalSurface::new((1, 3));
        let first = TerminalCell::new(
            "a\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
        );
        let second = TerminalCell::new(
            "b\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
        );
        surface.update(|u| {
            u.set_cell((0, 0), &first);
            u.set_cell((0, 1), &second);
            u.scroll_down(0..3, 1);
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(0, 1)], first);
        assert_eq!(snapshot[(0, 2)], second);
        surface.update(|u| {
            u.scroll_up(0..3, 1);
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(0, 0)], first);
        assert_eq!(snapshot[(0, 1)], second);
        assert_eq!(snapshot[(0, 2)], TerminalCell::EMPTY);
    }

    #[test]
    fn oversized_resize_panics_before_changing_the_surface() {
        let surface = TerminalSurface::new((2, 1));
        let before = surface.info();
        assert!(
            std::panic::catch_unwind(|| {
                surface.update(|update| {
                    update.resize((u16::MAX, u16::MAX));
                });
            })
            .is_err()
        );
        assert_eq!(surface.info(), before);
        assert_eq!(surface.snapshot().to_text(), "  ");
        assert!(std::panic::catch_unwind(|| TerminalSurface::new((u16::MAX, u16::MAX))).is_err());
    }

    #[test]
    fn snapshot_readers_cross_revision_wrap_independently() {
        let surface = TerminalSurface::new((1, 2));
        {
            let mut state = surface.lock();
            state.revision = u64::MAX - 1;
            state.row_revisions.fill(u64::MAX - 1);
        }
        let mut fast = surface.snapshot();
        let mut slow = fast.clone();
        surface.update(|update| {
            update.set_cell((0, 0), &TerminalCell::new("A"));
        });
        assert_eq!(surface.update_snapshot(&mut fast).changed_rows, [0]);
        surface.update(|update| {
            update.set_cell((0, 1), &TerminalCell::new("B"));
        });
        assert_eq!(surface.revision(), 0);
        assert_eq!(surface.update_snapshot(&mut fast).changed_rows, [1]);
        assert_eq!(surface.update_snapshot(&mut slow).changed_rows, [0, 1]);
        assert_eq!(fast.to_text(), slow.to_text());
    }

    #[test]
    fn scrolling_unchanged_content_does_not_publish() {
        let surface = TerminalSurface::new((4, 3));
        assert!(!surface.update(|u| {
            u.scroll_up(0..3, 1);
        }));
        assert!(!surface.update(|u| {
            u.scroll_down(0..3, 2);
        }));
        let empty = TerminalSurface::new((0, 3));
        assert!(!empty.update(|u| {
            u.scroll_up(0..3, 1);
        }));
    }

    fn fill(surface: &TerminalSurface, text: &str, width: u16) {
        surface.update(|update| {
            for (index, symbol) in text.chars().enumerate() {
                let cell = TerminalCell::new(&symbol.to_string());
                update.set_cell(((index as u16) % width, (index as u16) / width), &cell);
            }
        });
    }

    #[test]
    fn updates_publish_at_most_one_revision_and_none_when_unchanged() {
        let surface = TerminalSurface::new((3, 2));
        let initial = surface.revision();
        let published = surface.update(|update| {
            update.set_cell((0, 0), &TerminalCell::EMPTY);
            update.set_cursor_position((0, 0));
            update.set_cursor_visible(false);
            update.clear();
            update.resize((3, 2));
        });
        assert!(!published);
        assert_eq!(surface.revision(), initial);

        let published = surface.update(|update| {
            assert!(update.set_cell((0, 0), &TerminalCell::new("A")));
            assert!(update.set_cell((1, 0), &TerminalCell::new("B")));
            assert!(update.set_cursor_position((1, 1)));
            assert!(update.set_cursor_visible(true));
        });
        assert!(published);
        assert_eq!(surface.revision(), initial + 1);

        let published = surface.update(|update| {
            assert!(!update.set_cell((0, 0), &TerminalCell::new("A")));
        });
        assert!(!published);
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
        surface.update(|update| {
            update.set_cell((1, 0), &cell);
            assert!(!update.set_cell((99, 99), &cell));
        });

        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(1, 0)], cell);
        assert_eq!(snapshot[(0, 0)], TerminalCell::EMPTY);
    }

    #[test]
    fn incremental_snapshots_copy_only_changed_cells_and_report_rows() {
        let surface = TerminalSurface::new((4, 3));
        let mut snapshot = surface.snapshot();
        let changed = TerminalCell::new("X");
        surface.update(|u| {
            u.set_cell((2, 1), &changed);
        });

        let delta = surface.update_snapshot(&mut snapshot);
        assert_eq!(delta.changed_cells, 1);
        assert_eq!(delta.changed_rows, [1]);
        assert!(!delta.resized);
        assert_eq!(snapshot[(2, 1)], changed);
        assert_eq!(snapshot.revision(), surface.revision());

        surface.update(|u| {
            u.set_cursor_position((3, 2));
        });
        let cursor_delta = surface.update_snapshot(&mut snapshot);
        assert_eq!(cursor_delta.changed_cells, 0);
        assert!(cursor_delta.changed_rows.is_empty());
        assert!(cursor_delta.cursor_position_changed);
        assert!(!cursor_delta.cursor_visibility_changed);

        surface.update(|u| {
            u.resize((2, 2));
        });
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
        surface.update(|u| {
            u.set_cell((1, 0), &wide);
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(1, 0)].symbol(), "界");
        assert_eq!(snapshot[(1, 0)].columns(), 2);
        assert!(snapshot[(2, 0)].is_continuation());
        assert_eq!(snapshot[(2, 0)].style, wide.style);
        assert_eq!(snapshot[(3, 0)], TerminalCell::EMPTY);

        surface.update(|u| {
            u.set_cell((1, 0), &TerminalCell::new("a"));
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(1, 0)].symbol(), "a");
        assert_eq!(snapshot[(2, 0)], TerminalCell::EMPTY);

        // A wide glyph at the last column is clipped to the row.
        surface.update(|u| {
            u.set_cell((3, 0), &wide);
        });
        assert_eq!(surface.snapshot()[(3, 0)].columns(), 2);
    }

    #[test]
    fn clear_range_clamps_and_rejects_reversed_ranges() {
        let surface = TerminalSurface::new((4, 2));
        fill(&surface, "ABCDEFGH", 4);
        assert!(!surface.update(|u| {
            u.clear_range((2, 0), (1, 0));
        }));
        assert!(surface.update(|u| {
            u.clear_range((3, 1), (99, 99));
        }));
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
        surface.update(|u| {
            u.clear_range((1, 0), (3, 0));
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[(0, 0)].symbol(), "A");
        assert_eq!(snapshot[(1, 0)], TerminalCell::EMPTY);
        assert_eq!(snapshot[(3, 0)], TerminalCell::EMPTY);
        assert_eq!(snapshot[(0, 1)].symbol(), "E");

        fill(&surface, "ABCDEFGH", 4);
        surface.update(|u| {
            u.clear_range((1, 0), (u16::MAX, u16::MAX));
        });
        assert_eq!(surface.snapshot()[(0, 0)].symbol(), "A");
        assert_eq!(surface.snapshot()[(0, 1)], TerminalCell::EMPTY);

        fill(&surface, "ABCDEFGH", 4);
        surface.update(|u| {
            u.clear_range((0, 0), (1, 1));
        });
        assert_eq!(surface.snapshot()[(1, 1)], TerminalCell::EMPTY);
        assert_eq!(surface.snapshot()[(2, 1)].symbol(), "G");

        surface.update(|u| {
            u.clear_row(1);
        });
        assert_eq!(surface.snapshot()[(2, 1)], TerminalCell::EMPTY);
        assert!(!surface.update(|u| {
            u.clear_row(5);
        }));
        assert!(!surface.update(|u| {
            u.clear();
        }));
        fill(&surface, "ABCDEFGH", 4);
        assert!(surface.update(|u| {
            u.clear();
        }));
        assert_eq!(surface.snapshot().cells(), vec![TerminalCell::EMPTY; 8]);
    }

    #[test]
    fn scroll_regions_move_and_clear_rows() {
        let surface = TerminalSurface::new((2, 3));
        fill(&surface, "AABBCC", 2);
        surface.update(|u| {
            u.scroll_up(0..3, 1);
        });
        let up = surface.snapshot();
        assert_eq!(up[(0, 0)].symbol(), "B");
        assert_eq!(up[(0, 1)].symbol(), "C");
        assert_eq!(up[(0, 2)], TerminalCell::EMPTY);

        surface.update(|u| {
            u.scroll_down(0..3, 1);
        });
        let down = surface.snapshot();
        assert_eq!(down[(0, 0)], TerminalCell::EMPTY);
        assert_eq!(down[(0, 1)].symbol(), "B");
        assert_eq!(down[(0, 2)].symbol(), "C");

        assert!(!surface.update(|u| {
            u.scroll_up(0..3, 0);
        }));
        assert!(surface.update(|u| {
            u.scroll_up(1..3, 5);
        }));
        assert_eq!(surface.snapshot()[(0, 1)], TerminalCell::EMPTY);
    }

    #[test]
    fn resize_preserves_the_two_dimensional_overlap() {
        let surface = TerminalSurface::new((4, 3));
        fill(&surface, "AAAABBBBCCCC", 4);
        surface.update(|u| {
            u.set_cursor_position((3, 2));
        });
        surface.update(|u| {
            u.resize((2, 3));
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot.size(), GridSize::new(2, 3));
        assert_eq!(snapshot[(0, 0)].symbol(), "A");
        assert_eq!(snapshot[(0, 1)].symbol(), "B");
        assert_eq!(snapshot[(0, 2)].symbol(), "C");
        assert_eq!(snapshot.cursor_position(), CellPosition::new(1, 2));

        let info = surface.info();
        assert_eq!(info.size, snapshot.size());
        assert_eq!(info.cursor_position, snapshot.cursor_position());
        assert_eq!(info.revision, snapshot.revision());
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
