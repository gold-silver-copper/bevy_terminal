//! Required fallback, color, placement, and changing-geometry GPU checks.
//! Runs headlessly with pinned bundled fonts and no host font discovery.
//! `--text-presentation` also requires VS15 text fallback. This strict capability
//! probe currently fails in Parley 0.9 and is reported separately from the
//! supported required suite; see the coverage report for the upstream evidence.
#[path = "common/fidelity_oracle.rs"]
mod fidelity_oracle;

use bevy::{
    app::ScheduleRunnerPlugin,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
    },
    winit::WinitPlugin,
};
use bevy_terminal_ratatui::prelude::*;
use fidelity_oracle::Rasterizer;
use std::sync::{Arc, Mutex};

const BACKGROUNDS: [[u8; 3]; 2] = [[24, 41, 60], [89, 38, 25]];
const CJK: &[&str] = &[
    "汉", "字", "日", "本", "語", "한", "글", "｜", "ｆ", "ｕ", "ｌ", "ｗ", "ｉ", "ｄ", "ｔ", "ｈ",
];
const MARKS: &[&str] = &[
    "Ĳ",
    "ĳ",
    "ῃ",
    "ῳ",
    "ῷ",
    "E\u{300}\u{306}",
    "a\u{328}\u{308}",
    "e\u{30a}",
    "o\u{308}\u{304}",
    "u\u{30c}\u{307}",
    "x\u{302}",
    "Ǚ",
    "ǟ",
    "ǭ",
    "ȫ",
    "ṩ",
    "Ệ",
    "ệ",
];
const COLOR: &[&str] = &[
    "🙂",
    "🚀",
    "🎉",
    "👍🏽",
    "🇺🇸",
    "❤\u{fe0f}",
    "♥\u{fe0f}",
    "👩\u{200d}💻",
    "👨\u{200d}👩\u{200d}👧\u{200d}👦",
    "☕\u{fe0f}",
    "1\u{fe0f}\u{20e3}",
    "🏳\u{fe0f}\u{200d}🌈",
];
const TEXT_PRESENTATION: &[&str] = &["❤\u{fe0e}", "♥\u{fe0e}", "☕\u{fe0e}"];

#[derive(Component)]
struct Case {
    name: &'static str,
    required_font: u64,
    color: bool,
    palette: u16,
    samples: &'static [&'static str],
    scale: f32,
    scale_index: usize,
    face_ids: [u64; 4],
    last: Option<(Handle<Image>, TerminalGeometry, Vec<u8>)>,
}
#[derive(Resource, Default, Clone)]
struct Captures(Arc<Mutex<Vec<Capture>>>);
type Capture = (Entity, Vec<u8>);
#[derive(Resource)]
struct Output(String);
#[derive(Resource, Default)]
struct Frame {
    ticks: u32,
    phase: usize,
}
#[derive(Resource)]
struct Replacement(Handle<Font>, u64);
const PHASES: &[&str] = &[
    "roomy",
    "same-pixels",
    "fractional-natural",
    "compact",
    "fractional-fixed",
    "font-switch",
    "content-replace",
    "idle",
];
const MIXED: &[&str] = &["W", "g", "Á", "j", "A", "W", "g", "j"];
fn flags(index: usize) -> StyleFlags {
    [
        StyleFlags::empty(),
        StyleFlags::BOLD,
        StyleFlags::ITALIC,
        StyleFlags::BOLD | StyleFlags::ITALIC,
    ][index % 4]
}
fn paint(surface: &TerminalSurface, samples: &[&str], name: &str, replace: bool, palette: u16) {
    surface.update(|writer| {
        for y in 0..samples.len() as u16 + 2 {
            for x in 0..8 {
                let [r, g, b] = BACKGROUNDS[((x + y + palette) % 2) as usize];
                writer.set_cell(
                    (x, y),
                    &TerminalCell::new(" ").with_style(TerminalStyle {
                        foreground: TerminalColor::Rgb(255, 255, 255),
                        background: TerminalColor::Rgb(r, g, b),
                        ..default()
                    }),
                );
            }
        }
        for (index, symbol) in samples.iter().enumerate() {
            if replace && index % 2 == 1 {
                continue;
            }
            let symbol = if replace { "a\u{301}" } else { symbol };
            let y = index as u16 + 1;
            let [r, g, b] = BACKGROUNDS[((2 + y + palette) % 2) as usize];
            let style = TerminalStyle {
                foreground: if name.starts_with("color") && !replace {
                    TerminalColor::Rgb(30, 240, 90)
                } else {
                    TerminalColor::Rgb(255, 255, 255)
                },
                background: TerminalColor::Rgb(r, g, b),
                flags: if name == "mixed" && !replace {
                    flags(index)
                } else {
                    StyleFlags::empty()
                },
                ..default()
            };
            let span = if !replace && (name == "cjk" || name.starts_with("color")) {
                2
            } else {
                1
            };
            writer.set_cell((2, y), &TerminalCell::wide(symbol, span).with_style(style));
        }
    });
}

fn add_font(app: &mut App, bytes: &[u8]) -> (Handle<Font>, u64) {
    let font = Font::from_bytes(bytes.to_vec());
    let id = font.data.id();
    (app.world_mut().resource_mut::<Assets<Font>>().add(font), id)
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let output = args
        .windows(2)
        .find(|a| a[0] == "--output")
        .map_or("target/glyph-coverage", |a| a[1].as_str())
        .to_owned();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins((
        ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(1)),
        TerminalPlugin,
    ))
    .init_resource::<Captures>()
    .init_resource::<Frame>()
    .insert_resource(Output(output));
    app.world_mut()
        .resource_mut::<bevy::text::FontCx>()
        .collection = fontique::Collection::new(fontique::CollectionOptions {
        system_fonts: false,
        ..default()
    });
    let (primary, primary_id) = add_font(
        &mut app,
        include_bytes!("../assets/fonts/fidelity/FidelityASCII.ttf"),
    );
    let mut fallbacks = Vec::new();
    for (bytes, scripts) in [
        (
            include_bytes!("../assets/fonts/fidelity/FidelityCJK.otf").as_slice(),
            &["Hani", "Hang", "Hira", "Kana"][..],
        ),
        (
            include_bytes!("../assets/fonts/dejavu-sans-mono/DejaVuSansMono.ttf").as_slice(),
            &["Latn", "Grek", "Cyrl"][..],
        ),
        (
            include_bytes!("../assets/fonts/fidelity/FidelityColorEmoji.ttf").as_slice(),
            &[][..],
        ),
    ] {
        let font = Font::from_bytes(bytes.to_vec());
        let blob_id = font.data.id();
        let mut cx = app.world_mut().resource_mut::<bevy::text::FontCx>();
        let families = cx.collection.register_fonts(font.data, None);
        let family = families[0].0;
        for script in scripts {
            assert!(cx.collection.set_fallbacks(
                fontique::Script::from_str_unchecked(script),
                std::iter::once(family)
            ));
        }
        fallbacks.push((family, blob_id));
    }
    // Full-width Latin resolves through Latn, so include CJK after Latin fallback.
    app.world_mut()
        .resource_mut::<bevy::text::FontCx>()
        .collection
        .append_fallbacks(
            fontique::Script::from_str_unchecked("Latn"),
            std::iter::once(fallbacks[0].0),
        );
    app.world_mut()
        .resource_mut::<bevy::text::FontCx>()
        .set_emoji_family("Fidelity Color Emoji")
        .unwrap();
    let (bold, bold_id) = add_font(
        &mut app,
        include_bytes!("../assets/fonts/dejavu-sans-mono/DejaVuSansMono-Bold.ttf"),
    );
    let (italic, italic_id) = add_font(
        &mut app,
        include_bytes!("../assets/fonts/hack/Hack-Italic.ttf"),
    );
    let (bold_italic, bold_italic_id) = add_font(
        &mut app,
        include_bytes!("../assets/fonts/source-code-pro/SourceCodePro-BoldIt.ttf"),
    );
    let (replacement, replacement_id) = add_font(
        &mut app,
        include_bytes!("../assets/fonts/hack/Hack-Regular.ttf"),
    );
    app.insert_resource(Replacement(replacement, replacement_id));
    let scales = args
        .windows(2)
        .find(|a| a[0] == "--scale")
        .map_or(vec![1., 1.5, 2., 3.], |a| {
            vec![a[1].parse::<f32>().expect("numeric scale")]
        });
    for (scale_index, scale) in scales.into_iter().enumerate() {
        for (name, samples, font, color, palette) in [
            ("cjk", CJK, fallbacks[0].1, false, 0),
            ("marks", MARKS, fallbacks[1].1, false, 0),
            ("presentation", TEXT_PRESENTATION, fallbacks[1].1, false, 0),
            ("color", COLOR, fallbacks[2].1, true, 0),
            ("color-alt", COLOR, fallbacks[2].1, true, 1),
            ("mixed", MIXED, 0, false, 0),
            ("blocks", &["█"][..], 0, false, 0),
        ] {
            if name == "presentation" && !args.iter().any(|arg| arg == "--text-presentation") {
                continue;
            }
            let surface = TerminalSurface::new((8, samples.len() as u16 + 2));
            paint(&surface, samples, name, false, palette);
            let mut faces = FontFaces::regular(primary.clone());
            if name == "mixed" {
                faces.bold = Some(bold.clone().into());
                faces.italic = Some(italic.clone().into());
                faces.bold_italic = Some(bold_italic.clone().into());
            }
            app.world_mut().spawn((
                TerminalRenderer::new(surface),
                TerminalRenderConfig {
                    font: faces,
                    sizing: TerminalSizing::Fixed {
                        cell_size: Vec2::new(14., 40.),
                        font_size: 18.,
                    },
                    raster: RasterConfig { scale, ..default() },
                    cursor: CursorConfig {
                        blink_hz: None,
                        ..default()
                    },
                    ..default()
                },
                Case {
                    name,
                    required_font: font,
                    color,
                    palette,
                    samples,
                    scale,
                    scale_index,
                    face_ids: [primary_id, bold_id, italic_id, bold_italic_id],
                    last: None,
                },
            ));
        }
    }
    app.add_systems(
        Update,
        (readbacks, check).chain().after(TerminalSystems::Sync),
    );
    app.run();
}

fn readbacks(
    mut commands: Commands,
    textures: Query<(Entity, &TerminalTexture), With<Case>>,
    mut started: Local<Vec<Entity>>,
) {
    for (entity, texture) in &textures {
        if texture.measured().is_none() || started.contains(&entity) {
            continue;
        }
        started.push(entity);
        commands
            .spawn(Readback::texture(texture.image.clone()))
            .observe(
                move |readback: On<ReadbackComplete>, captures: Res<Captures>| {
                    let mut captures = captures.0.lock().unwrap();
                    if let Some(capture) = captures.iter_mut().find(|(e, _)| *e == entity) {
                        capture.1.clone_from(&readback.data);
                    } else {
                        captures.push((entity, readback.data.clone()));
                    }
                },
            );
    }
}

fn check(
    mut frame: ResMut<Frame>,
    captures: Res<Captures>,
    mut cases: Query<(
        Entity,
        &mut Case,
        &TerminalRenderer,
        &TerminalTexture,
        &mut TerminalRenderConfig,
        &TerminalStats,
    )>,
    mut oracle: Rasterizer,
    output: Res<Output>,
    replacement: Res<Replacement>,
    mut exit: MessageWriter<AppExit>,
) {
    frame.ticks += 1;
    let captures = captures.0.lock().unwrap();
    let current_sizes = cases.iter().all(|(entity, _, _, texture, _, _)| {
        texture.measured().is_some_and(|geometry| {
            let size = geometry.size();
            let bytes = (size.x as usize * 4).next_multiple_of(256) * size.y as usize;
            captures
                .iter()
                .any(|(e, data)| *e == entity && data.len() == bytes)
        })
    });
    if frame.ticks < 30 || !current_sizes {
        assert!(frame.ticks < 600, "coverage readback timed out");
        return;
    }
    let out = std::path::Path::new(&output.0).join(PHASES[frame.phase]);
    std::fs::create_dir_all(&out).unwrap();
    let mut failures = Vec::new();
    let mut rows = String::from(
        "case\tscale\trow\tsymbol\tclassification\tfont_ids\tbaseline\tphysical_cell\tphysical_font\tshift\n",
    );
    for (entity, mut case, renderer, texture, mut config, stats) in &mut cases {
        let data = &captures.iter().find(|(e, _)| *e == entity).unwrap().1;
        let geometry = texture.measured().unwrap();
        let size = geometry.size();
        let cell = (geometry.cell_size() * geometry.raster_scale())
            .round()
            .as_uvec2();
        let font_size = geometry.font_size() * geometry.raster_scale();
        if let Some((image, last, _)) = &case.last {
            assert_eq!(
                texture.image, *image,
                "stable image handle across {}",
                PHASES[frame.phase]
            );
            if frame.phase == 1 {
                assert_eq!(geometry.size(), last.size());
                assert_ne!(geometry.logical_size(), last.logical_size());
            }
            if frame.phase >= 6 {
                assert_eq!(geometry, last, "content must not change geometry");
            }
            if frame.phase == 7 {
                assert_eq!(*stats, TerminalStats::default());
            }
        }
        let stride = data.len() / size.y as usize;
        assert!(
            stride >= size.x as usize * 4 && data.len() % size.y as usize == 0,
            "current readback geometry"
        );
        let raw: Vec<u8> = data
            .chunks_exact(stride)
            .flat_map(|r| r[..size.x as usize * 4].iter().copied())
            .collect();
        if frame.phase == 7 {
            assert_eq!(raw, case.last.as_ref().unwrap().2, "idle image changed");
        }
        // GPU row padding is not part of the image and need not stay identical.
        case.last = Some((texture.image.clone(), geometry.clone(), raw.clone()));
        assert!(
            raw.chunks_exact(4).all(|pixel| pixel[3] == 255),
            "opaque backgrounds remain opaque"
        );
        fidelity_oracle::save_png(
            &out.join(format!("{}-{}x.png", case.name, case.scale)),
            size,
            raw,
        );
        let baseline = oracle.baseline(&config, font_size, cell.y as f32).unwrap();
        let snapshot = renderer.surface().snapshot();
        for (index, symbol) in case.samples.iter().enumerate() {
            let y = index as u16 + 1;
            let expected = if frame.phase >= 6 {
                if index % 2 == 1 { " " } else { "a\u{301}" }
            } else {
                symbol
            };
            let anchor = snapshot.cell((2, y)).unwrap();
            let span = if frame.phase < 6 && (case.name == "cjk" || case.color) {
                2
            } else {
                1
            };
            if anchor.symbol() != expected || anchor.columns() != span {
                failures.push(format!(
                    "{} row {y}: required symbol/occupancy lost",
                    case.name
                ));
            }
            let next = snapshot.cell((3, y)).unwrap();
            if next.is_continuation() != (span == 2) || (span == 2 && next.style != anchor.style) {
                failures.push(format!(
                    "{} row {y}: continuation style/occupancy differs",
                    case.name
                ));
            }
        }
        for y in 0..snapshot.size().height {
            for x in 0..snapshot.size().width {
                let source = snapshot.cell((x, y)).unwrap();
                if source.is_continuation() {
                    continue;
                }
                let TerminalColor::Rgb(r, g, b) = source.style.background else {
                    panic!("explicit test background")
                };
                let bg = [r, g, b];
                let span = UVec2::new(cell.x * u32::from(source.columns()), cell.y);
                let actual: Vec<[u8; 3]> = (0..span.y)
                    .flat_map(|dy| {
                        (0..span.x).map(move |dx| {
                            let start = (u32::from(y) * cell.y + dy) as usize * stride
                                + (u32::from(x) * cell.x + dx) as usize * 4;
                            data[start..start + 3].try_into().unwrap()
                        })
                    })
                    .collect();
                if source.symbol() == " " {
                    if actual.iter().any(|p| *p != bg) {
                        failures.push(format!(
                            "{} blank ({x},{y}) has stale/neighbor ink",
                            case.name
                        ));
                    }
                    continue;
                }
                if source.symbol() == "█" {
                    if actual.iter().any(|p| *p != [255; 3]) {
                        failures.push("procedural block did not fill fixed cell".into());
                    }
                    rows.push_str(&format!("{}\t{}\t{y}\tblock\tprocedural-fill\t-\t{baseline}\t{cell:?}\t{font_size}\t-\n",case.name,case.scale));
                    continue;
                }
                let reference = match oracle.shape(source, &config, font_size, cell.y as f32) {
                    Ok(reference) => reference,
                    Err(error) => {
                        failures.push(format!(
                            "{} {:?}: required raster unavailable: {error}",
                            case.name,
                            source.symbol()
                        ));
                        rows.push_str(&format!("{}\t{}\t{y}\t{:?}\trequired-raster-unavailable\t-\t{baseline}\t{cell:?}\t{font_size}\t-\n",case.name,case.scale,source.symbol()));
                        continue;
                    }
                };
                let required = if case.name == "mixed" {
                    case.face_ids[(y as usize - 1) % 4]
                } else {
                    case.required_font
                };
                let allowed = |id: u64| id == required || (frame.phase >= 5 && id == replacement.1);
                let font_ok = reference.faces.iter().all(|(id, _)| allowed(*id));
                if !reference.supported || !font_ok {
                    failures.push(format!(
                        "{} {:?}: required font missing, faces {:?}, required {required}",
                        case.name,
                        source.symbol(),
                        reference.faces
                    ));
                }
                let color_ok = !(case.color && frame.phase < 6)
                    || !(reference.color_glyphs == 0
                        || reference.glyph_count != 1
                        || !reference.rgba.iter().any(|p| p.w > 0. && p.w < 1.));
                if !color_ok {
                    failures.push(format!(
                        "color {:?}: requires one shaped color glyph, got {} glyphs, {} color",
                        source.symbol(),
                        reference.glyph_count,
                        reference.color_glyphs
                    ));
                }
                // Every face shares the baseline derived from configured font
                // metrics, including when compact cells deliberately crop ink.
                let dy = baseline - reference.baseline.round() as i32;
                let Some((min, max)) = reference.ink_bounds() else {
                    failures.push("empty required raster".into());
                    continue;
                };
                let placement = reference
                    .fitting_shift(span.x)
                    .map(|dx| IVec2::new(dx, dy))
                    .or_else(|| reference.matching_crop(&actual, span, dy, bg));
                let shift = placement.unwrap_or(IVec2::new(0, dy));
                let dx = shift.x;
                let differences = reference.differences(&actual, span, shift, bg);
                if placement.is_none() || differences > 0 {
                    failures.push(format!("{} {:?}: {differences} pixel differences at {shift:?}; baseline {} expected {}, ascent {} descent {}, faces {:?}",case.name,source.symbol(),reference.baseline,baseline,reference.ascent,reference.descent,reference.faces));
                }
                let clipped = min.x + dx < 0
                    || max.x + dx > span.x as i32
                    || min.y + dy < 0
                    || max.y + dy > span.y as i32;
                let classification = if !reference.supported {
                    "unexpected-missing-glyph"
                } else if !font_ok {
                    "unexpected-font"
                } else if !color_ok {
                    "required-color-failure"
                } else if placement.is_none() || differences > 0 {
                    "rendering-failure"
                } else if clipped {
                    "verified-crop"
                } else {
                    "required-success"
                };
                rows.push_str(&format!("{}\t{}\t{y}\t{:?}\t{classification}\t{:?}\t{baseline}\t{cell:?}\t{font_size}\t{shift:?}\n",case.name,case.scale,source.symbol(),reference.faces));
                if y > 1 && differences == 0 {
                    continue;
                }
                let name = format!("{}-{}x-{y}", case.name, case.scale);
                let raw_reference: Vec<u8> = (0..reference.size.y)
                    .flat_map(|y| {
                        (0..reference.size.x)
                            .flat_map(|x| {
                                let p = reference
                                    .pixel(reference.origin + UVec2::new(x, y).as_ivec2(), bg);
                                [p[0], p[1], p[2], 255]
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                fidelity_oracle::save_detail(
                    &out.join(format!("{name}-raw-reference.png")),
                    reference.size,
                    raw_reference,
                );
                fidelity_oracle::save_detail(
                    &out.join(format!("{name}-actual.png")),
                    span,
                    actual
                        .iter()
                        .flat_map(|p| [p[0], p[1], p[2], 255])
                        .collect(),
                );
                let expected: Vec<u8> = (0..span.y)
                    .flat_map(|y| {
                        (0..span.x)
                            .flat_map(|x| {
                                let p = reference.pixel(UVec2::new(x, y).as_ivec2() - shift, bg);
                                [p[0], p[1], p[2], 255]
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                fidelity_oracle::save_detail(
                    &out.join(format!("{name}-reference.png")),
                    span,
                    expected,
                );
            }
        }
        match frame.phase {
            0 => config.raster.scale = case.scale + 0.001,
            1 => {
                config.raster.scale = [1.49, 1.51, 1.99, 2.01][case.scale_index];
                config.sizing = TerminalSizing::FromFont {
                    font_size: 23.25,
                    line_height: 1.,
                };
            }
            2 => {
                config.sizing = TerminalSizing::FromFont {
                    font_size: 23.25,
                    line_height: 0.85,
                }
            }
            3 => {
                config.raster.scale = case.scale;
                config.sizing = TerminalSizing::Fixed {
                    cell_size: Vec2::new(9.25, 18.25),
                    font_size: 18.75,
                };
            }
            4 => config.font = FontFaces::regular(replacement.0.clone()),
            5 => paint(
                renderer.surface(),
                case.samples,
                case.name,
                true,
                case.palette,
            ),
            _ => {}
        }
    }
    std::fs::write(out.join("results.tsv"), rows).unwrap();
    std::fs::write(out.join("failures.txt"), failures.join("\n")).unwrap();
    for failure in &failures {
        eprintln!("FAIL {failure}");
    }
    assert!(
        failures.is_empty(),
        "{} required coverage failures",
        failures.len()
    );
    println!(
        "PASS {} ({} cases)",
        PHASES[frame.phase],
        cases.iter().count()
    );
    frame.phase += 1;
    frame.ticks = 0;
    if frame.phase == PHASES.len() {
        println!("all required coverage checks passed");
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires pinned full source fonts; set FIDELITY_FONT_SOURCES"]
    fn subsets_preserve_required_shaping_metrics_and_rasters() {
        let sources = std::path::PathBuf::from(
            std::env::var("FIDELITY_FONT_SOURCES").expect("source directory"),
        );
        let ascii: Vec<String> = (33u8..=126)
            .map(|c| char::from(c).to_string())
            .chain(std::iter::once("█".into()))
            .collect();
        let ascii_refs: Vec<_> = ascii.iter().map(String::as_str).collect();
        for (source, subset, samples) in [
            (
                "CascadiaMono-Regular.ttf",
                include_bytes!("../assets/fonts/fidelity/FidelityASCII.ttf").as_slice(),
                ascii_refs.as_slice(),
            ),
            (
                "noto-cjk-NotoSansCJKsc-Regular.otf",
                include_bytes!("../assets/fonts/fidelity/FidelityCJK.otf").as_slice(),
                CJK,
            ),
            (
                "noto-emoji-NotoColorEmoji.ttf",
                include_bytes!("../assets/fonts/fidelity/FidelityColorEmoji.ttf").as_slice(),
                COLOR,
            ),
        ] {
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                bevy::asset::AssetPlugin::default(),
                bevy::text::TextPlugin,
            ))
            .init_asset::<Image>();
            app.world_mut()
                .resource_mut::<bevy::text::FontCx>()
                .collection = fontique::Collection::new(fontique::CollectionOptions {
                system_fonts: false,
                ..default()
            });
            let (full, _) = add_font(&mut app, &std::fs::read(sources.join(source)).unwrap());
            let (subset, _) = add_font(&mut app, subset);
            app.update();
            let mut state = bevy::ecs::system::SystemState::<Rasterizer>::new(app.world_mut());
            let mut oracle = state.get_mut(app.world_mut()).unwrap();
            for size in [18.0, 23.25] {
                for symbol in samples {
                    let sample = TerminalCell::new(symbol);
                    let full = oracle
                        .shape(
                            &sample,
                            &TerminalRenderConfig {
                                font: FontFaces::regular(full.clone()),
                                ..default()
                            },
                            size,
                            40.,
                        )
                        .unwrap();
                    let subset = oracle
                        .shape(
                            &sample,
                            &TerminalRenderConfig {
                                font: FontFaces::regular(subset.clone()),
                                ..default()
                            },
                            size,
                            40.,
                        )
                        .unwrap();
                    assert!(full.supported && subset.supported, "{source} {symbol:?}");
                    assert_eq!(
                        (full.baseline, full.ascent, full.descent),
                        (subset.baseline, subset.ascent, subset.descent),
                        "{source} {symbol:?}"
                    );
                    assert_eq!(
                        (full.origin, full.size, full.glyph_count, full.color_glyphs),
                        (
                            subset.origin,
                            subset.size,
                            subset.glyph_count,
                            subset.color_glyphs
                        ),
                        "{source} {symbol:?}"
                    );
                    assert_eq!(full.rgba, subset.rgba, "{source} {symbol:?} at {size}");
                }
            }
        }
    }
}
