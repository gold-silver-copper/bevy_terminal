#![doc = include_str!("../README.md")]
// Library code must not panic; tests may.
#![cfg_attr(
    not(test),
    forbid(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unreachable
    )
)]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminal};
/// The renderer crate used by this backend.
pub use bevy_terminal;
pub use bevy_terminal::prelude::TerminalRenderer;

/// Common renderer and Ratatui adapter types.
pub mod prelude {
    pub use crate::{RatatuiBackend, RatatuiTerminal};
    pub use bevy_terminal::prelude::{
        BlinkConfig, CellOccupancy, CellPosition, CursorConfig, CursorStyle, FontFaces,
        FontHinting, FontSource, GridSize, RasterConfig, StyleFlags, SurfaceInfo, SurfaceUpdate,
        TerminalCell, TerminalColor, TerminalGeometry, TerminalPlugin, TerminalRenderConfig,
        TerminalRenderer, TerminalSizing, TerminalSnapshot, TerminalStats, TerminalStatus,
        TerminalStyle, TerminalSurface, TerminalSystems, TerminalTexture, TerminalTheme,
    };
}
