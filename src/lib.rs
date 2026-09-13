#![doc = include_str!("../README.md")]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminal};
/// The renderer crate used by this backend.
pub use bevy_terminal;
pub use bevy_terminal::prelude::TerminalRenderer;

/// Common renderer and Ratatui adapter types.
pub mod prelude {
    pub use crate::{RatatuiBackend, RatatuiTerminal};
    pub use bevy_terminal::prelude::{
        BlinkConfig, CellOccupancy, CellPosition, CellSymbol, CursorConfig, CursorStyle, FontFaces,
        FontHinting, FontSource, GridSize, RasterConfig, StyleFlags, SurfaceInfo, SurfaceUpdate,
        TerminalCell, TerminalColor, TerminalGeometry, TerminalPlugin, TerminalRenderConfig,
        TerminalRenderer, TerminalSizing, TerminalSnapshot, TerminalStats, TerminalStatus,
        TerminalStyle, TerminalSurface, TerminalSystems, TerminalTexture, TerminalTheme,
        font_family,
    };
}
