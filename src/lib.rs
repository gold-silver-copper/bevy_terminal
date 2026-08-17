#![doc = include_str!("../README.md")]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminalExt};
/// The Bevy terminal renderer this backend writes into, re-exported in full.
pub use bevy_terminal;
pub use bevy_terminal::{
    BlinkConfig, CellOccupancy, CellPosition, CellSymbol, CursorConfig, CursorStyle, FontFaces,
    FontHinting, FontSizing, FontSource, GridSize, Presentation, StyleFlags, SurfaceMetrics,
    SurfaceUpdate, Terminal, TerminalCell, TerminalColor, TerminalNode, TerminalPlugin,
    TerminalRenderConfig, TerminalRenderScale, TerminalResized, TerminalSnapshot, TerminalStats,
    TerminalStyle, TerminalSurface, TerminalSystems, TerminalTexture, TerminalTheme,
};

/// Convenient imports for applications rendering Ratatui with Bevy.
pub mod prelude {
    pub use crate::{
        BlinkConfig, CursorConfig, CursorStyle, FontFaces, FontSizing, Presentation,
        RatatuiBackend, RatatuiTerminalExt, Terminal, TerminalNode, TerminalPlugin,
        TerminalRenderConfig, TerminalRenderScale, TerminalResized, TerminalStats, TerminalSurface,
        TerminalTexture, TerminalTheme,
    };
}
