use bevy::{app::SubApps, prelude::*, text::FontSource};
use bevy_grid::{
    BevyBackend, BevyGridBatchPlugin, TerminalBatchStats, TerminalRenderConfig, TerminalSurface,
};
use ratatui::Terminal;
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    linear_rgba8_to_srgb, measure, read_bevy_image_rgba, render_workload, run,
};

struct BevyGridAdapter {
    terminal: Terminal<BevyBackend>,
    surface: TerminalSurface,
    last_stats: TerminalBatchStats,
    max_extracted_bytes: u64,
    max_shape_misses: u32,
}

impl RendererAdapter for BevyGridAdapter {
    fn new(config: &BenchConfig) -> BenchResult<Self> {
        let backend = BevyBackend::new(config.cols, config.rows);
        let surface = backend.surface();
        Ok(Self {
            terminal: Terminal::new(backend)?,
            surface,
            last_stats: TerminalBatchStats::default(),
            max_extracted_bytes: 0,
            max_shape_misses: 0,
        })
    }

    fn configure_app(&mut self, app: &mut App, config: &BenchConfig) -> BenchResult<()> {
        let bytes = app.world().resource::<SharedFontFixture>().0.to_vec();
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(bytes));
        app.add_plugins(
            BevyGridBatchPlugin::new(self.surface.clone())
                .with_config(TerminalRenderConfig {
                    cell_size: Vec2::new(config.cell_width, config.cell_height),
                    font_size: config.font_size as f32,
                    font: FontSource::Handle(font),
                    cursor_blink_hz: None,
                    slow_blink_hz: 0.0,
                    rapid_blink_hz: 0.0,
                    ..default()
                })
                .headless(),
        );
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
        self.last_stats = *world.resource::<TerminalBatchStats>();
        self.max_extracted_bytes = self
            .max_extracted_bytes
            .max(self.last_stats.extracted_bytes);
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
            .resource::<bevy_grid::TerminalBatchOutput>()
            .image
            .clone();
        let mut rgba = read_bevy_image_rgba(sub_apps, image)?;
        // The batch target is linear RGBA8 because the shader emits linear colors. Normalize the
        // diagnostic PNG bytes to the same visual encoding used by the other adapters.
        linear_rgba8_to_srgb(&mut rgba);
        Ok(rgba)
    }

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "bevy_grid".to_owned(),
            name: "bevy_grid".to_owned(),
            renderer_version: env!("CARGO_PKG_VERSION").to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Ratatui diff -> compact Bevy text-atlas quad batch -> renderer-owned texture"
                .to_owned(),
            notes: vec![
                "The terminal texture is rendered directly in Bevy RenderApp; no benchmark camera is spawned"
                    .to_owned(),
                "Bevy text shaping, glyph atlas preparation, extraction, upload, render submission, and synchronization are included"
                    .to_owned(),
                format!(
                    "renderer counters: glyph_quads={}, solid_quads={}, batches={}, last_changed_rows={}, last_snapshot_cells={}, cached_shapes={}, max_shape_misses={}, max_extracted_bytes={}, snapshot_ns={}, scene_ns={}, gpu_buffer_reallocations={}, gpu_write_calls={}, gpu_bytes_written={}, render_passes={}, draw_calls={}, pipeline_switches={}, atlas_bindings={}",
                    self.last_stats.glyph_quads,
                    self.last_stats.solid_quads,
                    self.last_stats.draw_batches,
                    self.last_stats.changed_rows,
                    self.last_stats.snapshot_cells,
                    self.last_stats.cached_shapes,
                    self.max_shape_misses,
                    self.max_extracted_bytes,
                    self.last_stats.snapshot_ns,
                    self.last_stats.scene_ns,
                    self.last_stats.gpu_buffer_reallocations,
                    self.last_stats.gpu_write_calls,
                    self.last_stats.gpu_bytes_written,
                    self.last_stats.render_passes,
                    self.last_stats.draw_calls,
                    self.last_stats.pipeline_switches,
                    self.last_stats.atlas_bindings,
                ),
            ],
        }
    }
}

fn main() -> BenchResult<()> {
    run::<BevyGridAdapter>()
}
