#![doc = include_str!("../README.md")]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminal};
/// The Bevy terminal renderer this backend writes into.
pub use bevy_terminal;
/// The renderer component, under a name that does not collide with
/// [`ratatui::Terminal`] when both preludes are imported.
pub use bevy_terminal::prelude::Terminal as TerminalRenderer;
pub use bevy_terminal::*;

/// Convenient imports for applications rendering Ratatui with Bevy.
///
/// Re-exports `bevy_terminal::prelude` (minus the `Terminal` component, which
/// is available as [`TerminalRenderer`] so it can sit next to
/// `ratatui::Terminal`) plus the backend types:
///
/// ```
/// use bevy::prelude::*;
/// use bevy_terminal_ratatui::prelude::*;
///
/// let bundle = (RatatuiTerminal::new(10, 3), TerminalRenderConfig::default(), ImageNode::default());
/// # let _ = bundle;
/// ```
pub mod prelude {
    #[cfg(feature = "3d")]
    pub use bevy_terminal::prelude::TerminalWorldQuad;
    pub use bevy_terminal::prelude::{
        BlinkConfig, CellOccupancy, CellPosition, CellSizing, CellSymbol, CursorConfig,
        CursorStyle, FontFaces, FontHinting, FontSizing, FontSource, GridSize, RasterConfig,
        SmolStr, StyleFlags, SurfaceUpdate, TerminalCell, TerminalColor, TerminalFonts,
        TerminalPlugin, TerminalReady, TerminalRemeasured, TerminalRenderConfig,
        TerminalRenderScale, TerminalSnapshot, TerminalStats, TerminalStyle, TerminalSurface,
        TerminalSystems, TerminalTexture, TerminalTheme, font_family,
    };

    pub use crate::{RatatuiBackend, RatatuiTerminal, TerminalRenderer};
}
