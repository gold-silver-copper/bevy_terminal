//! The renderer-neutral cell model: grid coordinates, cells, symbols, styles,
//! colors and wide-glyph occupancy. Producers translate their own
//! representation into these types once, when writing cells to a
//! [`crate::grid::TerminalGrid`].

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

    /// Drops rows until the grid holds at most
    /// [`TerminalGrid::MAX_CELLS`](crate::grid::TerminalGrid::MAX_CELLS).
    pub(crate) fn bounded(self) -> Self {
        let rows = crate::grid::TerminalGrid::MAX_CELLS / usize::from(self.width.max(1));
        let rows = u16::try_from(rows).unwrap_or(u16::MAX);
        Self::new(self.width, self.height.min(rows))
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

/// Compact text attributes. The bit positions match Ratatui's modifiers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct StyleFlags(u16);

impl StyleFlags {
    /// Bold font weight.
    pub const BOLD: Self = Self(1 << 0);
    /// Reduced contrast.
    pub const DIM: Self = Self(1 << 1);
    /// Italic font style.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underline decoration.
    pub const UNDERLINED: Self = Self(1 << 3);
    /// Slow text blink.
    pub const SLOW_BLINK: Self = Self(1 << 4);
    /// Rapid text blink.
    pub const RAPID_BLINK: Self = Self(1 << 5);
    /// Swap foreground and background.
    pub const REVERSED: Self = Self(1 << 6);
    /// Hide foreground content.
    pub const HIDDEN: Self = Self(1 << 7);
    /// Strike-through decoration.
    pub const CROSSED_OUT: Self = Self(1 << 8);
    const ALL: u16 = (1 << 9) - 1;

    /// No attributes.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Flags from raw bits, dropping unknown ones.
    #[must_use]
    pub const fn from_bits_truncate(bits: u16) -> Self {
        Self(bits & Self::ALL)
    }

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any flag in `other` is set.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Both sets of flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Sets the flags in `other`.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Clears the flags in `other`.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for StyleFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
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
    /// Creates the default style: default colors and no attributes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            foreground: TerminalColor::Default,
            background: TerminalColor::Default,
            underline: TerminalColor::Default,
            flags: StyleFlags::empty(),
        }
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

/// The grapheme cluster shown in one cell.
///
/// Symbols up to 22 UTF-8 bytes long are stored inline and longer clusters
/// spill to the heap. The representation is private; the type is 24 bytes and
/// never allocates for ASCII, box drawing, CJK, combining sequences or most
/// emoji.
#[derive(Clone, Eq, Hash, PartialEq)]
enum CellSymbol {
    Inline {
        len: u8,
        bytes: [u8; INLINE_SYMBOL_BYTES],
    },
    Heap(Box<str>),
}

impl CellSymbol {
    /// A single ASCII space.
    const SPACE: Self = {
        let mut bytes = [0; INLINE_SYMBOL_BYTES];
        bytes[0] = b' ';
        Self::Inline { len: 1, bytes }
    };

    fn new(symbol: &str) -> Self {
        if symbol.len() > INLINE_SYMBOL_BYTES {
            return Self::Heap(symbol.into());
        }
        let mut bytes = [0; INLINE_SYMBOL_BYTES];
        for (byte, source) in bytes.iter_mut().zip(symbol.as_bytes()) {
            *byte = *source;
        }
        Self::Inline {
            len: symbol.len() as u8,
            bytes,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Inline { len, bytes } => bytes
                .get(..usize::from(*len))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .unwrap_or_default(),
            Self::Heap(text) => text,
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

/// One terminal cell: a symbol, its style and its column occupancy.
///
/// The occupancy is set by the constructors ([`TerminalCell::new`],
/// [`TerminalCell::wide`]) and read through
/// [`TerminalCell::occupancy`]; it cannot be edited in place, so a cell can
/// never claim a span it was not created with. Grid writes generate the
/// continuation cells and clip a span that would leave the row.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct TerminalCell {
    /// The grapheme cluster shown in this cell.
    symbol: CellSymbol,
    /// Colors and attributes.
    pub style: TerminalStyle,
    occupancy: CellOccupancy,
}

impl TerminalCell {
    /// An empty cell: a space with the default style.
    pub const EMPTY: Self = Self {
        symbol: CellSymbol::SPACE,
        style: TerminalStyle::new(),
        occupancy: CellOccupancy::Single,
    };

    /// Creates a single-column cell with the default style.
    #[must_use]
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: CellSymbol::new(symbol),
            style: TerminalStyle::new(),
            occupancy: CellOccupancy::Single,
        }
    }

    /// Creates a wide anchor cell declared to span `columns` columns.
    #[must_use]
    pub fn wide(symbol: &str, columns: u16) -> Self {
        Self {
            symbol: CellSymbol::new(symbol),
            style: TerminalStyle::new(),
            occupancy: CellOccupancy::spanning(columns),
        }
    }

    /// Creates the continuation cell that follows a wide anchor.
    #[must_use]
    pub(crate) const fn continuation_of(anchor: &Self) -> Self {
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

    /// Returns whether this is a continuation cell of a wide glyph, which
    /// shows its anchor's glyph rather than a symbol of its own.
    #[must_use]
    pub const fn is_continuation(&self) -> bool {
        matches!(self.occupancy, CellOccupancy::Continuation)
    }

    /// This cell with its span clipped to `columns`.
    pub(crate) fn clipped(&self, columns: u16) -> Self {
        Self {
            occupancy: CellOccupancy::spanning(columns),
            ..self.clone()
        }
    }
}

impl From<&str> for TerminalCell {
    fn from(symbol: &str) -> Self {
        Self::new(symbol)
    }
}

impl From<char> for TerminalCell {
    fn from(symbol: char) -> Self {
        Self::new(symbol.encode_utf8(&mut [0; 4]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_symbols_are_stored_inline_and_long_ones_on_the_heap() {
        for short in ["A", "\u{7f}", "\\", "e\u{301}", "👍🏽", "界"] {
            let symbol = CellSymbol::new(short);
            assert!(matches!(symbol, CellSymbol::Inline { .. }));
            assert_eq!(symbol.as_str(), short);
        }
        let long = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        let symbol = CellSymbol::new(long);
        assert!(matches!(symbol, CellSymbol::Heap(_)));
        assert_eq!(symbol.as_str(), long);
        assert_eq!(CellSymbol::default().as_str(), " ");
        assert_eq!(TerminalCell::from('界').symbol(), "界");
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
        assert!(flags.intersects(StyleFlags::HIDDEN | StyleFlags::DIM));
        assert_eq!(StyleFlags::from_bits_truncate(0xffff).bits(), 0x1ff);
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
}
