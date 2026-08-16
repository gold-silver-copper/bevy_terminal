#![doc = include_str!("../README.md")]

mod backend;
mod color;
mod renderer;

pub use backend::{BevyBackend, TerminalSnapshot, TerminalSurface};
pub use color::TerminalTheme;
pub use renderer::{
    BevyGridBatchPlugin, BevyGridPlugin, CursorStyle, RetainedBevyGridPlugin, TerminalBatchOutput,
    TerminalBatchPresentation, TerminalBatchRoot, TerminalBatchStats, TerminalRenderConfig,
    TerminalRenderScale, TerminalRenderStats, TerminalRoot, TerminalSystems,
};

/// Convenient imports for applications using `bevy_grid`.
pub mod prelude {
    pub use crate::{
        BevyBackend, BevyGridBatchPlugin, BevyGridPlugin, CursorStyle, TerminalBatchOutput,
        TerminalBatchPresentation, TerminalBatchRoot, TerminalBatchStats, TerminalRenderConfig,
        TerminalRenderScale, TerminalRenderStats, TerminalRoot, TerminalSnapshot, TerminalSurface,
        TerminalTheme,
    };
}
