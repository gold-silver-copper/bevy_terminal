//! The thread-safe retained surface producers write into and renderers read
//! from. Every write goes through [`TerminalSurface::update`], which takes the
//! lock once and publishes at most one new revision per call.

use std::{
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use crate::scene::{CellPosition, GridSize, TerminalCell, TerminalSnapshot};

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
    /// This bounds producer-controlled allocation independently of GPU limits:
    /// larger grids keep their width and are clamped to fewer rows.
    pub const MAX_CELLS: usize = 1 << 20;

    /// Creates an empty terminal surface with the given size in cells
    /// (`(columns, rows)` or a [`GridSize`]), clamped to [`Self::MAX_CELLS`].
    #[must_use]
    pub fn new(size: impl Into<GridSize>) -> Self {
        let size = size.into().bounded();
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

    /// Returns an owned snapshot of the most recently published terminal state.
    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        let state = self.lock();
        state.snapshot()
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
    /// The surface stays locked while `f` runs, so calling a method of this
    /// surface (through any handle) inside `f` deadlocks. Read the size and
    /// cursor through the [`SurfaceUpdate`] instead.
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
        // Dropping the update publishes the revision.
        update.changed
    }

    /// Brings `snapshot` up to date by copying only the cells changed since it
    /// was last synchronized, and reports what changed; `changed_rows` is
    /// replaced by the rows containing a changed cell, in ascending order
    /// (a buffer the caller reuses).
    ///
    /// The renderer calls this once per frame whose revision differs from the
    /// snapshot's; the lock is held only while dirty cells are copied.
    pub(crate) fn update_snapshot(
        &self,
        snapshot: &mut TerminalSnapshot,
        changed_rows: &mut Vec<u16>,
    ) -> SnapshotDelta {
        let state = self.lock();
        changed_rows.clear();
        if snapshot.size != state.size || snapshot.resize_generation != state.resize_generation {
            let changed_cells = state.cells.len();
            changed_rows.extend(0..state.size.height);
            *snapshot = state.snapshot();
            return SnapshotDelta {
                changed_cells,
                resized: true,
                cursor_position_changed: true,
                cursor_visibility_changed: true,
            };
        }

        // Compare ages rather than raw revisions so wrapping the counter is
        // harmless. Revisions belong to each reader; synchronizing never
        // consumes another reader's changes.
        let snapshot_age = state.revision.wrapping_sub(snapshot.revision);
        let width = usize::from(state.size.width).max(1);
        let rows = (snapshot.cells.chunks_mut(width))
            .zip(state.cells.chunks(width))
            .zip(&state.row_revisions);
        let mut changed_cells = 0;
        for (((retained, current), revision), row) in rows.zip(0..) {
            if state.revision.wrapping_sub(*revision) >= snapshot_age {
                continue;
            }
            let mut row_changed = false;
            for (retained, current) in retained.iter_mut().zip(current) {
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
            changed_cells,
            resized: false,
            cursor_position_changed,
            cursor_visibility_changed,
        }
    }

    /// Reads grid, cursor and revision together without copying cell content.
    ///
    /// The revision is incremented once per changed update. It wraps at
    /// `u64::MAX`; compare revisions for equality rather than ordering.
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

    /// Writes `cell` at `position`, marking its row dirty when anything changed.
    ///
    /// Every cell write goes through here, and it keeps wide glyphs whole as
    /// real terminals do: overwriting any column of a wide glyph resets the
    /// rest of it, and a wide anchor writes its continuation cells, with its
    /// span clipped to the row. Continuation cells only come from their
    /// anchor, so writing one is ignored, as are positions outside the grid.
    fn write(&mut self, position: CellPosition, cell: &TerminalCell) -> bool {
        let x = usize::from(position.x);
        let next_revision = self.revision.wrapping_add(1);
        let (Some(row), Some(row_revision)) = (
            (self.cells.chunks_mut(usize::from(self.size.width).max(1))).nth(position.y.into()),
            self.row_revisions.get_mut(usize::from(position.y)),
        ) else {
            return false;
        };
        let Some(head) = row.get(..=x).filter(|_| !cell.is_continuation()) else {
            return false;
        };
        // The overwritten columns extend left to the anchor of a glyph whose
        // continuation is at `x`, and right over the continuations of a glyph
        // anchored inside the new one.
        let start = head
            .iter()
            .rposition(|cell| !cell.is_continuation())
            .unwrap_or(0);
        let span = cell.occupancy().columns().min(self.size.width - position.x);
        let covered = x + usize::from(span);
        let trailing = row
            .iter()
            .skip(covered)
            .take_while(|cell| cell.is_continuation());
        let end = covered + trailing.count();

        let clipped;
        let anchor = if span < cell.occupancy().columns() {
            clipped = cell.clipped(span);
            &clipped
        } else {
            cell
        };
        let continuation = TerminalCell::continuation_of(cell);
        let mut changed = false;
        for (column, slot) in row.iter_mut().enumerate().take(end).skip(start) {
            let target = match column {
                column if column == x => anchor,
                column if column > x && column < covered => &continuation,
                _ => &TerminalCell::EMPTY,
            };
            if slot != target {
                slot.clone_from(target);
                changed = true;
            }
        }
        if changed {
            *row_revision = next_revision;
        }
        changed
    }

    fn scroll(&mut self, region: Range<u16>, line_count: u16, down: bool) -> bool {
        let width = usize::from(self.size.width);
        let start = usize::from(region.start.min(self.size.height));
        let end = usize::from(region.end.min(self.size.height)).max(start);
        let shift = usize::from(line_count).min(end - start) * width;
        let next_revision = self.revision.wrapping_add(1);
        let (Some(cells), Some(revisions)) = (
            self.cells.get_mut(start * width..end * width),
            self.row_revisions.get_mut(start..end),
        ) else {
            return false;
        };
        if shift == 0 {
            return false;
        }
        // Mark only the rows whose content actually changes.
        let mut changed = false;
        for (row, revision) in (0..cells.len()).step_by(width).zip(revisions) {
            let source = if down {
                row.checked_sub(shift)
            } else {
                Some(row + shift)
            };
            let destination = cells.get(row..row + width).unwrap_or_default();
            let differs = match source.and_then(|source| cells.get(source..source + width)) {
                Some(source) => destination != source,
                None => destination.iter().any(|cell| *cell != TerminalCell::EMPTY),
            };
            if differs {
                *revision = next_revision;
                changed = true;
            }
        }
        // Rotating moves cells without cloning them; the rows that wrapped
        // around are then cleared.
        let entering = if down {
            cells.rotate_right(shift);
            0..shift
        } else {
            cells.rotate_left(shift);
            cells.len() - shift..cells.len()
        };
        if let Some(entering) = cells.get_mut(entering) {
            entering.fill(TerminalCell::EMPTY);
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
    /// Wide glyphs stay whole, as in a real terminal: a
    /// [`Wide`](crate::scene::CellOccupancy::Wide) anchor also writes continuation cells for the
    /// columns it spans, clipped to the row, and overwriting any column of an
    /// existing wide glyph resets the rest of it. Continuation cells (as read
    /// from a snapshot) are written by their anchor, so passing one is a no-op.
    pub fn set_cell(&mut self, position: impl Into<CellPosition>, cell: &TerminalCell) -> bool {
        let changed = self.state.write(position.into(), cell);
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
        self.clear_range((0, 0), (u16::MAX, u16::MAX))
    }

    /// Resets one row.
    pub fn clear_row(&mut self, row: u16) -> bool {
        row < self.state.size.height && self.clear_range((0, row), (u16::MAX, row))
    }

    /// Resets the cells from `start` through `end` (both inclusive) in
    /// row-major order, together with the rest of any wide glyph cut by
    /// either end. Positions are clamped into the grid; nothing happens when
    /// `end` precedes `start`.
    pub fn clear_range(
        &mut self,
        start: impl Into<CellPosition>,
        end: impl Into<CellPosition>,
    ) -> bool {
        let GridSize { width, height } = self.state.size;
        if width == 0 || height == 0 {
            return false;
        }
        let index = |position: CellPosition| {
            usize::from(position.y.min(height - 1)) * usize::from(width)
                + usize::from(position.x.min(width - 1))
        };
        let width = usize::from(width);
        let mut changed = false;
        for index in index(start.into())..=index(end.into()) {
            let position = CellPosition::new((index % width) as u16, (index / width) as u16);
            changed |= self.state.write(position, &TerminalCell::EMPTY);
        }
        self.changed |= changed;
        changed
    }

    /// Resizes the grid, clamped to [`TerminalSurface::MAX_CELLS`], keeping
    /// the overlapping cells (a wide glyph cut by the new right edge is
    /// clipped) and clamping the cursor into the new bounds. Every row is
    /// invalidated.
    pub fn resize(&mut self, size: impl Into<GridSize>) -> bool {
        let state = &mut *self.state;
        let size = size.into().bounded();
        if state.size == size {
            return false;
        }
        let old_width = usize::from(state.size.width).max(1);
        let old_cells = std::mem::replace(&mut state.cells, vec![TerminalCell::EMPTY; size.area()]);
        state.size = size;
        state.row_revisions = vec![0; usize::from(size.height)];
        // Writing the old anchors regenerates their continuations.
        for (y, row) in (0..size.height).zip(old_cells.chunks(old_width)) {
            for (x, cell) in (0..size.width).zip(row) {
                state.write(CellPosition::new(x, y), cell);
            }
        }
        state.row_revisions.fill(state.revision.wrapping_add(1));
        state.resize_generation = state.resize_generation.wrapping_add(1);
        state.cursor_position.x = state.cursor_position.x.min(size.width.saturating_sub(1));
        state.cursor_position.y = state.cursor_position.y.min(size.height.saturating_sub(1));
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
}

impl Drop for SurfaceUpdate<'_> {
    fn drop(&mut self) {
        // Publishes the batch, also when a producer panic unwinds through
        // `update`, so completed writes are never left unpublished.
        if self.changed {
            self.state.revision = self.state.revision.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CellOccupancy, StyleFlags, TerminalColor, TerminalStyle};

    /// The changed rows and delta of one synchronization.
    fn sync(
        surface: &TerminalSurface,
        snapshot: &mut TerminalSnapshot,
    ) -> (Vec<u16>, SnapshotDelta) {
        let mut rows = Vec::new();
        let delta = surface.update_snapshot(snapshot, &mut rows);
        (rows, delta)
    }

    #[test]
    fn independent_readers_observe_updates_at_different_rates() {
        let surface = TerminalSurface::new((4, 2));
        let mut fast = surface.snapshot();
        let mut slow = surface.snapshot();
        surface.update(|u| {
            u.set_cell((1, 0), &TerminalCell::new("A"));
        });
        sync(&surface, &mut fast);
        surface.update(|u| {
            u.set_cell((2, 1), &TerminalCell::new("B"));
        });
        sync(&surface, &mut fast);
        let (delta_rows, _) = sync(&surface, &mut slow);
        assert_eq!(delta_rows, [0, 1]);
        assert_eq!(slow.to_string(), fast.to_string());
        assert_eq!(slow.revision(), fast.revision());
        assert!(sync(&surface, &mut slow).0.is_empty());
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
        assert_ne!(surface.info().revision, snapshot.revision());
        sync(&surface, &mut snapshot);
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
        assert_eq!(snapshot.cell((0, 1)).unwrap(), &first);
        assert_eq!(snapshot.cell((0, 2)).unwrap(), &second);
        surface.update(|u| {
            u.scroll_up(0..3, 1);
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot.cell((0, 0)).unwrap(), &first);
        assert_eq!(snapshot.cell((0, 1)).unwrap(), &second);
        assert_eq!(snapshot.cell((0, 2)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn oversized_grids_are_clamped_to_max_cells() {
        let surface = TerminalSurface::new((u16::MAX, u16::MAX));
        assert_eq!(surface.info().size, GridSize::new(u16::MAX, 16));
        assert!(surface.update(|update| {
            update.resize((2000, 1000));
        }));
        assert_eq!(surface.info().size, GridSize::new(2000, 524));
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
        assert_eq!(sync(&surface, &mut fast).0, [0]);
        surface.update(|update| {
            update.set_cell((0, 1), &TerminalCell::new("B"));
        });
        assert_eq!(surface.info().revision, 0);
        assert_eq!(sync(&surface, &mut fast).0, [1]);
        assert_eq!(sync(&surface, &mut slow).0, [0, 1]);
        assert_eq!(fast.to_string(), slow.to_string());
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
        let initial = surface.info().revision;
        let published = surface.update(|update| {
            update.set_cell((0, 0), &TerminalCell::EMPTY);
            update.set_cursor_position((0, 0));
            update.set_cursor_visible(false);
            update.clear();
            update.resize((3, 2));
        });
        assert!(!published);
        assert_eq!(surface.info().revision, initial);

        let published = surface.update(|update| {
            assert!(update.set_cell((0, 0), &TerminalCell::new("A")));
            assert!(update.set_cell((1, 0), &TerminalCell::new("B")));
            assert!(update.set_cursor_position((1, 1)));
            assert!(update.set_cursor_visible(true));
        });
        assert!(published);
        assert_eq!(surface.info().revision, initial + 1);

        let published = surface.update(|update| {
            assert!(!update.set_cell((0, 0), &TerminalCell::new("A")));
        });
        assert!(!published);
        assert_eq!(surface.info().revision, initial + 1);
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
        assert_eq!(snapshot.cell((1, 0)).unwrap(), &cell);
        assert_eq!(snapshot.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn incremental_snapshots_copy_only_changed_cells_and_report_rows() {
        let surface = TerminalSurface::new((4, 3));
        let mut snapshot = surface.snapshot();
        let changed = TerminalCell::new("X");
        surface.update(|u| {
            u.set_cell((2, 1), &changed);
        });

        let (delta_rows, delta) = sync(&surface, &mut snapshot);
        assert_eq!(delta.changed_cells, 1);
        assert_eq!(delta_rows, [1]);
        assert!(!delta.resized);
        assert_eq!(snapshot.cell((2, 1)).unwrap(), &changed);
        assert_eq!(snapshot.revision(), surface.info().revision);

        surface.update(|u| {
            u.set_cursor_position((3, 2));
        });
        let (cursor_delta_rows, cursor_delta) = sync(&surface, &mut snapshot);
        assert_eq!(cursor_delta.changed_cells, 0);
        assert!(cursor_delta_rows.is_empty());
        assert!(cursor_delta.cursor_position_changed);
        assert!(!cursor_delta.cursor_visibility_changed);

        surface.update(|u| {
            u.resize((2, 2));
        });
        let (resized_rows, resized) = sync(&surface, &mut snapshot);
        assert!(resized.resized);
        assert_eq!(resized.changed_cells, 4);
        assert_eq!(resized_rows, [0, 1]);
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
        assert_eq!(snapshot.cell((1, 0)).unwrap().symbol(), "界");
        assert_eq!(snapshot.cell((1, 0)).unwrap().occupancy().columns(), 2);
        assert!(snapshot.cell((2, 0)).unwrap().is_continuation());
        assert_eq!(snapshot.cell((2, 0)).unwrap().style, wide.style);
        assert_eq!(snapshot.cell((3, 0)).unwrap(), &TerminalCell::EMPTY);

        surface.update(|u| {
            u.set_cell((1, 0), &TerminalCell::new("a"));
        });
        let snapshot = surface.snapshot();
        assert_eq!(snapshot.cell((1, 0)).unwrap().symbol(), "a");
        assert_eq!(snapshot.cell((2, 0)).unwrap(), &TerminalCell::EMPTY);

        // A wide glyph at the last column is clipped to the row.
        surface.update(|u| {
            u.set_cell((3, 0), &wide);
        });
        assert_eq!(
            surface.snapshot().cell((3, 0)).unwrap().occupancy(),
            CellOccupancy::Single
        );
    }

    #[test]
    fn overwriting_any_part_of_a_wide_glyph_resets_the_rest_of_it() {
        let surface = TerminalSurface::new((4, 1));
        let wide =
            TerminalCell::wide("界", 2).with_style(TerminalStyle::new().bg(TerminalColor::RED));
        let write = |x, cell: &TerminalCell| {
            surface.update(|u| {
                u.set_cell((x, 0), cell);
            });
            surface.snapshot()
        };
        // Clearing the anchor leaves no colored continuation behind.
        write(1, &wide);
        surface.update(|u| {
            u.clear_range((1, 0), (1, 0));
        });
        assert_eq!(surface.snapshot().cells(), [TerminalCell::EMPTY; 4]);
        // Writing over the continuation resets the anchor.
        write(1, &wide);
        assert_eq!(write(2, &TerminalCell::new("x")).row_text(0), "  x ");
        // A wide glyph overlapping another's continuation resets its anchor.
        write(0, &wide);
        let snapshot = write(1, &wide);
        assert_eq!(snapshot.row_text(0), " 界 ");
        assert_eq!(snapshot.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        // Continuation cells come only from their anchor.
        let continuation = snapshot.cell((2, 0)).unwrap().clone();
        assert!(!surface.update(|u| {
            u.set_cell((0, 0), &continuation);
        }));
        // Shrinking clips a wide glyph cut by the new edge.
        surface.update(|u| {
            u.resize((2, 1));
        });
        assert_eq!(
            surface.snapshot().cell((1, 0)).unwrap().occupancy(),
            CellOccupancy::Single
        );
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
        assert_eq!(
            surface.snapshot().cell((3, 1)).unwrap(),
            &TerminalCell::EMPTY
        );
        assert_eq!(surface.snapshot().cell((2, 1)).unwrap().symbol(), "G");
        assert!(surface.update(|update| {
            update.clear_range((0, 0), (0, 0));
        }));
        assert_eq!(
            surface.snapshot().cell((0, 0)).unwrap(),
            &TerminalCell::EMPTY
        );
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
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "A");
        assert_eq!(snapshot.cell((1, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(snapshot.cell((3, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(snapshot.cell((0, 1)).unwrap().symbol(), "E");

        fill(&surface, "ABCDEFGH", 4);
        surface.update(|u| {
            u.clear_range((1, 0), (u16::MAX, u16::MAX));
        });
        assert_eq!(surface.snapshot().cell((0, 0)).unwrap().symbol(), "A");
        assert_eq!(
            surface.snapshot().cell((0, 1)).unwrap(),
            &TerminalCell::EMPTY
        );

        fill(&surface, "ABCDEFGH", 4);
        surface.update(|u| {
            u.clear_range((0, 0), (1, 1));
        });
        assert_eq!(
            surface.snapshot().cell((1, 1)).unwrap(),
            &TerminalCell::EMPTY
        );
        assert_eq!(surface.snapshot().cell((2, 1)).unwrap().symbol(), "G");

        surface.update(|u| {
            u.clear_row(1);
        });
        assert_eq!(
            surface.snapshot().cell((2, 1)).unwrap(),
            &TerminalCell::EMPTY
        );
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
        assert_eq!(up.cell((0, 0)).unwrap().symbol(), "B");
        assert_eq!(up.cell((0, 1)).unwrap().symbol(), "C");
        assert_eq!(up.cell((0, 2)).unwrap(), &TerminalCell::EMPTY);

        surface.update(|u| {
            u.scroll_down(0..3, 1);
        });
        let down = surface.snapshot();
        assert_eq!(down.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(down.cell((0, 1)).unwrap().symbol(), "B");
        assert_eq!(down.cell((0, 2)).unwrap().symbol(), "C");

        assert!(!surface.update(|u| {
            u.scroll_up(0..3, 0);
        }));
        assert!(surface.update(|u| {
            u.scroll_up(1..3, 5);
        }));
        assert_eq!(
            surface.snapshot().cell((0, 1)).unwrap(),
            &TerminalCell::EMPTY
        );
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
        assert_eq!(snapshot.cell((0, 0)).unwrap().symbol(), "A");
        assert_eq!(snapshot.cell((0, 1)).unwrap().symbol(), "B");
        assert_eq!(snapshot.cell((0, 2)).unwrap().symbol(), "C");
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
