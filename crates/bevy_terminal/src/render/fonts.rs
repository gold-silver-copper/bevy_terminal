//! Constructing named Bevy font sources.

use bevy::text::FontSource;

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
    FontSource::Family(name.as_ref().into())
}
