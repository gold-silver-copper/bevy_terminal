use bevy::{app::SubApps, prelude::*, text::FontSource};
use bevy_terminal_ratatui::{RatatuiBackend, TerminalRenderer};
use bevy_terminal_ratatui::prelude::{BlinkConfig, CursorConfig, FontSizing, TerminalPlugin, TerminalRenderConfig, TerminalStats, TerminalSurface, TerminalTexture};
use ratatui::Terminal;
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    measure, read_bevy_image_rgba, render_workload, run,
};

struct BevyTerminalRatatuiAdapter {
    terminal: Terminal<RatatuiBackend>,
    surface: TerminalSurface,
    last_stats: TerminalStats,
    max_glyph_quads: u32,
    max_shape_misses: u32,
}

impl RendererAdapter for BevyTerminalRatatuiAdapter {
    fn new(config: &BenchConfig) -> BenchResult<Self> {
        let backend = RatatuiBackend::new(config.cols, config.rows);
        let surface = backend.surface();
        Ok(Self {
            terminal: Terminal::new(backend)?,
            surface,
            last_stats: TerminalStats::default(),
            max_glyph_quads: 0,
            max_shape_misses: 0,
        })
    }

    fn configure_app(&mut self, app: &mut App, config: &BenchConfig) -> BenchResult<()> {
        let bytes = app.world().resource::<SharedFontFixture>().0.to_vec();
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(bytes));
        app.add_plugins(TerminalPlugin);
        app.world_mut().spawn((
            TerminalRenderer::new(self.surface.clone()),
            TerminalRenderConfig {
                cell_size: Vec2::new(config.cell_width, config.cell_height).into(),
                font_size: FontSizing::Px(config.font_size as f32),
                font: FontSource::Handle(font).into(),
                cursor: CursorConfig {
                    blink_hz: None,
                    ..default()
                },
                blink: BlinkConfig::NONE,
                ..default()
            },
        ));
        Ok(())
    }

    fn initialize(&mut self, _world: &mut World, _config: &BenchConfig) -> BenchResult<()> {
        Ok(())
    }

    fn render_frame(
        &mut self,
        world: &mut World,
        config: &BenchConfig,
        frame_index: u64,
    ) -> BenchResult<AdapterFrame> {
        // Stats are attached by TerminalPlugin during the first update, so they
        // may not exist yet on the very first measured frame.
        self.last_stats = world
            .iter_entities()
            .find_map(|entity| entity.get::<TerminalStats>().copied())
            .unwrap_or_default();
        self.max_glyph_quads = self.max_glyph_quads.max(self.last_stats.glyph_quads);
        self.max_shape_misses = self.max_shape_misses.max(self.last_stats.shape_misses);
        let (result, draw_ns) = measure(|| {
            self.terminal.draw(|frame| {
                render_workload(frame, config.workload, frame_index);
            })
        });
        result?;
        Ok(AdapterFrame {
            draw_ns,
            ..default()
        })
    }

    fn capture_rgba(
        &mut self,
        sub_apps: &mut SubApps,
        _config: &BenchConfig,
    ) -> BenchResult<Vec<u8>> {
        let image = sub_apps
            .main
            .world()
            .iter_entities()
            .find_map(|entity| entity.get::<TerminalTexture>())
            .expect("the adapter creates exactly one terminal output")
            .image
            .clone();
        // The terminal texture is sRGB-encoded, matching the other adapters' capture encoding.
        read_bevy_image_rgba(sub_apps, image)
    }

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "bevy_terminal_ratatui".to_owned(),
            name: "bevy_terminal_ratatui".to_owned(),
            renderer_version: env!("CARGO_PKG_VERSION").to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Ratatui diff -> bevy_terminal neutral surface -> compact Bevy terminal renderer -> renderer-owned texture"
                .to_owned(),
            notes: vec![
                "The terminal texture is rendered directly in Bevy RenderApp; no benchmark camera is spawned"
                    .to_owned(),
                "Bevy text shaping, glyph atlas preparation, extraction, upload, render submission, and synchronization are included"
                    .to_owned(),
                format!(
                    "renderer counters: glyph_quads={}, solid_quads={}, batches={}, last_changed_rows={}, last_snapshot_cells={}, max_shape_misses={}, snapshot_ns={}, scene_ns={}, max_glyph_quads={}",
                    self.last_stats.glyph_quads,
                    self.last_stats.solid_quads,
                    self.last_stats.draw_batches,
                    self.last_stats.changed_rows,
                    self.last_stats.snapshot_cells,
                    self.max_shape_misses,
                    self.last_stats.snapshot_ns,
                    self.last_stats.scene_ns,
                    self.max_glyph_quads,
                ),
            ],
        }
    }
}

fn main() -> BenchResult<()> {
    run::<BevyTerminalRatatuiAdapter>()
}
