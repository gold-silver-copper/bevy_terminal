//! Shared vendored font setup for the executable examples.

use bevy::prelude::*;
use bevy_terminal_ratatui::TerminalRenderConfig;

const FONT_FACES: [&[u8]; 16] = [
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Thin.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-ThinItalic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-ExtraLight.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-ExtraLightItalic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Light.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-LightItalic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Medium.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-MediumItalic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-SemiBoldItalic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-ExtraBold.ttf"),
    include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-ExtraBoldItalic.ttf"),
];

const REGULAR: usize = 6;
const ITALIC: usize = 7;
const BOLD: usize = 12;
const BOLD_ITALIC: usize = 13;

/// Handles for the four font faces Ratatui can select through its modifiers.
#[derive(Clone, Resource)]
pub struct JetBrainsMonoFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
    italic: Handle<Font>,
    bold_italic: Handle<Font>,
}

impl JetBrainsMonoFonts {
    /// Applies deterministic regular, bold, italic, and bold-italic faces.
    pub fn configure(&self, mut config: TerminalRenderConfig) -> TerminalRenderConfig {
        config.font = self.regular.clone().into();
        config.bold_font = Some(self.bold.clone().into());
        config.italic_font = Some(self.italic.clone().into());
        config.bold_italic_font = Some(self.bold_italic.clone().into());
        config
    }

    /// Creates ordinary Bevy UI text using the regular face.
    #[allow(dead_code)]
    pub fn text_font(&self, font_size: f32) -> TextFont {
        TextFont::from_font_size(font_size).with_font(self.regular.clone())
    }
}

/// Registers all 16 static JetBrains Mono faces and returns the four Ratatui style handles.
///
/// Call this after adding `DefaultPlugins`, which initializes Bevy's font assets.
pub fn load(app: &mut App) -> JetBrainsMonoFonts {
    let handles = {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        FONT_FACES.map(|bytes| fonts.add(Font::from_bytes(bytes.to_vec())))
    };
    JetBrainsMonoFonts {
        regular: handles[REGULAR].clone(),
        bold: handles[BOLD].clone(),
        italic: handles[ITALIC].clone(),
        bold_italic: handles[BOLD_ITALIC].clone(),
    }
}
