use bevy::{
    app::SubApps,
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use ratatui::Terminal;
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    measure, render_workload, run, spawn_offscreen_ui_target,
};
use soft_ratatui::{EmbeddedTTF, SoftBackend, rusttype::Font as RusttypeFont};

type SoftTerminal = Terminal<SoftBackend<EmbeddedTTF>>;

struct SoftRatatuiAdapter {
    terminal: Option<SoftTerminal>,
    image: Option<Handle<Image>>,
    output_size: (u32, u32),
}

impl RendererAdapter for SoftRatatuiAdapter {
    fn new(_config: &BenchConfig) -> BenchResult<Self> {
        Ok(Self {
            terminal: None,
            image: None,
            output_size: (0, 0),
        })
    }

    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()> {
        let bytes = world.resource::<SharedFontFixture>().0.to_vec();
        let font = RusttypeFont::try_from_vec(bytes).ok_or("rusttype rejected benchmark font")?;
        let mut backend = SoftBackend::<EmbeddedTTF>::new(
            config.cols,
            config.rows,
            config.font_size,
            font,
            None,
            None,
        );
        backend.char_width = rounded_cell_dimension(config.cell_width)?;
        backend.char_height = rounded_cell_dimension(config.cell_height)?;
        backend.resize(config.cols, config.rows);
        let width = u32::try_from(backend.get_pixmap_width())?;
        let height = u32::try_from(backend.get_pixmap_height())?;
        let rgba = backend.get_pixmap_data_as_rgba();
        let terminal = Terminal::new(backend)?;

        spawn_offscreen_ui_target(world, config);
        let mut image = Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.sampler = ImageSampler::nearest();
        let handle = world.resource_mut::<Assets<Image>>().add(image);
        world.spawn((
            ImageNode::new(handle.clone()),
            Node {
                width: Val::Px(width as f32),
                height: Val::Px(height as f32),
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            },
        ));

        self.terminal = Some(terminal);
        self.image = Some(handle);
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

        let (rgba, prepare_ns) = measure(|| terminal.backend().get_pixmap_data_as_rgba());
        let submit_start = std::time::Instant::now();
        world
            .resource_mut::<Assets<Image>>()
            .get_mut(self.image.as_ref().ok_or("missing Bevy image")?)
            .ok_or("Bevy image was removed")?
            .data = Some(rgba);
        let submit_ns = u64::try_from(submit_start.elapsed().as_nanos()).unwrap_or(u64::MAX);
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
        sub_apps
            .main
            .world()
            .resource::<Assets<Image>>()
            .get(self.image.as_ref().ok_or("missing Bevy image")?)
            .and_then(|image| image.data.clone())
            .ok_or_else(|| "soft_ratatui capture image has no CPU pixels".into())
    }

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "soft_ratatui".to_owned(),
            name: "soft_ratatui (EmbeddedTTF)".to_owned(),
            renderer_version: "0.2.0".to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "Ratatui diff -> CPU RGB raster -> RGBA conversion -> Bevy Image upload -> offscreen Bevy UI render".to_owned(),
            notes: vec![
                "prepare_ns includes the renderer's public RGB-to-RGBA conversion on every presented frame".to_owned(),
                "The calibrated font is rasterized into the requested integral cell dimensions before the pixmap is allocated".to_owned(),
            ],
        }
    }
}

fn rounded_cell_dimension(value: f32) -> BenchResult<usize> {
    let rounded = value.round();
    if (value - rounded).abs() > f32::EPSILON {
        return Err(format!("soft_ratatui requires integral cell dimensions, got {value}").into());
    }
    Ok(usize::try_from(rounded as u32)?)
}

fn main() -> BenchResult<()> {
    run::<SoftRatatuiAdapter>()
}
