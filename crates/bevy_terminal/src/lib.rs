#![doc = include_str!("../README.md")]

mod color;
mod renderer;
mod scene;
mod surface;

pub use color::TerminalTheme;
pub use renderer::{
    BevyTerminalPlugin, CursorStyle, RetainedBevyTerminalPlugin, TerminalBatch,
    TerminalBatchOutput, TerminalBatchPresentation, TerminalBatchRoot, TerminalBatchStats,
    TerminalRenderConfig, TerminalRenderScale, TerminalRenderStats, TerminalRoot, TerminalSystems,
};
pub use scene::{
    CellOccupancy, CellPosition, CellSymbol, GridSize, StyleFlags, TerminalCell, TerminalColor,
    TerminalSnapshot, TerminalStyle,
};
pub use surface::{SnapshotDelta, SurfaceMetrics, SurfaceUpdate, TerminalSurface};

/// Convenient imports for applications using `bevy_terminal`.
pub mod prelude {
    pub use crate::{
        BevyTerminalPlugin, CellOccupancy, CellPosition, CursorStyle, GridSize, StyleFlags,
        TerminalBatch, TerminalBatchOutput, TerminalBatchPresentation, TerminalBatchRoot,
        TerminalBatchStats, TerminalCell, TerminalColor, TerminalRenderConfig, TerminalRenderScale,
        TerminalRenderStats, TerminalRoot, TerminalSnapshot, TerminalStyle, TerminalSurface,
        TerminalTheme,
    };
}
