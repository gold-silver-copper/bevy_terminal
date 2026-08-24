//! The renderer-neutral scene model: grid coordinates, cells, symbols, styles,
//! colors, wide-glyph occupancy and owned snapshots. Producers translate their
//! own representation into these types once, when submitting cells to a
//! [`crate::surface::TerminalSurface`].

use std::fmt;

/// Terminal grid dimensions in cells.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GridSize {
    /// Number of columns.
    pub width: u16,
    /// Number of rows.
    pub height: u16,
}

impl GridSize {
    /// Creates a grid size.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// Returns the number of cells in the grid.
    pub(crate) const fn area(self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Returns whether `position` lies inside the grid.
    #[must_use]
    pub fn contains(self, position: impl Into<CellPosition>) -> bool {
        let position = position.into();
        position.x < self.width && position.y < self.height
    }
}

impl From<(u16, u16)> for GridSize {
    fn from((width, height): (u16, u16)) -> Self {
        Self::new(width, height)
    }
}

/// A cell coordinate; `x` is the column and `y` the row, both zero-based.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct CellPosition {
    /// Column.
    pub x: u16,
    /// Row.
    pub y: u16,
}

impl CellPosition {
    /// Creates a cell position.
    #[must_use]
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

impl From<(u16, u16)> for CellPosition {
    fn from((x, y): (u16, u16)) -> Self {
        Self::new(x, y)
    }
}

/// A terminal color.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TerminalColor {
    /// The contextual default: the theme foreground for text, the theme
    /// background for backgrounds, and the resolved foreground for underlines.
    #[default]
    Default,
    /// One of the 256 indexed colors: 0–15 are the theme's ANSI palette,
    /// 16–231 the 6×6×6 cube and 232–255 the grayscale ramp.
    Indexed(u8),
    /// A true color.
    Rgb(u8, u8, u8),
}

impl TerminalColor {
    /// ANSI black (index 0).
    pub const BLACK: Self = Self::Indexed(0);
    /// ANSI red (index 1).
    pub const RED: Self = Self::Indexed(1);
    /// ANSI green (index 2).
    pub const GREEN: Self = Self::Indexed(2);
    /// ANSI yellow (index 3).
    pub const YELLOW: Self = Self::Indexed(3);
    /// ANSI blue (index 4).
    pub const BLUE: Self = Self::Indexed(4);
    /// ANSI magenta (index 5).
    pub const MAGENTA: Self = Self::Indexed(5);
    /// ANSI cyan (index 6).
    pub const CYAN: Self = Self::Indexed(6);
    /// ANSI white / gray (index 7).
    pub const GRAY: Self = Self::Indexed(7);
    /// ANSI bright black / dark gray (index 8).
    pub const DARK_GRAY: Self = Self::Indexed(8);
    /// ANSI bright red (index 9).
    pub const LIGHT_RED: Self = Self::Indexed(9);
    /// ANSI bright green (index 10).
    pub const LIGHT_GREEN: Self = Self::Indexed(10);
    /// ANSI bright yellow (index 11).
    pub const LIGHT_YELLOW: Self = Self::Indexed(11);
    /// ANSI bright blue (index 12).
    pub const LIGHT_BLUE: Self = Self::Indexed(12);
    /// ANSI bright magenta (index 13).
    pub const LIGHT_MAGENTA: Self = Self::Indexed(13);
    /// ANSI bright cyan (index 14).
    pub const LIGHT_CYAN: Self = Self::Indexed(14);
    /// ANSI bright white (index 15).
    pub const WHITE: Self = Self::Indexed(15);
}

/// A compact set of text attribute flags.
///
/// The set is a plain `u16` bit field so style comparison on the render hot
/// path is a couple of integer compares. The bit layout is a stable contract:
/// bit 0 `BOLD`, 1 `DIM`, 2 `ITALIC`, 3 `UNDERLINED`, 4 `SLOW_BLINK`,
/// 5 `RAPID_BLINK`, 6 `REVERSED`, 7 `HIDDEN`, 8 `CROSSED_OUT` — the same order
/// as Ratatui's `Modifier`, so adapters can translate with a mask.
#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub struct StyleFlags(u16);

impl StyleFlags {
    /// No attributes.
    pub const NONE: Self = Self(0);
    /// Bold weight; selects the bold font face when configured.
    pub const BOLD: Self = Self(1 << 0);
    /// Reduced-contrast foreground.
    pub const DIM: Self = Self(1 << 1);
    /// Italic style; selects the italic font face when configured.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underline decoration in the underline color.
    pub const UNDERLINED: Self = Self(1 << 3);
    /// Slow text blink.
    pub const SLOW_BLINK: Self = Self(1 << 4);
    /// Rapid text blink.
    pub const RAPID_BLINK: Self = Self(1 << 5);
    /// Swap foreground and background.
    pub const REVERSED: Self = Self(1 << 6);
    /// Paint the foreground in the background color.
    pub const HIDDEN: Self = Self(1 << 7);
    /// Strike-through decoration in the foreground color.
    pub const CROSSED_OUT: Self = Self(1 << 8);

    const ALL_NAMED: [(Self, &'static str); 9] = [
        (Self::BOLD, "BOLD"),
        (Self::DIM, "DIM"),
        (Self::ITALIC, "ITALIC"),
        (Self::UNDERLINED, "UNDERLINED"),
        (Self::SLOW_BLINK, "SLOW_BLINK"),
        (Self::RAPID_BLINK, "RAPID_BLINK"),
        (Self::REVERSED, "REVERSED"),
        (Self::HIDDEN, "HIDDEN"),
        (Self::CROSSED_OUT, "CROSSED_OUT"),
    ];

    /// Creates a flag set from raw bits; unknown bits are discarded.
    #[must_use]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits & 0x1ff)
    }

    /// Returns the raw bits.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Returns whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether no flag is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the union of both sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Adds `other` to the set.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Removes `other` from the set.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for StyleFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for StyleFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

impl fmt::Debug for StyleFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("StyleFlags(NONE)");
        }
        f.write_str("StyleFlags(")?;
        let mut first = true;
        for (flag, name) in Self::ALL_NAMED {
            if self.contains(flag) {
                if !first {
                    f.write_str(" | ")?;
                }
                first = false;
                f.write_str(name)?;
            }
        }
        f.write_str(")")
    }
}

/// Colors and attributes of one terminal cell.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TerminalStyle {
    /// Text color.
    pub foreground: TerminalColor,
    /// Cell background color.
    pub background: TerminalColor,
    /// Underline color; [`TerminalColor::Default`] follows the foreground.
    pub underline: TerminalColor,
    /// Attribute flags.
    pub flags: StyleFlags,
}

impl TerminalStyle {
    /// The default style: default colors and no attributes.
    pub const DEFAULT: Self = Self {
        foreground: TerminalColor::Default,
        background: TerminalColor::Default,
        underline: TerminalColor::Default,
        flags: StyleFlags::NONE,
    };

    /// Creates the default style.
    #[must_use]
    pub const fn new() -> Self {
        Self::DEFAULT
    }

    /// Sets the foreground color.
    #[must_use]
    pub const fn fg(mut self, color: TerminalColor) -> Self {
        self.foreground = color;
        self
    }

    /// Sets the background color.
    #[must_use]
    pub const fn bg(mut self, color: TerminalColor) -> Self {
        self.background = color;
        self
    }

    /// Sets the underline color.
    #[must_use]
    pub const fn underline_color(mut self, color: TerminalColor) -> Self {
        self.underline = color;
        self
    }

    /// Adds attribute flags.
    #[must_use]
    pub const fn with(mut self, flags: StyleFlags) -> Self {
        self.flags = self.flags.union(flags);
        self
    }

    /// Returns whether `flags` are all set.
    #[must_use]
    pub const fn has(self, flags: StyleFlags) -> bool {
        self.flags.contains(flags)
    }
}

/// How many columns a cell occupies and whether it is the anchor of a wide glyph.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CellOccupancy {
    /// An ordinary glyph occupying one column.
    #[default]
    Single,
    /// The anchor of a glyph spanning `columns` columns (at least two). The
    /// following `columns - 1` cells are [`CellOccupancy::Continuation`].
    Wide {
        /// Total number of columns covered by the glyph, including the anchor.
        columns: u16,
    },
    /// A column covered by the wide glyph anchored to its left. Continuation
    /// cells carry no visible symbol of their own but keep the anchor's style
    /// so backgrounds and decorations stay continuous.
    Continuation,
}

impl CellOccupancy {
    /// Creates the occupancy for a glyph declared to span `columns` columns.
    ///
    /// Zero and one produce [`CellOccupancy::Single`].
    pub(crate) const fn spanning(columns: u16) -> Self {
        if columns <= 1 {
            Self::Single
        } else {
            Self::Wide { columns }
        }
    }

    /// Returns the number of columns claimed by this cell: one for single and
    /// continuation cells, `columns` for a wide anchor.
    #[must_use]
    pub const fn columns(self) -> u16 {
        match self {
            Self::Single | Self::Continuation => 1,
            Self::Wide { columns } => columns,
        }
    }
}

const INLINE_SYMBOL_BYTES: usize = 22;

const ASCII_SYMBOLS: [&str; 128] = [
    "\u{0}", "\u{1}", "\u{2}", "\u{3}", "\u{4}", "\u{5}", "\u{6}", "\u{7}", "\u{8}", "\u{9}",
    "\u{a}", "\u{b}", "\u{c}", "\u{d}", "\u{e}", "\u{f}", "\u{10}", "\u{11}", "\u{12}", "\u{13}",
    "\u{14}", "\u{15}", "\u{16}", "\u{17}", "\u{18}", "\u{19}", "\u{1a}", "\u{1b}", "\u{1c}",
    "\u{1d}", "\u{1e}", "\u{1f}", " ", "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+", ",",
    "-", ".", "/", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ":", ";", "<", "=", ">", "?",
    "@", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
    "S", "T", "U", "V", "W", "X", "Y", "Z", "[", "\\", "]", "^", "_", "`", "a", "b", "c", "d", "e",
    "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x",
    "y", "z", "{", "|", "}", "~", "\u{7f}",
];

/// The grapheme cluster shown in one cell.
///
/// Single ASCII characters are stored as one byte and read back through a
/// static table, symbols up to 22 UTF-8 bytes long are stored inline, and
/// longer clusters spill to the heap. The representation is private; the type
/// is 24 bytes and never allocates for ASCII, box drawing, CJK, combining
/// sequences or most emoji.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct CellSymbol(SymbolRepr);

#[derive(Clone, Eq, Hash, PartialEq)]
enum SymbolRepr {
    Ascii(u8),
    Inline {
        len: u8,
        bytes: [u8; INLINE_SYMBOL_BYTES],
    },
    Heap(Box<str>),
}

impl CellSymbol {
    /// A single ASCII space.
    pub const SPACE: Self = Self(SymbolRepr::Ascii(b' '));

    /// Creates a symbol from a string slice.
    #[must_use]
    pub fn new(symbol: &str) -> Self {
        let source = symbol.as_bytes();
        if let [byte] = source
            && byte.is_ascii()
        {
            Self(SymbolRepr::Ascii(*byte))
        } else if source.len() <= INLINE_SYMBOL_BYTES {
            let mut bytes = [0; INLINE_SYMBOL_BYTES];
            bytes[..source.len()].copy_from_slice(source);
            Self(SymbolRepr::Inline {
                len: source.len() as u8,
                bytes,
            })
        } else {
            Self(SymbolRepr::Heap(symbol.into()))
        }
    }

    /// Returns the symbol text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            SymbolRepr::Ascii(byte) => ASCII_SYMBOLS[usize::from(*byte & 0x7f)],
            SymbolRepr::Inline { len, bytes } => {
                std::str::from_utf8(&bytes[..usize::from(*len)]).unwrap_or("")
            }
            SymbolRepr::Heap(text) => text,
        }
    }
}

impl Default for CellSymbol {
    fn default() -> Self {
        Self::SPACE
    }
}

impl fmt::Debug for CellSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for CellSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for CellSymbol {
    fn from(symbol: &str) -> Self {
        Self::new(symbol)
    }
}

impl From<char> for CellSymbol {
    fn from(symbol: char) -> Self {
        let mut buffer = [0; 4];
        Self::new(symbol.encode_utf8(&mut buffer))
    }
}

impl std::ops::Deref for CellSymbol {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

/// One terminal cell: a symbol, its style and its column occupancy.
///
/// The occupancy is set by the constructors ([`TerminalCell::new`],
/// [`TerminalCell::wide`], [`TerminalCell::continuation_of`]) and read through
/// [`TerminalCell::occupancy`]; it cannot be edited in place, so a cell can
/// never claim a span it was not created with.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct TerminalCell {
    /// The grapheme cluster shown in this cell.
    pub symbol: CellSymbol,
    /// Colors and attributes.
    pub style: TerminalStyle,
    occupancy: CellOccupancy,
}

impl TerminalCell {
    /// An empty cell: a space with the default style.
    pub const EMPTY: Self = Self {
        symbol: CellSymbol::SPACE,
        style: TerminalStyle::DEFAULT,
        occupancy: CellOccupancy::Single,
    };

    /// Creates a single-column cell with the default style.
    #[must_use]
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: CellSymbol::new(symbol),
            style: TerminalStyle::DEFAULT,
            occupancy: CellOccupancy::Single,
        }
    }

    /// Creates a wide anchor cell declared to span `columns` columns.
    #[must_use]
    pub fn wide(symbol: &str, columns: u16) -> Self {
        Self {
            symbol: CellSymbol::new(symbol),
            style: TerminalStyle::DEFAULT,
            occupancy: CellOccupancy::spanning(columns),
        }
    }

    /// Creates the continuation cell that follows a wide anchor.
    #[must_use]
    pub const fn continuation_of(anchor: &Self) -> Self {
        Self {
            symbol: CellSymbol::SPACE,
            style: anchor.style,
            occupancy: CellOccupancy::Continuation,
        }
    }

    /// Replaces the style.
    #[must_use]
    pub const fn with_style(mut self, style: TerminalStyle) -> Self {
        self.style = style;
        self
    }

    /// Returns the column occupancy.
    #[must_use]
    pub const fn occupancy(&self) -> CellOccupancy {
        self.occupancy
    }

    /// Returns the symbol text.
    #[must_use]
    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    /// Returns whether this is a continuation cell of a wide glyph.
    #[must_use]
    pub const fn is_continuation(&self) -> bool {
        matches!(self.occupancy, CellOccupancy::Continuation)
    }

    /// Returns the number of columns claimed by this cell.
    #[must_use]
    pub const fn columns(&self) -> u16 {
        self.occupancy.columns()
    }
}

/// An owned copy of everything the renderer needs to draw a terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalSnapshot {
    pub(crate) size: GridSize,
    pub(crate) cells: Vec<TerminalCell>,
    pub(crate) cursor_position: CellPosition,
    pub(crate) cursor_visible: bool,
    pub(crate) revision: u64,
}

impl TerminalSnapshot {
    /// Returns the grid size.
    #[must_use]
    pub const fn size(&self) -> GridSize {
        self.size
    }

    /// Returns all cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[TerminalCell] {
        &self.cells
    }

    /// Returns the cells of one row, or an empty slice for an out-of-range row.
    #[must_use]
    pub fn row(&self, row: u16) -> &[TerminalCell] {
        if row >= self.size.height {
            return &[];
        }
        let width = usize::from(self.size.width);
        let start = usize::from(row) * width;
        &self.cells[start..start + width]
    }

    /// Returns the cell at a position, or `None` outside the grid.
    #[must_use]
    pub fn cell(&self, position: impl Into<CellPosition>) -> Option<&TerminalCell> {
        let position = position.into();
        if !self.size.contains(position) {
            return None;
        }
        self.cells
            .get(usize::from(position.y) * usize::from(self.size.width) + usize::from(position.x))
    }

    /// Returns the cursor position in cells.
    #[must_use]
    pub const fn cursor_position(&self) -> CellPosition {
        self.cursor_position
    }

    /// Returns whether the producer requested a visible cursor.
    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Returns the surface change revision captured by this snapshot.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Iterates over every cell with its grid position, row-major.
    ///
    /// Together with the public `style` field this supports content and style
    /// assertions against the real backend in tests:
    ///
    /// ```
    /// # use bevy_terminal::scene::TerminalColor;
    /// # use bevy_terminal::surface::TerminalSurface;
    /// # let snapshot = TerminalSurface::new((4, 1)).snapshot();
    /// assert!(snapshot.iter().all(|(_, cell)| {
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
    /// (continuation cells are skipped), styles dropped and trailing spaces
    /// kept so every row has a predictable width. An out-of-range row is
    /// empty.
    #[must_use]
    pub fn row_text(&self, row: u16) -> String {
        self.row(row)
            .iter()
            .filter(|cell| !cell.is_continuation())
            .map(TerminalCell::symbol)
            .collect()
    }

    /// Returns the whole grid as plain text, rows joined with `'\n'` — the
    /// lossy "what is on screen" view for quick assertions:
    ///
    /// ```
    /// # use bevy_terminal::scene::TerminalCell;
    /// # use bevy_terminal::surface::TerminalSurface;
    /// let surface = TerminalSurface::new((8, 2));
    /// surface.update(|update| {
    ///     for (x, ch) in "Loading".chars().enumerate() {
    ///         update.set_cell((x as u16, 1), &TerminalCell::new(&ch.to_string()));
    ///     }
    /// });
    /// assert!(surface.snapshot().to_text().contains("Loading"));
    /// assert_eq!(surface.snapshot().row_text(1), "Loading ");
    /// ```
    ///
    /// [`TerminalSnapshot`] also implements [`fmt::Display`] with this output.
    #[must_use]
    pub fn to_text(&self) -> String {
        (0..self.size.height)
            .map(|row| self.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl fmt::Display for TerminalSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..self.size.height {
            if row > 0 {
                f.write_str("\n")?;
            }
            f.write_str(&self.row_text(row))?;
        }
        Ok(())
    }
}

impl From<&str> for TerminalCell {
    fn from(symbol: &str) -> Self {
        Self::new(symbol)
    }
}

impl From<char> for TerminalCell {
    fn from(symbol: char) -> Self {
        Self {
            symbol: symbol.into(),
            style: TerminalStyle::DEFAULT,
            occupancy: CellOccupancy::Single,
        }
    }
}

impl std::ops::Index<(u16, u16)> for TerminalSnapshot {
    type Output = TerminalCell;

    fn index(&self, (x, y): (u16, u16)) -> &TerminalCell {
        self.cell((x, y))
            .unwrap_or_else(|| panic!("cell ({x}, {y}) is outside {:?}", self.size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_symbols_are_stored_inline_and_long_ones_on_the_heap() {
        assert!(matches!(CellSymbol::new("A").0, SymbolRepr::Ascii(b'A')));
        assert_eq!(CellSymbol::new("A").as_str(), "A");
        assert_eq!(CellSymbol::new("\u{7f}").as_str(), "\u{7f}");
        assert_eq!(CellSymbol::new("\"").as_str(), "\"");
        assert_eq!(CellSymbol::new("\\").as_str(), "\\");
        for byte in 0..128_u8 {
            assert_eq!(ASCII_SYMBOLS[usize::from(byte)].as_bytes(), [byte]);
        }
        assert!(matches!(
            CellSymbol::new("e\u{301}").0,
            SymbolRepr::Inline { .. }
        ));
        assert!(matches!(CellSymbol::new("👍🏽").0, SymbolRepr::Inline { .. }));
        let long = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        let symbol = CellSymbol::new(long);
        assert!(matches!(symbol.0, SymbolRepr::Heap(_)));
        assert_eq!(&*symbol, long);
        assert_eq!(symbol.as_str(), long);
        assert_eq!(CellSymbol::from('界').as_str(), "界");
        assert_eq!(CellSymbol::default(), CellSymbol::SPACE);
    }

    #[test]
    fn cell_types_stay_compact() {
        assert_eq!(std::mem::size_of::<CellSymbol>(), 24);
        assert_eq!(std::mem::size_of::<TerminalStyle>(), 14);
        assert!(std::mem::size_of::<TerminalCell>() <= 48);
    }

    #[test]
    fn style_flags_are_a_bit_set() {
        let mut flags = StyleFlags::BOLD | StyleFlags::ITALIC;
        assert!(flags.contains(StyleFlags::BOLD));
        assert!(!flags.contains(StyleFlags::DIM));
        flags.remove(StyleFlags::BOLD);
        assert_eq!(flags, StyleFlags::ITALIC);
        flags.insert(StyleFlags::HIDDEN);
        assert_eq!(format!("{flags:?}"), "StyleFlags(ITALIC | HIDDEN)");
        assert_eq!(StyleFlags::from_bits(0xffff).bits(), 0x1ff);
    }

    #[test]
    fn occupancy_spanning_normalizes_narrow_widths() {
        assert_eq!(CellOccupancy::spanning(0), CellOccupancy::Single);
        assert_eq!(CellOccupancy::spanning(1), CellOccupancy::Single);
        assert_eq!(CellOccupancy::spanning(2).columns(), 2);
        assert_eq!(CellOccupancy::Continuation.columns(), 1);
        let anchor =
            TerminalCell::wide("界", 2).with_style(TerminalStyle::new().bg(TerminalColor::RED));
        let continuation = TerminalCell::continuation_of(&anchor);
        assert!(continuation.is_continuation());
        assert_eq!(continuation.style, anchor.style);
        assert_eq!(continuation.symbol(), " ");
    }

    #[test]
    fn snapshot_indexing_follows_row_major_order() {
        let size = GridSize::new(3, 2);
        let mut snapshot = TerminalSnapshot {
            size,
            cells: vec![TerminalCell::EMPTY; size.area()],
            cursor_position: CellPosition::new(0, 0),
            cursor_visible: false,
            revision: 0,
        };
        snapshot.cells[4] = TerminalCell::new("X");
        assert_eq!(snapshot[(1, 1)].symbol(), "X");
        assert_eq!(snapshot.row(1)[1].symbol(), "X");
        assert!(snapshot.cell((3, 0)).is_none());
        assert!(snapshot.row(2).is_empty());
    }
}
