#![doc = include_str!("../README.md")]

mod backend;
mod color;
mod renderer;

pub use backend::{BevyBackend, TerminalSnapshot, TerminalSurface};
pub use color::TerminalTheme;
pub use renderer::{
    BevyGridPlugin, CursorStyle, TerminalRenderConfig, TerminalRenderStats, TerminalRoot,
    TerminalSystems,
};

/// Convenient imports for applications using `bevy_grid`.
pub mod prelude {
    pub use crate::{
        BevyBackend, BevyGridPlugin, CursorStyle, TerminalRenderConfig, TerminalRenderStats,
        TerminalRoot, TerminalSnapshot, TerminalSurface, TerminalTheme,
    };
}
