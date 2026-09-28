//! GPU quad instances and the only ways to build them.

use super::scene::{clip_rect, snap_geometry};
use bevy::prelude::*;

/// One quad as the shader reads it (`batch.wgsl`'s `VertexInput`): 52
/// tightly packed bytes, uploaded as they are.
///
/// The shader tells quads apart by values no real quad has (a negative `v1`
/// marks a solid quad, a negative alpha a colour glyph, a negative
/// background no correction); [`QuadInstance::solid`] and
/// [`QuadInstance::glyph`] are the only ways to build one, so those
/// encodings stay here.
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(super) struct QuadInstance {
    /// Clip-space `[left, top, right, bottom]`.
    rect: [f32; 4],
    /// Atlas `[u0, v0, u1, v1]`.
    uv: [f32; 4],
    /// Linear colour.
    color: [f32; 4],
    /// Linear luminance of the cell background under a coverage glyph, for
    /// Ghostty's linear-corrected blending.
    background: f32,
}

/// What a glyph quad's texels hold, which decides how it blends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Ink {
    /// Coverage (alpha) in the quad's colour, blended with Ghostty's linear
    /// correction against a background of this linear luminance.
    Coverage { background: f32 },
    /// Colour texels (emoji), drawn as they are.
    Color,
}

impl QuadInstance {
    /// A solid rectangle of `color`, snapped to whole pixels of a
    /// `target`-sized texture.
    pub(super) fn solid(geometry: Rect, color: impl Into<LinearRgba>, target: Vec2) -> Self {
        Self {
            rect: clip_rect(snap_geometry(geometry), target).to_array(),
            uv: [0.0, 0.0, 0.0, -1.0],
            color: color.into().to_f32_array(),
            background: -1.0,
        }
    }

    /// Atlas texels `uv` drawn over `geometry`, snapped to whole pixels of a
    /// `target`-sized texture.
    pub(super) fn glyph(
        geometry: Rect,
        uv: Vec4,
        color: impl Into<LinearRgba>,
        ink: Ink,
        target: Vec2,
    ) -> Self {
        let mut color = color.into().to_f32_array();
        let background = match ink {
            Ink::Coverage { background } => background,
            Ink::Color => {
                color[3] = -1.0;
                -1.0
            }
        };
        Self {
            rect: clip_rect(snap_geometry(geometry), target).to_array(),
            uv: uv.to_array(),
            color,
            background,
        }
    }
}

#[cfg(test)]
impl QuadInstance {
    /// A quad with raw field values, for tests of layout and batching.
    pub(super) fn raw(rect: [f32; 4], uv: [f32; 4], color: [f32; 4], background: f32) -> Self {
        Self {
            rect,
            uv,
            color,
            background,
        }
    }

    pub(super) fn rect(&self) -> [f32; 4] {
        self.rect
    }

    pub(super) fn uv(&self) -> [f32; 4] {
        self.uv
    }

    pub(super) fn color(&self) -> [f32; 4] {
        self.color
    }

    pub(super) fn background(&self) -> f32 {
        self.background
    }

    pub(super) fn is_solid(&self) -> bool {
        self.uv[3] < 0.0
    }

    pub(super) fn is_color(&self) -> bool {
        self.color[3] < 0.0
    }
}
