use crate::scene::TerminalColor;
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
    /// Resolves a foreground color; [`TerminalColor::Default`] is the theme foreground.
    pub(crate) fn foreground(&self, color: TerminalColor) -> BevyColor {
        self.resolve(color, self.foreground)
    }

    /// Resolves a background color; [`TerminalColor::Default`] is the theme background.
    pub(crate) fn background(&self, color: TerminalColor) -> BevyColor {
        self.resolve(color, self.background)
    }

    /// Resolves `color`, using `default` for [`TerminalColor::Default`].
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

pub(crate) fn dim(foreground: BevyColor, background: BevyColor) -> BevyColor {
    let foreground = foreground.to_srgba();
    let background = background.to_srgba();
    BevyColor::srgba(
        foreground.red.mul_add(0.5, background.red * 0.5),
        foreground.green.mul_add(0.5, background.green * 0.5),
        foreground.blue.mul_add(0.5, background.blue * 0.5),
        foreground.alpha,
    )
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
        assert_eq!(theme.foreground(TerminalColor::Default), theme.foreground);
        assert_eq!(theme.background(TerminalColor::Default), theme.background);
    }
}
