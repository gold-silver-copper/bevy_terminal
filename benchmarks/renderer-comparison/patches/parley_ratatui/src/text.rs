use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::env;
use std::sync::Arc;

use parley::fontique::{
    Blob, FallbackKey, FamilyId, FontInfoOverride, FontStyle as FontiqueFontStyle,
    FontWeight as FontiqueFontWeight, Language, Script, ScriptExt,
};
use parley::layout::PositionedLayoutItem;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, FontStyle, FontWeight,
    GenericFamily, Layout, LayoutContext, LineHeight, StyleProperty,
};
use swash::text::Codepoint as _;

/// How measured cell dimensions are quantized to the physical pixel grid.
///
/// Quantized cells keep cell edges on whole pixels, but a font-size step whose
/// advance delta is smaller than one pixel can leave the cell width unchanged
/// while the cell height grows, making zoom look like a vertical-only stretch.
/// [`CellQuantization::Fractional`] keeps the exact measured size so both axes
/// scale proportionally on every font-size step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellQuantization {
    /// Rounds cell dimensions down to whole physical pixels.
    ///
    /// This under-allocates up to one pixel per cell and can cramp glyphs.
    Floor,
    /// Rounds cell dimensions to the nearest whole physical pixel.
    #[default]
    Round,
    /// Keeps the exact fractional cell dimensions.
    ///
    /// Cell edges may fall between pixels and rely on anti-aliasing.
    Fractional,
}

impl CellQuantization {
    /// Applies the quantization policy to a measured dimension.
    pub fn apply(self, value: f32) -> f32 {
        match self {
            Self::Floor => value.floor(),
            Self::Round => value.round(),
            Self::Fractional => value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FontOptions {
    /// Font size in physical pixels.
    pub size: f32,
    /// Optional fixed line height in physical pixels.
    pub line_height: Option<f32>,
    /// Extra cell width in physical pixels, applied before cell quantization.
    pub cell_width_offset: f32,
    /// Extra cell height in physical pixels, applied before cell quantization.
    pub cell_height_offset: f32,
    /// How measured cell dimensions are quantized to the pixel grid.
    pub cell_quantization: CellQuantization,
    /// Horizontal glyph offset inside each cell, in physical pixels.
    pub glyph_offset_x: f32,
    /// Vertical glyph offset inside each cell, in physical pixels.
    pub glyph_offset_y: f32,
    /// Preferred regular, styled, and fallback fonts.
    pub fonts: FontStack,
}

/// Font selection for terminal text.
///
/// The regular source is required. Styled sources are optional; when a styled
/// source is missing, Parley asks the regular family for that style and may use
/// a matching system face or synthesize the style. Fallbacks are appended to
/// every variant before generic system fallbacks.
#[derive(Clone, Debug)]
pub struct FontStack {
    /// Preferred source for unstyled text.
    pub regular: FontSource,
    /// Preferred source for bold text.
    pub bold: Option<FontSource>,
    /// Preferred source for italic text.
    pub italic: Option<FontSource>,
    /// Preferred source for bold italic text.
    pub bold_italic: Option<FontSource>,
    /// Ordered fallback sources for CJK, emoji, symbols, and private-use glyphs.
    pub fallbacks: Vec<FontSource>,
}

/// A font specified either by installed family name or bundled bytes.
#[derive(Clone, Debug)]
pub enum FontSource {
    /// An installed family name or CSS-style comma-separated family list.
    Family(String),
    /// Application-provided font bytes.
    Bundled(BundledFont),
}

/// Font bytes bundled by the application.
///
/// Use [`BundledFont::from_static`] with `include_bytes!` for the zero-copy
/// static path, or [`BundledFont::from_vec`] for runtime-loaded font files.
#[derive(Clone, Debug)]
pub struct BundledFont {
    data: Blob<u8>,
    family_name: Option<String>,
}

impl BundledFont {
    /// Registers font data that lives for the duration of the program.
    ///
    /// This is the zero-copy path for `include_bytes!("font.ttf")`.
    pub fn from_static(data: &'static [u8]) -> Self {
        Self {
            data: Blob::new(Arc::new(data)),
            family_name: None,
        }
    }

    /// Registers owned font data.
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self {
            data: Blob::from(data),
            family_name: None,
        }
    }

    /// Overrides the family name exposed to Parley for this font.
    ///
    /// This is useful for selecting bundled fonts by a stable app-defined name,
    /// independent of the internal family name in the font file.
    pub fn with_family_name(mut self, family_name: impl Into<String>) -> Self {
        self.family_name = Some(family_name.into());
        self
    }
}

impl Default for FontOptions {
    fn default() -> Self {
        Self {
            size: 16.0,
            line_height: None,
            cell_width_offset: 0.0,
            cell_height_offset: 0.0,
            cell_quantization: CellQuantization::default(),
            glyph_offset_x: 0.0,
            glyph_offset_y: 0.0,
            fonts: FontStack::default(),
        }
    }
}

impl FontOptions {
    /// Returns options scaled from logical pixels to physical pixels.
    ///
    /// This is intended for renderers that draw into a physical-pixel texture
    /// while sizing terminal cells in logical window coordinates.
    pub fn scaled(mut self, scale: f32) -> Self {
        let scale = scale.max(1.0);
        self.size *= scale;
        if let Some(line_height) = &mut self.line_height {
            *line_height *= scale;
        }
        self.cell_width_offset *= scale;
        self.cell_height_offset *= scale;
        self.glyph_offset_x *= scale;
        self.glyph_offset_y *= scale;
        self
    }

    /// Adds extra measured cell width in physical pixels.
    pub fn with_cell_width_offset(mut self, offset: f32) -> Self {
        self.cell_width_offset = offset;
        self
    }

    /// Adds extra measured cell height in physical pixels.
    pub fn with_cell_height_offset(mut self, offset: f32) -> Self {
        self.cell_height_offset = offset;
        self
    }

    /// Sets how measured cell dimensions are quantized to the pixel grid.
    pub fn with_cell_quantization(mut self, quantization: CellQuantization) -> Self {
        self.cell_quantization = quantization;
        self
    }

    /// Sets a glyph offset inside each cell, in physical pixels.
    pub fn with_glyph_offset(mut self, x: f32, y: f32) -> Self {
        self.glyph_offset_x = x;
        self.glyph_offset_y = y;
        self
    }

    /// Replaces all font variant and fallback choices.
    pub fn with_font_stack(mut self, fonts: FontStack) -> Self {
        self.fonts = fonts;
        self
    }

    /// Sets the regular font from an installed family name.
    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.fonts.regular = FontSource::family(family);
        self
    }

    /// Sets the regular font from a family name or bundled font source.
    pub fn with_regular_font(mut self, source: impl Into<FontSource>) -> Self {
        self.fonts.regular = source.into();
        self
    }

    /// Sets the preferred bold font from a family name or bundled font source.
    pub fn with_bold_font(mut self, source: impl Into<FontSource>) -> Self {
        self.fonts.bold = Some(source.into());
        self
    }

    /// Sets the preferred italic font from a family name or bundled font source.
    pub fn with_italic_font(mut self, source: impl Into<FontSource>) -> Self {
        self.fonts.italic = Some(source.into());
        self
    }

    /// Sets the preferred bold italic font from a family name or bundled source.
    pub fn with_bold_italic_font(mut self, source: impl Into<FontSource>) -> Self {
        self.fonts.bold_italic = Some(source.into());
        self
    }

    /// Appends an ordered fallback font used by every style variant.
    pub fn with_fallback_font(mut self, source: impl Into<FontSource>) -> Self {
        self.fonts.fallbacks.push(source.into());
        self
    }

    /// Appends an installed fallback family used by every style variant.
    pub fn with_fallback_family(self, family: impl Into<String>) -> Self {
        self.with_fallback_font(FontSource::family(family))
    }

    /// Appends a bundled fallback font.
    pub fn with_bundled_font(mut self, font: BundledFont) -> Self {
        self.fonts.fallbacks.push(FontSource::Bundled(font));
        self
    }

    /// Adds a bundled fallback font from `include_bytes!`-style data.
    pub fn with_bundled_font_data(self, data: &'static [u8]) -> Self {
        self.with_bundled_font(BundledFont::from_static(data))
    }

    /// Adds a bundled regular font, exposes it under `family_name`, and selects
    /// that family as the primary font family.
    pub fn with_bundled_font_family(
        self,
        family_name: impl Into<String>,
        data: &'static [u8],
    ) -> Self {
        self.with_regular_font(BundledFont::from_static(data).with_family_name(family_name))
    }
}

impl Default for FontStack {
    fn default() -> Self {
        Self {
            regular: FontSource::family("FiraMono Nerd Font"),
            bold: None,
            italic: None,
            bold_italic: None,
            fallbacks: Vec::new(),
        }
    }
}

impl FontStack {
    /// Creates a stack with a required regular font source.
    pub fn new(regular: impl Into<FontSource>) -> Self {
        Self {
            regular: regular.into(),
            ..Self::default()
        }
    }

    /// Sets the preferred bold font source.
    pub fn with_bold(mut self, source: impl Into<FontSource>) -> Self {
        self.bold = Some(source.into());
        self
    }

    /// Sets the preferred italic font source.
    pub fn with_italic(mut self, source: impl Into<FontSource>) -> Self {
        self.italic = Some(source.into());
        self
    }

    /// Sets the preferred bold italic font source.
    pub fn with_bold_italic(mut self, source: impl Into<FontSource>) -> Self {
        self.bold_italic = Some(source.into());
        self
    }

    /// Appends an ordered fallback font source.
    pub fn with_fallback(mut self, source: impl Into<FontSource>) -> Self {
        self.fallbacks.push(source.into());
        self
    }
}

impl FontSource {
    /// Selects an installed family name or CSS-style family list.
    pub fn family(family: impl Into<String>) -> Self {
        Self::Family(family.into())
    }

    /// Selects bundled font bytes.
    pub fn bundled(font: impl Into<BundledFont>) -> Self {
        Self::Bundled(font.into())
    }
}

impl From<&'static [u8]> for BundledFont {
    fn from(data: &'static [u8]) -> Self {
        Self::from_static(data)
    }
}

impl From<Vec<u8>> for BundledFont {
    fn from(data: Vec<u8>) -> Self {
        Self::from_vec(data)
    }
}

impl From<&'static [u8]> for FontSource {
    fn from(data: &'static [u8]) -> Self {
        Self::Bundled(BundledFont::from_static(data))
    }
}

impl From<Vec<u8>> for FontSource {
    fn from(data: Vec<u8>) -> Self {
        Self::Bundled(BundledFont::from_vec(data))
    }
}

impl From<BundledFont> for FontSource {
    fn from(font: BundledFont) -> Self {
        Self::Bundled(font)
    }
}

impl From<&str> for FontSource {
    fn from(family: &str) -> Self {
        Self::family(family)
    }
}

impl From<String> for FontSource {
    fn from(family: String) -> Self {
        Self::Family(family)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub cell_width: f32,
    pub cell_height: f32,
    pub baseline: f32,
    pub descent: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub strikeout_position: f32,
    pub strikeout_thickness: f32,
    pub glyph_offset_x: f32,
    pub glyph_offset_y: f32,
}

impl TextMetrics {
    /// Returns metrics converted from physical pixels to logical pixels.
    pub fn logical(self, scale: f32) -> Self {
        let scale = scale.max(1.0);
        Self {
            cell_width: self.cell_width / scale,
            cell_height: self.cell_height / scale,
            baseline: self.baseline / scale,
            descent: self.descent / scale,
            underline_position: self.underline_position / scale,
            underline_thickness: self.underline_thickness / scale,
            strikeout_position: self.strikeout_position / scale,
            strikeout_thickness: self.strikeout_thickness / scale,
            glyph_offset_x: self.glyph_offset_x / scale,
            glyph_offset_y: self.glyph_offset_y / scale,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextStyle {
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FontVariant {
    Normal,
    Bold,
    Italic,
    BoldItalic,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum LayoutTextKey {
    Char(char),
    String(Box<str>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: LayoutTextKey,
    variant: FontVariant,
    font_size_bits: u32,
}

struct VariantFamilyStacks {
    normal: Arc<[FontFamilyName<'static>]>,
    bold: Arc<[FontFamilyName<'static>]>,
    italic: Arc<[FontFamilyName<'static>]>,
    bold_italic: Arc<[FontFamilyName<'static>]>,
}

impl VariantFamilyStacks {
    fn get(&self, variant: FontVariant) -> &[FontFamilyName<'static>] {
        match variant {
            FontVariant::Normal => &self.normal,
            FontVariant::Bold => &self.bold,
            FontVariant::Italic => &self.italic,
            FontVariant::BoldItalic => &self.bold_italic,
        }
    }
}

pub(crate) struct TextSystem {
    font_cx: FontContext,
    layout_cx: LayoutContext<()>,
    options: FontOptions,
    metrics: TextMetrics,
    font_families: VariantFamilyStacks,
    locale: Option<Language>,
    fallback_search_families: Arc<[FamilyId]>,
    checked_fallbacks: HashSet<(FallbackKey, char)>,
    fallback_family_scratch: Vec<FamilyId>,
    cache: HashMap<LayoutKey, Arc<Layout<()>>>,
}

impl TextSystem {
    pub fn new(options: FontOptions) -> Self {
        let mut font_cx = FontContext::default();
        let font_families = register_font_stack(&mut font_cx, &options.fonts);
        let fallback_search_families = fallback_search_families(&mut font_cx);
        let mut text = Self {
            font_cx,
            layout_cx: LayoutContext::default(),
            font_families,
            locale: text_locale(),
            fallback_search_families,
            checked_fallbacks: HashSet::default(),
            fallback_family_scratch: Vec::new(),
            cache: HashMap::default(),
            options,
            metrics: TextMetrics::default(),
        };
        text.metrics = text.measure_metrics();
        text
    }

    pub fn metrics(&self) -> TextMetrics {
        self.metrics
    }

    pub fn shape(&mut self, text: &str, style: TextStyle) -> Arc<Layout<()>> {
        let variant = FontVariant::from_style(style);
        if let Some(character) = single_char(text) {
            self.shape_char(character, variant)
        } else {
            self.shape_text(text.to_owned(), variant)
        }
    }

    pub fn register_font(&mut self, font: BundledFont) -> usize {
        let count = register_bundled_fallback_font(
            &mut FontContext::default(),
            &font,
            self.options.fonts.fallbacks.len(),
        );
        if count > 0 {
            self.options.fonts.fallbacks.push(FontSource::Bundled(font));
            self.rebuild_fonts();
        }
        count
    }

    pub fn register_font_data(&mut self, data: &'static [u8]) -> usize {
        self.register_font(BundledFont::from_static(data))
    }

    pub fn register_font_family(
        &mut self,
        family_name: impl Into<String>,
        data: &'static [u8],
    ) -> usize {
        let family_name = family_name.into();
        let font = BundledFont::from_static(data).with_family_name(family_name);
        let count =
            register_bundled_variant_font(&mut FontContext::default(), &font, FontVariant::Normal);
        if count > 0 {
            self.options.fonts.regular = FontSource::Bundled(font);
            self.rebuild_fonts();
        }
        count
    }

    pub fn set_font_stack(&mut self, fonts: FontStack) {
        self.options.fonts = fonts;
        self.rebuild_fonts();
    }

    pub fn set_family(&mut self, family: impl Into<String>) {
        self.options.fonts.regular = FontSource::family(family);
        self.rebuild_fonts();
    }

    fn rebuild_fonts(&mut self) {
        self.font_cx = FontContext::default();
        self.font_families = register_font_stack(&mut self.font_cx, &self.options.fonts);
        self.fallback_search_families = fallback_search_families(&mut self.font_cx);
        self.checked_fallbacks.clear();
        self.cache.clear();
        self.metrics = self.measure_metrics();
    }

    fn shape_char(&mut self, character: char, variant: FontVariant) -> Arc<Layout<()>> {
        let mut buffer = [0; 4];
        let text = character.encode_utf8(&mut buffer);
        self.ensure_fontique_fallbacks(text);

        let key = LayoutKey {
            text: LayoutTextKey::Char(character),
            variant,
            font_size_bits: self.options.size.to_bits(),
        };
        if let Some(layout) = self.cache.get(&key) {
            return Arc::clone(layout);
        }

        self.build_and_cache_layout(key, text, variant)
    }

    fn shape_text(&mut self, text: String, variant: FontVariant) -> Arc<Layout<()>> {
        self.ensure_fontique_fallbacks(&text);

        let key = LayoutKey {
            text: LayoutTextKey::String(text.clone().into_boxed_str()),
            variant,
            font_size_bits: self.options.size.to_bits(),
        };
        if let Some(layout) = self.cache.get(&key) {
            return Arc::clone(layout);
        }

        self.build_and_cache_layout(key, &text, variant)
    }

    fn build_and_cache_layout(
        &mut self,
        key: LayoutKey,
        text: &str,
        variant: FontVariant,
    ) -> Arc<Layout<()>> {
        let (font_style, font_weight) = font_style(variant);
        let families = self.font_families.get(variant);
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(FontFamily::from(families));
        builder.push_default(StyleProperty::FontSize(self.options.size));
        builder.push_default(StyleProperty::FontStyle(font_style));
        builder.push_default(StyleProperty::FontWeight(font_weight));
        builder.push_default(StyleProperty::Locale(self.locale.clone()));
        builder.push_default(LineHeight::Absolute(self.metrics.cell_height));

        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        let layout = Arc::new(layout);
        self.cache.insert(key, Arc::clone(&layout));
        layout
    }

    fn measure_metrics(&mut self) -> TextMetrics {
        let sample = "M";
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, sample, 1.0, true);
        builder.push_default(FontFamily::from(
            self.font_families.get(FontVariant::Normal),
        ));
        builder.push_default(StyleProperty::FontSize(self.options.size));
        builder.push_default(StyleProperty::Locale(self.locale.clone()));
        if let Some(line_height) = self.options.line_height {
            builder.push_default(LineHeight::Absolute(line_height));
        }

        let mut layout = builder.build(sample);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());

        let line = layout.lines().next().expect("sample line");
        let run_metrics = line
            .items()
            .find_map(|item| match item {
                PositionedLayoutItem::GlyphRun(glyph_run) => Some(*glyph_run.run().metrics()),
                _ => None,
            })
            .unwrap_or_default();

        let quantization = self.options.cell_quantization;
        TextMetrics {
            cell_width: quantization
                .apply(layout.full_width() + self.options.cell_width_offset)
                .max(1.0),
            cell_height: quantization
                .apply(line.metrics().line_height + self.options.cell_height_offset)
                .max(1.0),
            baseline: line.metrics().baseline,
            descent: line.metrics().descent,
            underline_position: run_metrics.underline_offset,
            underline_thickness: run_metrics.underline_size.max(1.0),
            strikeout_position: run_metrics.strikethrough_offset,
            strikeout_thickness: run_metrics.strikethrough_size.max(1.0),
            glyph_offset_x: self.options.glyph_offset_x,
            glyph_offset_y: self.options.glyph_offset_y,
        }
    }

    fn ensure_fontique_fallbacks(&mut self, text: &str) {
        let mut changed = false;

        for character in text.chars() {
            let Some(key) = self.fallback_key_for_char(character) else {
                continue;
            };

            if !self.checked_fallbacks.insert((key, character)) {
                continue;
            }

            if self.fallbacks_support_character(key, character) {
                continue;
            }

            changed |= self.seed_fontique_fallbacks(key, character);
        }

        if changed {
            self.checked_fallbacks.clear();
            self.cache.clear();
        }
    }

    fn fallback_key_for_char(&self, character: char) -> Option<FallbackKey> {
        let script = unicode_script_for_char(character);
        if script_uses_common_fallback(script) {
            return fallback_key_for_common_char(character);
        }

        Some(self.fallback_key_for_script(script))
    }

    fn fallback_key_for_script(&self, script: Script) -> FallbackKey {
        let localized = self
            .locale
            .as_ref()
            .map(|locale| FallbackKey::from((script, locale)));
        match localized {
            Some(key) if key.is_tracked() => key,
            _ => FallbackKey::from(script),
        }
    }

    fn fallbacks_support_character(&mut self, key: FallbackKey, character: char) -> bool {
        let mut fallback_families = std::mem::take(&mut self.fallback_family_scratch);
        fallback_families.clear();
        fallback_families.extend(self.font_cx.collection.fallback_families(key));
        let mut buffer = [0; 4];
        let character_text = character.encode_utf8(&mut buffer);
        let supports_character = fallback_families
            .iter()
            .copied()
            .any(|family_id| self.family_supports_text(family_id, character_text));
        self.fallback_family_scratch = fallback_families;
        supports_character
    }

    fn seed_fontique_fallbacks(&mut self, key: FallbackKey, character: char) -> bool {
        let fallback_families = self.find_fallback_families(key.script(), character);
        if fallback_families.is_empty() {
            return false;
        }

        self.font_cx
            .collection
            .append_fallbacks(key, fallback_families.into_iter())
    }

    fn find_fallback_families(
        &mut self,
        script: parley::fontique::Script,
        character: char,
    ) -> Vec<FamilyId> {
        let mut character_buffer = [0; 4];
        let character_text = character.encode_utf8(&mut character_buffer);
        let sample_text = script.sample().unwrap_or(character_text);
        let use_sample_text = sample_text != character_text;
        let search_families = Arc::clone(&self.fallback_search_families);

        let mut preferred = Vec::new();
        let mut fallback_only = Vec::new();
        for &family_id in search_families.iter() {
            if !self.family_supports_text(family_id, character_text) {
                continue;
            }

            if use_sample_text && self.family_supports_text(family_id, sample_text) {
                preferred.push(family_id);
            } else {
                fallback_only.push(family_id);
            }
        }

        preferred.extend(fallback_only);
        preferred
    }

    fn family_supports_text(&mut self, family_id: FamilyId, text: &str) -> bool {
        let Some(family) = self.font_cx.collection.family(family_id) else {
            return false;
        };

        family.fonts().iter().any(|font| {
            let Some(data) = font.load(Some(&mut self.font_cx.source_cache)) else {
                return false;
            };
            let Some(charmap) = font.charmap_index().charmap(data.as_ref()) else {
                return false;
            };

            text.chars()
                .all(|character| charmap.map(character).is_some_and(|glyph_id| glyph_id != 0))
        })
    }
}

impl FontVariant {
    fn from_style(style: TextStyle) -> Self {
        match (style.bold, style.italic) {
            (true, true) => Self::BoldItalic,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (false, false) => Self::Normal,
        }
    }
}

fn register_font_stack(font_cx: &mut FontContext, stack: &FontStack) -> VariantFamilyStacks {
    let fallback_families = fallback_family_stack(font_cx, &stack.fallbacks);
    let regular_families = source_family_names(
        font_cx,
        &stack.regular,
        SourceRegistration::Variant {
            variant: FontVariant::Normal,
            role: "regular",
        },
    );

    VariantFamilyStacks {
        normal: Arc::from(complete_family_stack(
            regular_families.clone(),
            &fallback_families,
        )),
        bold: Arc::from(match &stack.bold {
            Some(source) => variant_family_stack(
                font_cx,
                source,
                FontVariant::Bold,
                "bold",
                &fallback_families,
            ),
            None => complete_family_stack(regular_families.clone(), &fallback_families),
        }),
        italic: Arc::from(match &stack.italic {
            Some(source) => variant_family_stack(
                font_cx,
                source,
                FontVariant::Italic,
                "italic",
                &fallback_families,
            ),
            None => complete_family_stack(regular_families.clone(), &fallback_families),
        }),
        bold_italic: Arc::from(match &stack.bold_italic {
            Some(source) => variant_family_stack(
                font_cx,
                source,
                FontVariant::BoldItalic,
                "bold_italic",
                &fallback_families,
            ),
            None => complete_family_stack(regular_families, &fallback_families),
        }),
    }
}

fn variant_family_stack(
    font_cx: &mut FontContext,
    source: &FontSource,
    variant: FontVariant,
    role: &str,
    fallback_families: &[FontFamilyName<'static>],
) -> Vec<FontFamilyName<'static>> {
    let families = source_family_names(
        font_cx,
        source,
        SourceRegistration::Variant { variant, role },
    );
    complete_family_stack(families, fallback_families)
}

fn complete_family_stack(
    mut families: Vec<FontFamilyName<'static>>,
    fallback_families: &[FontFamilyName<'static>],
) -> Vec<FontFamilyName<'static>> {
    families.extend_from_slice(fallback_families);
    append_generic_families(&mut families);
    families
}

fn fallback_family_stack(
    font_cx: &mut FontContext,
    fallbacks: &[FontSource],
) -> Vec<FontFamilyName<'static>> {
    let mut families = Vec::new();
    for (index, source) in fallbacks.iter().enumerate() {
        families.extend(source_family_names(
            font_cx,
            source,
            SourceRegistration::Fallback { index },
        ));
    }
    families
}

enum SourceRegistration<'a> {
    Variant { variant: FontVariant, role: &'a str },
    Fallback { index: usize },
}

fn source_family_names(
    font_cx: &mut FontContext,
    source: &FontSource,
    registration: SourceRegistration<'_>,
) -> Vec<FontFamilyName<'static>> {
    match source {
        FontSource::Family(family) => parse_font_family_list(family),
        FontSource::Bundled(font) => {
            let family_name = match &font.family_name {
                Some(family_name) => family_name.clone(),
                None => match registration {
                    SourceRegistration::Variant { role, .. } => {
                        format!("parley_ratatui::{role}")
                    }
                    SourceRegistration::Fallback { index } => {
                        format!("parley_ratatui::fallback::{index}")
                    }
                },
            };

            match registration {
                SourceRegistration::Variant { variant, .. } => {
                    register_bundled_variant_font_with_family(font_cx, font, &family_name, variant);
                }
                SourceRegistration::Fallback { .. } => {
                    register_bundled_fallback_font_with_family(font_cx, font, &family_name);
                }
            }

            vec![FontFamilyName::Named(Cow::Owned(family_name))]
        }
    }
}

fn register_bundled_variant_font(
    font_cx: &mut FontContext,
    font: &BundledFont,
    variant: FontVariant,
) -> usize {
    let family_name = font
        .family_name
        .clone()
        .unwrap_or_else(|| String::from("parley_ratatui::registered"));
    register_bundled_variant_font_with_family(font_cx, font, &family_name, variant)
}

fn register_bundled_variant_font_with_family(
    font_cx: &mut FontContext,
    font: &BundledFont,
    family_name: &str,
    variant: FontVariant,
) -> usize {
    let (style, weight) = fontique_style(variant);
    register_bundled_font(
        font_cx,
        font,
        FontInfoOverride {
            family_name: Some(family_name),
            style: Some(style),
            weight: Some(weight),
            ..FontInfoOverride::default()
        },
    )
}

fn register_bundled_fallback_font(
    font_cx: &mut FontContext,
    font: &BundledFont,
    index: usize,
) -> usize {
    let family_name = font
        .family_name
        .clone()
        .unwrap_or_else(|| format!("parley_ratatui::fallback::{index}"));
    register_bundled_fallback_font_with_family(font_cx, font, &family_name)
}

fn register_bundled_fallback_font_with_family(
    font_cx: &mut FontContext,
    font: &BundledFont,
    family_name: &str,
) -> usize {
    register_bundled_font(
        font_cx,
        font,
        FontInfoOverride {
            family_name: Some(family_name),
            ..FontInfoOverride::default()
        },
    )
}

fn register_bundled_font(
    font_cx: &mut FontContext,
    font: &BundledFont,
    override_info: FontInfoOverride<'_>,
) -> usize {
    font_cx
        .collection
        .register_fonts(font.data.clone(), Some(override_info))
        .into_iter()
        .map(|(_, fonts)| fonts.len())
        .sum()
}

fn font_style(variant: FontVariant) -> (FontStyle, FontWeight) {
    match variant {
        FontVariant::Normal => (FontStyle::Normal, FontWeight::NORMAL),
        FontVariant::Bold => (FontStyle::Normal, FontWeight::BOLD),
        FontVariant::Italic => (FontStyle::Italic, FontWeight::NORMAL),
        FontVariant::BoldItalic => (FontStyle::Italic, FontWeight::BOLD),
    }
}

fn fontique_style(variant: FontVariant) -> (FontiqueFontStyle, FontiqueFontWeight) {
    match variant {
        FontVariant::Normal => (FontiqueFontStyle::Normal, FontiqueFontWeight::NORMAL),
        FontVariant::Bold => (FontiqueFontStyle::Normal, FontiqueFontWeight::BOLD),
        FontVariant::Italic => (FontiqueFontStyle::Italic, FontiqueFontWeight::NORMAL),
        FontVariant::BoldItalic => (FontiqueFontStyle::Italic, FontiqueFontWeight::BOLD),
    }
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

fn parse_font_family_list(family: &str) -> Vec<FontFamilyName<'static>> {
    let mut families = Vec::new();
    for family in FontFamilyName::parse_css_list(family).filter_map(Result::ok) {
        match family {
            FontFamilyName::Named(name) => {
                families.push(FontFamilyName::Named(Cow::Owned(name.into_owned())))
            }
            FontFamilyName::Generic(family) => families.push(FontFamilyName::Generic(family)),
        }
    }

    if families.is_empty() {
        families.push(FontFamilyName::Named(Cow::Owned(family.to_owned())));
    }
    families
}

fn append_generic_families(families: &mut Vec<FontFamilyName<'static>>) {
    families.push(FontFamilyName::Generic(GenericFamily::UiMonospace));
    families.push(FontFamilyName::Generic(GenericFamily::Monospace));
    families.push(FontFamilyName::Generic(GenericFamily::SystemUi));
    families.push(FontFamilyName::Generic(GenericFamily::Emoji));
}

fn fallback_search_families(font_cx: &mut FontContext) -> Arc<[FamilyId]> {
    let mut families = Vec::new();
    let mut seen = HashSet::new();

    for generic_family in [
        GenericFamily::UiMonospace,
        GenericFamily::Monospace,
        GenericFamily::SystemUi,
        GenericFamily::Emoji,
    ] {
        for family_id in font_cx.collection.generic_families(generic_family) {
            if seen.insert(family_id) {
                families.push(family_id);
            }
        }
    }

    let mut family_names = font_cx
        .collection
        .family_names()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    family_names.sort_unstable_by_key(|family_name| family_name_sort_key(family_name));
    family_names.dedup();

    for family_name in family_names {
        let Some(family_id) = font_cx.collection.family_id(&family_name) else {
            continue;
        };
        if seen.insert(family_id) {
            families.push(family_id);
        }
    }

    Arc::from(families)
}

fn family_name_sort_key(family_name: &str) -> (bool, String) {
    (
        family_name.starts_with('.'),
        family_name.to_ascii_lowercase(),
    )
}

fn text_locale() -> Option<Language> {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(|key| env::var(key).ok())
        .and_then(|value| normalize_locale(&value))
        .and_then(|locale| Language::parse(&locale).ok())
}

fn normalize_locale(locale: &str) -> Option<String> {
    let locale = locale.trim();
    if locale.is_empty() || matches!(locale, "C" | "POSIX") {
        return None;
    }

    let locale = locale
        .split_once('.')
        .map(|(locale, _)| locale)
        .unwrap_or(locale);
    let locale = locale
        .split_once('@')
        .map(|(locale, _)| locale)
        .unwrap_or(locale);
    let locale = locale.replace('_', "-");

    (!locale.is_empty()).then_some(locale)
}

fn unicode_script_for_char(character: char) -> Script {
    let tag = character.script().to_opentype();
    let mut bytes = [
        (tag >> 24) as u8,
        (tag >> 16) as u8,
        (tag >> 8) as u8,
        tag as u8,
    ];
    bytes[0] = bytes[0].to_ascii_uppercase();
    bytes[1] = bytes[1].to_ascii_lowercase();
    bytes[2] = bytes[2].to_ascii_lowercase();
    bytes[3] = bytes[3].to_ascii_lowercase();
    Script::from_bytes(bytes)
}

fn script_uses_common_fallback(script: Script) -> bool {
    matches!(script, Script::COMMON | Script::INHERITED | Script::UNKNOWN)
}

fn fallback_key_for_common_char(character: char) -> Option<FallbackKey> {
    (!character.is_ascii()).then_some(FallbackKey::from(Script::COMMON))
}

#[cfg(test)]
mod tests {
    use super::{
        CellQuantization, FontOptions, FontSource, FontStack, FontVariant, Script, TextSystem,
        fallback_key_for_common_char, normalize_locale, script_uses_common_fallback,
        unicode_script_for_char,
    };

    fn fallback_script_for_char(character: char) -> Option<Script> {
        TextSystem::new(FontOptions::default())
            .fallback_key_for_char(character)
            .map(|key| key.script())
    }

    #[test]
    fn explicit_line_height_is_honored_without_forcing_the_default() {
        let natural = TextSystem::new(FontOptions::default()).metrics();
        let explicit = TextSystem::new(FontOptions {
            line_height: Some(64.0),
            ..FontOptions::default()
        })
        .metrics();

        assert_eq!(explicit.cell_height, 64.0);
        assert_ne!(natural.cell_height, explicit.cell_height);
    }

    #[test]
    fn cell_offsets_adjust_measured_terminal_cell_size() {
        let natural = TextSystem::new(FontOptions::default()).metrics();
        let adjusted = TextSystem::new(FontOptions {
            cell_width_offset: 2.0,
            cell_height_offset: 3.0,
            ..FontOptions::default()
        })
        .metrics();

        assert_eq!(adjusted.cell_width, natural.cell_width + 2.0);
        assert_eq!(adjusted.cell_height, natural.cell_height + 3.0);
    }

    #[test]
    fn cell_quantization_modes_quantize_the_measured_size() {
        let metrics_for = |quantization| {
            TextSystem::new(FontOptions {
                cell_quantization: quantization,
                ..FontOptions::default()
            })
            .metrics()
        };
        let fractional = metrics_for(CellQuantization::Fractional);
        let floored = metrics_for(CellQuantization::Floor);
        let rounded = metrics_for(CellQuantization::Round);

        assert_eq!(floored.cell_width, fractional.cell_width.floor().max(1.0));
        assert_eq!(floored.cell_height, fractional.cell_height.floor().max(1.0));
        assert_eq!(rounded.cell_width, fractional.cell_width.round().max(1.0));
        assert_eq!(rounded.cell_height, fractional.cell_height.round().max(1.0));
    }

    #[test]
    fn fractional_cells_grow_on_every_font_size_step() {
        let mut previous: Option<(f32, f32)> = None;
        for size in 8..=24 {
            let metrics = TextSystem::new(FontOptions {
                size: size as f32,
                cell_quantization: CellQuantization::Fractional,
                ..FontOptions::default()
            })
            .metrics();

            if let Some((width, height)) = previous {
                assert!(
                    metrics.cell_width > width,
                    "cell width must grow at size {size}: {width} -> {}",
                    metrics.cell_width
                );
                assert!(
                    metrics.cell_height > height,
                    "cell height must grow at size {size}: {height} -> {}",
                    metrics.cell_height
                );
            }
            previous = Some((metrics.cell_width, metrics.cell_height));
        }
    }

    #[test]
    fn glyph_offsets_are_tracked_and_scaled_with_font_options() {
        let options = FontOptions::default()
            .with_glyph_offset(0.5, -1.0)
            .scaled(2.0);
        let metrics = TextSystem::new(options).metrics();

        assert_eq!(metrics.glyph_offset_x, 1.0);
        assert_eq!(metrics.glyph_offset_y, -2.0);
        assert_eq!(metrics.logical(2.0).glyph_offset_x, 0.5);
        assert_eq!(metrics.logical(2.0).glyph_offset_y, -1.0);
    }

    #[test]
    fn non_ascii_common_script_characters_seed_fontique_fallbacks() {
        for character in [
            '│', '█', '≤', '€', '♔', '♕', '♖', '♗', '♘', '♙', '♚', '♛', '♜', '♝', '♞', '♟', '¹',
            '²', '¼', '½', '⁰', '₀', '①', '⓪', '❶', '…', '—', '“', '”', '\u{a0}', '', '\u{301}',
        ] {
            assert_eq!(
                fallback_script_for_char(character),
                Some(parley::fontique::Script::COMMON)
            );
        }
    }

    #[test]
    fn ordinary_common_characters_do_not_seed_fontique_fallbacks() {
        for character in ['!', '.', ',', ' ', '$', '+', '|', '0', '1'] {
            assert_eq!(fallback_script_for_char(character), None);
        }
    }

    #[test]
    fn non_common_scripts_still_seed_fontique_fallbacks() {
        assert_eq!(
            fallback_script_for_char('今'),
            Some(parley::fontique::Script::from_bytes(*b"Hani"))
        );
        assert_eq!(
            fallback_script_for_char('あ'),
            Some(parley::fontique::Script::from_bytes(*b"Hira"))
        );
        assert_eq!(
            fallback_script_for_char('한'),
            Some(parley::fontique::Script::from_bytes(*b"Hang"))
        );
    }

    #[test]
    fn fallback_key_uses_tracked_locale_for_script_specific_fallbacks() {
        let mut text = TextSystem::new(FontOptions::default());
        text.locale = Some(parley::fontique::Language::parse("ja-JP").unwrap());

        let hani = text.fallback_key_for_char('今').unwrap();
        assert_eq!(
            hani.script(),
            parley::fontique::Script::from_bytes(*b"Hani")
        );
        assert_eq!(hani.locale_str(), Some("ja"));

        let common = text.fallback_key_for_char('♚').unwrap();
        assert_eq!(common.script(), parley::fontique::Script::COMMON);
        assert_eq!(common.locale_str(), None);
    }

    #[test]
    fn unicode_script_extraction_stays_separate_from_fallback_selection() {
        assert_eq!(
            unicode_script_for_char('♚'),
            parley::fontique::Script::COMMON
        );
        assert!(script_uses_common_fallback(unicode_script_for_char('♚')));
        assert_eq!(
            fallback_key_for_common_char('♚').map(|key| key.script()),
            Some(parley::fontique::Script::COMMON)
        );

        assert_eq!(
            unicode_script_for_char('!'),
            parley::fontique::Script::COMMON
        );
        assert!(script_uses_common_fallback(unicode_script_for_char('!')));
        assert!(fallback_key_for_common_char('!').is_none());
    }

    #[test]
    fn locale_normalization_strips_encoding_and_uses_bcp47_separators() {
        assert_eq!(normalize_locale("ja_JP.UTF-8"), Some(String::from("ja-JP")));
        assert_eq!(
            normalize_locale("zh_Hans_CN@calendar=gregorian"),
            Some(String::from("zh-Hans-CN"))
        );
        assert_eq!(normalize_locale("C"), None);
    }

    #[test]
    fn invalid_bundled_font_data_is_ignored() {
        let invalid_font = &b"not a font"[..];
        let mut text = TextSystem::new(
            FontOptions::default().with_bundled_font_family("Bundled Test Font", invalid_font),
        );
        let metrics = text.metrics();

        assert_eq!(text.register_font_data(invalid_font), 0);
        assert_eq!(text.metrics(), metrics);
    }

    #[test]
    fn font_stack_tracks_explicit_variant_and_fallback_sources() {
        let mut text = TextSystem::new(
            FontOptions::default().with_font_stack(
                FontStack::new("Regular Mono")
                    .with_bold("Bold Mono")
                    .with_italic("Italic Mono")
                    .with_bold_italic("Bold Italic Mono")
                    .with_fallback("Emoji Fallback"),
            ),
        );

        text.set_font_stack(
            FontStack::new(FontSource::family("Runtime Regular"))
                .with_italic(FontSource::family("Runtime Italic")),
        );

        assert!(matches!(
            text.options.fonts.italic,
            Some(FontSource::Family(ref family)) if family == "Runtime Italic"
        ));
        assert!(!text.font_families.get(FontVariant::Italic).is_empty());
    }
}
