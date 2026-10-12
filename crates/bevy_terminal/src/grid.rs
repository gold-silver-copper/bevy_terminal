//! The terminal grid: cells, cursor and per-row change tracking, as a Bevy
//! component. Producers write through `&mut` access (queries, or a component
//! that owns a grid); the renderer reads it in [`TerminalSystems::Sync`].
//!
//! [`TerminalSystems::Sync`]: crate::render::TerminalSystems::Sync

use std::{
    fmt,
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

use bevy::prelude::Component;

use crate::render::TerminalRenderConfig;
use crate::scene::{CellPosition, GridSize, TerminalCell};

/// A grid of terminal cells and a cursor.
///
/// Spawned as a component, it is drawn into the entity's
/// [`TerminalTexture`](crate::render::TerminalTexture) after adding
/// [`TerminalPlugin`](crate::render::TerminalPlugin). A component that owns a
/// grid, such as a Ratatui terminal, is drawn through
/// [`TerminalSource`](crate::render::TerminalSource) instead.
///
/// Every write keeps wide glyphs whole, as real terminals do: a
/// [`Wide`](crate::scene::CellOccupancy::Wide) anchor writes the continuation
/// cells it covers (clipped to the row), and overwriting any column of a wide
/// glyph resets the rest of it. Only the grid creates continuation cells.
/// Positions outside the grid are ignored, and the cursor is clamped into it.
///
/// ```
/// # use bevy_terminal::prelude::{TerminalCell, TerminalGrid};
/// let mut grid = TerminalGrid::new((4, 1));
/// grid.set_cell((0, 0), &TerminalCell::new("A"));
/// grid.set_cell((1, 0), &TerminalCell::wide("界", 2));
/// assert_eq!(grid.row_text(0), "A界 ");
/// ```
#[derive(Debug, Component)]
#[require(TerminalRenderConfig)]
pub struct TerminalGrid {
    /// Unique to this grid, and new for each clone, so a reader can tell a
    /// replaced grid from an edited one: revisions only order one grid's
    /// changes.
    identity: u64,
    size: GridSize,
    cells: Vec<TerminalCell>,
    cursor_position: CellPosition,
    cursor_visible: bool,
    /// Counts changes; it wraps at `u64::MAX`, so revisions are compared by
    /// age, never by order.
    revision: u64,
    /// The revision of each row's last change: eight bytes per row, so a
    /// reader compares only the rows changed since it last drew.
    row_revisions: Vec<u64>,
}

impl TerminalGrid {
    /// Maximum cells in a grid (about 48 MiB of cell storage). This bounds
    /// producer-controlled allocation independently of GPU limits: larger
    /// grids keep their width and are clamped to fewer rows.
    pub const MAX_CELLS: usize = 1 << 20;

    /// Creates an empty grid with the given size in cells (`(columns, rows)`
    /// or a [`GridSize`]), clamped to [`Self::MAX_CELLS`].
    #[must_use]
    pub fn new(size: impl Into<GridSize>) -> Self {
        let size = size.into().bounded();
        Self {
            identity: next_identity(),
            size,
            cells: vec![TerminalCell::EMPTY; size.area()],
            cursor_position: CellPosition::new(0, 0),
            cursor_visible: false,
            revision: 0,
            row_revisions: vec![0; usize::from(size.height)],
        }
    }

    /// Returns the grid size.
    #[must_use]
    pub const fn size(&self) -> GridSize {
        self.size
    }

    /// Returns the cursor position, which is inside the grid unless the grid
    /// is empty.
    #[must_use]
    pub const fn cursor_position(&self) -> CellPosition {
        self.cursor_position
    }

    /// Returns whether the producer requests a visible cursor.
    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Returns all cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[TerminalCell] {
        &self.cells
    }

    /// Returns the cells of one row, or an empty slice for an out-of-range row.
    #[must_use]
    pub fn row(&self, row: u16) -> &[TerminalCell] {
        let width = usize::from(self.size.width);
        let start = usize::from(row) * width;
        self.cells.get(start..start + width).unwrap_or_default()
    }

    /// Returns the cell at a position, or `None` outside the grid.
    #[must_use]
    pub fn cell(&self, position: impl Into<CellPosition>) -> Option<&TerminalCell> {
        let position = position.into();
        self.row(position.y).get(usize::from(position.x))
    }

    /// Iterates over every cell with its grid position, row-major.
    ///
    /// Together with the public `style` field this supports content and style
    /// assertions in tests:
    ///
    /// ```
    /// # use bevy_terminal::prelude::{TerminalColor, TerminalGrid};
    /// # let grid = TerminalGrid::new((4, 1));
    /// assert!(grid.iter().all(|(_, cell)| {
    ///     cell.style.foreground == TerminalColor::Default
    /// }));
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = (CellPosition, &TerminalCell)> + '_ {
        let width = usize::from(self.size.width.max(1));
        self.cells.iter().enumerate().map(move |(index, cell)| {
            let position = CellPosition::new(
                u16::try_from(index % width).unwrap_or(u16::MAX),
                u16::try_from(index / width).unwrap_or(u16::MAX),
            );
            (position, cell)
        })
    }

    /// Returns one row as plain text: the symbols in order, wide glyphs once
    /// (continuation cells are skipped) and styles dropped. Trailing spaces
    /// are kept, so every row of single-column symbols is as long as the grid
    /// is wide. An out-of-range row is empty.
    ///
    /// The [`fmt::Display`] implementation joins every row with `'\n'`, the
    /// lossy "what is on screen" view for quick assertions:
    ///
    /// ```
    /// # use bevy_terminal::prelude::{TerminalCell, TerminalGrid};
    /// let mut grid = TerminalGrid::new((8, 2));
    /// for (x, ch) in "Loading".chars().enumerate() {
    ///     grid.set_cell((x as u16, 1), &TerminalCell::from(ch));
    /// }
    /// assert!(grid.to_string().contains("Loading"));
    /// assert_eq!(grid.row_text(1), "Loading ");
    /// ```
    #[must_use]
    pub fn row_text(&self, row: u16) -> String {
        self.row_symbols(row).collect()
    }

    fn row_symbols(&self, row: u16) -> impl Iterator<Item = &str> {
        self.row(row)
            .iter()
            .filter(|cell| !cell.is_continuation())
            .map(TerminalCell::symbol)
    }

    /// Writes `cell` at `position` and returns whether anything changed;
    /// positions outside the grid are ignored.
    ///
    /// A [`Wide`](crate::scene::CellOccupancy::Wide) anchor also writes
    /// continuation cells for the columns it spans, clipped to the row, and
    /// overwriting any column of an existing wide glyph resets the rest of it.
    /// Continuation cells (as read from a grid) are written by their anchor,
    /// so passing one is a no-op.
    pub fn set_cell(&mut self, position: impl Into<CellPosition>, cell: &TerminalCell) -> bool {
        let position = position.into();
        let changed = self.write(position, cell);
        if changed {
            self.touch_row(position.y);
        }
        changed
    }

    /// Moves the cursor, clamped into the grid, and returns whether it moved.
    pub fn set_cursor_position(&mut self, position: impl Into<CellPosition>) -> bool {
        let position = self.clamp(position.into());
        if self.cursor_position == position {
            return false;
        }
        self.cursor_position = position;
        self.touch();
        true
    }

    /// Shows or hides the cursor and returns whether that changed anything.
    pub fn set_cursor_visible(&mut self, visible: bool) -> bool {
        if self.cursor_visible == visible {
            return false;
        }
        self.cursor_visible = visible;
        self.touch();
        true
    }

    /// Resets every cell.
    pub fn clear(&mut self) -> bool {
        self.clear_range((0, 0), (u16::MAX, u16::MAX))
    }

    /// Resets one row.
    pub fn clear_row(&mut self, row: u16) -> bool {
        row < self.size.height && self.clear_range((0, row), (u16::MAX, row))
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
        let width = usize::from(self.size.width);
        if width == 0 || self.size.height == 0 {
            return false;
        }
        let index = |position: CellPosition| {
            let position = self.clamp(position);
            usize::from(position.y) * width + usize::from(position.x)
        };
        let mut changed = false;
        for index in index(start.into())..=index(end.into()) {
            let position = CellPosition::new((index % width) as u16, (index / width) as u16);
            if self.write(position, &TerminalCell::EMPTY) {
                self.touch_row(position.y);
                changed = true;
            }
        }
        changed
    }

    /// Resizes the grid, clamped to [`Self::MAX_CELLS`], keeping the
    /// overlapping cells (a wide glyph cut by the new right edge is clipped)
    /// and clamping the cursor into the new bounds. Every row is marked
    /// changed.
    pub fn resize(&mut self, size: impl Into<GridSize>) -> bool {
        let size = size.into().bounded();
        if self.size == size {
            return false;
        }
        let old_width = usize::from(self.size.width).max(1);
        let old_cells = std::mem::replace(&mut self.cells, vec![TerminalCell::EMPTY; size.area()]);
        self.size = size;
        // Writing the old anchors regenerates their continuations.
        for (y, row) in (0..size.height).zip(old_cells.chunks(old_width)) {
            for (x, cell) in (0..size.width).zip(row) {
                self.write(CellPosition::new(x, y), cell);
            }
        }
        self.cursor_position = self.clamp(self.cursor_position);
        self.touch();
        self.row_revisions = vec![self.revision; usize::from(size.height)];
        true
    }

    /// Scrolls the rows in `region` up by `line_count`, clearing the rows that
    /// enter at the bottom.
    pub fn scroll_up(&mut self, region: Range<u16>, line_count: u16) -> bool {
        self.scroll(region, line_count, false)
    }

    /// Scrolls the rows in `region` down by `line_count`, clearing the rows
    /// that enter at the top.
    pub fn scroll_down(&mut self, region: Range<u16>, line_count: u16) -> bool {
        self.scroll(region, line_count, true)
    }

    /// The change revision, for readers comparing it with the one they last
    /// drew.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// Identifies this grid among all others, clones included.
    pub(crate) const fn identity(&self) -> u64 {
        self.identity
    }

    /// The rows changed since `revision`, in ascending order.
    pub(crate) fn rows_changed_since(&self, revision: u64) -> impl Iterator<Item = u16> + '_ {
        // Ages rather than raw revisions, so wrapping the counter is harmless.
        let age = self.revision.wrapping_sub(revision);
        self.row_revisions
            .iter()
            .zip(0..)
            .filter(move |(row, _)| self.revision.wrapping_sub(**row) < age)
            .map(|(_, row)| row)
    }

    /// `position` clamped into the grid; the origin for an empty grid.
    fn clamp(&self, position: CellPosition) -> CellPosition {
        CellPosition::new(
            position.x.min(self.size.width.saturating_sub(1)),
            position.y.min(self.size.height.saturating_sub(1)),
        )
    }

    /// Records a change that is not tied to a row (the cursor).
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Records a change to `row`.
    fn touch_row(&mut self, row: u16) {
        self.touch();
        if let Some(revision) = self.row_revisions.get_mut(usize::from(row)) {
            *revision = self.revision;
        }
    }

    /// Writes `cell` at `position` without recording the change; the one
    /// place cells are written.
    ///
    /// The overwritten columns extend left to the anchor of a glyph whose
    /// continuation is at `position`, and right over the continuations of a
    /// glyph anchored inside the new one. Continuation cells only come from
    /// their anchor, so writing one is ignored, as are positions outside the
    /// grid.
    fn write(&mut self, position: CellPosition, cell: &TerminalCell) -> bool {
        let x = usize::from(position.x);
        let Some(row) =
            (self.cells.chunks_mut(usize::from(self.size.width).max(1))).nth(position.y.into())
        else {
            return false;
        };
        let Some(head) = row.get(..=x).filter(|_| !cell.is_continuation()) else {
            return false;
        };
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
        if changed {
            self.revision = next_revision;
        }
        changed
    }
}

fn next_identity() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Clone for TerminalGrid {
    fn clone(&self) -> Self {
        Self {
            identity: next_identity(),
            size: self.size,
            cells: self.cells.clone(),
            cursor_position: self.cursor_position,
            cursor_visible: self.cursor_visible,
            revision: self.revision,
            row_revisions: self.row_revisions.clone(),
        }
    }
}

impl fmt::Display for TerminalGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..self.size.height {
            if row > 0 {
                f.write_str("\n")?;
            }
            self.row_symbols(row)
                .try_for_each(|symbol| f.write_str(symbol))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CellOccupancy, StyleFlags, TerminalColor, TerminalStyle};

    fn fill(grid: &mut TerminalGrid, text: &str, width: u16) {
        for (index, symbol) in text.chars().enumerate() {
            let cell = TerminalCell::new(&symbol.to_string());
            grid.set_cell(((index as u16) % width, (index as u16) / width), &cell);
        }
    }

    /// The rows changed since `revision`, and the grid's revision now.
    fn changed_since(grid: &TerminalGrid, revision: u64) -> (Vec<u16>, u64) {
        (grid.rows_changed_since(revision).collect(), grid.revision())
    }

    #[test]
    fn readers_see_the_rows_changed_since_their_revision() {
        let mut grid = TerminalGrid::new((4, 3));
        let start = grid.revision();
        assert!(changed_since(&grid, start).0.is_empty());
        grid.set_cell((2, 1), &TerminalCell::new("X"));
        let (rows, fast) = changed_since(&grid, start);
        assert_eq!(rows, [1]);
        grid.set_cell((0, 2), &TerminalCell::new("Y"));
        assert_eq!(changed_since(&grid, fast).0, [2]);
        assert_eq!(changed_since(&grid, start).0, [1, 2]);
        // The cursor changes the revision but no row.
        let now = grid.revision();
        grid.set_cursor_position((3, 2));
        assert_ne!(grid.revision(), now);
        assert!(changed_since(&grid, now).0.is_empty());
        // A resize changes every row.
        let now = grid.revision();
        grid.resize((2, 2));
        assert_eq!(changed_since(&grid, now).0, [0, 1]);
    }

    #[test]
    fn row_changes_are_tracked_across_revision_wrap() {
        let mut grid = TerminalGrid::new((1, 2));
        grid.revision = u64::MAX - 1;
        grid.row_revisions.fill(u64::MAX - 1);
        let start = grid.revision();
        grid.set_cell((0, 0), &TerminalCell::new("A"));
        let (rows, fast) = changed_since(&grid, start);
        assert_eq!(rows, [0]);
        grid.set_cell((0, 1), &TerminalCell::new("B"));
        assert_eq!(grid.revision(), 0);
        assert_eq!(changed_since(&grid, fast).0, [1]);
        assert_eq!(changed_since(&grid, start).0, [0, 1]);
    }

    #[test]
    fn writes_that_change_nothing_leave_the_revision_alone() {
        let mut grid = TerminalGrid::new((3, 2));
        let initial = grid.revision();
        assert!(!grid.set_cell((0, 0), &TerminalCell::EMPTY));
        assert!(!grid.set_cursor_position((0, 0)));
        assert!(!grid.set_cursor_visible(false));
        assert!(!grid.clear());
        assert!(!grid.resize((3, 2)));
        assert!(!grid.scroll_up(0..2, 1));
        assert!(!grid.scroll_down(0..2, 2));
        assert!(!TerminalGrid::new((0, 3)).scroll_up(0..3, 1));
        assert_eq!(grid.revision(), initial);
        assert!(grid.set_cell((0, 0), &TerminalCell::new("A")));
        assert!(!grid.set_cell((0, 0), &TerminalCell::new("A")));
    }

    #[test]
    fn the_cursor_is_clamped_into_the_grid() {
        let mut grid = TerminalGrid::new((4, 2));
        grid.set_cursor_position((9, 9));
        assert_eq!(grid.cursor_position(), CellPosition::new(3, 1));
        grid.resize((2, 1));
        assert_eq!(grid.cursor_position(), CellPosition::new(1, 0));
        let mut empty = TerminalGrid::new((0, 0));
        assert!(!empty.set_cursor_position((5, 5)));
        assert_eq!(empty.cursor_position(), CellPosition::new(0, 0));
    }

    #[test]
    fn oversized_grids_are_clamped_to_max_cells() {
        let mut grid = TerminalGrid::new((u16::MAX, u16::MAX));
        assert_eq!(grid.size(), GridSize::new(u16::MAX, 16));
        assert!(grid.resize((2000, 1000)));
        assert_eq!(grid.size(), GridSize::new(2000, 524));
    }

    #[test]
    fn partial_updates_preserve_other_cells_and_clip_invalid_coordinates() {
        let mut grid = TerminalGrid::new((3, 2));
        let cell = TerminalCell::new("X").with_style(
            TerminalStyle::new()
                .fg(TerminalColor::RED)
                .with(StyleFlags::BOLD),
        );
        grid.set_cell((1, 0), &cell);
        assert!(!grid.set_cell((99, 99), &cell));
        assert_eq!(grid.cell((1, 0)).unwrap(), &cell);
        assert_eq!(grid.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn wide_anchors_write_continuations_and_narrow_replacements_clear_them() {
        let mut grid = TerminalGrid::new((4, 1));
        let wide =
            TerminalCell::wide("界", 2).with_style(TerminalStyle::new().bg(TerminalColor::BLUE));
        grid.set_cell((1, 0), &wide);
        assert_eq!(grid.cell((1, 0)).unwrap().symbol(), "界");
        assert_eq!(grid.cell((1, 0)).unwrap().occupancy().columns(), 2);
        assert!(grid.cell((2, 0)).unwrap().is_continuation());
        assert_eq!(grid.cell((2, 0)).unwrap().style, wide.style);
        assert_eq!(grid.cell((3, 0)).unwrap(), &TerminalCell::EMPTY);

        grid.set_cell((1, 0), &TerminalCell::new("a"));
        assert_eq!(grid.cell((1, 0)).unwrap().symbol(), "a");
        assert_eq!(grid.cell((2, 0)).unwrap(), &TerminalCell::EMPTY);

        // A wide glyph at the last column is clipped to the row.
        grid.set_cell((3, 0), &wide);
        assert_eq!(
            grid.cell((3, 0)).unwrap().occupancy(),
            CellOccupancy::Single
        );
    }

    #[test]
    fn overwriting_any_part_of_a_wide_glyph_resets_the_rest_of_it() {
        let mut grid = TerminalGrid::new((4, 1));
        let wide =
            TerminalCell::wide("界", 2).with_style(TerminalStyle::new().bg(TerminalColor::RED));
        // Clearing the anchor leaves no colored continuation behind.
        grid.set_cell((1, 0), &wide);
        grid.clear_range((1, 0), (1, 0));
        assert_eq!(grid.cells(), [TerminalCell::EMPTY; 4]);
        // Writing over the continuation resets the anchor.
        grid.set_cell((1, 0), &wide);
        grid.set_cell((2, 0), &TerminalCell::new("x"));
        assert_eq!(grid.row_text(0), "  x ");
        // A wide glyph overlapping another's continuation resets its anchor.
        grid.set_cell((0, 0), &wide);
        grid.set_cell((1, 0), &wide);
        assert_eq!(grid.row_text(0), " 界 ");
        assert_eq!(grid.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        // Continuation cells come only from their anchor.
        let continuation = grid.cell((2, 0)).unwrap().clone();
        assert!(!grid.set_cell((0, 0), &continuation));
        // Shrinking clips a wide glyph cut by the new edge.
        grid.resize((2, 1));
        assert_eq!(
            grid.cell((1, 0)).unwrap().occupancy(),
            CellOccupancy::Single
        );
    }

    #[test]
    fn clear_range_clamps_and_rejects_reversed_ranges() {
        let mut grid = TerminalGrid::new((4, 2));
        fill(&mut grid, "ABCDEFGH", 4);
        assert!(!grid.clear_range((2, 0), (1, 0)));
        assert!(grid.clear_range((3, 1), (99, 99)));
        assert_eq!(grid.cell((3, 1)).unwrap(), &TerminalCell::EMPTY);
        assert_eq!(grid.cell((2, 1)).unwrap().symbol(), "G");
        assert!(grid.clear_range((0, 0), (0, 0)));
        assert_eq!(grid.cell((0, 0)).unwrap(), &TerminalCell::EMPTY);
        assert!(!grid.clear_range((0, 0), (0, 0)));
    }

    #[test]
    fn clear_operations_follow_row_major_semantics() {
        let mut grid = TerminalGrid::new((4, 2));
        fill(&mut grid, "ABCDEFGH", 4);
        grid.clear_range((1, 0), (3, 0));
        assert_eq!(grid.to_string(), "A   \nEFGH");

        fill(&mut grid, "ABCDEFGH", 4);
        grid.clear_range((1, 0), (u16::MAX, u16::MAX));
        assert_eq!(grid.to_string(), "A   \n    ");

        fill(&mut grid, "ABCDEFGH", 4);
        grid.clear_range((0, 0), (1, 1));
        assert_eq!(grid.to_string(), "    \n  GH");

        grid.clear_row(1);
        assert_eq!(grid.row_text(1), "    ");
        assert!(!grid.clear_row(5));
        assert!(!grid.clear());
        fill(&mut grid, "ABCDEFGH", 4);
        assert!(grid.clear());
        assert_eq!(grid.cells(), vec![TerminalCell::EMPTY; 8]);
    }

    #[test]
    fn scroll_regions_move_and_clear_rows() {
        let mut grid = TerminalGrid::new((2, 3));
        fill(&mut grid, "AABBCC", 2);
        grid.scroll_up(0..3, 1);
        assert_eq!(grid.to_string(), "BB\nCC\n  ");
        grid.scroll_down(0..3, 1);
        assert_eq!(grid.to_string(), "  \nBB\nCC");
        assert!(!grid.scroll_up(0..3, 0));
        assert!(grid.scroll_up(1..3, 5));
        assert_eq!(grid.to_string(), "  \n  \n  ");
    }

    #[test]
    fn scrolling_heap_symbols_preserves_overlap_in_both_directions() {
        let mut grid = TerminalGrid::new((1, 3));
        let first = TerminalCell::new(
            "a\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
        );
        let second = TerminalCell::new(
            "b\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
        );
        grid.set_cell((0, 0), &first);
        grid.set_cell((0, 1), &second);
        grid.scroll_down(0..3, 1);
        assert_eq!(grid.cell((0, 1)).unwrap(), &first);
        assert_eq!(grid.cell((0, 2)).unwrap(), &second);
        grid.scroll_up(0..3, 1);
        assert_eq!(grid.cell((0, 0)).unwrap(), &first);
        assert_eq!(grid.cell((0, 1)).unwrap(), &second);
        assert_eq!(grid.cell((0, 2)).unwrap(), &TerminalCell::EMPTY);
    }

    #[test]
    fn resize_preserves_the_two_dimensional_overlap() {
        let mut grid = TerminalGrid::new((4, 3));
        fill(&mut grid, "AAAABBBBCCCC", 4);
        grid.set_cursor_position((3, 2));
        grid.resize((2, 3));
        assert_eq!(grid.size(), GridSize::new(2, 3));
        assert_eq!(grid.to_string(), "AA\nBB\nCC");
        assert_eq!(grid.cursor_position(), CellPosition::new(1, 2));
    }

    #[test]
    fn every_grid_and_clone_has_its_own_identity() {
        let grid = TerminalGrid::new((2, 1));
        let clone = grid.clone();
        assert_ne!(grid.identity(), clone.identity());
        assert_ne!(grid.identity(), TerminalGrid::new((2, 1)).identity());
        assert_eq!(grid.revision(), clone.revision());
    }

    #[test]
    fn indexing_follows_row_major_order() {
        let mut grid = TerminalGrid::new((2, 2));
        fill(&mut grid, "abcd", 2);
        assert_eq!(grid.cell((1, 0)).unwrap().symbol(), "b");
        assert_eq!(grid.cell((0, 1)).unwrap().symbol(), "c");
        assert_eq!(grid.row(1).len(), 2);
        assert!(grid.cell((2, 0)).is_none());
        assert!(grid.row(2).is_empty());
        let positions: Vec<_> = grid.iter().map(|(position, _)| position).collect();
        assert_eq!(positions[2], CellPosition::new(0, 1));
    }
}
