use ratatui::style::{Color, Modifier, Style};
use vello::peniko;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_peniko(self) -> peniko::Color {
        peniko::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub foreground: Rgba,
    pub background: Rgba,
    pub cursor: Rgba,
    pub palette: [Rgba; 16],
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: Rgba::rgb(229, 231, 235),
            background: Rgba::rgb(17, 24, 39),
            cursor: Rgba::rgb(245, 158, 11),
            palette: [
                Rgba::rgb(0, 0, 0),
                Rgba::rgb(205, 49, 49),
                Rgba::rgb(13, 188, 121),
                Rgba::rgb(229, 229, 16),
                Rgba::rgb(36, 114, 200),
                Rgba::rgb(188, 63, 188),
                Rgba::rgb(17, 168, 205),
                Rgba::rgb(229, 229, 229),
                Rgba::rgb(102, 102, 102),
                Rgba::rgb(241, 76, 76),
                Rgba::rgb(35, 209, 139),
                Rgba::rgb(245, 245, 67),
                Rgba::rgb(59, 142, 234),
                Rgba::rgb(214, 112, 214),
                Rgba::rgb(41, 184, 219),
                Rgba::rgb(255, 255, 255),
            ],
        }
    }
}

impl Theme {
    pub fn foreground(&self, style: Style) -> Rgba {
        self.resolve(style.fg, self.foreground, style.add_modifier)
    }

    pub fn background(&self, style: Style) -> Rgba {
        self.resolve(style.bg, self.background, style.add_modifier)
    }

    fn resolve(&self, color: Option<Color>, default: Rgba, modifiers: Modifier) -> Rgba {
        let mut rgba = match color {
            Some(Color::Reset) | None => default,
            Some(Color::Black) => self.palette[0],
            Some(Color::Red) => self.palette[1],
            Some(Color::Green) => self.palette[2],
            Some(Color::Yellow) => self.palette[3],
            Some(Color::Blue) => self.palette[4],
            Some(Color::Magenta) => self.palette[5],
            Some(Color::Cyan) => self.palette[6],
            Some(Color::Gray) | Some(Color::White) => self.palette[7],
            Some(Color::DarkGray) => self.palette[8],
            Some(Color::LightRed) => self.palette[9],
            Some(Color::LightGreen) => self.palette[10],
            Some(Color::LightYellow) => self.palette[11],
            Some(Color::LightBlue) => self.palette[12],
            Some(Color::LightMagenta) => self.palette[13],
            Some(Color::LightCyan) => self.palette[14],
            Some(Color::Indexed(index)) => indexed_color(index, &self.palette),
            Some(Color::Rgb(r, g, b)) => Rgba::rgb(r, g, b),
        };

        if modifiers.contains(Modifier::DIM) {
            rgba.r = (f32::from(rgba.r) * 0.66) as u8;
            rgba.g = (f32::from(rgba.g) * 0.66) as u8;
            rgba.b = (f32::from(rgba.b) * 0.66) as u8;
        }

        rgba
    }
}

fn indexed_color(index: u8, palette: &[Rgba; 16]) -> Rgba {
    match index {
        0..=15 => palette[index as usize],
        16..=231 => {
            let index = index - 16;
            let r = index / 36;
            let g = (index % 36) / 6;
            let b = index % 6;
            Rgba::rgb(
                color_cube_channel(r),
                color_cube_channel(g),
                color_cube_channel(b),
            )
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            Rgba::rgb(value, value, value)
        }
    }
}

fn color_cube_channel(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}
