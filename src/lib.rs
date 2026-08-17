#![doc = include_str!("../README.md")]

mod backend;

pub use backend::RatatuiBackend;
/// The Bevy terminal renderer this backend writes into, re-exported in full.
pub use bevy_terminal;
pub use bevy_terminal::{
    BevyTerminalPlugin, CellOccupancy, CellPosition, CellSymbol, CursorStyle, GridSize,
    RetainedBevyTerminalPlugin, SnapshotDelta, StyleFlags, SurfaceMetrics, SurfaceUpdate,
    TerminalBatch, TerminalBatchOutput, TerminalBatchPresentation, TerminalBatchRoot,
    TerminalBatchStats, TerminalCell, TerminalColor, TerminalRenderConfig, TerminalRenderScale,
    TerminalRenderStats, TerminalRoot, TerminalSnapshot, TerminalStyle, TerminalSurface,
    TerminalSystems, TerminalTheme,
};

/// Convenient imports for applications using `bevy_terminal_ratatui`.
pub mod prelude {
    pub use crate::RatatuiBackend;
    pub use bevy_terminal::prelude::*;
}
