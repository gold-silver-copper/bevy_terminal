#![doc = include_str!("../README.md")]

pub mod render;
pub mod scene;
pub mod surface;

/// The Bevy version this crate is built against, re-exported so dependents that
/// do not depend on `bevy` directly can name its types.
pub use bevy;

/// Everything an application needs, for glob import.
pub mod prelude {
    pub use bevy::text::{FontHinting, FontSource};

    #[cfg(feature = "3d")]
    pub use crate::render::TerminalWorldQuad;
    pub use crate::{
        render::{
            BlinkConfig, CellSizing, CursorConfig, CursorStyle, FontFaces, FontSizing,
            RasterConfig, SmolStr, Terminal, TerminalFonts, TerminalPlugin, TerminalReady,
            TerminalRemeasured, TerminalRenderConfig, TerminalRenderScale, TerminalStats,
            TerminalSystems, TerminalTexture, TerminalTheme, font_family,
        },
        scene::{
            CellOccupancy, CellPosition, CellSymbol, GridSize, StyleFlags, TerminalCell,
            TerminalColor, TerminalSnapshot, TerminalStyle,
        },
        surface::{SurfaceUpdate, TerminalSurface},
    };
}
