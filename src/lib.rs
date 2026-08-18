#![doc = include_str!("../README.md")]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminalExt};
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
/// use ratatui::Terminal;
///
/// let (backend, renderer) = RatatuiBackend::with_terminal(10, 3);
/// let terminal: Terminal<RatatuiBackend> = Terminal::new(backend).unwrap();
/// let bundle = (renderer, TerminalRenderConfig::default(), ImageNode::default(), Node::default());
/// # let _ = (terminal, bundle);
/// ```
pub mod prelude {
    pub use bevy_terminal::prelude::{
        BlinkConfig, CellOccupancy, CellPosition, CellSizing, CellSymbol, CursorConfig,
        CursorStyle, FontFaces, FontHinting, FontSizing, FontSource, GridSize, RasterConfig,
        StyleFlags, SurfaceUpdate, TerminalCell, TerminalColor, TerminalPlugin, TerminalReady,
        TerminalRenderConfig, TerminalRenderScale, TerminalSnapshot, TerminalStats, TerminalStyle,
        TerminalSurface, TerminalSystems, TerminalTexture, TerminalTheme,
    };

    pub use crate::{RatatuiBackend, RatatuiTerminalExt, TerminalRenderer};
}
