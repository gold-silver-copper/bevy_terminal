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

pub mod grid;
pub mod render;
pub mod scene;

/// The Bevy version this crate is built against, re-exported so dependents that
/// do not depend on `bevy` directly can name its types.
pub use bevy;

/// Everything an application needs, for glob import.
pub mod prelude {
    pub use bevy::text::{FontHinting, FontSource};

    pub use crate::{
        grid::TerminalGrid,
        render::{
            BlinkConfig, CursorConfig, CursorStyle, FontFaces, RasterConfig, ResourceTerminal,
            TerminalFailure, TerminalGeometry, TerminalPlugin, TerminalRenderConfig,
            TerminalResourcePlugin, TerminalSizing, TerminalSource, TerminalSourcePlugin,
            TerminalStats, TerminalStatus, TerminalSystems, TerminalTexture, TerminalTheme,
        },
        scene::{
            CellOccupancy, CellPosition, GridSize, StyleFlags, TerminalCell, TerminalColor,
            TerminalStyle,
        },
    };
}
