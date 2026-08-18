//! Shared font setup for the executable examples.
//!
//! The examples use the vendored Iosevka Fixed family (`assets/fonts/iosevka-fixed`),
//! which covers box drawing, blocks, braille, arrows, geometric shapes and
//! powerline glyphs so almost nothing in the terminal scenes needs a system
//! fallback font. The faces are read from disk at runtime (they are too large
//! to embed or publish); if they are missing, the smaller JetBrains Mono faces
//! embedded in the package are used instead.

use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::{FontFaces, TerminalRenderConfig};

const IOSEVKA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/iosevka-fixed");
const IOSEVKA_FACES: [&str; 4] = [
    "IosevkaFixed-Regular.ttf",
    "IosevkaFixed-Bold.ttf",
    "IosevkaFixed-Italic.ttf",
    "IosevkaFixed-BoldItalic.ttf",
];

const JETBRAINS_FACES: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
];

/// Handles for the four font faces Ratatui can select through its modifiers.
#[derive(Clone, Resource)]
pub struct ExampleFonts {
    /// Human-readable family name, for titles and captions.
    #[allow(dead_code)]
    pub family: &'static str,
    regular: Handle<Font>,
    bold: Handle<Font>,
    italic: Handle<Font>,
    bold_italic: Handle<Font>,
}

impl ExampleFonts {
    /// Applies deterministic regular, bold, italic, and bold-italic faces. The
    /// renderer measures the regular face and sizes it to the cell width itself.
    pub fn configure(&self, mut config: TerminalRenderConfig) -> TerminalRenderConfig {
        config.font = self.faces();
        config
    }

    /// Returns the four faces as a [`FontFaces`] value.
    pub fn faces(&self) -> FontFaces {
        FontFaces {
            regular: self.regular.clone().into(),
            bold: Some(self.bold.clone().into()),
            italic: Some(self.italic.clone().into()),
            bold_italic: Some(self.bold_italic.clone().into()),
            synthesize: true,
        }
    }

    /// Creates ordinary Bevy UI text using the regular face.
    #[allow(dead_code)]
    pub fn text_font(&self, font_size: f32) -> TextFont {
        TextFont::from_font_size(font_size).with_font(self.regular.clone())
    }
}

/// Registers the four Iosevka Fixed faces (or the embedded JetBrains Mono
/// fallback) and returns their handles.
///
/// Call this after adding `DefaultPlugins`, which initializes Bevy's font assets.
pub fn load(app: &mut App) -> ExampleFonts {
    let iosevka: Option<Vec<Vec<u8>>> = IOSEVKA_FACES
        .iter()
        .map(|face| std::fs::read(std::path::Path::new(IOSEVKA_DIR).join(face)).ok())
        .collect();
    let (family, faces): (&str, Vec<Vec<u8>>) = match iosevka {
        Some(faces) => ("Iosevka Fixed", faces),
        None => {
            warn!("Iosevka Fixed faces not found under {IOSEVKA_DIR}; using JetBrains Mono");
            (
                "JetBrains Mono",
                JETBRAINS_FACES.iter().map(|bytes| bytes.to_vec()).collect(),
            )
        }
    };
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    let mut handles = faces
        .into_iter()
        .map(|bytes| fonts.add(Font::from_bytes(bytes)));
    let regular = handles.next().expect("four faces");
    let bold = handles.next().expect("four faces");
    let italic = handles.next().expect("four faces");
    let bold_italic = handles.next().expect("four faces");
    ExampleFonts {
        family,
        regular,
        bold,
        italic,
        bold_italic,
    }
}
