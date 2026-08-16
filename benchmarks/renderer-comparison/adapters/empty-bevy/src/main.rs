use bevy::prelude::*;
use ratatui::{Terminal, backend::TestBackend};
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, measure,
    render_workload, run, spawn_offscreen_ui_target,
};

/// Diagnostic floor: canonical Ratatui work plus an otherwise empty Bevy
/// offscreen UI camera. This is intentionally not registered as a renderer.
struct EmptyBevyAdapter {
    terminal: Terminal<TestBackend>,
}

impl RendererAdapter for EmptyBevyAdapter {
    fn new(config: &BenchConfig) -> BenchResult<Self> {
        Ok(Self {
            terminal: Terminal::new(TestBackend::new(config.cols, config.rows))?,
        })
    }

    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()> {
        spawn_offscreen_ui_target(world, config);
        Ok(())
    }

    fn render_frame(
        &mut self,
        _world: &mut World,
        config: &BenchConfig,
        frame_index: u64,
    ) -> BenchResult<AdapterFrame> {
        let (result, draw_ns) = measure(|| {
            self.terminal
                .draw(|frame| render_workload(frame, config.workload, frame_index))
        });
        result?;
        Ok(AdapterFrame {
            draw_ns,
            ..default()
        })
    }

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "empty_bevy".to_owned(),
            name: "empty Bevy diagnostic floor".to_owned(),
            renderer_version: env!("CARGO_PKG_VERSION").to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Ratatui TestBackend -> empty offscreen Bevy UI camera".to_owned(),
            notes: vec![
                "Diagnostic only: renders no terminal pixels and is excluded from renderer rankings"
                    .to_owned(),
            ],
        }
    }
}

fn main() -> BenchResult<()> {
    run::<EmptyBevyAdapter>()
}
