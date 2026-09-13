//! Independent, unclipped text reference for the GPU fidelity harness.
//!
//! This uses Bevy's font rasterizer, but none of the terminal renderer's
//! measurement probes, fitting, geometry, atlas copy, or clipping functions.

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    text::{
        ComputedTextBlock, FontAtlasSet, FontCx, FontStyle, FontWeight, LayoutCx, LetterSpacing,
        LineHeight, ScaleCx, TextBounds, TextLayoutInfo, TextPipeline,
    },
};
use bevy_terminal_ratatui::prelude::{StyleFlags, TerminalCell, TerminalRenderConfig};

#[derive(SystemParam)]
pub struct Rasterizer<'w> {
    fonts: Res<'w, Assets<Font>>,
    pipeline: ResMut<'w, TextPipeline>,
    font_cx: ResMut<'w, FontCx>,
    layout_cx: ResMut<'w, LayoutCx>,
    scale_cx: ResMut<'w, ScaleCx>,
    atlases: ResMut<'w, FontAtlasSet>,
    images: ResMut<'w, Assets<Image>>,
}

/// One shaped run, composed into an unbounded premultiplied linear RGBA image. `origin` is relative
/// to the line origin, so negative bearings and accents are retained.
pub struct Reference {
    pub origin: IVec2,
    pub size: UVec2,
    pub rgba: Vec<Vec4>,
    pub color_glyphs: usize,
    pub glyph_count: usize,
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
    pub faces: Vec<(u64, u32)>,
    pub supported: bool,
}

impl Rasterizer<'_> {
    /// Center the configured faces' typographic pixel enclosure on a shared
    /// baseline. This expectation comes only from raw font metrics, never from
    /// captured ink or the renderer's measurement/fitting functions.
    pub fn baseline(
        &mut self,
        config: &TerminalRenderConfig,
        font_size: f32,
        height: f32,
    ) -> Result<i32, String> {
        let mut above = 0.0_f32;
        let mut below = 0.0_f32;
        for flags in [
            StyleFlags::empty(),
            StyleFlags::BOLD,
            StyleFlags::ITALIC,
            StyleFlags::BOLD | StyleFlags::ITALIC,
        ] {
            let mut cell = TerminalCell::new("0");
            cell.style.flags = flags;
            let run = self.shape(&cell, config, font_size, height)?;
            let baseline = (run.baseline + 0.5).floor();
            above = above.max(baseline - (run.baseline - run.ascent).floor());
            below = below.max((run.baseline + run.descent).ceil() - baseline);
        }
        Ok(((height + above - below) / 2.0 + 0.5).floor() as i32)
    }

    pub fn shape(
        &mut self,
        cell: &TerminalCell,
        config: &TerminalRenderConfig,
        font_size: f32,
        line_height: f32,
    ) -> Result<Reference, String> {
        let bold = cell.style.has(StyleFlags::BOLD);
        let italic = cell.style.has(StyleFlags::ITALIC);
        // Fixtures use either four explicit faces or one regular source with
        // synthesis enabled. Reject partial face chains instead of reproducing
        // the renderer's font-resolution policy in the oracle.
        let faces = &config.font;
        let explicit = [&faces.bold, &faces.italic, &faces.bold_italic];
        let source = if explicit.iter().all(|face| face.is_none()) && faces.synthesize {
            &faces.regular
        } else if explicit.iter().all(|face| face.is_some()) {
            match (bold, italic) {
                (false, false) => &faces.regular,
                (true, false) => faces.bold.as_ref().unwrap(),
                (false, true) => faces.italic.as_ref().unwrap(),
                (true, true) => faces.bold_italic.as_ref().unwrap(),
            }
        } else {
            return Err(
                "oracle requires four explicit faces or a regular-only synthesized fixture".into(),
            );
        };
        let font = TextFont {
            font: source.clone(),
            font_size: font_size.into(),
            weight: if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..default()
        };
        let mut computed = ComputedTextBlock::default();
        self.pipeline
            .update_buffer(
                &self.fonts,
                std::iter::once((
                    Entity::PLACEHOLDER,
                    0,
                    cell.symbol(),
                    &font,
                    Color::WHITE,
                    LineHeight::Px(line_height),
                    LetterSpacing::default(),
                )),
                LineBreak::NoWrap,
                Justify::Left,
                TextBounds::UNBOUNDED,
                1.0,
                &mut computed,
                &mut self.font_cx,
                &mut self.layout_cx,
                Vec2::splat(4096.0),
                20.0,
            )
            .map_err(|e| e.to_string())?;
        let mut layout = TextLayoutInfo::default();
        self.pipeline
            .update_text_layout_info(
                &mut layout,
                &mut self.atlases,
                &mut self.images,
                &mut computed,
                &mut self.scale_cx,
                TextBounds::UNBOUNDED,
                Justify::Left,
                config.raster.hinting,
            )
            .map_err(|e| e.to_string())?;
        let line = computed.buffer().lines().next().ok_or("missing line")?;
        let metrics = line.metrics();
        let mut reference = Reference {
            origin: IVec2::ZERO,
            size: UVec2::ZERO,
            rgba: Vec::new(),
            color_glyphs: layout
                .glyphs
                .iter()
                .filter(|g| !g.atlas_info.is_alpha_mask)
                .count(),
            glyph_count: layout.glyphs.len(),
            baseline: metrics.baseline,
            ascent: metrics.ascent,
            descent: metrics.descent,
            supported: line.runs().all(|run| {
                run.clusters()
                    .all(|cluster| cluster.glyphs().all(|glyph| glyph.id != 0))
            }),
            faces: line
                .runs()
                .map(|run| (run.font().data.id(), run.font().index))
                .collect(),
        };
        if layout.glyphs.is_empty() {
            return Err("no rasterized glyphs".into());
        }
        // Integer translation with ties toward +infinity preserves identical
        // rasterization phase on either side of zero. No terminal fit is used.
        let origins: Vec<IVec2> = layout
            .glyphs
            .iter()
            .map(|glyph| {
                (glyph.position - glyph.atlas_info.rect.size() / 2.0 + Vec2::splat(0.5))
                    .floor()
                    .as_ivec2()
            })
            .collect();
        let min = origins.iter().copied().reduce(IVec2::min).unwrap();
        let max = layout
            .glyphs
            .iter()
            .zip(&origins)
            .map(|(glyph, origin)| *origin + glyph.atlas_info.rect.size().as_ivec2())
            .reduce(IVec2::max)
            .unwrap();
        reference.origin = min;
        reference.size = (max - min).as_uvec2();
        reference
            .rgba
            .resize((reference.size.x * reference.size.y) as usize, Vec4::ZERO);
        for (glyph, origin) in layout.glyphs.iter().zip(origins) {
            let image = self
                .images
                .get(glyph.atlas_info.texture)
                .ok_or("missing glyph atlas")?;
            if image.texture_descriptor.format
                != bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb
            {
                return Err("reference requires Bevy's RGBA8 sRGB glyph atlas".into());
            }
            let data = image.data.as_ref().ok_or("unreadable glyph atlas")?;
            let rect = glyph.atlas_info.rect;
            let size = rect.size().as_uvec2();
            for y in 0..size.y {
                for x in 0..size.x {
                    let source = ((rect.min.y as u32 + y) * image.width() + rect.min.x as u32 + x)
                        as usize
                        * 4;
                    // Bevy stores mask glyphs as white RGB plus coverage and
                    // CBDT color glyphs as straight RGBA. The sRGB atlas decodes
                    // RGB before the shader's straight-alpha blend.
                    let color = LinearRgba::from(Srgba::from_u8_array(
                        data[source..source + 4].try_into().unwrap(),
                    ));
                    let sample = Vec4::new(
                        color.red * color.alpha,
                        color.green * color.alpha,
                        color.blue * color.alpha,
                        color.alpha,
                    );
                    let target = (origin - min).as_uvec2() + UVec2::new(x, y);
                    let dest =
                        &mut reference.rgba[(target.y * reference.size.x + target.x) as usize];
                    *dest = sample + *dest * (1.0 - sample.w);
                }
            }
        }
        Ok(reference)
    }
}

impl Reference {
    /// Compares full RGB coverage, not just a count or thresholded silhouette.
    /// One sRGB code value accommodates CPU/GPU UNORM conversion rounding.
    pub fn differences(
        &self,
        actual: &[[u8; 3]],
        size: UVec2,
        shift: IVec2,
        background: [u8; 3],
    ) -> usize {
        assert_eq!(actual.len(), (size.x * size.y) as usize);
        actual
            .iter()
            .enumerate()
            .filter(|(index, actual)| {
                let p = IVec2::new(
                    (*index as u32 % size.x) as i32,
                    (*index as u32 / size.x) as i32,
                );
                actual
                    .iter()
                    .zip(self.pixel(p - shift, background))
                    .any(|(a, b)| a.abs_diff(b) > 1)
            })
            .count()
    }

    /// An oversized run must still be an unchanged crop of its source pixels.
    /// Exhaustive translation search is intentionally independent of production
    /// fitting's per-column coverage scoring. Vertical placement is fixed by
    /// configured typographic metrics, never fitted per character.
    pub fn matching_crop(
        &self,
        actual: &[[u8; 3]],
        size: UVec2,
        vertical_shift: i32,
        background: [u8; 3],
    ) -> Option<IVec2> {
        let (min, max) = self.ink_bounds()?;
        if let Some(x) = self.fitting_shift(size.x) {
            let shift = IVec2::new(x, vertical_shift);
            return (self.differences(actual, size, shift, background) == 0).then_some(shift);
        }
        let a = -min.x;
        let b = size.x as i32 - max.x;
        (a.min(b)..=a.max(b))
            .map(|x| IVec2::new(x, vertical_shift))
            .find(|shift| self.differences(actual, size, *shift, background) == 0)
    }

    /// Preserve bearings when they fit; otherwise the minimum translation
    /// that encloses all source ink. Oversized runs have no such translation.
    pub fn fitting_shift(&self, width: u32) -> Option<i32> {
        let (min, max) = self.ink_bounds()?;
        if max.x - min.x > width as i32 {
            return None;
        }
        Some(0.clamp(-min.x, width as i32 - max.x))
    }

    pub fn ink_bounds(&self) -> Option<(IVec2, IVec2)> {
        let mut min = IVec2::splat(i32::MAX);
        let mut max = IVec2::splat(i32::MIN);
        for y in 0..self.size.y {
            for x in 0..self.size.x {
                if self.rgba[(y * self.size.x + x) as usize].w > 0.0 {
                    let p = self.origin + UVec2::new(x, y).as_ivec2();
                    min = min.min(p);
                    max = max.max(p + IVec2::ONE);
                }
            }
        }
        (min.x < max.x).then_some((min, max))
    }

    pub fn pixel(&self, position: IVec2, background: [u8; 3]) -> [u8; 3] {
        let p = position - self.origin;
        let ink = if p.x >= 0 && p.y >= 0 && p.x < self.size.x as i32 && p.y < self.size.y as i32 {
            self.rgba[(p.y as u32 * self.size.x + p.x as u32) as usize]
        } else {
            Vec4::ZERO
        };
        if ink.w == 0.0 {
            return background;
        }
        let bg = LinearRgba::from(Srgba::rgb(
            f32::from(background[0]) / 255.0,
            f32::from(background[1]) / 255.0,
            f32::from(background[2]) / 255.0,
        ));
        let srgb = Srgba::from(LinearRgba::rgb(
            ink.x + bg.red * (1.0 - ink.w),
            ink.y + bg.green * (1.0 - ink.w),
            ink.z + bg.blue * (1.0 - ink.w),
        ))
        .to_u8_array();
        [srgb[0], srgb[1], srgb[2]]
    }
}

/// Writes a diagnostic image without involving the terminal renderer.
pub fn save_png(path: &std::path::Path, size: UVec2, pixels: Vec<u8>) {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    )
    .try_into_dynamic()
    .expect("RGBA8 image")
    .save(path)
    .expect("save diagnostic PNG");
}

/// Native pixels and an 8x nearest-neighbor detail, for visual review.
pub fn save_detail(path: &std::path::Path, size: UVec2, pixels: Vec<u8>) {
    let zoom = size * 8;
    let mut expanded = Vec::with_capacity((zoom.x * zoom.y * 4) as usize);
    for y in 0..zoom.y {
        for x in 0..zoom.x {
            let index = (((y / 8) * size.x + x / 8) * 4) as usize;
            expanded.extend_from_slice(&pixels[index..index + 4]);
        }
    }
    save_png(&path.with_extension("8x.png"), zoom, expanded);
    save_png(path, size, pixels);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(origin: IVec2, size: UVec2, alpha: &[f32]) -> Reference {
        Reference {
            origin,
            size,
            rgba: alpha.iter().map(|a| Vec4::splat(*a)).collect(),
            color_glyphs: 0,
            glyph_count: 1,
            baseline: 2.0,
            ascent: 2.0,
            descent: 1.0,
            faces: Vec::new(),
            supported: true,
        }
    }

    fn render(reference: &Reference, size: UVec2, shift: IVec2) -> Vec<[u8; 3]> {
        (0..size.y)
            .flat_map(|y| {
                (0..size.x).map(move |x| {
                    reference.pixel(UVec2::new(x, y).as_ivec2() - shift, [40, 44, 64])
                })
            })
            .collect()
    }

    #[test]
    fn negative_bearings_and_faint_pixels_survive_the_reference() {
        let glyph = reference(
            IVec2::new(-1, -2),
            UVec2::new(3, 1),
            &[1.0, 0.0, 1.0 / 255.0],
        );
        assert_eq!(
            glyph.ink_bounds(),
            Some((IVec2::new(-1, -2), IVec2::new(2, -1)))
        );
        assert_eq!(glyph.pixel(IVec2::new(-1, -2), [40, 44, 64]), [255; 3]);
        assert_ne!(glyph.pixel(IVec2::new(1, -2), [40, 44, 64]), [40, 44, 64]);
    }

    #[test]
    fn equal_ink_counts_do_not_hide_changed_shapes_or_missing_pixels() {
        let glyph = reference(IVec2::ZERO, UVec2::new(2, 2), &[1.0, 0.0, 1.0, 1.0]);
        let size = UVec2::new(3, 2);
        let mut pixels = render(&glyph, size, IVec2::ZERO);
        assert_eq!(
            glyph.matching_crop(&pixels, size, 0, [40, 44, 64]),
            Some(IVec2::ZERO)
        );
        pixels.swap(0, 1); // Three ink pixels remain, but the shape changes.
        assert!(
            glyph
                .matching_crop(&pixels, size, 0, [40, 44, 64])
                .is_none()
        );
        pixels[1] = [40, 44, 64];
        assert!(
            glyph
                .matching_crop(&pixels, size, 0, [40, 44, 64])
                .is_none()
        );
    }

    #[test]
    fn oversized_crops_preserve_pixels_and_the_shared_vertical_position() {
        let glyph = reference(
            IVec2::new(-1, -1),
            UVec2::new(4, 2),
            &[0.2, 0.4, 0.6, 0.8, 1.0, 0.0, 0.3, 1.0],
        );
        let size = UVec2::new(3, 3);
        let pixels = render(&glyph, size, IVec2::new(0, 1));
        assert!(
            glyph
                .matching_crop(&pixels, size, 1, [40, 44, 64])
                .is_some()
        );
        assert!(
            glyph
                .matching_crop(&pixels, size, 0, [40, 44, 64])
                .is_none()
        );
    }

    #[test]
    fn color_tolerance_is_one_code_value_not_a_silhouette_threshold() {
        let glyph = reference(IVec2::ZERO, UVec2::ONE, &[0.5]);
        let mut pixels = render(&glyph, UVec2::ONE, IVec2::ZERO);
        pixels[0][0] += 1;
        assert_eq!(
            glyph.differences(&pixels, UVec2::ONE, IVec2::ZERO, [40, 44, 64]),
            0
        );
        pixels[0][0] += 1;
        assert_eq!(
            glyph.differences(&pixels, UVec2::ONE, IVec2::ZERO, [40, 44, 64]),
            1
        );
    }

    #[test]
    fn color_reference_preserves_hue_and_blends_in_linear_space() {
        let mut glyph = reference(IVec2::ZERO, UVec2::ONE, &[0.5]);
        glyph.rgba[0] = Vec4::new(0.5, 0.0, 0.0, 0.5);
        // Half transparent red over opaque blue: linear 0.5 encodes to 188.
        assert_eq!(glyph.pixel(IVec2::ZERO, [0, 0, 255]), [188, 0, 188]);
        assert_ne!(glyph.pixel(IVec2::ZERO, [0, 0, 255]), [128, 0, 128]);
        assert_eq!(glyph.pixel(IVec2::ONE, [4, 8, 12]), [4, 8, 12]);
        assert!(glyph.differences(&[[188, 188, 255]], UVec2::ONE, IVec2::ZERO, [0, 0, 255]) > 0);
    }

    #[test]
    fn partial_face_chains_are_rejected_instead_of_emulating_production_resolution() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_asset::<Image>();
        let mut config = TerminalRenderConfig::default();
        config.font.bold = Some(bevy::text::FontSource::Monospace);
        let mut state = bevy::ecs::system::SystemState::<Rasterizer>::new(app.world_mut());
        let mut rasterizer = state.get_mut(app.world_mut()).unwrap();
        assert!(
            rasterizer
                .shape(&TerminalCell::new("W"), &config, 18.0, 24.0)
                .err()
                .is_some_and(|error| error.contains("four explicit faces"))
        );
    }

    #[test]
    fn font_line_metrics_are_independent_of_procedural_block_cell_height() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_asset::<Image>();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                include_bytes!("../../assets/fonts/cascadia-mono/CascadiaMono-Regular.ttf")
                    .to_vec(),
            ));
        app.update();
        let config = TerminalRenderConfig {
            font: bevy_terminal_ratatui::prelude::FontFaces::regular(handle),
            ..default()
        };
        let mut state = bevy::ecs::system::SystemState::<Rasterizer>::new(app.world_mut());
        let mut rasterizer = state.get_mut(app.world_mut()).unwrap();
        let small = rasterizer
            .shape(&TerminalCell::new("█"), &config, 18.77, 23.0)
            .unwrap();
        let large = rasterizer
            .shape(&TerminalCell::new("█"), &config, 18.77, 200.0)
            .unwrap();
        assert_eq!(small.ascent, large.ascent);
        assert_eq!(small.descent, large.descent);
        assert_eq!(small.rgba, large.rgba);
        assert!(
            large.size.y < 30,
            "raw block raster is not a 200px cell fill"
        );
        assert!(large.baseline > small.baseline);
    }
}
