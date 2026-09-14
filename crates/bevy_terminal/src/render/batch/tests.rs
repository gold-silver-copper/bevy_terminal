use super::*;
use super::{
    gpu::{append_instance_bytes, collect_batch_scenes},
    metrics::*,
    scene::*,
    shaping::*,
};
use crate::render::TerminalSizing;
use crate::render::grid_for;
use crate::scene::{GridSize, TerminalCell, TerminalStyle};

fn quad(value: f32) -> QuadInstance {
    QuadInstance {
        rect: Vec4::splat(value),
        uv: Vec4::ZERO,
        color: Vec4::ONE,
    }
}

#[test]
fn unsuccessful_atlas_insertion_preserves_packing_state() {
    let mut atlas = UnifiedGlyphAtlas::new(Handle::default());
    atlas.cursor = UVec2::new(GLYPH_ATLAS_SIZE - 2, 10);
    atlas.row_height = 30;
    let cursor = atlas.cursor;
    let mut images = Assets::<Image>::default();
    let source = SourceGlyph {
        texture: Handle::<Image>::default().id(),
        x: 0,
        y: 0,
        width: 8,
        height: 8,
    };
    assert!(atlas.cache(source, &mut images).is_none());
    assert!(
        atlas
            .cache(
                SourceGlyph {
                    width: u32::MAX,
                    ..source
                },
                &mut images
            )
            .is_none()
    );
    assert!(
        atlas
            .cache(
                SourceGlyph {
                    height: u32::MAX,
                    ..source
                },
                &mut images
            )
            .is_none()
    );
    assert_eq!(atlas.cursor, cursor);
    assert_eq!(atlas.row_height, 30);
    assert!(atlas.glyphs.is_empty());
}

fn test_app() -> App {
    let mut app = App::new();
    app.init_resource::<Assets<Image>>()
        .add_plugins(TerminalPlugin);
    app
}

#[test]
fn spawned_terminals_own_distinct_textures_and_can_be_despawned() {
    let first_surface = TerminalSurface::new((12, 4));
    let second_surface = TerminalSurface::new((7, 9));
    let mut app = test_app();
    // A presented terminal: the user owns the UI node.
    let first = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(first_surface.clone()),
            ImageNode::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(30.0),
                top: px(40.0),
                ..default()
            },
        ))
        .id();
    app.update();
    // A second, headless terminal spawned later with an explicit config.
    let second = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(second_surface.clone()),
            TerminalRenderConfig {
                sizing: TerminalSizing::FitCellWidth(Vec2::new(11.0, 20.0)),
                ..default()
            },
        ))
        .id();
    app.update();

    let mut terminals =
        app.world_mut()
            .query::<(Entity, &TerminalRenderer, &TerminalTexture, &TerminalStats)>();
    let mut instances = terminals
        .iter(app.world())
        .map(|(entity, terminal, texture, _)| {
            (
                entity,
                terminal.surface().size(),
                texture.image.id(),
                texture.geometry.size(),
            )
        })
        .collect::<Vec<_>>();
    instances.sort_by_key(|(_, size, _, _)| size.width);

    assert_eq!(instances.len(), 2);
    assert_eq!(instances[0].0, second);
    assert_eq!(instances[0].1, GridSize::new(7, 9));
    assert_eq!(instances[0].3, UVec2::new(77, 180));
    assert_eq!(instances[1].0, first);
    assert_eq!(instances[1].1, GridSize::new(12, 4));
    assert_eq!(instances[1].3, UVec2::new(132, 80));
    assert_ne!(instances[0].2, instances[1].2);
    // Both required a config; the first got the default one.
    assert!(app.world().get::<TerminalRenderConfig>(first).is_some());
    assert_eq!(first_surface.snapshot().size().width, 12);
    assert_eq!(second_surface.snapshot().size().width, 7);

    // Despawning a terminal removes everything with the entity.
    app.world_mut().despawn(first);
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&TerminalTexture>()
            .iter(app.world())
            .count(),
        1
    );
}

/// An app with Bevy's text pipeline but no window or renderer: enough to
/// shape glyphs into the main-world caches and exercise the sync system.
fn text_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        TerminalPlugin,
    ))
    .init_asset::<Image>();
    app
}

#[test]
fn failed_measurement_and_shapes_retry_after_font_registration() {
    use bevy::ecs::system::SystemState;
    let mut app = text_app();
    let regular = app.world().resource::<Assets<Font>>().reserve_handle();
    let config = TerminalRenderConfig {
        font: super::super::FontFaces::regular(regular.clone()),
        sizing: TerminalSizing::Fixed {
            cell_size: Vec2::new(12.0, 24.0),
            font_size: 20.0,
        },
        ..default()
    };
    let raster = physical_config(resolve_metrics(&config, None), 1.0);
    let atlas = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(make_glyph_atlas_image());
    let mut atlas = UnifiedGlyphAtlas::new(atlas);
    let mut shapes = ShapeCaches::default();
    let mut stats = TerminalStats::default();
    let mut resources = SystemState::<(TextResources, ResMut<Assets<Image>>)>::new(app.world_mut());
    {
        let (mut text, mut images) = resources.get_mut(app.world_mut()).unwrap();
        let mut cx = text.context(&mut images);
        refine_metrics(&config, None, raster, &mut cx);
        assert!(
            cx.failure.is_some(),
            "a probe error must invalidate measurement"
        );
        let run = cached_shape(
            "A",
            1,
            &ResolvedStyle::plain(),
            &config,
            raster,
            Vec2::splat(100.0),
            &mut cx,
            &mut shapes,
            &mut atlas,
            &mut stats,
        );
        assert!(run.is_empty());
        assert!(
            shapes.entries.is_empty(),
            "a failed run is not a valid cache entry"
        );
    }
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(
            regular.id(),
            Font::from_bytes(
                include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                    .to_vec(),
            ),
        )
        .unwrap();
    for _ in 0..3 {
        app.update();
    }
    let (mut text, mut images) = resources.get_mut(app.world_mut()).unwrap();
    let mut cx = text.context(&mut images);
    let raster = refine_metrics(&config, None, raster, &mut cx);
    let run = cached_shape(
        "A",
        1,
        &ResolvedStyle::plain(),
        &config,
        raster,
        Vec2::splat(100.0),
        &mut cx,
        &mut shapes,
        &mut atlas,
        &mut stats,
    );
    assert!(!run.is_empty());
    assert!(cx.failure.is_none());
    assert_eq!(shapes.entries.len(), 1);
}

#[test]
fn failed_advance_discards_previous_measurement_and_pending_content() {
    use bevy::ecs::system::SystemState;
    let mut app = text_app();
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                .to_vec(),
        ));
    let surface = TerminalSurface::new((4, 2));
    write_text(&surface, "test");
    let renderer = TerminalRenderer::new(surface);
    let config = TerminalRenderConfig {
        font: super::super::FontFaces::regular(font),
        sizing: TerminalSizing::font(20.0),
        ..default()
    };
    let entity = app
        .world_mut()
        .spawn((renderer.clone(), config.clone()))
        .id();
    for _ in 0..3 {
        app.update();
    }
    let mut output = app.world().get::<TerminalTexture>(entity).unwrap().clone();
    assert!(output.measured().is_some());
    let mut state = app
        .world_mut()
        .entity_mut(entity)
        .take::<BatchMainState>()
        .unwrap();
    assert!(state.metrics.is_some());
    assert!(state.last_snapshot.is_some());
    assert!(state.pending.is_some());
    let missing = app.world().resource::<Assets<Font>>().reserve_handle();
    let unavailable = TerminalRenderConfig {
        font: super::super::FontFaces::regular(missing),
        ..config.clone()
    };
    let mut resources = SystemState::<(TextResources, ResMut<Assets<Image>>)>::new(app.world_mut());
    let (mut text, mut images) = resources.get_mut(app.world_mut()).unwrap();
    let mut stats = TerminalStats::default();
    let result = sync_batch_terminal(
        &renderer,
        SyncInput {
            config: &unavailable,
            config_changed: true,
            shaping_changed: true,
            fonts_changed: false,
            raster_scale: 1.0,
            elapsed: 0.0,
            texture_limit: 8192,
        },
        &mut state,
        &mut output,
        &mut stats,
        &mut text.context(&mut images),
    );
    assert_eq!(result, Err(TerminalStatus::ShapingFailed));
    // Apply the same invalidation boundary as the outer Bevy system.
    state.invalidate();
    output.status = result.unwrap_err();
    assert_eq!(output.status, TerminalStatus::ShapingFailed);
    assert!(state.metrics.is_none());
    assert!(state.last_snapshot.is_none());
    assert!(state.pending.is_none());

    // A successful retry must rebuild even if the effective geometry equals
    // the old geometry and no further configuration event arrives.
    output.status = TerminalStatus::Ready;
    sync_batch_terminal(
        &renderer,
        SyncInput {
            config: &config,
            config_changed: false,
            shaping_changed: false,
            fonts_changed: false,
            raster_scale: 1.0,
            elapsed: 0.0,
            texture_limit: 8192,
        },
        &mut state,
        &mut output,
        &mut stats,
        &mut text.context(&mut images),
    )
    .unwrap();
    assert!(output.measured().is_some());
    assert!(state.metrics.is_some());
    assert!(state.pending.as_ref().unwrap().clear);
    assert!(stats.shape_misses > 0);
}

#[test]
fn idle_output_is_stable_and_application_ui_is_untouched() {
    #[derive(Resource, Default)]
    struct Changes(usize);
    let mut app = text_app();
    app.init_resource::<Changes>().add_systems(
        Update,
        (|changed: Query<Entity, Changed<TerminalTexture>>, mut changes: ResMut<Changes>| {
            changes.0 += changed.iter().count();
        })
        .after(super::super::TerminalSystems::Sync),
    );
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(TerminalSurface::new((4, 2))))
        .id();
    for _ in 0..4 {
        app.update();
    }
    app.world_mut().resource_mut::<Changes>().0 = 0;
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(app.world().resource::<Changes>().0, 0);
    // UI is application-owned even if its components share the renderer entity.
    let image_node = ImageNode::default();
    let image_handle = image_node.image.clone();
    app.world_mut().entity_mut(entity).insert((
        Node {
            width: px(123.0),
            height: px(45.0),
            ..default()
        },
        image_node,
    ));
    app.update();
    assert!(
        app.world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .is_some()
    );
    assert_eq!(app.world().get::<Node>(entity).unwrap().width, px(123.0));
    assert_eq!(app.world().get::<Node>(entity).unwrap().height, px(45.0));
    assert_eq!(
        app.world().get::<ImageNode>(entity).unwrap().image,
        image_handle
    );
}

#[test]
fn replacing_surface_with_equal_revision_refreshes_retained_content() {
    let mut app = text_app();
    let first = TerminalSurface::new((4, 1));
    let second = TerminalSurface::new((4, 1));
    write_text(&first, "AAAA");
    write_text(&second, "BBBB");
    assert_eq!(first.revision(), second.revision());
    let entity = app.world_mut().spawn(TerminalRenderer::new(first)).id();
    for _ in 0..4 {
        app.update();
    }
    let handle = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .image
        .clone();
    app.world_mut()
        .entity_mut(entity)
        .insert(TerminalRenderer::new(second));
    app.update();
    let state = app.world().get::<BatchMainState>(entity).unwrap();
    assert_eq!(state.last_snapshot.as_ref().unwrap().row_text(0), "BBBB");
    assert!(state.pending.as_ref().unwrap().clear);
    assert_eq!(
        app.world().get::<TerminalTexture>(entity).unwrap().image,
        handle
    );
}

#[test]
fn shared_content_has_independent_renderers_and_measurements() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 2));
    write_text(&surface, "AAAA");
    let entities = [10.0, 20.0].map(|width| {
        app.world_mut()
            .spawn((
                TerminalRenderer::new(surface.clone()),
                TerminalRenderConfig {
                    sizing: TerminalSizing::Fixed {
                        cell_size: Vec2::new(width, 24.0),
                        font_size: 16.0,
                    },
                    ..default()
                },
            ))
            .id()
    });
    for _ in 0..4 {
        app.update();
    }
    let handles = entities.map(|entity| {
        app.world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .image
            .clone()
    });
    assert_ne!(handles[0], handles[1]);
    drain_pending(&mut app);
    surface.update(|update| {
        update.set_cell((0, 1), &TerminalCell::new("B"));
    });
    app.update();
    for (entity, width) in entities.into_iter().zip([10.0, 20.0]) {
        let output = app
            .world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .unwrap();
        assert_eq!(output.cell_size(), Vec2::new(width, 24.0));
        assert_eq!(output.size, UVec2::new(width as u32 * 4, 48));
        let state = app.world().get::<BatchMainState>(entity).unwrap();
        let snapshot = state.last_snapshot.as_ref().unwrap();
        assert_eq!(snapshot.row_text(0), "AAAA");
        assert_eq!(snapshot.row_text(1), "B   ");
        assert_eq!(
            app.world()
                .get::<TerminalStats>(entity)
                .unwrap()
                .changed_rows,
            1
        );
    }
}

#[test]
fn repeated_despawns_release_owned_images_and_pending_scenes() {
    let mut app = text_app();
    // Fontique independently prunes/reloads system-font data, which can create
    // new Bevy atlas identities. Use a persistent asset to isolate ownership.
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                .to_vec(),
        ));
    let mut pending = PendingBatchScenes::default();
    let mut image_counts = Vec::new();
    for _ in 0..4 {
        let entities = std::array::from_fn::<_, 3, _>(|_| {
            let surface = TerminalSurface::new((4, 2));
            write_text(&surface, "text");
            app.world_mut()
                .spawn((
                    TerminalRenderer::new(surface),
                    TerminalRenderConfig {
                        font: super::super::FontFaces::regular(font.clone()),
                        ..default()
                    },
                ))
                .id()
        });
        for _ in 0..4 {
            app.update();
        }
        collect_batch_scenes(app.world_mut(), &mut pending);
        assert_eq!(pending.scenes.len(), entities.len());
        let owned = entities.map(|entity| {
            let state = app.world().get::<BatchMainState>(entity).unwrap();
            [state.output.id(), state.glyph_atlas.image.id()]
        });
        for entity in entities {
            app.world_mut().despawn(entity);
        }
        for _ in 0..4 {
            app.update();
        }
        collect_batch_scenes(app.world_mut(), &mut pending);
        assert!(pending.scenes.is_empty());
        assert!(pending.live_textures.is_empty());
        let images = app.world().resource::<Assets<Image>>();
        for id in owned.into_iter().flatten() {
            assert!(
                !images.contains(id),
                "terminal-owned image must be released"
            );
        }
        // Bevy may retain its shared source font atlas after the first cycle.
        image_counts.push(images.len());
    }
    assert_eq!(image_counts[1..], [image_counts[1]; 3], "{image_counts:?}");
}

fn write_text(surface: &TerminalSurface, text: &str) {
    surface.update(|update| {
        for (column, symbol) in text.chars().enumerate() {
            update.set_cell((column as u16, 0), &TerminalCell::from(symbol));
        }
    });
}

#[test]
fn paint_changes_preserve_shaping_and_repaint_content() {
    let mut app = text_app();
    let surface = TerminalSurface::new((6, 1));
    write_text(&surface, "hello!");
    let entity = app.world_mut().spawn(TerminalRenderer::new(surface)).id();
    for _ in 0..4 {
        app.update();
    }
    let cached = app
        .world()
        .get::<BatchMainState>(entity)
        .unwrap()
        .shapes
        .entries
        .len();
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .theme
        .foreground = Color::WHITE;
    app.update();
    let stats = app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(stats.shape_misses, 0);
    assert_eq!(stats.changed_rows, 1);
    assert_eq!(
        app.world()
            .get::<BatchMainState>(entity)
            .unwrap()
            .shapes
            .entries
            .len(),
        cached
    );
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .cursor
        .blink_hz = None;
    app.update();
    assert_eq!(
        app.world()
            .get::<TerminalStats>(entity)
            .unwrap()
            .shape_misses,
        0
    );
}

#[test]
fn config_changes_rebuild_but_unrelated_changes_keep_the_shape_cache() {
    #[derive(Component)]
    struct Unrelated(u32);
    let mut app = text_app();
    let surface = TerminalSurface::new((6, 1));
    write_text(&surface, "hello!");
    let entity = app
        .world_mut()
        .spawn((TerminalRenderer::new(surface.clone()), Unrelated(0)))
        .id();
    for _ in 0..4 {
        app.update();
    }
    // Redrawing the same content re-uses the shape cache: no misses, no rows.
    write_text(&surface, "hello!");
    app.update();
    let idle = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(idle.shape_misses, 0, "{idle}");
    assert_eq!(idle.changed_rows, 0);
    // Rewriting a cell with a new glyph shapes only that glyph.
    surface.update(|u| {
        u.set_cell((0, 0), &TerminalCell::new("Z"));
    });
    app.update();
    let one = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(one.shape_misses, 1, "{one}");
    assert_eq!(one.changed_rows, 1);

    // Touching an unrelated component and redrawing identical content: the
    // shape cache survives and nothing is rebuilt.
    app.world_mut().get_mut::<Unrelated>(entity).unwrap().0 += 1;
    write_text(&surface, "Zello!");
    app.update();
    let after_unrelated = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(after_unrelated.shape_misses, 0);
    assert_eq!(after_unrelated.changed_rows, 0);

    // Changing the render config re-shapes everything.
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .sizing = super::super::TerminalSizing::FitCellWidth(Vec2::new(12.0, 22.0));
    app.update();
    let after_config = *app.world().get::<TerminalStats>(entity).unwrap();
    assert!(after_config.shape_misses > 0, "{after_config}");
    assert_eq!(after_config.changed_rows, 1);
    // The font is sized to the 12 px width; the requested 22 px height is a
    // minimum that grows to the (default) font's line box.
    let size = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .geometry
        .size();
    assert_eq!(size.x, 72);
    assert!(size.y >= 22, "{size:?}");
}

#[test]
fn resizing_keeps_the_texture_handle_without_modifying_application_layout() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 2));
    write_text(&surface, "abcd");
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(surface.clone()),
            TerminalRenderConfig {
                sizing: TerminalSizing::FitCellWidth(Vec2::new(10.0, 20.0)),
                ..default()
            },
            ImageNode::default(),
            Node::default(),
        ))
        .id();
    for _ in 0..3 {
        app.update();
    }
    let texture = app.world().get::<TerminalTexture>(entity).unwrap().clone();
    assert_eq!(texture.geometry.size(), UVec2::new(40, 40));
    let node = app.world().get::<Node>(entity).unwrap();
    assert_eq!(node.width, Val::Auto);
    assert_eq!(node.height, Val::Auto);
    assert_eq!(
        app.world().get::<ImageNode>(entity).unwrap().image,
        ImageNode::default().image
    );

    surface.update(|update| {
        update.resize((8, 3));
    });
    app.update();
    let resized = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(resized.image, texture.image, "handle must stay stable");
    assert_eq!(resized.geometry.size(), UVec2::new(80, 60));
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&texture.image)
        .expect("the image was reallocated in place");
    assert_eq!(image.width(), 80);
    assert_eq!(image.height(), 60);
    let node = app.world().get::<Node>(entity).unwrap();
    assert_eq!(node.width, Val::Auto);
    assert_eq!(node.height, Val::Auto);

    surface.update(|update| {
        update.resize((2, 1));
    });
    app.update();
    let shrunk = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(shrunk.image, texture.image);
    assert_eq!(shrunk.geometry.size(), UVec2::new(20, 20));
}

#[test]
fn unrelated_font_assets_do_not_trigger_remeasurement() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 1));
    write_text(&surface, "abcd");
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..3 {
        app.update();
    }
    let baseline = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(baseline.shape_misses, 0, "{baseline}");
    // Adding a font this terminal does not use must not clear its caches:
    // the next redraw of the same text shapes nothing.
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(Vec::new()));
    app.update();
    write_text(&surface, "abcd");
    app.update();
    let after = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(after.shape_misses, 0, "{after}");
    assert_eq!(after.changed_rows, 0);
}

#[test]
fn font_driven_cells_measure_the_embedded_font() {
    let mut app = text_app();
    let regular = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                .to_vec(),
        ));
    let surface = TerminalSurface::new((4, 1));
    write_text(&surface, "abcd");
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(surface.clone()),
            TerminalRenderConfig {
                sizing: super::super::TerminalSizing::FromFont {
                    font_size: 20.0,
                    line_height: 1.0,
                },
                font: super::super::FontFaces::regular(regular),
                ..default()
            },
        ))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let texture = app.world().get::<TerminalTexture>(entity).unwrap();
    // JetBrains Mono's advance is 0.6 em: 12 px wide at 20 px. Its line
    // box is 1.32 em (ascender 1020, descender 300): 26.4 px, so the cell
    // is 27 px tall.
    assert!(
        (texture.geometry.cell_size().x - 12.0).abs() < 0.05,
        "{:?}",
        texture.geometry.cell_size()
    );
    assert!(
        (texture.geometry.cell_size().y - 27.0).abs() < 0.05,
        "{:?}",
        texture.geometry.cell_size()
    );
    assert_eq!(texture.geometry.size(), UVec2::new(48, 27));
    assert_eq!(
        texture.geometry.grid_for(Vec2::new(125.0, 60.0)),
        GridSize::new(10, 2)
    );

    // Zoom: a larger font grows the cell and the texture (same handle).
    let handle = texture.image.clone();
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .sizing = super::super::TerminalSizing::font(30.0);
    app.update();
    let zoomed = app.world().get::<TerminalTexture>(entity).unwrap();
    assert!((zoomed.geometry.cell_size().x - 18.0).abs() < 0.05);
    // 30 px: 18 px advance and a 39.6 px line box, so 40 px rows.
    assert_eq!(zoomed.geometry.size(), UVec2::new(72, 40));
    assert_eq!(zoomed.image, handle);
}

#[test]
fn font_driven_cells_wait_for_a_loading_handle_before_ready() {
    let mut app = text_app();
    let regular = Handle::<Font>::from(bevy::asset::uuid::Uuid::from_u128(0x5241_5454_5901));
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(TerminalSurface::new((4, 1))),
            TerminalRenderConfig {
                sizing: super::super::TerminalSizing::FromFont {
                    font_size: 20.0,
                    line_height: 1.0,
                },
                font: super::super::FontFaces::regular(regular.clone()),
                ..default()
            },
        ))
        .id();

    for _ in 0..2 {
        app.update();
    }
    assert!(
        app.world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .is_none()
    );
    let state = app.world().get::<BatchMainState>(entity).unwrap();
    assert!(state.measured_advance.is_none());
    assert!(
        state.last_snapshot.is_none(),
        "no 1x1 scene may be published"
    );

    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(
            regular.id(),
            Font::from_bytes(
                include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                    .to_vec(),
            ),
        )
        .expect("the pending font id is unused");
    for _ in 0..3 {
        app.update();
    }

    assert!(
        app.world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .is_some()
    );
    let texture = app.world().get::<TerminalTexture>(entity).unwrap();
    assert!((texture.geometry.cell_size().x - 12.0).abs() < 0.05);
    assert!((texture.geometry.cell_size().y - 27.0).abs() < 0.05);
    assert_eq!(texture.geometry.size(), UVec2::new(48, 27));
}

#[test]
fn ready_terminal_exposes_font_failure_and_recovers_with_the_same_image() {
    let mut app = text_app();
    let bytes = include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
    let regular = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(bytes.to_vec()));
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(TerminalSurface::new((4, 1))),
            TerminalRenderConfig {
                font: super::super::FontFaces::regular(regular),
                ..default()
            },
        ))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let image = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .image
        .clone();
    let initial = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .measured()
        .unwrap()
        .clone();
    let mut pending = PendingBatchScenes::default();
    collect_batch_scenes(app.world_mut(), &mut pending);
    assert_eq!(pending.scenes.len(), 1);
    let replacement = app.world().resource::<Assets<Font>>().reserve_handle();
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .font = super::super::FontFaces::regular(replacement.clone());
    app.update();
    assert_eq!(
        app.world().get::<TerminalTexture>(entity).unwrap().status,
        TerminalStatus::Loading
    );
    collect_batch_scenes(app.world_mut(), &mut pending);
    assert!(
        pending.scenes.is_empty(),
        "old payload cannot render after a font switch"
    );
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(replacement.id(), Font::from_bytes(Vec::new()))
        .unwrap();
    for _ in 0..3 {
        app.update();
    }
    let failed = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(failed.status, TerminalStatus::FontFailed);
    assert!(failed.measured().is_none());
    assert_eq!(failed.image, image);
    assert!(
        app.world()
            .get::<BatchMainState>(entity)
            .unwrap()
            .pending
            .is_none()
    );

    // Bevy 0.19 registers new asset IDs, not data replaced under an already
    // registered ID. Recover a malformed asset by selecting a fresh handle.
    let recovered_font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(bytes.to_vec()));
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .font = super::super::FontFaces::regular(recovered_font);
    for _ in 0..4 {
        app.update();
    }
    let recovered = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .measured()
        .unwrap();
    assert_eq!(
        app.world().get::<TerminalTexture>(entity).unwrap().image,
        image
    );
    assert_eq!(recovered.cell_size(), initial.cell_size());
}

#[test]
fn late_family_registration_invalidates_previously_shaped_fallbacks() {
    let mut app = text_app();
    let regular = app.world().resource::<Assets<Font>>().reserve_handle();
    // Bevy registers assets under both their embedded family and this unique
    // alias. The alias makes the test independent of installed system fonts.
    let family = bevy::text::FontSource::Family(format!("asset_id:{:?}", regular.id()).into());
    let surface = TerminalSurface::new((4, 1));
    write_text(&surface, "text");
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(surface),
            TerminalRenderConfig {
                font: super::super::FontFaces::regular(family),
                ..default()
            },
        ))
        .id();
    for _ in 0..4 {
        app.update();
    }
    drain_pending(&mut app);
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(
            regular.id(),
            Font::from_bytes(
                include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                    .to_vec(),
            ),
        )
        .unwrap();
    let mut misses = 0;
    for _ in 0..4 {
        app.update();
        misses += app
            .world()
            .get::<TerminalStats>(entity)
            .unwrap()
            .shape_misses;
    }
    assert!(
        misses > 0,
        "registration must invalidate the unchanged text's fallback shapes"
    );
    assert!(
        app.world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .is_some()
    );
}

fn glyph(offset_x: f32, columns: &[u32]) -> CachedGlyph {
    CachedGlyph::new(
        AssetId::default(),
        Vec2::new(offset_x, 0.0),
        Vec2::new(columns.len() as f32, 10.0),
        Vec4::ZERO,
        true,
        columns.to_vec(),
    )
}

#[test]
fn horizontal_fit_pushes_overhang_inside_and_lets_overflow_keep_its_bearings() {
    // Inside the span: bearings are kept.
    assert_eq!(
        fit_horizontally(&[glyph(2.0, &[9, 9, 9])], 11.0, false),
        0.0
    );
    // Overhanging left (an italic): pushed right by the overhang.
    assert_eq!(fit_horizontally(&[glyph(-2.0, &[9; 8])], 11.0, false), 2.0);
    // Overhanging right: pushed left.
    assert_eq!(fit_horizontally(&[glyph(6.0, &[9; 8])], 11.0, false), -3.0);
    // Leading transparent columns do not count as ink.
    assert_eq!(
        fit_horizontally(&[glyph(-2.0, &[0, 0, 9, 9])], 11.0, false),
        0.0
    );
    // Wider than the span: drawn as shaped, overflowing the neighbour.
    assert_eq!(fit_horizontally(&[glyph(0.0, &[9; 15])], 11.0, false), 0.0);
    assert_eq!(fit_horizontally(&[glyph(-3.0, &[9; 15])], 11.0, false), 0.0);
    // A combined run is fitted as one unit, not one translation per glyph.
    assert_eq!(
        fit_horizontally(
            &[glyph(-1.0, &[2, 2]), glyph(1.0, &[10, 19, 1])],
            5.0,
            false
        ),
        1.0
    );
    // Blank runs never shift.
    assert_eq!(fit_horizontally(&[glyph(3.0, &[0, 0])], 11.0, false), 0.0);
}

#[test]
fn ordinary_text_keeps_faint_edge_columns_that_fit() {
    // Cascadia Mono italic W at 1x: 11 ink columns at x=1 in an 11px cell.
    let mut columns = vec![255; 11];
    columns[10] = 120;
    let run = [glyph(1.0, &columns)];
    assert_eq!(fit_horizontally(&run, 11.0, false), -1.0);
    assert_eq!(fit_horizontally(&run, 11.0, true), 0.0);
    columns.reverse();
    assert_eq!(fit_horizontally(&[glyph(-1.0, &columns)], 11.0, false), 1.0);
}

#[test]
fn symbols_before_blank_cells_may_spread_into_them() {
    let row = |text: &str| -> Vec<TerminalCell> { text.chars().map(TerminalCell::from).collect() };
    // A symbol before a space spreads; before text or at the row's end it does not.
    assert_eq!(visual_columns(&row("→ "), 0, 1), 2);
    assert_eq!(visual_columns(&row("→x"), 0, 1), 1);
    assert_eq!(visual_columns(&row("→"), 0, 1), 1);
    assert_eq!(visual_columns(&row(" →"), 1, 1), 1);
    // Runs of symbols stay one cell each so they keep their alignment...
    assert_eq!(visual_columns(&row("→→ "), 1, 1), 1);
    // ...unless the previous symbol is a Powerline graphic.
    assert_eq!(visual_columns(&row("\u{e0b0}→ "), 1, 1), 2);
    // Ordinary text and declared wide cells keep their occupancy.
    assert_eq!(visual_columns(&row("W "), 0, 1), 1);
    assert_eq!(visual_columns(&row("∑ "), 0, 1), 1);
    let wide = [
        TerminalCell::wide("🙂", 2),
        TerminalCell::new(" "),
        TerminalCell::new(" "),
    ];
    assert_eq!(visual_columns(&wide, 0, 2), 2);
    assert!(is_symbol("🙂") && is_symbol("↔") && is_symbol("★") && is_symbol("\u{e0b0}"));
    assert!(!is_symbol("∑") && !is_symbol("◆") && !is_symbol("⣿") && !is_symbol("─"));
}

#[test]
fn block_elements_map_to_cell_fractions() {
    assert_eq!(block_element("█"), Some(&[(0.0, 0.0, 1.0, 1.0)][..]));
    assert_eq!(block_element("▄"), Some(&[(0.0, 0.5, 1.0, 1.0)][..]));
    assert_eq!(block_element("▁"), Some(&[(0.0, 0.875, 1.0, 1.0)][..]));
    assert_eq!(block_element("▏"), Some(&[(0.0, 0.0, 0.125, 1.0)][..]));
    assert_eq!(
        block_element("▚"),
        Some(&[(0.0, 0.0, 0.5, 0.5), (0.5, 0.5, 1.0, 1.0)][..])
    );
    // Shades keep their font glyphs; text and clusters are never blocks.
    assert_eq!(block_element("░"), None);
    assert_eq!(block_element("a"), None);
    assert_eq!(block_element("█\u{fe0f}"), None);
    // Every quadrant combination covers exactly the quadrants it names.
    for (symbol, quadrants) in [
        ("▖", 0b0010),
        ("▗", 0b0001),
        ("▘", 0b1000),
        ("▙", 0b1011),
        ("▚", 0b1001),
        ("▛", 0b1110),
        ("▜", 0b1101),
        ("▝", 0b0100),
        ("▞", 0b0110),
        ("▟", 0b0111),
    ] {
        let rects = block_element(symbol).expect(symbol);
        let covered = |x: f32, y: f32| {
            rects
                .iter()
                .any(|&(l, t, r, b)| x >= l && x < r && y >= t && y < b)
        };
        let mask = u8::from(covered(0.25, 0.25)) << 3
            | u8::from(covered(0.75, 0.25)) << 2
            | u8::from(covered(0.25, 0.75)) << 1
            | u8::from(covered(0.75, 0.75));
        assert_eq!(mask, quadrants, "{symbol}");
    }
}

/// A box-drawing bar drawn a fraction past its advance rasterises to one
/// faint column outside the span; that is overshoot to clip, not overhang
/// to push, or `┌` would land a pixel away from `│`.
#[test]
fn horizontal_fit_ignores_sub_pixel_overshoot() {
    // `─`: full-strength bar across the cell plus a 47% column past it.
    let mut bar = vec![255; 11];
    bar.push(120);
    assert_eq!(fit_horizontally(&[glyph(0.0, &bar)], 11.0, true), 0.0);
    // The same on the left (`┐`'s bar reaching into the previous cell).
    let mut bar = vec![120];
    bar.extend([255; 11]);
    assert_eq!(fit_horizontally(&[glyph(-1.0, &bar)], 11.0, true), 0.0);
    // Overshoot on both sides at once.
    let mut bar = vec![120];
    bar.extend([255; 11]);
    bar.push(120);
    assert_eq!(fit_horizontally(&[glyph(-1.0, &bar)], 11.0, true), 0.0);
    // A full-strength column outside the span is real overhang: pushed.
    assert_eq!(fit_horizontally(&[glyph(3.0, &[255; 9])], 11.0, true), -1.0);
    // Two faint columns are past the tolerance: the run is wider than the
    // span and placed by retained coverage, which keeps the solid columns.
    let mut bar = vec![255; 11];
    bar.extend([120, 120]);
    assert_eq!(fit_horizontally(&[glyph(0.0, &bar)], 11.0, true), 0.0);
    // A negative-bearing italic with a solid first column is still pushed.
    assert_eq!(fit_horizontally(&[glyph(-1.0, &[255; 9])], 11.0, true), 1.0);
}

#[test]
fn snapping_and_clipping_keep_glyphs_that_fit_inside_their_cell() {
    let cell = PixelGeometry {
        x: 22.0,
        y: 40.0,
        width: 11.0,
        height: 20.0,
    };
    // A glyph that fits mathematically survives snapping intact.
    let glyph = PixelGeometry {
        x: 22.0,
        y: 40.0,
        width: 11.0,
        height: 20.0,
    };
    let (clipped, _) = clip_glyph_to_row(glyph, Vec4::new(0.0, 0.0, 1.0, 1.0), cell).unwrap();
    let snapped = snap_geometry(clipped);
    assert_eq!(
        (snapped.x, snapped.y, snapped.width, snapped.height),
        (22.0, 40.0, 11.0, 20.0)
    );
    // A glyph a pixel below the cell loses exactly that pixel row and its UVs.
    let glyph = PixelGeometry {
        x: 22.0,
        y: 41.0,
        width: 11.0,
        height: 20.0,
    };
    let (clipped, uv) = clip_glyph_to_row(glyph, Vec4::new(0.0, 0.0, 1.0, 1.0), cell).unwrap();
    assert_eq!(clipped.height, 19.0);
    assert!((uv.w - 0.95).abs() < 1e-6, "{uv:?}");
    // Halves snap consistently: a rectangle at .5 keeps its size.
    let snapped = snap_geometry(PixelGeometry {
        x: 0.5,
        y: -0.5,
        width: 4.0,
        height: 4.0,
    });
    assert_eq!(
        (snapped.x, snapped.y, snapped.width, snapped.height),
        (1.0, 0.0, 4.0, 4.0)
    );
}

#[test]
fn grid_and_scale_helpers() {
    assert_eq!(
        grid_for(Vec2::new(10.0, 5.0), Vec2::splat(0.5)),
        GridSize::new(20, 10)
    );
    assert_eq!(
        grid_for(Vec2::new(f32::NAN, f32::INFINITY), Vec2::ONE),
        GridSize::new(1, 1)
    );
    let largest = grid_for(Vec2::splat(f32::MAX), Vec2::splat(f32::MIN_POSITIVE));
    assert!(largest.area() <= TerminalSurface::MAX_CELLS);
    assert!(largest.width > 0 && largest.height > 0);
    assert_eq!(
        grid_for(Vec2::new(805.0, 245.0), Vec2::new(10.0, 20.0)),
        GridSize::new(80, 12)
    );
    assert_eq!(
        grid_for(Vec2::ZERO, Vec2::new(10.0, 20.0)),
        GridSize::new(1, 1)
    );
}

#[test]
fn missing_text_resources_never_report_measured_geometry() {
    let mut app = test_app();
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(TerminalSurface::new((2, 1))))
        .id();
    app.update();
    let output = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(output.status, TerminalStatus::MissingTextResources);
    assert!(output.measured().is_none());
}

#[test]
fn invalid_and_oversized_geometry_is_observable_and_can_recover() {
    let mut app = text_app();
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(TerminalSurface::new((80, 24))),
            TerminalRenderConfig {
                sizing: TerminalSizing::Fixed {
                    cell_size: Vec2::splat(f32::NAN),
                    font_size: 12.0,
                },
                ..default()
            },
        ))
        .id();
    app.update();
    let initial = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(initial.status, TerminalStatus::InvalidSizing);
    let image = initial.image.clone();
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .sizing = TerminalSizing::Fixed {
        cell_size: Vec2::splat(10000.0),
        font_size: 12.0,
    };
    app.update();
    let output = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(output.status, TerminalStatus::TextureTooLarge);
    assert!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&image)
            .unwrap()
            .width()
            <= 8192
    );
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .sizing = TerminalSizing::default();
    for _ in 0..3 {
        app.update();
    }
    let output = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .measured()
        .unwrap();
    assert!(output.cell_size().cmpgt(Vec2::ZERO).all());
    assert_eq!(
        app.world().get::<TerminalTexture>(entity).unwrap().image,
        image
    );
}

#[test]
fn late_consumer_observes_current_geometry_and_resizes_keep_the_image() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 2));
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let initial = app.world().get::<TerminalTexture>(entity).unwrap().clone();
    assert!(initial.measured().is_some());
    surface.update(|update| {
        update.resize((8, 3));
    });
    assert!(
        initial.measured().is_none(),
        "a resize immediately invalidates old geometry"
    );
    app.update();
    let resized = app.world().get::<TerminalTexture>(entity).unwrap().clone();
    assert_eq!(resized.image, initial.image);
    assert_eq!(resized.measured().unwrap().grid(), GridSize::new(8, 3));
    assert_ne!(resized.geometry.size(), initial.geometry.size());
    app.update();
    assert_eq!(
        app.world().get::<TerminalTexture>(entity).unwrap(),
        &resized
    );
}

#[test]
fn measured_output_changes_when_only_logical_metrics_change() {
    let mut app = text_app();
    let regular = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                .to_vec(),
        ));
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(TerminalSurface::new((4, 2))),
            TerminalRenderConfig {
                sizing: super::super::TerminalSizing::FromFont {
                    font_size: 20.0,
                    line_height: 1.0,
                },
                font: super::super::FontFaces::regular(regular),
                raster: super::super::RasterConfig {
                    scale: 1.0,
                    ..default()
                },
                ..default()
            },
        ))
        .id();
    for _ in 0..3 {
        app.update();
    }
    let initial = app.world().get::<TerminalTexture>(entity).unwrap().clone();
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .raster
        .scale = 1.001;
    app.update();
    let texture = app.world().get::<TerminalTexture>(entity).unwrap();
    assert_eq!(
        texture.geometry.size(),
        initial.geometry.size(),
        "physical pixels remain snapped"
    );
    assert_ne!(texture.geometry.cell_size(), initial.geometry.cell_size());
    assert_ne!(
        texture.geometry.logical_size(),
        initial.geometry.logical_size()
    );
    let changed = texture.clone();
    app.update();
    assert_eq!(
        app.world().get::<TerminalTexture>(entity).unwrap(),
        &changed
    );
}

#[test]
fn pixel_rectangles_map_exactly_to_clip_space() {
    assert_eq!(
        clip_rect(
            PixelGeometry {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 480.0,
            },
            Vec2::new(800.0, 480.0),
        ),
        Vec4::new(-1.0, 1.0, 1.0, -1.0)
    );
    let cell = clip_rect(
        PixelGeometry {
            x: 400.0,
            y: 240.0,
            width: 10.0,
            height: 20.0,
        },
        Vec2::new(800.0, 480.0),
    );
    assert!(cell.abs_diff_eq(Vec4::new(0.0, 0.0, 0.025, -1.0 / 12.0), 1e-6));
}

#[test]
fn terminal_target_uses_nearest_sampling() {
    let image = make_target_image(UVec2::new(80, 40));
    assert_eq!(image.sampler, ImageSampler::nearest());
}

#[test]
fn explicit_raster_scale_is_bounded_and_invalid_values_fall_back() {
    assert_eq!(resolve_raster_scale(1.5), 1.5);
    assert_eq!(resolve_raster_scale(0.5), 1.0);
    assert_eq!(resolve_raster_scale(16.0), 8.0);
    for scale in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
        assert_eq!(resolve_raster_scale(scale), 1.0);
    }
}

#[test]
fn physical_metrics_and_geometry_are_pixel_aligned() {
    let physical = physical_config(
        LogicalMetrics {
            font_size: 17.6,
            cell_size: Vec2::new(10.8, 19.6),
        },
        2.0,
    );
    assert_eq!(physical.cell_size, Vec2::new(22.0, 39.0));
    assert!((physical.font_size - 35.2).abs() < 1e-4);

    assert_eq!(
        snap_geometry(PixelGeometry {
            x: 4.5,
            y: 9.5,
            width: 1.0,
            height: 2.0,
        }),
        PixelGeometry {
            x: 5.0,
            y: 10.0,
            width: 1.0,
            height: 2.0,
        }
    );
}

#[test]
fn font_driven_cells_refit_the_font_after_physical_pixel_rounding() {
    let raster = physical_config(
        LogicalMetrics {
            font_size: 23.0,
            cell_size: Vec2::new(11.5, 1.0),
        },
        1.0,
    );
    let from_font = TerminalRenderConfig {
        sizing: super::super::TerminalSizing::FromFont {
            font_size: 23.0,
            line_height: 1.0,
        },
        ..default()
    };
    assert_eq!(raster.cell_size.x, 12.0);
    assert_eq!(font_size_for_cell(&from_font, Some(32.0), raster), 24.0);

    let explicit = TerminalRenderConfig {
        sizing: super::super::TerminalSizing::Fixed {
            cell_size: Vec2::new(11.5, 20.0),
            font_size: 23.0,
        },
        ..from_font
    };
    assert_eq!(font_size_for_cell(&explicit, Some(32.0), raster), 23.0);
}

#[test]
fn glyph_bitmaps_are_clipped_to_their_row_band() {
    let clipped = clip_glyph_to_row(
        PixelGeometry {
            x: -2.0,
            y: 3.0,
            width: 16.0,
            height: 20.0,
        },
        Vec4::new(0.1, 0.2, 0.9, 0.8),
        PixelGeometry {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 10.0,
        },
    )
    .expect("the glyph overlaps the row");

    // Ink past the row's bottom or the texture's edge is dropped; columns
    // past the glyph's own cell are kept.
    assert_eq!(
        clipped.0,
        PixelGeometry {
            x: 0.0,
            y: 3.0,
            width: 14.0,
            height: 7.0,
        }
    );
    assert!(clipped.1.abs_diff_eq(Vec4::new(0.2, 0.2, 0.9, 0.41), 1e-6));
    assert!(
        clip_glyph_to_row(
            PixelGeometry {
                x: 20.0,
                y: 20.0,
                width: 5.0,
                height: 5.0,
            },
            Vec4::ONE,
            PixelGeometry {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        )
        .is_none()
    );
}

#[test]
fn glyph_batches_preserve_paint_order_and_coalesce_adjacent_atlases() {
    let mut images = Assets::<Image>::default();
    let atlas_a = images.add(Image::default()).id();
    let atlas_b = images.add(Image::default()).id();
    let glyphs = vec![
        (atlas_a, quad(1.0)),
        (atlas_a, quad(2.0)),
        (atlas_b, quad(3.0)),
        (atlas_a, quad(4.0)),
    ];
    let mut instances = Vec::new();
    let mut batches = Vec::new();
    append_glyph_batches(&mut instances, &mut batches, &glyphs);

    assert_eq!(instances.len(), 4);
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].texture, atlas_a);
    assert_eq!((batches[0].start, batches[0].count), (0, 2));
    assert_eq!(batches[1].texture, atlas_b);
    assert_eq!((batches[1].start, batches[1].count), (2, 1));
    assert_eq!(batches[2].texture, atlas_a);
    assert_eq!((batches[2].start, batches[2].count), (3, 1));
    assert_eq!(instances[0].rect, Vec4::splat(1.0));
    assert_eq!(instances[1].rect, Vec4::splat(2.0));
    assert_eq!(instances[2].rect, Vec4::splat(3.0));
    assert_eq!(instances[3].rect, Vec4::splat(4.0));
}

#[test]
fn replacement_batches_never_address_stale_capacity() {
    let mut images = Assets::<Image>::default();
    let atlas = images.add(Image::default()).id();
    let mut instances = Vec::with_capacity(32);
    let mut batches = Vec::new();
    let first = vec![quad(1.0); 12];
    append_batch(&mut instances, &mut batches, atlas, &first);
    assert_eq!(batches[0].count, 12);

    instances.clear();
    batches.clear();
    let second = vec![quad(2.0); 2];
    append_batch(&mut instances, &mut batches, atlas, &second);
    assert_eq!(instances.len(), 2);
    assert_eq!((batches[0].start, batches[0].count), (0, 2));
}

#[test]
fn empty_scene_produces_no_upload_or_draw_batch() {
    let mut images = Assets::<Image>::default();
    let atlas = images.add(Image::default()).id();
    let mut instances = Vec::new();
    let mut batches = Vec::new();
    append_batch(&mut instances, &mut batches, atlas, &[]);
    append_glyph_batches(&mut instances, &mut batches, &[]);
    assert!(instances.is_empty());
    assert!(batches.is_empty());
    let mut bytes = Vec::new();
    append_instance_bytes(&instances, &mut bytes);
    assert!(bytes.is_empty());
}

#[test]
fn unified_atlas_copies_each_bevy_glyph_once_and_reuses_its_uv() {
    let mut images = Assets::<Image>::default();
    let mut source_pixels = vec![0; 4 * 4 * 4];
    let source_offset = (4 + 1) * 4;
    source_pixels[source_offset..source_offset + 4].copy_from_slice(&[11, 22, 33, 44]);
    let source = images.add(Image::new(
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        source_pixels,
        GLYPH_FORMAT,
        RenderAssetUsages::MAIN_WORLD,
    ));
    let target = images.add(make_glyph_atlas_image());
    let mut atlas = UnifiedGlyphAtlas::new(target.clone());
    let glyph = SourceGlyph {
        texture: source.id(),
        x: 1,
        y: 1,
        width: 1,
        height: 1,
    };

    let first = atlas.cache(glyph, &mut images).expect("glyph should fit");
    let cursor = atlas.cursor;
    let second = atlas
        .cache(glyph, &mut images)
        .expect("glyph should be cached");
    assert_eq!(first, second);
    assert_eq!(atlas.cursor, cursor);
    assert_eq!(atlas.glyphs.len(), 1);

    let target_offset = (GLYPH_ATLAS_SIZE as usize + 1) * 4;
    {
        let target = images.get(&target).expect("target atlas exists");
        assert_eq!(
            &target.data.as_ref().expect("atlas has CPU data")[target_offset..target_offset + 4],
            &[11, 22, 33, 44]
        );
    }

    atlas.clear(&mut images);
    assert!(atlas.glyphs.is_empty());
    assert_eq!(atlas.cursor, UVec2::splat(1));
    assert_eq!(atlas.row_height, 0);
    let target = images.get(&target).expect("target atlas exists");
    assert_eq!(
        &target.data.as_ref().expect("atlas has CPU data")[target_offset..target_offset + 4],
        &[0, 0, 0, 0]
    );
}

#[test]
fn blink_phases_follow_slow_rapid_and_disabled_cursor_rates() {
    let mut config = TerminalRenderConfig {
        blink: super::super::BlinkConfig {
            slow_hz: Some(1.0),
            rapid_hz: Some(2.0),
        },
        cursor: super::super::CursorConfig {
            blink_hz: None,
            ..default()
        },
        ..default()
    };
    let visible = BlinkPhases::at(0.1, &config);
    assert!(!visible.slow_hidden && !visible.rapid_hidden && !visible.cursor_hidden);
    let hidden = BlinkPhases::at(0.3, &config);
    assert!(!hidden.slow_hidden && hidden.rapid_hidden && !hidden.cursor_hidden);

    config.cursor.blink_hz = Some(1.0);
    assert!(BlinkPhases::at(0.6, &config).cursor_hidden);
}

/// A text app with a manually driven clock, for deterministic blink tests.
fn timed_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.build().disable::<bevy::time::TimePlugin>(),
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        TerminalPlugin,
    ))
    .init_asset::<Image>()
    .init_resource::<Time>();
    app
}

fn advance(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(seconds));
}

/// Consumes pending payloads the way render-world extraction would, so a
/// later partial repaint is not upgraded to a full one.
fn drain_pending(app: &mut App) {
    let mut states = app.world_mut().query::<&mut BatchMainState>();
    for mut state in states.iter_mut(app.world_mut()) {
        state.pending = None;
        state.submitted.store(state.generation, Ordering::Release);
    }
}

#[test]
fn delayed_scenes_coalesce_completely_and_removed_renderers_release_state() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 3));
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }
    drain_pending(&mut app);
    let mut pending = PendingBatchScenes::default();
    let target = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .image
        .id();

    surface.update(|update| {
        update.set_cell((0, 0), &TerminalCell::new("A"));
    });
    app.update();
    collect_batch_scenes(app.world_mut(), &mut pending);
    assert!(
        !pending.scenes[&target].clear,
        "acknowledged base permits a partial repaint"
    );

    // Extraction without submission models delayed GPU assets. The next scene
    // must contain both edits, even though only the second row just changed.
    surface.update(|update| {
        update.set_cell((0, 1), &TerminalCell::new("B"));
    });
    app.update();
    collect_batch_scenes(app.world_mut(), &mut pending);
    assert_eq!(pending.scenes.len(), 1);
    assert!(pending.scenes[&target].clear);
    let snapshot = app
        .world()
        .get::<BatchMainState>(entity)
        .unwrap()
        .last_snapshot
        .as_ref()
        .unwrap();
    assert_eq!(snapshot.row_text(0), "A   ");
    assert_eq!(snapshot.row_text(1), "B   ");
    assert_eq!(
        app.world()
            .get::<TerminalStats>(entity)
            .unwrap()
            .changed_rows,
        3
    );

    for width in [8, 2, 6] {
        surface.update(|update| {
            update.resize((width, 3));
        });
        app.update();
        collect_batch_scenes(app.world_mut(), &mut pending);
        assert_eq!(pending.scenes.len(), 1);
        assert!(pending.scenes[&target].clear);
        assert_eq!(
            pending.scenes[&target].destination_size,
            app.world()
                .get::<TerminalTexture>(entity)
                .unwrap()
                .geometry
                .size()
        );
    }

    app.world_mut()
        .entity_mut(entity)
        .remove::<TerminalRenderer>();
    collect_batch_scenes(app.world_mut(), &mut pending);
    assert!(pending.scenes.is_empty());
    assert!(
        pending.live_textures.is_empty(),
        "removed owner cannot keep bind groups alive"
    );
    app.update();
    assert!(app.world().get::<BatchMainState>(entity).is_none());
    assert!(app.world().get::<TerminalTexture>(entity).is_none());
}

#[test]
fn blink_phases_only_rebuild_blinking_content() {
    let mut app = timed_app();
    let surface = TerminalSurface::new((6, 3));
    write_text(&surface, "hello!");
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }

    // No blinking cells, hidden cursor: a text/cursor phase flip is
    // invisible and must not rebuild anything.
    advance(&mut app, 0.6);
    app.update();
    let idle = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(idle.changed_rows, 0, "{idle}");

    // A visible blinking cursor dirties only its own row on a phase flip.
    surface.update(|update| {
        update.set_cursor_position((0, 2));
        update.set_cursor_visible(true);
    });
    app.update();
    drain_pending(&mut app);
    advance(&mut app, 0.5);
    app.update();
    let cursor_only = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(cursor_only.changed_rows, 1, "{cursor_only}");

    // A SLOW_BLINK cell restores the full-surface phase rebuild.
    surface.update(|update| {
        let mut cell = TerminalCell::new("x");
        cell.style = TerminalStyle::new().with(StyleFlags::SLOW_BLINK);
        update.set_cell((0, 0), &cell);
    });
    app.update();
    drain_pending(&mut app);
    advance(&mut app, 0.5);
    app.update();
    let blinking = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(blinking.changed_rows, 3, "{blinking}");
}

#[test]
fn full_rebuilds_merge_identical_backgrounds_vertically() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 3));
    surface.update(|update| {
        for row in 0..3 {
            for column in 0..4 {
                let mut cell = TerminalCell::new(" ");
                cell.style = TerminalStyle::new().bg(crate::scene::TerminalColor::Rgb(200, 30, 30));
                update.set_cell((column, row), &cell);
            }
        }
    });
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }
    // Force a full rebuild and check the uniform background collapsed into
    // a single quad instead of one per row.
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .sizing = super::super::TerminalSizing::FitCellWidth(Vec2::new(12.0, 22.0));
    app.update();
    let stats = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(stats.changed_rows, 3, "{stats}");
    assert_eq!(stats.solid_quads, 1, "{stats}");
}

#[test]
fn ascii_and_non_ascii_symbols_reuse_the_shape_cache() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 1));
    write_text(&surface, "abéé");
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }
    // Re-shuffling the same symbols shapes nothing new: the ASCII fast
    // path and the map fallback both hit.
    write_text(&surface, "ébéa");
    app.update();
    let stats = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(stats.shape_misses, 0, "{stats}");
    assert_eq!(stats.changed_rows, 1, "{stats}");
    // A genuinely new symbol still misses once.
    write_text(&surface, "cbéa");
    app.update();
    let stats = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!(stats.shape_misses, 1, "{stats}");
}

#[test]
fn instance_bytes_append_whole_instances_in_order() {
    let instances = [
        QuadInstance {
            rect: Vec4::new(1.0, 2.0, 3.0, 4.0),
            uv: Vec4::new(5.0, 6.0, 7.0, 8.0),
            color: Vec4::new(9.0, 10.0, 11.0, 12.0),
        },
        quad(42.0),
    ];
    let mut bytes = Vec::new();
    append_instance_bytes(&instances, &mut bytes);
    assert_eq!(bytes.len(), 96);
    let floats: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_ne_bytes(*chunk))
        .collect();
    assert_eq!(
        &floats[..12],
        &[
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0
        ]
    );
    assert_eq!(&floats[12..16], &[42.0; 4]);
    // Appending again extends at the previous end, as the shared staging
    // buffer relies on.
    append_instance_bytes(&instances[1..], &mut bytes);
    assert_eq!(bytes.len(), 144);
}

#[test]
fn invalid_scale_settles_without_idle_redraws() {
    let mut app = text_app();
    let mut entities = Vec::new();
    for (scale, expected) in [
        (f32::NAN, 1.0),
        (f32::INFINITY, 1.0),
        (f32::NEG_INFINITY, 1.0),
        (0.0, 1.0),
        (-1.0, 1.0),
        (0.5, 1.0),
        (9.0, 8.0),
    ] {
        let surface = TerminalSurface::new((4, 2));
        write_text(&surface, "A");
        let entity = app
            .world_mut()
            .spawn((
                TerminalRenderer::new(surface),
                TerminalRenderConfig {
                    raster: super::super::RasterConfig { scale, ..default() },
                    blink: super::super::BlinkConfig {
                        slow_hz: Some(f32::NAN),
                        rapid_hz: Some(f32::INFINITY),
                    },
                    ..default()
                },
            ))
            .id();
        entities.push((entity, expected));
    }
    for _ in 0..4 {
        app.update();
    }
    for (entity, expected) in entities {
        let output = app.world().get::<TerminalTexture>(entity).unwrap();
        assert!(output.measured().is_some());
        assert_eq!(output.geometry.raster_scale(), expected);
        let generation = app
            .world()
            .get::<BatchMainState>(entity)
            .unwrap()
            .generation;
        for _ in 0..3 {
            // Even touching the raw component without changing its effective
            // value must not turn NaN into a perpetual invalidation.
            app.world_mut()
                .get_mut::<TerminalRenderConfig>(entity)
                .unwrap()
                .set_changed();
            app.update();
            assert_eq!(
                app.world()
                    .get::<BatchMainState>(entity)
                    .unwrap()
                    .generation,
                generation
            );
            assert_eq!(
                *app.world().get::<TerminalStats>(entity).unwrap(),
                TerminalStats::default()
            );
        }
    }
}

#[test]
fn stats_reset_when_font_starts_loading() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 2));
    write_text(&surface, "A");
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }
    write_text(&surface, "B");
    app.update();
    let previous = app
        .world()
        .get::<TerminalStats>(entity)
        .unwrap()
        .changed_rows;
    assert!(previous > 0);
    let missing = app.world().resource::<Assets<Font>>().reserve_handle();
    app.world_mut()
        .get_mut::<TerminalRenderConfig>(entity)
        .unwrap()
        .font = super::super::FontFaces::regular(missing);
    for _ in 0..3 {
        app.update();
        assert_eq!(
            app.world().get::<TerminalTexture>(entity).unwrap().status,
            TerminalStatus::Loading
        );
        assert_eq!(
            app.world()
                .get::<TerminalStats>(entity)
                .unwrap()
                .changed_rows,
            0
        );
        assert!(
            app.world()
                .get::<BatchMainState>(entity)
                .unwrap()
                .pending
                .is_none()
        );
    }
}

#[test]
fn idle_terminals_do_not_rescan_fonts_or_publish_statistics() {
    #[derive(Resource, Default)]
    struct StatsChanges(usize);
    let mut app = text_app();
    app.init_resource::<StatsChanges>().add_systems(
        Update,
        (|changed: Query<Entity, Changed<TerminalStats>>, mut count: ResMut<StatsChanges>| {
            count.0 += changed.iter().count();
        })
        .after(super::super::TerminalSystems::Sync),
    );
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<FontCatalog>().scans, 0);
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(TerminalSurface::new((4, 2))))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let scans = app.world().resource::<FontCatalog>().scans;
    assert!(scans > 0);
    app.world_mut().resource_mut::<StatsChanges>().0 = 0;
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(app.world().resource::<FontCatalog>().scans, scans);
    assert_eq!(app.world().resource::<StatsChanges>().0, 0);
    app.world_mut().despawn(entity);
    app.update();
    app.world_mut()
        .spawn(TerminalRenderer::new(TerminalSurface::new((4, 2))));
    app.update();
    assert!(app.world().resource::<FontCatalog>().scans > scans);
}

#[test]
fn glyph_atlas_stays_small_until_actual_glyphs_are_drawn() {
    let mut app = text_app();
    let surface = TerminalSurface::new((4, 2));
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(surface.clone()))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let atlas = app
        .world()
        .get::<BatchMainState>(entity)
        .unwrap()
        .glyph_atlas
        .image
        .clone();
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&atlas)
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .len(),
        4
    );
    write_text(&surface, "A");
    app.update();
    assert_eq!(
        app.world()
            .get::<BatchMainState>(entity)
            .unwrap()
            .glyph_atlas
            .image,
        atlas
    );
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&atlas)
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .len(),
        2048 * 2048 * 4
    );
}

#[test]
fn retained_geometry_does_not_keep_surface_content_alive() {
    let mut app = text_app();
    let entity = app
        .world_mut()
        .spawn(TerminalRenderer::new(TerminalSurface::new((4, 2))))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let geometry = app
        .world()
        .get::<TerminalTexture>(entity)
        .unwrap()
        .measured()
        .unwrap()
        .clone();
    assert!(geometry.is_current());
    app.world_mut().despawn(entity);
    assert!(!geometry.is_current());
}

/// Ink rectangle of the cached run for `text` over `columns` cells, in cell
/// coordinates before the uniform text offset, read back from the unified
/// atlas the way the scene draws it.
fn cached_ink(app: &App, entity: Entity, text: &str, columns: u16) -> Rect {
    let state = app.world().get::<BatchMainState>(entity).unwrap();
    let index = state
        .shapes
        .lookup(&ResolvedStyle::plain(), text, columns)
        .unwrap_or_else(|| panic!("{text:?} over {columns} cells is cached"));
    let images = app.world().resource::<Assets<Image>>();
    let atlas = images.get(&state.glyph_atlas.image).unwrap();
    let data = atlas.data.as_ref().unwrap();
    let mut ink: Option<Rect> = None;
    for glyph in &state.shapes.entries[index] {
        assert_eq!(glyph.texture, state.glyph_atlas.image.id());
        let rect = glyph.uv * GLYPH_ATLAS_SIZE as f32;
        for y in 0..glyph.size.y as u32 {
            for x in 0..glyph.size.x as u32 {
                let offset =
                    ((rect.y as u32 + y) * GLYPH_ATLAS_SIZE + rect.x as u32 + x) as usize * 4;
                if data[offset + 3] == 0 {
                    continue;
                }
                let pixel = glyph.offset + Vec2::new(x as f32, y as f32);
                let pixel = Rect::from_corners(pixel, pixel + Vec2::ONE);
                ink = Some(ink.map_or(pixel, |ink| ink.union(pixel)));
            }
        }
    }
    ink.unwrap_or_else(|| panic!("{text:?} has ink"))
}

#[test]
fn wide_symbols_are_rescaled_to_their_cells_and_ordinary_text_overflows() {
    let mut app = text_app();
    // Regular Iosevka's double-advance `↔∑∞◆★`: symbols and ordinary text
    // whose ink is twice as wide as the one cell the terminal assigns them.
    let regular = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            include_bytes!("../../../assets/fonts/fidelity/FidelityWideSymbols.ttf").to_vec(),
        ));
    let surface = TerminalSurface::new((8, 1));
    write_text(&surface, "↔ ↔↔∑ ★x");
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(surface.clone()),
            TerminalRenderConfig {
                font: super::super::FontFaces::regular(regular),
                sizing: TerminalSizing::FitCellWidth(Vec2::new(11.0, 20.0)),
                ..default()
            },
        ))
        .id();
    for _ in 0..6 {
        app.update();
    }
    let raster = app
        .world()
        .get::<BatchMainState>(entity)
        .unwrap()
        .raster_config;
    let cell = raster.cell_size;
    assert!(raster.font_size > 20.0, "{raster:?}");
    let inside = |ink: Rect, columns: f32| {
        ink.min.x >= 0.0
            && ink.max.x <= columns * cell.x
            && ink.min.y + raster.glyph_offset >= 0.0
            && ink.max.y + raster.glyph_offset <= cell.y
    };

    // A symbol before a blank cell keeps its size and spreads into that cell.
    let spread = cached_ink(&app, entity, "↔", 2);
    assert!(
        spread.width() > cell.x && inside(spread, 2.0),
        "{spread:?} {cell:?}"
    );
    // Before another symbol it is rescaled to its own cell: complete, uniform
    // and no smaller than needed.
    let fitted = cached_ink(&app, entity, "↔", 1);
    assert!(inside(fitted, 1.0), "{fitted:?} {cell:?}");
    assert!(
        fitted.width() >= cell.x - 2.0,
        "{fitted:?} is smaller than needed"
    );
    assert!(
        fitted.height() < spread.height(),
        "{fitted:?} vs {spread:?}"
    );
    let aspect = (fitted.width() / fitted.height()) / (spread.width() / spread.height());
    assert!(
        (0.8..=1.25).contains(&aspect),
        "non-uniform rescale: {aspect}"
    );
    let star = cached_ink(&app, entity, "★", 1);
    assert!(
        inside(star, 1.0) && star.width() >= cell.x - 2.0,
        "{star:?}"
    );
    // Ordinary text is drawn as shaped; the wide `∑` overflows its neighbour.
    let sum = cached_ink(&app, entity, "∑", 1);
    assert!(sum.width() > cell.x && sum.min.x >= 0.0, "{sum:?} {cell:?}");
    assert!(
        app.world()
            .get::<BatchMainState>(entity)
            .unwrap()
            .shapes
            .lookup(&ResolvedStyle::plain(), "∑", 2)
            .is_none()
    );

    // Blanking the cell after `★` widens its allowance: a new shape; the
    // one-cell shape is kept for when the neighbour is written again.
    surface.update(|u| {
        u.set_cell((7, 0), &TerminalCell::new(" "));
    });
    app.update();
    let stats = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!((stats.shape_misses, stats.changed_rows), (1, 1), "{stats}");
    let spread_star = cached_ink(&app, entity, "★", 2);
    assert!(spread_star.width() > star.width() && inside(spread_star, 2.0));
    surface.update(|u| {
        u.set_cell((7, 0), &TerminalCell::new("x"));
    });
    app.update();
    let stats = *app.world().get::<TerminalStats>(entity).unwrap();
    assert_eq!((stats.shape_misses, stats.changed_rows), (0, 1), "{stats}");
}
