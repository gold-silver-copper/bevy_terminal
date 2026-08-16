//! A Ratatui backend that renders through Parley and Vello.
//!
//! `ParleyBackend` records Ratatui's cell updates in memory. `TerminalRenderer`
//! turns that buffer into a Vello scene and can render the scene into any
//! compatible `wgpu::Texture`.

mod backend;
mod color;
mod presentation;
mod renderer;
mod text;

pub use backend::ParleyBackend;
pub use color::{Rgba, Theme};
pub use presentation::{
    PresentationScale, TexturePresentation, snap_logical_position_to_physical_pixel,
    snap_logical_to_physical_pixel,
};
pub use renderer::{
    AsyncTextureReadback, GpuRenderer, GpuRendererOptions, RenderError, TerminalRenderer,
    TextureReadback, TextureTarget,
};
pub use text::{BundledFont, CellQuantization, FontOptions, FontSource, FontStack, TextMetrics};

pub use ratatui;
pub use vello;
