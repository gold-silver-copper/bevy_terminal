//! Shared font setup for the executable examples: the four JetBrains Mono faces
//! vendored with the core crate, embedded so the examples need no font files at
//! runtime.

use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::{FontFaces, TerminalRenderConfig};

const FACES: [&[u8]; 4] = [
    include_bytes!(
        "../../crates/bevy_terminal/assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
    ),
    include_bytes!("../../crates/bevy_terminal/assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    include_bytes!(
        "../../crates/bevy_terminal/assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"
    ),
    include_bytes!(
        "../../crates/bevy_terminal/assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"
    ),
];

/// Handles for the four font faces Ratatui can select through its modifiers.
#[derive(Clone, Resource)]
pub struct ExampleFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
    italic: Handle<Font>,
    bold_italic: Handle<Font>,
}

impl ExampleFonts {
    /// Applies the regular, bold, italic, and bold-italic faces. The renderer
    /// measures the regular face and sizes it to the cell width itself.
    pub fn configure(&self, mut config: TerminalRenderConfig) -> TerminalRenderConfig {
        config.font = FontFaces {
            regular: self.regular.clone().into(),
            bold: Some(self.bold.clone().into()),
            italic: Some(self.italic.clone().into()),
            bold_italic: Some(self.bold_italic.clone().into()),
            synthesize: true,
        };
        config
    }

    /// Creates ordinary Bevy UI text using the regular face.
    #[allow(dead_code)]
    pub fn text_font(&self, font_size: f32) -> TextFont {
        TextFont::from_font_size(font_size).with_font(self.regular.clone())
    }
}

/// Registers the four faces and returns their handles.
///
/// Call this after adding `DefaultPlugins`, which initializes Bevy's font assets.
pub fn load(app: &mut App) -> ExampleFonts {
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    let [regular, bold, italic, bold_italic] =
        FACES.map(|bytes| fonts.add(Font::from_bytes(bytes.to_vec())));
    ExampleFonts {
        regular,
        bold,
        italic,
        bold_italic,
    }
}
