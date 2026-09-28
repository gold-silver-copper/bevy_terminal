//! Glyph size and alignment constraints, ported from Ghostty's
//! `font/Glyph.zig` (`RenderOptions.Constraint`) at
//! `b40acce58dcf77df52231c3798ea58e924647c89`.
//!
//! Ghostty is MIT licensed: Copyright (c) 2024 Mitchell Hashimoto, Ghostty
//! contributors.
//!
//! Coordinates follow Ghostty: pixels relative to the bottom-left corner of
//! the glyph's first cell, `y` pointing up, fractional until the renderer
//! snaps the result.

use super::nerd_font;

/// How a constraint scales a glyph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Size {
    /// Keep the glyph's size.
    None,
    /// Scale down, preserving the aspect ratio, until the glyph fits.
    Fit,
    /// Scale up or down, preserving the aspect ratio, to exactly fill the
    /// bounds along one axis.
    Cover,
    /// Like `Fit`, but also scale up to cover a single cell (Nerd Fonts).
    FitCover1,
    /// Fill the bounds in both directions, ignoring the aspect ratio.
    Stretch,
}

/// How a constraint positions a glyph along one axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Align {
    /// Keep the position, moved only as far as needed to stay inside.
    None,
    /// Align the leading (bottom/left) edges.
    Start,
    /// Align the trailing (top/right) edges.
    End,
    /// Center within the bounds.
    Center,
    /// Center within the first cell even when several are available (Nerd
    /// Fonts).
    Center1,
}

/// The height a constraint sizes against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Height {
    /// The primary face's line height.
    Cell,
    /// The Nerd Fonts icon height.
    Icon,
}

/// Size and alignment rules for one glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Constraint {
    pub(super) size: Size,
    pub(super) align_vertical: Align,
    pub(super) align_horizontal: Align,
    pub(super) pad_top: f64,
    pub(super) pad_left: f64,
    pub(super) pad_right: f64,
    pub(super) pad_bottom: f64,
    /// Size and bearings of the glyph relative to the bounding box of its
    /// scale group.
    pub(super) relative_width: f64,
    pub(super) relative_height: f64,
    pub(super) relative_x: f64,
    pub(super) relative_y: f64,
    /// Largest width/height ratio a stretch may produce.
    pub(super) max_xy_ratio: Option<f64>,
    /// Most cells the glyph may use horizontally.
    pub(super) max_constraint_width: u8,
    pub(super) height: Height,
}

/// The grid metrics constraints are computed against, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Metrics {
    pub(super) cell_width: f64,
    pub(super) cell_height: f64,
    /// Unrounded advance of the primary face.
    pub(super) face_width: f64,
    /// Unrounded line height (ascent + descent + line gap) of the primary face.
    pub(super) face_height: f64,
    /// Offset from the bottom of the cell to the bottom of the face's box.
    pub(super) face_y: f64,
    /// Nerd Fonts icon heights for multi-cell and single-cell constraints.
    pub(super) icon_height: f64,
    pub(super) icon_height_single: f64,
}

impl Metrics {
    /// Ghostty's metrics for a face: `icon_height` is the face height and
    /// `icon_height_single` weights it with the cap height, like the Nerd
    /// Fonts patcher.
    pub(super) fn new(
        cell: (f64, f64),
        face_width: f64,
        face_height: f64,
        face_y: f64,
        cap_height: f64,
    ) -> Self {
        Self {
            cell_width: cell.0,
            cell_height: cell.1,
            face_width,
            face_height,
            face_y,
            icon_height: face_height,
            icon_height_single: (2.0 * cap_height + face_height) / 3.0,
        }
    }
}

/// A glyph's size and position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GlyphSize {
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) x: f64,
    pub(super) y: f64,
}

impl Constraint {
    /// No constraint.
    pub(super) const NONE: Self = Self {
        size: Size::None,
        align_vertical: Align::None,
        align_horizontal: Align::None,
        pad_top: 0.0,
        pad_left: 0.0,
        pad_right: 0.0,
        pad_bottom: 0.0,
        relative_width: 1.0,
        relative_height: 1.0,
        relative_x: 0.0,
        relative_y: 0.0,
        max_xy_ratio: None,
        max_constraint_width: 2,
        height: Height::Cell,
    };

    /// Ghostty's rule for symbols (`renderer/generic.zig`, `addGlyph`).
    pub(super) const SYMBOL: Self = Self {
        size: Size::Fit,
        ..Self::NONE
    };

    /// Ghostty's rule for every emoji-presentation glyph
    /// (`font/SharedGrid.zig`, `renderGlyph`): as large as possible while
    /// preserving the aspect ratio, centered, with a little side padding.
    pub(super) const EMOJI: Self = Self {
        size: Size::Cover,
        align_horizontal: Align::Center,
        align_vertical: Align::Center,
        pad_left: 0.025,
        pad_right: 0.025,
        ..Self::NONE
    };

    /// The Nerd Fonts constraint of a codepoint, if it has one.
    pub(super) fn nerd_font(codepoint: u32) -> Option<Self> {
        let ranges = &nerd_font::RANGES;
        let index = ranges.partition_point(|(_, end, _)| *end < codepoint);
        ranges
            .get(index)
            .filter(|(start, _, _)| *start <= codepoint)
            .map(|(_, _, constraint)| nerd_font::CONSTRAINTS[usize::from(*constraint)])
    }

    /// Whether the constraint changes anything.
    pub(super) fn does_anything(&self) -> bool {
        self.size != Size::None
            || self.align_horizontal != Align::None
            || self.align_vertical != Align::None
    }

    /// Applies the constraint to `glyph` with `constraint_width` cells
    /// available horizontally.
    pub(super) fn constrain(
        &self,
        glyph: GlyphSize,
        metrics: Metrics,
        constraint_width: u8,
    ) -> GlyphSize {
        if !self.does_anything() {
            return glyph;
        }
        if self.size == Size::Stretch {
            // Stretched glyphs align across cell boundaries, so they are
            // scaled and aligned to the grid rather than the face.
            let grid = Metrics {
                face_width: metrics.cell_width,
                face_height: metrics.cell_height,
                face_y: 0.0,
                ..metrics
            };
            // Negative padding would only avoid gaps when aligning to the face.
            let constraint = Self {
                pad_bottom: self.pad_bottom.max(0.0),
                pad_top: self.pad_top.max(0.0),
                pad_left: self.pad_left.max(0.0),
                pad_right: self.pad_right.max(0.0),
                ..*self
            };
            return constraint.constrain_inner(glyph, grid, constraint_width);
        }
        self.constrain_inner(glyph, metrics, constraint_width)
    }

    fn constrain_inner(
        &self,
        glyph: GlyphSize,
        metrics: Metrics,
        constraint_width: u8,
    ) -> GlyphSize {
        // For extra wide faces, never stretch glyphs across two cells.
        let width = if self.size == Size::Stretch && metrics.face_width > 0.9 * metrics.face_height
        {
            1
        } else {
            self.max_constraint_width.min(constraint_width)
        };
        // The bounding box of the glyph's scale group.
        let mut group = GlyphSize {
            width: glyph.width / self.relative_width,
            height: glyph.height / self.relative_height,
            x: 0.0,
            y: 0.0,
        };
        group.x = glyph.x - group.width * self.relative_x;
        group.y = glyph.y - group.height * self.relative_y;

        // Scale about the group's center, then align.
        let (width_factor, height_factor) = self.scale_factors(group, metrics, width);
        let center_x = group.x + group.width / 2.0;
        let center_y = group.y + group.height / 2.0;
        group.width *= width_factor;
        group.height *= height_factor;
        group.x = center_x - group.width / 2.0;
        group.y = center_y - group.height / 2.0;
        group.y = self.aligned_y(group, metrics);
        group.x = self.aligned_x(group, metrics, width);

        GlyphSize {
            width: width_factor * glyph.width,
            height: height_factor * glyph.height,
            x: group.x + group.width * self.relative_x,
            y: group.y + group.height * self.relative_y,
        }
    }

    fn scale_factors(&self, group: GlyphSize, metrics: Metrics, width: u8) -> (f64, f64) {
        if self.size == Size::None {
            return (1.0, 1.0);
        }
        let multi_cell = width > 1;
        let target_width =
            (f64::from(width) - (self.pad_left + self.pad_right)) * metrics.face_width;
        let target_height = (1.0 - (self.pad_bottom + self.pad_top))
            * match self.height {
                Height::Cell => metrics.face_height,
                Height::Icon if multi_cell => metrics.icon_height,
                Height::Icon => metrics.icon_height_single,
            };
        let mut width_factor = target_width / group.width;
        let mut height_factor = target_height / group.height;
        match self.size {
            Size::None => unreachable!(),
            Size::Fit => {
                height_factor = 1.0_f64.min(width_factor).min(height_factor);
                width_factor = height_factor;
            }
            Size::Cover => {
                height_factor = width_factor.min(height_factor);
                width_factor = height_factor;
            }
            Size::FitCover1 => {
                // Scale down to fit, or up to cover at least one cell; the
                // same size whatever the constraint width.
                height_factor = width_factor.min(height_factor);
                if multi_cell && height_factor > 1.0 {
                    let (_, single) = self.scale_factors(group, metrics, 1);
                    height_factor = single.max(1.0);
                }
                width_factor = height_factor;
            }
            Size::Stretch => {}
        }
        if let Some(ratio) = self.max_xy_ratio
            && group.width * width_factor > group.height * height_factor * ratio
        {
            width_factor = group.height * height_factor * ratio / group.width;
        }
        (width_factor, height_factor)
    }

    fn aligned_y(&self, group: GlyphSize, metrics: Metrics) -> f64 {
        if self.size == Size::None && self.align_vertical == Align::None {
            return group.y;
        }
        let pad_bottom = self.pad_bottom * metrics.face_height;
        let pad_top = self.pad_top * metrics.face_height;
        let start = metrics.face_y + pad_bottom;
        let end = metrics.face_y + (metrics.face_height - group.height - pad_top);
        let center = (start + end) / 2.0;
        match self.align_vertical {
            // Every size rule implies staying inside; a group too tall to fit
            // is centered.
            Align::None if end < start => center,
            Align::None => group.y.min(end).max(start),
            Align::Start => start,
            Align::End => end,
            Align::Center | Align::Center1 => center,
        }
    }

    fn aligned_x(&self, group: GlyphSize, metrics: Metrics, width: u8) -> f64 {
        if self.size == Size::None && self.align_horizontal == Align::None {
            return group.x;
        }
        // Multi-cell spans run from the first cell's left edge to the right
        // edge of the last face cell.
        let span = metrics.face_width + f64::from(width - 1) * metrics.cell_width;
        let pad_left = self.pad_left * metrics.face_width;
        let pad_right = self.pad_right * metrics.face_width;
        let start = pad_left;
        let end = span - group.width - pad_right;
        match self.align_horizontal {
            // The left edge wins when the group is too wide.
            Align::None => group.x.min(end).max(start),
            Align::Start => start,
            Align::End => start.max(end),
            Align::Center => start.max((start + end) / 2.0),
            Align::Center1 => {
                let end1 = metrics.face_width - group.width - pad_right;
                start.max((start + end1) / 2.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ghostty's `test "Constraints"` (`font/Glyph.zig`), ported verbatim.
    use super::*;

    /// CoreText metrics of JetBrains Mono at size 12 and 96 DPI.
    fn metrics() -> Metrics {
        Metrics {
            cell_width: 10.0,
            cell_height: 22.0,
            face_width: 9.6,
            face_height: 21.12,
            face_y: 0.2,
            icon_height: 21.12,
            icon_height_single: 44.48 / 3.0,
        }
    }

    fn assert_close(expected: GlyphSize, actual: GlyphSize) {
        for (e, a) in [
            (expected.width, actual.width),
            (expected.height, actual.height),
            (expected.x, actual.x),
            (expected.y, actual.y),
        ] {
            assert!(
                (e - a).abs() < 1e-9,
                "expected {expected:?}, got {actual:?}"
            );
        }
    }

    #[test]
    fn unconstrained_text_is_untouched() {
        let x = GlyphSize {
            width: 6.784,
            height: 15.28,
            x: 1.408,
            y: 4.84,
        };
        for width in [1, 2] {
            assert_close(x, Constraint::NONE.constrain(x, metrics(), width));
        }
    }

    #[test]
    fn symbols_fit_the_face() {
        // Iosevka's two-cell '■'.
        let square = GlyphSize {
            width: 10.272,
            height: 10.272,
            x: 2.864,
            y: 5.304,
        };
        assert_close(
            GlyphSize {
                width: 9.6,
                height: 9.6,
                x: 0.0,
                y: 5.64,
            },
            Constraint::SYMBOL.constrain(square, metrics(), 1),
        );
        assert_close(square, Constraint::SYMBOL.constrain(square, metrics(), 2));
    }

    #[test]
    fn emoji_cover_their_cells() {
        let emoji = GlyphSize {
            width: 20.0,
            height: 20.0,
            x: 0.46,
            y: 1.0,
        };
        assert_close(
            GlyphSize {
                width: 18.72,
                height: 18.72,
                x: 0.44,
                y: 1.4,
            },
            Constraint::EMOJI.constrain(emoji, metrics(), 2),
        );
    }

    #[test]
    fn nerd_font_icons_fit_and_cover_one_cell() {
        let constraint = Constraint::nerd_font(0xea61).unwrap();
        assert_eq!(constraint.size, Size::FitCover1);
        assert_eq!(constraint.height, Height::Icon);
        assert_eq!(constraint.align_horizontal, Align::Center1);
        assert_eq!(constraint.align_vertical, Align::Center1);
        let lightbulb = GlyphSize {
            width: 9.015625,
            height: 13.015625,
            x: 3.015625,
            y: 3.76525,
        };
        assert_close(
            GlyphSize {
                width: 7.2125,
                height: 10.4125,
                x: 0.8125,
                y: 5.950695224719102,
            },
            constraint.constrain(lightbulb, metrics(), 1),
        );
        assert_close(
            GlyphSize {
                width: lightbulb.width,
                height: lightbulb.height,
                x: 1.015625,
                y: 4.7483690308988775,
            },
            constraint.constrain(lightbulb, metrics(), 2),
        );
    }

    #[test]
    fn nerd_font_stretch_covers_the_cells() {
        let constraint = Constraint::nerd_font(0xe0c0).unwrap();
        assert_eq!(constraint.size, Size::Stretch);
        assert_eq!(constraint.height, Height::Cell);
        assert_eq!(constraint.align_horizontal, Align::Start);
        assert_eq!(constraint.align_vertical, Align::Center1);
        let flame = GlyphSize {
            width: 16.796875,
            height: 16.46875,
            x: -0.796875,
            y: 1.7109375,
        };
        for width in [1, 2] {
            assert_close(
                GlyphSize {
                    width: 10.0 * f64::from(width),
                    height: 22.0,
                    x: 0.0,
                    y: 0.0,
                },
                constraint.constrain(flame, metrics(), width),
            );
        }
    }

    #[test]
    fn nerd_font_lookup_covers_ranges_and_gaps() {
        assert!(Constraint::nerd_font(0x2630).is_some());
        assert!(Constraint::nerd_font(0xf0001).is_some());
        assert!(Constraint::nerd_font(0xf1af0).is_some());
        assert!(Constraint::nerd_font(u32::from('a')).is_none());
        assert!(Constraint::nerd_font(0x10ffff).is_none());
        assert!(
            nerd_font::RANGES
                .windows(2)
                .all(|pair| pair[0].1 < pair[1].0)
        );
    }
}
