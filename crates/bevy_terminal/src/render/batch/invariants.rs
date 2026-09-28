//! Deterministic resource invariants established by the Ghostty parity
//! work, asserted as counts rather than timings (the scenarios follow
//! `benchmarks/audit-workloads`): adding glyphs never touches a
//! terminal-owned image, terminals retain no CPU copy of their images, the
//! shape cache keeps working sets across a retirement, queued atlas uploads
//! stay within one atlas, and idle terminals do no work.

use super::*;
use crate::render::{FontFaces, TerminalSizing};
use crate::scene::TerminalCell;

/// A headless app with `terminals` 40×12 terminals in 8×12 cells of
/// JetBrains Mono at 8 px, settled for five updates.
fn app(terminals: usize, font_size: f32) -> (App, Vec<(Entity, TerminalSurface)>) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        TerminalPlugin,
    ))
    .init_asset::<Image>();
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(super::probe::asset(
            "jetbrains-mono/JetBrainsMono-Regular.ttf",
        )));
    let surfaces = (0..terminals)
        .map(|_| {
            let surface = TerminalSurface::new((40, 12));
            let entity = app
                .world_mut()
                .spawn((
                    TerminalRenderer::new(surface.clone()),
                    TerminalRenderConfig {
                        font: FontFaces::regular(font.clone()),
                        sizing: TerminalSizing::Fixed {
                            cell_size: Vec2::new(font_size, font_size * 1.5),
                            font_size,
                        },
                        ..default()
                    },
                ))
                .id();
            (entity, surface)
        })
        .collect();
    for _ in 0..5 {
        app.update();
    }
    (app, surfaces)
}

/// Fills the grid with consecutive codepoints from `first`.
fn fill(surface: &TerminalSurface, first: u32, wide: bool) {
    surface.update(|update| {
        let step = if wide { 2 } else { 1 };
        for y in 0..12u16 {
            for x in (0..40u16).step_by(step) {
                let index = u32::from(y) * 40 / step as u32 + u32::from(x) / step as u32;
                let symbol = char::from_u32(first + index).unwrap().to_string();
                let cell = if wide {
                    TerminalCell::wide(&symbol, 2)
                } else {
                    TerminalCell::new(&symbol)
                };
                update.set_cell((x, y), &cell);
            }
        }
    });
}

fn cpu_image_bytes(app: &App) -> usize {
    app.world()
        .resource::<Assets<Image>>()
        .iter()
        .map(|(_, image)| image.data.as_ref().map_or(0, Vec::len))
        .sum()
}

fn shape_misses(app: &App, entity: Entity) -> u32 {
    app.world()
        .get::<TerminalStats>(entity)
        .unwrap()
        .shape_misses
}

#[test]
fn adding_glyphs_never_modifies_terminal_owned_images() {
    #[derive(Resource, Default)]
    struct Modified(Vec<AssetId<Image>>);
    let (mut app, surfaces) = app(1, 8.0);
    let (entity, surface) = &surfaces[0];
    app.init_resource::<Modified>().add_systems(
        Last,
        |mut events: MessageReader<AssetEvent<Image>>, mut modified: ResMut<Modified>| {
            modified
                .0
                .extend(events.read().filter_map(|event| match *event {
                    AssetEvent::Modified { id } => Some(id),
                    _ => None,
                }));
        },
    );
    let output = app
        .world()
        .get::<TerminalTexture>(*entity)
        .unwrap()
        .image
        .id();
    // Every step shows 240 CJK ideographs never seen before.
    for step in 0..6 {
        fill(surface, 0x4e00 + step * 240, true);
        app.update();
        assert!(
            shape_misses(&app, *entity) > 0,
            "step {step} shaped new glyphs"
        );
    }
    assert!(
        !app.world().resource::<Modified>().0.contains(&output),
        "the output image is only reallocated on resize"
    );
    let atlas_sized = app
        .world()
        .resource::<Assets<Image>>()
        .iter()
        .filter(|(_, image)| {
            image.width() == GLYPH_ATLAS_SIZE && image.height() == GLYPH_ATLAS_SIZE
        })
        .count();
    assert_eq!(
        atlas_sized, 0,
        "the unified atlas lives only in the render world"
    );
}

#[test]
fn terminals_retain_no_cpu_copy_of_their_images() {
    let text = "The quick brown fox jumps over 13 lazy dogs";
    let show = |terminals: usize| {
        let (mut app, surfaces) = app(terminals, 8.0);
        for (_, surface) in &surfaces {
            surface.update(|update| {
                for (x, c) in text.chars().enumerate() {
                    update.set_cell((x as u16 % 40, 0), &TerminalCell::from(c));
                }
            });
        }
        for _ in 0..3 {
            app.update();
        }
        for (entity, _) in &surfaces {
            let output = &app.world().get::<TerminalTexture>(*entity).unwrap().image;
            let image = app.world().resource::<Assets<Image>>().get(output).unwrap();
            assert!(
                image.data.is_none(),
                "output images live in the render world"
            );
        }
        cpu_image_bytes(&app)
    };
    // Only Bevy's shared font atlases hold CPU pixels: eight terminals
    // showing the same text retain exactly what one does.
    assert_eq!(show(8), show(1));
}

#[test]
fn alternating_working_sets_keep_their_shape_cache_misses() {
    let (mut app, surfaces) = app(1, 8.0);
    let (entity, surface) = &surfaces[0];
    // Two working sets of 2880 glyphs (six frames each), together larger
    // than the shape cache, shown in turn: A, B, then A again.
    let mut phases = [0; 3];
    for step in 0..18u32 {
        let base = if (step / 6) % 2 == 0 { 0x4e00 } else { 0x5b00 };
        fill(surface, base + (step % 6) * 480, false);
        app.update();
        phases[(step / 6) as usize] += shape_misses(&app, *entity);
    }
    // Every glyph of A and B misses once; the retired generation still holds
    // most of A when it returns, so the third phase misses far less.
    assert_eq!(phases, [2880, 2880, 447]);
}

#[test]
fn queued_atlas_uploads_never_exceed_one_atlas() {
    // Large glyphs, no render world to take scenes: uploads accumulate
    // across superseded scenes and the atlas overflows and restarts.
    let (mut app, surfaces) = app(1, 40.0);
    let (entity, surface) = &surfaces[0];
    let limit = u64::from(GLYPH_ATLAS_SIZE) * u64::from(GLYPH_ATLAS_SIZE);
    let mut restarts = 0;
    let mut previous = UVec2::ZERO;
    for step in 0..40 {
        fill(surface, 0x4e00 + step * 240, true);
        app.update();
        let state = app.world().get::<BatchMainState>(*entity).unwrap();
        let cursor = state.glyph_atlas.cursor;
        if cursor.y < previous.y {
            restarts += 1;
        }
        previous = cursor;
        let queued: u64 = state
            .pending
            .iter()
            .flat_map(|scene| &scene.atlas_uploads)
            .chain(&state.glyph_atlas.uploads)
            .map(|upload| u64::from(upload.size.x) * u64::from(upload.size.y))
            .sum();
        assert!(queued <= limit, "step {step}: {queued} texels queued");
    }
    assert!(restarts > 0, "the scenario must overflow the atlas");
}

#[test]
fn idle_terminals_build_no_scene_and_change_no_component() {
    #[derive(Resource, Default)]
    struct Changes(usize);
    let (mut app, surfaces) = app(4, 8.0);
    app.init_resource::<Changes>().add_systems(
        Update,
        (|changed: Query<(), Or<(Changed<TerminalTexture>, Changed<TerminalStats>)>>,
          mut count: ResMut<Changes>| {
            count.0 += changed.iter().count();
        })
        .after(super::super::TerminalSystems::Sync),
    );
    for (_, surface) in &surfaces {
        fill(surface, 0x41, false);
    }
    app.update();
    let generations: Vec<u64> = surfaces
        .iter()
        .map(|(entity, _)| {
            app.world()
                .get::<BatchMainState>(*entity)
                .unwrap()
                .generation
        })
        .collect();
    // The drawn scenes are taken, as extraction would.
    for (entity, _) in &surfaces {
        app.world_mut()
            .get_mut::<BatchMainState>(*entity)
            .unwrap()
            .pending = None;
    }
    // The first idle update zeroes the statistics of the drawing one.
    app.update();
    app.world_mut().resource_mut::<Changes>().0 = 0;
    for _ in 0..10 {
        app.update();
    }
    for ((entity, _), generation) in surfaces.iter().zip(generations) {
        let state = app.world().get::<BatchMainState>(*entity).unwrap();
        assert_eq!(state.generation, generation, "no scene was built");
        assert!(state.pending.is_none());
    }
    assert_eq!(app.world().resource::<Changes>().0, 0);
}
