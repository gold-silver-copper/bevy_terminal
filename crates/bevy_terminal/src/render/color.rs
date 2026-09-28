use crate::scene::TerminalColor;
use bevy::color::{LinearRgba, Srgba};
use bevy::prelude::Color as BevyColor;

/// Default colors and the 16-color ANSI palette used by the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalTheme {
    /// Foreground used for [`TerminalColor::Default`].
    pub foreground: BevyColor,
    /// Background used for [`TerminalColor::Default`].
    pub background: BevyColor,
    /// ANSI colors 0 through 15 (normal, then bright).
    pub ansi: [BevyColor; 16],
}

impl Default for TerminalTheme {
    fn default() -> Self {
        Self {
            foreground: BevyColor::srgb_u8(229, 229, 229),
            background: BevyColor::srgb_u8(18, 18, 18),
            ansi: [
                BevyColor::srgb_u8(0, 0, 0),
                BevyColor::srgb_u8(205, 0, 0),
                BevyColor::srgb_u8(0, 205, 0),
                BevyColor::srgb_u8(205, 205, 0),
                BevyColor::srgb_u8(0, 0, 238),
                BevyColor::srgb_u8(205, 0, 205),
                BevyColor::srgb_u8(0, 205, 205),
                BevyColor::srgb_u8(229, 229, 229),
                BevyColor::srgb_u8(127, 127, 127),
                BevyColor::srgb_u8(255, 0, 0),
                BevyColor::srgb_u8(0, 255, 0),
                BevyColor::srgb_u8(255, 255, 0),
                BevyColor::srgb_u8(92, 92, 255),
                BevyColor::srgb_u8(255, 0, 255),
                BevyColor::srgb_u8(0, 255, 255),
                BevyColor::srgb_u8(255, 255, 255),
            ],
        }
    }
}

impl TerminalTheme {
    /// Resolves a background color; [`TerminalColor::Default`] is the theme background.
    #[cfg(test)]
    pub(crate) fn background(&self, color: TerminalColor) -> BevyColor {
        self.resolve(color, self.background)
    }

    /// Resolves `color`, using `default` for [`TerminalColor::Default`]:
    /// the per-cell conversion [`Palette`] replaces, kept as its reference.
    #[cfg(test)]
    pub(crate) fn resolve(&self, color: TerminalColor, default: BevyColor) -> BevyColor {
        match color {
            TerminalColor::Default => default,
            TerminalColor::Rgb(red, green, blue) => BevyColor::srgb_u8(red, green, blue),
            TerminalColor::Indexed(index) => self.indexed(index),
        }
    }

    fn indexed(&self, index: u8) -> BevyColor {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let offset = index - 16;
                let red = offset / 36;
                let green = (offset % 36) / 6;
                let blue = offset % 6;
                BevyColor::srgb_u8(cube(red), cube(green), cube(blue))
            }
            232..=255 => {
                let value = 8 + (index - 232) * 10;
                BevyColor::srgb_u8(value, value, value)
            }
        }
    }
}

const fn cube(component: u8) -> u8 {
    if component == 0 {
        0
    } else {
        55 + component * 40
    }
}

/// A colour in the two forms the scene uses: linear for drawing, sRGB for
/// dimming (which mixes in sRGB space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resolved {
    pub(crate) linear: LinearRgba,
    srgba: Srgba,
}

impl Resolved {
    fn new(color: BevyColor) -> Self {
        Self {
            linear: color.to_linear(),
            srgba: color.to_srgba(),
        }
    }

    fn from_srgba(srgba: Srgba) -> Self {
        Self {
            linear: srgba.into(),
            srgba,
        }
    }

    /// Mixes halfway toward `background` in sRGB space, keeping the alpha.
    pub(crate) fn dim(self, background: Self) -> Self {
        let (foreground, background) = (self.srgba, background.srgba);
        Self::from_srgba(Srgba::new(
            foreground.red.mul_add(0.5, background.red * 0.5),
            foreground.green.mul_add(0.5, background.green * 0.5),
            foreground.blue.mul_add(0.5, background.blue * 0.5),
            foreground.alpha,
        ))
    }
}

/// A theme with every colour resolved once, so cells resolve theirs by
/// table lookup instead of converting sRGB to linear per cell. The
/// conversions are Bevy's own, so the results are bit-identical.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Palette {
    pub(crate) foreground: Resolved,
    pub(crate) background: Resolved,
    indexed: [Resolved; 256],
    /// Linear value of each 8-bit sRGB channel value, for true colours.
    channels: [f32; 256],
}

impl Palette {
    pub(crate) fn new(theme: &TerminalTheme) -> Self {
        Self {
            foreground: Resolved::new(theme.foreground),
            background: Resolved::new(theme.background),
            indexed: std::array::from_fn(|index| Resolved::new(theme.indexed(index as u8))),
            channels: std::array::from_fn(|value| Srgba::gamma_function(value as f32 / 255.0)),
        }
    }

    /// Resolves `color`, using `default` for [`TerminalColor::Default`].
    pub(crate) fn resolve(&self, color: TerminalColor, default: Resolved) -> Resolved {
        match color {
            TerminalColor::Default => default,
            TerminalColor::Indexed(index) => self.indexed[usize::from(index)],
            TerminalColor::Rgb(red, green, blue) => Resolved {
                linear: LinearRgba::new(
                    self.channels[usize::from(red)],
                    self.channels[usize::from(green)],
                    self.channels[usize::from(blue)],
                    1.0,
                ),
                srgba: Srgba::rgb_u8(red, green, blue),
            },
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new(&TerminalTheme::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(color: BevyColor) -> (u8, u8, u8) {
        let color = color.to_srgba();
        (
            (color.red * 255.0).round() as u8,
            (color.green * 255.0).round() as u8,
            (color.blue * 255.0).round() as u8,
        )
    }

    #[test]
    fn indexed_colors_cover_palette_cube_and_grayscale() {
        let theme = TerminalTheme::default();
        assert_eq!(
            theme.resolve(TerminalColor::Indexed(1), BevyColor::NONE),
            theme.ansi[1]
        );
        assert_eq!(
            rgb(theme.resolve(TerminalColor::Indexed(16), BevyColor::NONE)),
            (0, 0, 0)
        );
        assert_eq!(
            rgb(theme.resolve(TerminalColor::Indexed(196), BevyColor::NONE)),
            (255, 0, 0)
        );
        assert_eq!(
            rgb(theme.resolve(TerminalColor::Indexed(232), BevyColor::NONE)),
            (8, 8, 8)
        );
        assert_eq!(
            rgb(theme.resolve(TerminalColor::Indexed(255), BevyColor::NONE)),
            (238, 238, 238)
        );
    }

    #[test]
    fn default_uses_the_contextual_color() {
        let theme = TerminalTheme::default();
        assert_eq!(theme.background(TerminalColor::Default), theme.background);
        let palette = Palette::new(&theme);
        let default = palette.resolve(TerminalColor::Default, palette.foreground);
        assert_eq!(default.linear, theme.foreground.to_linear());
    }

    /// The tables must reproduce the per-cell conversions they replace
    /// bit for bit, or pixels would change.
    #[test]
    fn palette_tables_match_bevys_conversions_bit_for_bit() {
        let mut theme = TerminalTheme::default();
        // Palette entries of any colour space, as a theme may use.
        theme.ansi[3] = BevyColor::linear_rgba(0.2, 0.4, 0.6, 0.8);
        theme.ansi[4] = BevyColor::hsla(200.0, 0.5, 0.4, 1.0);
        let palette = Palette::new(&theme);
        use bevy::color::ColorToComponents;
        let bits = |color: LinearRgba| color.to_f32_array().map(f32::to_bits);
        for value in 0..=255u8 {
            let color = TerminalColor::Rgb(value, 255 - value, value / 3);
            let expected = theme.resolve(color, BevyColor::NONE);
            let resolved = palette.resolve(color, palette.foreground);
            assert_eq!(
                bits(resolved.linear),
                bits(expected.to_linear()),
                "{color:?}"
            );
            assert_eq!(resolved.srgba, expected.to_srgba(), "{color:?}");
            let color = TerminalColor::Indexed(value);
            let expected = theme.resolve(color, BevyColor::NONE);
            let resolved = palette.resolve(color, palette.foreground);
            assert_eq!(
                bits(resolved.linear),
                bits(expected.to_linear()),
                "{color:?}"
            );
            assert_eq!(resolved.srgba, expected.to_srgba(), "{color:?}");
        }
        // Dimming mixes in sRGB, as the replaced `Color` code did.
        let (fg, bg) = (theme.ansi[4], BevyColor::srgb_u8(18, 40, 90));
        let (f, b) = (fg.to_srgba(), bg.to_srgba());
        let expected = BevyColor::srgba(
            f.red.mul_add(0.5, b.red * 0.5),
            f.green.mul_add(0.5, b.green * 0.5),
            f.blue.mul_add(0.5, b.blue * 0.5),
            f.alpha,
        );
        let dimmed = Resolved::new(fg).dim(Resolved::new(bg));
        assert_eq!(bits(dimmed.linear), bits(expected.to_linear()));
    }
}
