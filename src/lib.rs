#![doc = include_str!("../README.md")]
// Library code must not panic; tests may.
#![cfg_attr(
    not(test),
    forbid(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented,
        clippy::string_slice,
        clippy::dbg_macro,
        clippy::print_stdout,
        clippy::print_stderr
    )
)]

mod backend;

pub use backend::{
    RatatuiBackend, RatatuiPlugin, RatatuiSource, RatatuiSourcePlugin, RatatuiTerminal,
};
/// The renderer crate used by this backend.
pub use bevy_terminal;

/// Common renderer and Ratatui adapter types.
pub mod prelude {
    pub use crate::{
        RatatuiBackend, RatatuiPlugin, RatatuiSource, RatatuiSourcePlugin, RatatuiTerminal,
    };
    pub use bevy_terminal::prelude::*;
}
