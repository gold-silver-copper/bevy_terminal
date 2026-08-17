#![doc = include_str!("../README.md")]

mod color;
mod renderer;
mod scene;
mod surface;

pub use bevy::text::{FontHinting, FontSource};
pub use color::TerminalTheme;
pub use renderer::{
    BlinkConfig, CursorConfig, CursorStyle, FontFaces, FontSizing, Presentation, Terminal,
    TerminalNode, TerminalPlugin, TerminalRenderConfig, TerminalRenderScale, TerminalResized,
    TerminalStats, TerminalSystems, TerminalTexture,
};
pub use scene::{
    CellOccupancy, CellPosition, CellSymbol, GridSize, StyleFlags, TerminalCell, TerminalColor,
    TerminalSnapshot, TerminalStyle,
};
pub use surface::{SurfaceMetrics, SurfaceUpdate, TerminalSurface};

/// Convenient imports for applications using `bevy_terminal`.
pub mod prelude {
    pub use crate::{
        BlinkConfig, CellOccupancy, CellPosition, CursorConfig, CursorStyle, FontFaces, FontSizing,
        GridSize, Presentation, StyleFlags, Terminal, TerminalCell, TerminalColor, TerminalNode,
        TerminalPlugin, TerminalRenderConfig, TerminalRenderScale, TerminalResized,
        TerminalSnapshot, TerminalStats, TerminalStyle, TerminalSurface, TerminalTexture,
        TerminalTheme,
    };
}
