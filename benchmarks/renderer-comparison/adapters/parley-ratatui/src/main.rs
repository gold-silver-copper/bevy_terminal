use bevy::{
    app::SubApps,
    prelude::*,
    render::renderer::{RenderDevice, RenderQueue},
};
use parley_ratatui::{
    FontOptions, GpuRenderer, ParleyBackend, TerminalRenderer, TextureReadback, TextureTarget,
    Theme,
    vello::{Scene, wgpu},
};
use ratatui::Terminal;
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    measure, render_workload, run,
};

struct ParleyRatatuiAdapter {
    terminal: Option<Terminal<ParleyBackend>>,
    renderer: Option<TerminalRenderer>,
    gpu_renderer: Option<GpuRenderer>,
    target: Option<TextureTarget>,
    spare_scene: Option<Scene>,
    output_size: (u32, u32),
}

impl RendererAdapter for ParleyRatatuiAdapter {
    fn new(_config: &BenchConfig) -> BenchResult<Self> {
        Ok(Self {
            terminal: None,
            renderer: None,
            gpu_renderer: None,
            target: None,
            spare_scene: Some(Scene::new()),
            output_size: (0, 0),
        })
    }

    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()> {
        let bytes: &'static [u8] = Box::leak(
            world
                .resource::<SharedFontFixture>()
                .0
                .to_vec()
                .into_boxed_slice(),
        );
        let mut base_options =
            FontOptions::default().with_bundled_font_family("Benchmark Mono", bytes);
        base_options.size = config.font_size as f32;
        let probe = TerminalRenderer::new(base_options.clone(), Theme::default());
        let metrics = probe.metrics();
        let options = base_options
            .with_cell_width_offset(config.cell_width - metrics.cell_width)
            .with_cell_height_offset(config.cell_height - metrics.cell_height);
        let renderer = TerminalRenderer::new(options, Theme::default());
        let terminal = Terminal::new(ParleyBackend::new(config.cols, config.rows))?;
        let (width, height) = renderer.texture_size_for_buffer(terminal.backend().buffer());
        let device = world.resource::<RenderDevice>().wgpu_device();
        let target = TextureTarget::new(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
            Some("renderer-comparison.parley-ratatui"),
        );
        let gpu_renderer = GpuRenderer::new(device)?;

        self.terminal = Some(terminal);
        self.renderer = Some(renderer);
        self.gpu_renderer = Some(gpu_renderer);
        self.target = Some(target);
        self.output_size = (width, height);
        Ok(())
    }

    fn render_frame(
        &mut self,
        world: &mut World,
        config: &BenchConfig,
        frame_index: u64,
    ) -> BenchResult<AdapterFrame> {
        let terminal = self.terminal.as_mut().ok_or("adapter not initialized")?;
        let (result, draw_ns) =
            measure(|| terminal.draw(|frame| render_workload(frame, config.workload, frame_index)));
        result?;

        let renderer = self.renderer.as_mut().ok_or("missing terminal renderer")?;
        let buffer = terminal.backend().buffer();
        let build_scene = self.spare_scene.take().unwrap_or_default();
        let (scene, prepare_ns) = measure(|| {
            let previous = renderer.replace_scene(build_scene);
            renderer.build_scene_with_elapsed(buffer, None, false, frame_index as f32 / 60.0);
            renderer.replace_scene(previous)
        });

        let device = world.resource::<RenderDevice>().wgpu_device();
        let queue = world.resource::<RenderQueue>();
        let target = self.target.as_ref().ok_or("missing texture target")?;
        let base_color = renderer.theme().background.to_peniko();
        let (result, submit_ns) = measure(|| {
            self.gpu_renderer
                .as_mut()
                .ok_or("missing GPU renderer")?
                .render_scene_to_texture_view(
                    device,
                    queue,
                    &target.view,
                    target.width,
                    target.height,
                    base_color,
                    &scene,
                )
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })
        });
        result?;
        self.spare_scene = Some(scene);
        Ok(AdapterFrame {
            draw_ns,
            prepare_ns,
            submit_ns,
        })
    }

    fn output_size(&self, _config: &BenchConfig) -> (u32, u32) {
        self.output_size
    }

    fn capture_rgba(
        &mut self,
        sub_apps: &mut SubApps,
        _config: &BenchConfig,
    ) -> BenchResult<Vec<u8>> {
        let world = sub_apps.main.world();
        let device = world.resource::<RenderDevice>().wgpu_device();
        let queue = world.resource::<RenderQueue>();
        let target = self.target.as_ref().ok_or("missing texture target")?;
        let mut rgba = Vec::new();
        TextureReadback::new().read_texture_to_rgba8_into(device, queue, target, &mut rgba)?;
        Ok(rgba)
    }

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "parley_ratatui".to_owned(),
            name: "parley_ratatui (direct Bevy WGPU device)".to_owned(),
            renderer_version: "0.3.4".to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Ratatui buffer -> Parley shaping/Vello scene -> Vello render into texture on Bevy's WGPU device".to_owned(),
            notes: vec![
                "Uses the upstream direct-device integration path; there is no GPU readback or CPU re-upload".to_owned(),
                "The configured font size is applied explicitly before cell metric offsets force the common requested pixel resolution".to_owned(),
            ],
        }
    }
}

fn main() -> BenchResult<()> {
    run::<ParleyRatatuiAdapter>()
}
