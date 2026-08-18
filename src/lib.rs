#![doc = include_str!("../README.md")]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminalExt};
/// The Bevy terminal renderer this backend writes into, re-exported in full.
pub use bevy_terminal;
/// The renderer component, under a name that does not collide with
/// [`ratatui::Terminal`] when both preludes are imported.
pub use bevy_terminal::Terminal as TerminalRenderer;
pub use bevy_terminal::{
    BlinkConfig, CellOccupancy, CellPosition, CellSymbol, CursorConfig, CursorStyle, FontFaces,
    FontHinting, FontSizing, FontSource, GridSize, StyleFlags, SurfaceMetrics, SurfaceUpdate,
    Terminal, TerminalCell, TerminalColor, TerminalPlugin, TerminalReady, TerminalRenderConfig,
    TerminalRenderScale, TerminalSnapshot, TerminalStats, TerminalStyle, TerminalSurface,
    TerminalSystems, TerminalTexture, TerminalTheme,
};

/// Convenient imports for applications rendering Ratatui with Bevy.
///
/// The renderer component is exported as [`TerminalRenderer`] so it can sit
/// next to `ratatui::Terminal` without an alias:
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
    pub use crate::{
        BlinkConfig, CursorConfig, CursorStyle, FontFaces, FontSizing, RatatuiBackend,
        RatatuiTerminalExt, TerminalPlugin, TerminalReady, TerminalRenderConfig,
        TerminalRenderScale, TerminalRenderer, TerminalStats, TerminalSurface, TerminalTexture,
        TerminalTheme,
    };
}
