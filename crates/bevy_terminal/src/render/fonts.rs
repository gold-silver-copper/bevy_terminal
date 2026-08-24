//! Font family helpers: constructing [`FontSource::Family`] values without
//! naming `SmolStr`, and querying which families Bevy's font system knows.

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    text::{FontCx, FontSource},
};
pub use smol_str::SmolStr;

/// A [`FontSource::Family`] for `name`, accepting any string type:
///
/// ```
/// # use bevy_terminal::render::{font_family, FontFaces};
/// let faces = FontFaces::regular(font_family("JetBrains Mono"));
/// let owned = String::from("Iosevka Fixed");
/// let faces = FontFaces::regular(font_family(owned));
/// # let _ = faces;
/// ```
///
/// A named family resolves against fonts registered as assets (under their
/// embedded family name) and, with the `system_fonts` feature, the system's
/// installed fonts.
#[must_use]
pub fn font_family(name: impl AsRef<str>) -> FontSource {
    FontSource::Family(SmolStr::new(name.as_ref()))
}

/// Read-only access to the font families Bevy's font system can resolve, for
/// preference and fallback logic without a separate font-discovery dependency:
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy_terminal::render::{FontFaces, TerminalFonts, TerminalRenderConfig};
/// fn pick_font(mut fonts: TerminalFonts, mut config: Single<&mut TerminalRenderConfig>) {
///     match fonts.resolve_family(&["JetBrainsMono Nerd Font Mono", "JetBrains Mono"]) {
///         Some(family) => config.font = FontFaces::regular(family),
///         None => warn!("JetBrains Mono is not installed; using the generic monospace family"),
///     }
/// }
/// ```
///
/// The collection contains the fonts loaded as `Font` assets (registered by
/// Bevy in `PostUpdate`, so a font added in `Startup` is visible from the next
/// frame) and, with the `system_fonts` feature, the installed system fonts,
/// which are enumerated lazily on first use. Lookups are case-insensitive.
#[derive(SystemParam)]
pub struct TerminalFonts<'w> {
    font_cx: ResMut<'w, FontCx>,
}

impl TerminalFonts<'_> {
    /// Whether a family called `name` is available.
    pub fn has_family(&mut self, name: &str) -> bool {
        self.font_cx.collection.family_by_name(name).is_some()
    }

    /// The first family in `preferred` that is available, as a
    /// [`FontSource::Family`] ready for [`super::FontFaces`].
    pub fn resolve_family(&mut self, preferred: &[&str]) -> Option<FontSource> {
        preferred
            .iter()
            .find(|name| self.has_family(name))
            .map(font_family)
    }

    /// Every known family name (registered assets first, then system fonts).
    pub fn families(&mut self) -> Vec<String> {
        self.font_cx
            .collection
            .family_names()
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_family_accepts_any_string_type() {
        assert_eq!(font_family("A"), FontSource::Family(SmolStr::new("A")));
        assert_eq!(
            font_family(String::from("B")),
            FontSource::Family("B".into())
        );
    }

    #[test]
    fn registered_font_assets_are_discoverable_by_family() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ));
        let bytes =
            include_bytes!("../../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
        let font = bevy::text::Font::from_bytes(bytes.to_vec());
        let handle = app.world_mut().resource_mut::<Assets<Font>>().add(font);
        app.update();
        let mut state = bevy::ecs::system::SystemState::<TerminalFonts>::new(app.world_mut());
        let mut fonts = state.get_mut(app.world_mut()).expect("FontCx exists");
        assert!(fonts.has_family("JetBrains Mono"), "{:?}", fonts.families());
        assert!(fonts.has_family("jetbrains mono"));
        assert_eq!(
            fonts.resolve_family(&["No Such Family", "JetBrains Mono"]),
            Some(font_family("JetBrains Mono"))
        );
        assert_eq!(fonts.resolve_family(&["No Such Family"]), None);
        drop(handle);
    }
}
