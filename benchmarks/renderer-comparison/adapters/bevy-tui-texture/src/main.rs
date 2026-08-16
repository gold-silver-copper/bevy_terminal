use std::sync::Arc;

use bevy::prelude::*;
use bevy_tui_texture::{
    Font as TerminalFont, Fonts, TerminalConfig, TerminalPlugin, Tui, TuiRequest,
};
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    measure, render_workload, run,
};

struct BevyTuiTextureAdapter {
    entity: Option<Entity>,
    output_size: (u32, u32),
}

impl RendererAdapter for BevyTuiTextureAdapter {
    fn new(_config: &BenchConfig) -> BenchResult<Self> {
        Ok(Self {
            entity: None,
            output_size: (0, 0),
        })
    }

    fn configure_app(&mut self, app: &mut App, _config: &BenchConfig) -> BenchResult<()> {
        app.add_plugins(TerminalPlugin::display_only());
        Ok(())
    }

    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()> {
        let font = TerminalFont::from_vec(world.resource::<SharedFontFixture>().0.to_vec())
            .ok_or("bevy_tui_texture rejected benchmark font")?;
        let fonts = Arc::new(Fonts::new(font, config.font_size));
        let entity = world
            .spawn(
                TuiRequest::headless(config.cols, config.rows, fonts).with_config(TerminalConfig {
                    keyboard: false,
                    mouse: false,
                    ..default()
                }),
            )
            .id();
        self.entity = Some(entity);
        Ok(())
    }

    fn ready(&mut self, world: &mut World) -> BenchResult<bool> {
        let Some(entity) = self.entity else {
            return Ok(false);
        };
        let Some(tui) = world.get::<Tui>(entity) else {
            return Ok(false);
        };
        let size = tui.size_px();
        self.output_size = (size.x, size.y);
        Ok(true)
    }

    fn render_frame(
        &mut self,
        world: &mut World,
        config: &BenchConfig,
        frame_index: u64,
    ) -> BenchResult<AdapterFrame> {
        let entity = self.entity.ok_or("adapter not initialized")?;
        let mut tui = world
            .get_mut::<Tui>(entity)
            .ok_or("Tui component disappeared")?;
        let (_, draw_ns) = measure(|| {
            tui.draw(|frame| render_workload(frame, config.workload, frame_index));
        });
        Ok(AdapterFrame {
            draw_ns,
            ..default()
        })
    }

    fn output_size(&self, _config: &BenchConfig) -> (u32, u32) {
        self.output_size
    }

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "bevy_tui_texture".to_owned(),
            name: "bevy_tui_texture (headless Tui)".to_owned(),
            renderer_version: "0.3.4".to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Tui::draw diff -> plugin CPU payload -> render-world WGPU terminal texture on Bevy's device".to_owned(),
            notes: vec![
                "Uses TuiRequest::headless, so the terminal texture is rendered without an OS window or presentation surface".to_owned(),
                "Plugin CPU flush, extraction, atlas upload, GPU draw, and completion are included in Bevy update/wait phases".to_owned(),
                "Before warmup, readiness probes and records the native texture dimensions; the controller rejects a default run whose calibration misses the requested comparison cell".to_owned(),
            ],
        }
    }
}

fn main() -> BenchResult<()> {
    run::<BevyTuiTextureAdapter>()
}
