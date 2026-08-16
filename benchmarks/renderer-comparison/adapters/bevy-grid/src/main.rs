use bevy::{prelude::*, text::FontSource};
use bevy_grid::{
    BevyBackend, BevyGridPlugin, TerminalRenderConfig, TerminalRenderStats, TerminalSurface,
};
use ratatui::Terminal;
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    measure, render_workload, run, spawn_offscreen_ui_target,
};

struct BevyGridAdapter {
    terminal: Terminal<BevyBackend>,
    surface: TerminalSurface,
    last_stats: TerminalRenderStats,
    max_pooled_primitives: u32,
    max_spawned_primitives: u32,
}

impl RendererAdapter for BevyGridAdapter {
    fn new(config: &BenchConfig) -> BenchResult<Self> {
        let backend = BevyBackend::new(config.cols, config.rows);
        let surface = backend.surface();
        Ok(Self {
            terminal: Terminal::new(backend)?,
            surface,
            last_stats: TerminalRenderStats::default(),
            max_pooled_primitives: 0,
            max_spawned_primitives: 0,
        })
    }

    fn configure_app(&mut self, app: &mut App, config: &BenchConfig) -> BenchResult<()> {
        let bytes = app.world().resource::<SharedFontFixture>().0.to_vec();
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(bytes));
        app.add_plugins(BevyGridPlugin::new(self.surface.clone()).with_config(
            TerminalRenderConfig {
                cell_size: Vec2::new(config.cell_width, config.cell_height),
                font_size: config.font_size as f32,
                font: FontSource::Handle(font),
                cursor_blink_hz: None,
                slow_blink_hz: 0.0,
                rapid_blink_hz: 0.0,
                ..default()
            },
        ));
        Ok(())
    }

    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()> {
        spawn_offscreen_ui_target(world, config);
        Ok(())
    }

    fn render_frame(
        &mut self,
        world: &mut World,
        config: &BenchConfig,
        frame_index: u64,
    ) -> BenchResult<AdapterFrame> {
        self.last_stats = *world.resource::<TerminalRenderStats>();
        self.max_pooled_primitives = self
            .max_pooled_primitives
            .max(self.last_stats.pooled_primitives);
        self.max_spawned_primitives = self
            .max_spawned_primitives
            .max(self.last_stats.spawned_primitives);
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

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "bevy_grid".to_owned(),
            name: "bevy_grid".to_owned(),
            renderer_version: env!("CARGO_PKG_VERSION").to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Ratatui diff -> retained Bevy UI nodes/text -> offscreen Bevy UI render"
                .to_owned(),
            notes: vec![
                "No intermediate terminal texture upload; output_size is the common Bevy camera target"
                    .to_owned(),
                "Bevy UI synchronization and all text layout/render work are included in bevy_update_ns"
                    .to_owned(),
                format!(
                    "renderer counters: active_text={}, active_solids={}, pooled={}, max_pooled={}, last_changed_rows={}, last_snapshot_cells={}, max_spawned_in_sync={}",
                    self.last_stats.active_text_primitives,
                    self.last_stats.active_solid_primitives,
                    self.last_stats.pooled_primitives,
                    self.max_pooled_primitives,
                    self.last_stats.changed_rows,
                    self.last_stats.snapshot_cells,
                    self.max_spawned_primitives,
                ),
            ],
        }
    }
}

fn main() -> BenchResult<()> {
    run::<BevyGridAdapter>()
}
