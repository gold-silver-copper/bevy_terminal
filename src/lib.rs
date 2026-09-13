#![doc = include_str!("../README.md")]

mod backend;

pub use backend::{RatatuiBackend, RatatuiTerminal};
/// The renderer crate used by this backend.
pub use bevy_terminal;
pub use bevy_terminal::prelude::TerminalRenderer;
pub use bevy_terminal::*;

/// Common renderer and Ratatui adapter types.
pub mod prelude {
    pub use crate::{RatatuiBackend, RatatuiTerminal};
    pub use bevy_terminal::prelude::*;
}
