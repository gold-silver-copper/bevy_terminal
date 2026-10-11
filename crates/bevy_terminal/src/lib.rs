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

pub mod render;
pub mod scene;
pub mod surface;

/// The Bevy version this crate is built against, re-exported so dependents that
/// do not depend on `bevy` directly can name its types.
pub use bevy;

/// Everything an application needs, for glob import.
pub mod prelude {
    pub use bevy::text::{FontHinting, FontSource};

    pub use crate::{
        render::{
            BlinkConfig, CursorConfig, CursorStyle, FontFaces, RasterConfig, TerminalGeometry,
            TerminalPlugin, TerminalRenderConfig, TerminalRenderer, TerminalSizing, TerminalStats,
            TerminalStatus, TerminalSystems, TerminalTexture, TerminalTheme,
        },
        scene::{
            CellOccupancy, CellPosition, GridSize, StyleFlags, TerminalCell, TerminalColor,
            TerminalSnapshot, TerminalStyle,
        },
        surface::{SurfaceInfo, SurfaceUpdate, TerminalSurface},
    };
}
