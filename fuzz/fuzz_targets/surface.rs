//! Drives `TerminalSurface` with structured batches of operations and checks
//! it against an independent reference model of the documented semantics
//! after every `update` call.

#![no_main]

use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, LazyLock, Once},
};

use arbitrary::Arbitrary;
use bevy_terminal_ratatui::bevy_terminal::{
    scene::{
        CellOccupancy, CellPosition, GridSize, StyleFlags, TerminalCell, TerminalColor,
        TerminalSnapshot, TerminalStyle,
    },
    surface::{SurfaceUpdate, TerminalSurface},
};
use libfuzzer_sys::fuzz_target;

const MAX_CELLS: usize = TerminalSurface::MAX_CELLS;

/// Symbols chosen to stress grapheme storage and width handling.
const SYMBOLS: &[&str] = &[
    "",
    " ",
    "A",
    "界",
    "😀",
    "👍🏽",
    "👨\u{200d}👩\u{200d}👧\u{200d}👦",
    "🇺🇸",
    "🏳\u{fe0f}\u{200d}🌈",
    "e\u{301}",
    "\u{301}",
    "\u{200b}",
    "\u{200d}",
    "\u{fe0f}",
    "\t",
    "\n",
    "\r\n",
    "\u{1b}[31m",
    "\0",
    "\u{7f}",
    "\u{fffd}",
    "ｱ",
    "─",
    "a\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
    // 21, 22 and 23 bytes around the 22-byte inline limit.
    "aaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaa",
    // A multibyte character straddling the inline limit.
    "aaaaaaaaaaaaaaaaaaaaa界",
    "aaaaaaaaaaaaaaaaaaa界",
    "aaaaaaaaaaaaaaaaaaaa界",
];

#[derive(Arbitrary, Debug, Clone)]
enum Sym {
    Ascii(u8),
    Table(u8),
    Repeat { which: u8, len: u8 },
    Raw(String),
}

impl Sym {
    fn resolve(&self) -> String {
        match self {
            Sym::Ascii(b) => char::from(b % 128).to_string(),
            Sym::Table(i) => SYMBOLS[usize::from(*i) % SYMBOLS.len()].to_string(),
            Sym::Repeat { which, len } => {
                let base = SYMBOLS[usize::from(*which) % SYMBOLS.len()];
                base.repeat(usize::from(*len) % 40)
            }
            Sym::Raw(s) => s.chars().take(64).collect(),
        }
    }
}

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<Color> for TerminalColor {
    fn from(c: Color) -> Self {
        match c {
            Color::Default => TerminalColor::Default,
            Color::Indexed(i) => TerminalColor::Indexed(i),
            Color::Rgb(r, g, b) => TerminalColor::Rgb(r, g, b),
        }
    }
}

#[derive(Arbitrary, Debug, Clone, Copy)]
struct Style {
    fg: Color,
    bg: Color,
    ul: Color,
    flags: u16,
}

impl From<Style> for TerminalStyle {
    fn from(s: Style) -> Self {
        TerminalStyle::new()
            .fg(s.fg.into())
            .bg(s.bg.into())
            .underline_color(s.ul.into())
            .with(StyleFlags::from_bits_truncate(s.flags))
    }
}

#[derive(Arbitrary, Debug, Clone)]
struct CellSpec {
    sym: Sym,
    /// `None` builds a narrow cell with `TerminalCell::new`.
    wide: Option<u8>,
    style: Option<Style>,
}

impl CellSpec {
    fn build(&self) -> TerminalCell {
        let sym = self.sym.resolve();
        let cell = match self.wide {
            None => TerminalCell::new(&sym),
            Some(columns) => TerminalCell::wide(&sym, u16::from(columns)),
        };
        match self.style {
            Some(style) => cell.with_style(style.into()),
            None => cell,
        }
    }
}

/// A coordinate: mostly small, sometimes relative to the grid edge or huge.
#[derive(Arbitrary, Debug, Clone, Copy)]
struct Coord(u8);

impl Coord {
    fn resolve(self, extent: u16) -> u16 {
        let v = self.0;
        if v < 200 {
            return u16::from(v % 20);
        }
        match v % 14 {
            0 => extent.saturating_sub(1),
            1 => extent,
            2 => extent.saturating_add(1),
            3 => extent.saturating_sub(2),
            4 => u16::MAX,
            5 => u16::MAX - 1,
            6 => 255,
            7 => 256,
            8 => 1024,
            9 => 32768,
            10 => extent / 2,
            11 => 0,
            12 => 4095,
            _ => 65000,
        }
    }
}

/// A grid dimension: mostly small, sometimes large enough to hit MAX_CELLS.
#[derive(Arbitrary, Debug, Clone, Copy)]
struct Dim(u8);

impl Dim {
    fn resolve(self) -> u16 {
        let v = self.0;
        if v < 244 {
            return u16::from(v % 24);
        }
        match v % 12 {
            0 => 0,
            1 => 1,
            2 => 255,
            3 => 256,
            4 => 1000,
            5 => 1024,
            6 => 4096,
            7 => 16384,
            8 => 65534,
            9 => 65535,
            10 => 2,
            _ => 513,
        }
    }
}

#[derive(Arbitrary, Debug, Clone)]
enum Op {
    SetCell {
        x: Coord,
        y: Coord,
        cell: CellSpec,
    },
    /// Writes a run of cells starting at a position, like a producer would.
    SetRun {
        x: Coord,
        y: Coord,
        cells: Vec<CellSpec>,
    },
    /// Writes a cell read back from the pre-batch snapshot (possibly a continuation).
    CopyCell {
        sx: Coord,
        sy: Coord,
        x: Coord,
        y: Coord,
    },
    Clear,
    ClearRow(Coord),
    ClearRange {
        x0: Coord,
        y0: Coord,
        x1: Coord,
        y1: Coord,
    },
    Resize {
        w: Dim,
        h: Dim,
    },
    ScrollUp {
        start: Coord,
        end: Coord,
        count: Coord,
    },
    ScrollDown {
        start: Coord,
        end: Coord,
        count: Coord,
    },
    CursorPosition {
        x: Coord,
        y: Coord,
    },
    CursorVisible(bool),
}

#[derive(Arbitrary, Debug)]
struct Batch {
    ops: Vec<Op>,
    /// Unwind out of the update closure after the ops.
    panic_after: bool,
}

#[derive(Arbitrary, Debug)]
struct Input {
    w: Dim,
    h: Dim,
    batches: Vec<Batch>,
}

// ---------------------------------------------------------------------------
// Reference model
// ---------------------------------------------------------------------------

static SPACE: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from(" "));

#[derive(Clone, Debug, PartialEq)]
struct MCell {
    sym: Arc<str>,
    style: TerminalStyle,
    occ: CellOccupancy,
}

impl MCell {
    fn empty() -> Self {
        Self {
            sym: SPACE.clone(),
            style: TerminalStyle::new(),
            occ: CellOccupancy::Single,
        }
    }

    fn from_cell(cell: &TerminalCell) -> Self {
        Self {
            sym: cell.symbol().into(),
            style: cell.style,
            occ: cell.occupancy(),
        }
    }

    fn columns(&self) -> usize {
        match self.occ {
            CellOccupancy::Wide { columns } => usize::from(columns),
            _ => 1,
        }
    }

    fn matches(&self, cell: &TerminalCell) -> bool {
        *self.sym == *cell.symbol() && self.style == cell.style && self.occ == cell.occupancy()
    }
}

fn occupancy(columns: usize) -> CellOccupancy {
    if columns <= 1 {
        CellOccupancy::Single
    } else {
        CellOccupancy::Wide {
            columns: columns as u16,
        }
    }
}

/// The documented clamp: keep the width, drop rows past MAX_CELLS.
fn bounded(w: u16, h: u16) -> (u16, u16) {
    if usize::from(w) * usize::from(h) <= MAX_CELLS {
        (w, h)
    } else {
        (w, (MAX_CELLS / usize::from(w)) as u16)
    }
}

#[derive(Clone)]
struct Model {
    w: u16,
    h: u16,
    cells: Vec<MCell>,
    cursor: CellPosition,
    visible: bool,
    revision: u64,
    resize_generation: u64,
}

impl Model {
    fn new(w: u16, h: u16) -> Self {
        let (w, h) = bounded(w, h);
        Self {
            w,
            h,
            cells: vec![MCell::empty(); usize::from(w) * usize::from(h)],
            cursor: CellPosition::new(0, 0),
            visible: false,
            revision: 0,
            resize_generation: 0,
        }
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * usize::from(self.w) + x
    }

    /// First column of the glyph covering column `x` of row `y`.
    fn glyph_start(&self, x: usize, y: usize) -> usize {
        let mut a = x;
        while a > 0 && self.cells[self.idx(a, y)].occ == CellOccupancy::Continuation {
            a -= 1;
        }
        a
    }

    /// One past the last column of the glyph covering column `x`.
    fn glyph_end(&self, x: usize, y: usize) -> usize {
        let a = self.glyph_start(x, y);
        (a + self.cells[self.idx(a, y)].columns()).max(x + 1)
    }

    fn write(&mut self, x: u16, y: u16, cell: &MCell) -> bool {
        if x >= self.w || y >= self.h || cell.occ == CellOccupancy::Continuation {
            return false;
        }
        let (x, y, w) = (usize::from(x), usize::from(y), usize::from(self.w));
        let span = cell.columns().min(w - x);
        // Every glyph overlapping [x, x + span) is removed entirely.
        let lo = self.glyph_start(x, y);
        let hi = self.glyph_end(x + span - 1, y);
        let mut changed = false;
        for col in lo..hi {
            let target = if col == x {
                MCell {
                    occ: occupancy(span),
                    ..cell.clone()
                }
            } else if col > x && col < x + span {
                MCell {
                    sym: SPACE.clone(),
                    style: cell.style,
                    occ: CellOccupancy::Continuation,
                }
            } else {
                MCell::empty()
            };
            let i = self.idx(col, y);
            if self.cells[i] != target {
                self.cells[i] = target;
                changed = true;
            }
        }
        changed
    }

    fn clear_range(&mut self, s: CellPosition, e: CellPosition) -> bool {
        if self.w == 0 || self.h == 0 {
            return false;
        }
        let clamp = |p: CellPosition| {
            usize::from(p.y.min(self.h - 1)) * usize::from(self.w)
                + usize::from(p.x.min(self.w - 1))
        };
        let (si, ei) = (clamp(s), clamp(e));
        let w = usize::from(self.w);
        let mut changed = false;
        for i in si..=ei {
            changed |= self.write((i % w) as u16, (i / w) as u16, &MCell::empty());
        }
        changed
    }

    fn resize(&mut self, w: u16, h: u16) -> bool {
        let (w, h) = bounded(w, h);
        if (w, h) == (self.w, self.h) {
            return false;
        }
        let old = std::mem::replace(self, Model::new(w, h));
        self.revision = old.revision;
        self.visible = old.visible;
        self.resize_generation = old.resize_generation.wrapping_add(1);
        for y in 0..old.h.min(h) {
            for x in 0..old.w.min(w) {
                let cell = &old.cells[old.idx(usize::from(x), usize::from(y))];
                if cell.occ != CellOccupancy::Continuation {
                    // Clipped by write.
                    self.write(x, y, cell);
                }
            }
        }
        self.cursor = CellPosition::new(
            old.cursor.x.min(w.saturating_sub(1)),
            old.cursor.y.min(h.saturating_sub(1)),
        );
        true
    }

    fn scroll(&mut self, start: u16, end: u16, count: u16, down: bool) -> bool {
        let start = usize::from(start.min(self.h));
        let end = usize::from(end.min(self.h)).max(start);
        let n = usize::from(count).min(end - start);
        let w = usize::from(self.w);
        let old: Vec<MCell> = self.cells[start * w..end * w].to_vec();
        let row = |r: usize| &old[(r - start) * w..(r - start + 1) * w];
        let mut changed = false;
        for r in start..end {
            let source = if down {
                (r >= start + n).then(|| r - n)
            } else {
                (r + n < end).then(|| r + n)
            };
            let new: Vec<MCell> = match source {
                Some(s) => row(s).to_vec(),
                None => vec![MCell::empty(); w],
            };
            if new.as_slice() != row(r) {
                changed = true;
            }
            self.cells[r * w..(r + 1) * w].clone_from_slice(&new);
        }
        changed
    }
}

// ---------------------------------------------------------------------------
// Driving and checking
// ---------------------------------------------------------------------------

fn apply(
    op: &Op,
    update: &mut SurfaceUpdate<'_>,
    model: &mut Model,
    before: &TerminalSnapshot,
) -> (bool, bool) {
    let (w, h) = (model.w, model.h);
    match op {
        Op::SetCell { x, y, cell } => {
            let (x, y) = (x.resolve(w), y.resolve(h));
            let cell = cell.build();
            assert_eq!(cell.occupancy(), MCell::from_cell(&cell).occ);
            (
                update.set_cell((x, y), &cell),
                model.write(x, y, &MCell::from_cell(&cell)),
            )
        }
        Op::SetRun { x, y, cells } => {
            let (mut x, y) = (x.resolve(w), y.resolve(h));
            let (mut got, mut want) = (false, false);
            for spec in cells.iter().take(32) {
                let cell = spec.build();
                got |= update.set_cell((x, y), &cell);
                want |= model.write(x, y, &MCell::from_cell(&cell));
                x = x.saturating_add(cell.occupancy().columns());
            }
            (got, want)
        }
        Op::CopyCell { sx, sy, x, y } => {
            let source = before.cell((sx.resolve(w), sy.resolve(h))).cloned();
            let Some(cell) = source else {
                return (false, false);
            };
            let (x, y) = (x.resolve(w), y.resolve(h));
            (
                update.set_cell((x, y), &cell),
                model.write(x, y, &MCell::from_cell(&cell)),
            )
        }
        Op::Clear => (
            update.clear(),
            model.clear_range((0, 0).into(), (u16::MAX, u16::MAX).into()),
        ),
        Op::ClearRow(row) => {
            let row = row.resolve(h);
            let want = row < h && model.clear_range((0, row).into(), (u16::MAX, row).into());
            (update.clear_row(row), want)
        }
        Op::ClearRange { x0, y0, x1, y1 } => {
            let s = CellPosition::new(x0.resolve(w), y0.resolve(h));
            let e = CellPosition::new(x1.resolve(w), y1.resolve(h));
            (update.clear_range(s, e), model.clear_range(s, e))
        }
        Op::Resize { w: nw, h: nh } => {
            let (nw, nh) = (nw.resolve(), nh.resolve());
            (update.resize((nw, nh)), model.resize(nw, nh))
        }
        Op::ScrollUp { start, end, count } | Op::ScrollDown { start, end, count } => {
            let down = matches!(op, Op::ScrollDown { .. });
            let (s, e, n) = (start.resolve(h), end.resolve(h), count.resolve(h));
            let got = if down {
                update.scroll_down(s..e, n)
            } else {
                update.scroll_up(s..e, n)
            };
            (got, model.scroll(s, e, n, down))
        }
        Op::CursorPosition { x, y } => {
            let p = CellPosition::new(x.resolve(w), y.resolve(h));
            let want = model.cursor != p;
            model.cursor = p;
            (update.set_cursor_position(p), want)
        }
        Op::CursorVisible(v) => {
            let want = model.visible != *v;
            model.visible = *v;
            (update.set_cursor_visible(*v), want)
        }
    }
}

/// Structural invariants that hold for any snapshot, independent of the model.
fn check_structure(snap: &TerminalSnapshot) {
    let GridSize { width, height } = snap.size();
    let (w, h) = (usize::from(width), usize::from(height));
    assert!(w * h <= MAX_CELLS, "grid {w}x{h} exceeds MAX_CELLS");
    assert_eq!(snap.cells().len(), w * h, "cells len != width*height");
    if w == 0 {
        return;
    }
    for (y, row) in snap.cells().chunks(w).enumerate() {
        let mut x = 0;
        while x < w {
            match row[x].occupancy() {
                CellOccupancy::Continuation => {
                    panic!("orphan continuation at ({x},{y})");
                }
                CellOccupancy::Single => x += 1,
                CellOccupancy::Wide { columns } => {
                    let columns = usize::from(columns);
                    assert!(
                        columns >= 2,
                        "wide anchor with {columns} columns at ({x},{y})"
                    );
                    assert!(x + columns <= w, "anchor at ({x},{y}) spans past row end");
                    for c in x + 1..x + columns {
                        assert!(row[c].is_continuation(), "({c},{y}) not a continuation");
                        assert_eq!(row[c].symbol(), " ");
                        assert_eq!(row[c].style, row[x].style, "continuation style differs");
                    }
                    x += columns;
                }
            }
        }
        // Column accounting: every column is claimed exactly once.
        let claimed: usize = row
            .iter()
            .filter(|c| !c.is_continuation())
            .map(|c| usize::from(c.occupancy().columns()))
            .sum();
        assert_eq!(claimed, w, "row {y} column accounting");
    }
}

fn check(surface: &TerminalSurface, model: &Model, full: bool) {
    let info = surface.info();
    assert_eq!(info.size, GridSize::new(model.w, model.h), "info size");
    assert_eq!(info.cursor_position, model.cursor, "info cursor");
    assert_eq!(info.cursor_visible, model.visible, "info visibility");
    assert_eq!(info.revision, model.revision, "info revision");
    assert_eq!(
        info.resize_generation, model.resize_generation,
        "resize generation"
    );
    if !full {
        return;
    }
    let snap = surface.snapshot();
    assert_eq!(snap.size(), info.size);
    assert_eq!(snap.cursor_position(), info.cursor_position);
    assert_eq!(snap.cursor_visible(), info.cursor_visible);
    assert_eq!(snap.revision(), info.revision);
    check_structure(&snap);
    for (i, (got, want)) in snap.cells().iter().zip(&model.cells).enumerate() {
        assert!(
            want.matches(got),
            "cell {i}: surface {got:?} model {want:?}"
        );
    }
    // iter() positions agree with cell().
    if model.w > 0 {
        for (pos, cell) in snap.iter().step_by(7) {
            assert!(snap.size().contains(pos));
            assert_eq!(snap.cell(pos), Some(cell));
        }
    }
    // Text views: anchors' symbols in order, continuations skipped.
    let mut rows = Vec::new();
    for y in 0..model.h {
        let w = usize::from(model.w);
        let start = usize::from(y) * w;
        let expected: String = model.cells[start..start + w]
            .iter()
            .filter(|c| c.occ != CellOccupancy::Continuation)
            .map(|c| &*c.sym)
            .collect();
        let text = snap.row_text(y);
        assert_eq!(text, expected, "row_text({y})");
        assert_eq!(snap.row(y).len(), w);
        rows.push(text);
    }
    assert_eq!(snap.to_string(), rows.join("\n"), "Display");
    for y in [model.h, model.h.saturating_add(1), u16::MAX] {
        if y >= model.h {
            assert_eq!(snap.row_text(y), "");
            assert!(snap.row(y).is_empty());
        }
    }
    assert!(snap.cell((model.w, 0)).is_none());
    assert!(snap.cell((0, model.h)).is_none());
}

thread_local! {
    static EXPECT_PANIC: Cell<bool> = const { Cell::new(false) };
}

fn install_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !EXPECT_PANIC.with(Cell::get) {
                previous(info);
            }
        }));
    });
}

fuzz_target!(|input: Input| {
    install_hook();
    let mut model = Model::new(input.w.resolve(), input.h.resolve());
    let surface = TerminalSurface::new((input.w.resolve(), input.h.resolve()));
    let handle = surface.clone();
    assert!(handle.shares_state_with(&surface));
    check(
        &surface,
        &model,
        usize::from(model.w) * usize::from(model.h) <= 4096,
    );

    for batch in input.batches.iter().take(24) {
        // Only batches that copy cells need the pre-batch snapshot.
        let copies = batch.ops.iter().any(|op| matches!(op, Op::CopyCell { .. }));
        let before = if copies {
            surface.snapshot()
        } else {
            TerminalSurface::new((0, 0)).snapshot()
        };
        let mut results = Vec::new();
        let mut working = model.clone();
        let run =
            |update: &mut SurfaceUpdate<'_>, results: &mut Vec<(bool, bool)>, m: &mut Model| {
                for op in batch.ops.iter().take(16) {
                    results.push(apply(op, update, m, &before));
                    assert_eq!(
                        update.size(),
                        GridSize::new(m.w, m.h),
                        "update size after {op:?}"
                    );
                    assert_eq!(
                        update.cursor_position(),
                        m.cursor,
                        "update cursor after {op:?}"
                    );
                    assert_eq!(update.cursor_visible(), m.visible);
                }
            };
        let published = if batch.panic_after {
            EXPECT_PANIC.with(|f| f.set(true));
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                handle.update(|u| {
                    run(u, &mut results, &mut working);
                    std::panic::panic_any("producer failure");
                })
            }));
            EXPECT_PANIC.with(|f| f.set(false));
            match outcome {
                Ok(_) => panic!("update swallowed the panic"),
                // A real assertion failure inside the closure must still be a finding.
                Err(payload) => assert!(
                    payload.downcast_ref::<&str>() == Some(&"producer failure"),
                    "assertion inside update: {:?}",
                    payload.downcast_ref::<String>()
                ),
            }
            None
        } else {
            Some(handle.update(|u| run(u, &mut results, &mut working)))
        };

        for (i, (got, want)) in results.iter().enumerate() {
            assert_eq!(
                got, want,
                "op {i} {:?} reported change {got}, model {want}",
                batch.ops[i]
            );
        }
        let any = results.iter().any(|(got, _)| *got);
        if let Some(published) = published {
            assert_eq!(published, any, "update return");
        }
        model = working;
        if any {
            model.revision = model.revision.wrapping_add(1);
        }
        let small = usize::from(model.w) * usize::from(model.h) <= 4096;
        check(&surface, &model, small);
    }
    check(&surface, &model, true);
});
