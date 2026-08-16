use std::hint::black_box;

use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use egui::{
    Color32, Context, FullOutput, ImageData, Pos2, RawInput, Rect, TextureId, Vec2 as EguiVec2,
};
use egui_ratatui::RataguiBackend;
use ratatui::Terminal;
use renderer_bench_sdk::{
    AdapterFrame, AdapterMetadata, BenchConfig, BenchResult, RendererAdapter, SharedFontFixture,
    measure, render_workload, run, spawn_offscreen_ui_target,
};
use soft_ratatui::{EmbeddedTTF, SoftBackend, rusttype::Font as RusttypeFont};

type EguiTerminal = Terminal<RataguiBackend<EmbeddedTTF>>;

struct EguiRatatuiAdapter {
    terminal: Option<EguiTerminal>,
    context: Context,
    image: Option<Handle<Image>>,
    output_size: (u32, u32),
}

impl RendererAdapter for EguiRatatuiAdapter {
    fn new(_config: &BenchConfig) -> BenchResult<Self> {
        Ok(Self {
            terminal: None,
            context: Context::default(),
            image: None,
            output_size: (0, 0),
        })
    }

    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()> {
        let font = RusttypeFont::try_from_vec(world.resource::<SharedFontFixture>().0.to_vec())
            .ok_or("rusttype rejected benchmark font")?;
        let mut soft = SoftBackend::<EmbeddedTTF>::new(
            config.cols,
            config.rows,
            config.font_size,
            font,
            None,
            None,
        );
        soft.char_width = rounded_cell_dimension(config.cell_width)?;
        soft.char_height = rounded_cell_dimension(config.cell_height)?;
        soft.resize(config.cols, config.rows);
        let width = u32::try_from(soft.get_pixmap_width())?;
        let height = u32::try_from(soft.get_pixmap_height())?;
        let rgba = soft.get_pixmap_data_as_rgba();
        let terminal = Terminal::new(RataguiBackend::new("renderer-comparison", soft))?;

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

        let size = EguiVec2::new(self.output_size.0 as f32, self.output_size.1 as f32);
        #[allow(clippy::cast_precision_loss)]
        let raw_input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(frame_index as f64 / 60.0),
            ..default()
        };
        let (texture_rgba, prepare_ns) = measure(|| {
            let output = self.context.run_ui(raw_input, |root_ui| {
                egui::CentralPanel::no_frame().show_inside(root_ui, |ui| {
                    ui.set_min_size(size);
                    ui.add_sized(size, terminal.backend_mut());
                });
            });
            let texture_id = terminal
                .backend()
                .text_handle
                .as_ref()
                .ok_or("egui_ratatui did not create its terminal texture")?
                .id();
            consume_egui_output(&self.context, output, texture_id, self.output_size)
        });
        let rgba = texture_rgba?;

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

    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            id: "egui_ratatui".to_owned(),
            name: "egui_ratatui (Bevy image presentation)".to_owned(),
            renderer_version: "2.2.0".to_owned(),
            bevy_version: "0.19.1".to_owned(),
            ratatui_version: "0.30.2".to_owned(),
            render_path: "soft_ratatui CPU raster -> egui widget texture/tessellation -> Bevy Image upload -> offscreen Bevy UI render".to_owned(),
            notes: vec![
                "The upstream egui widget owns presentation preparation; the harness consumes its egui texture delta and presents identical pixels in Bevy".to_owned(),
                "This deliberately includes egui's per-frame ColorImage creation and texture update, unlike the bare soft_ratatui adapter".to_owned(),
                "The underlying soft renderer is padded to the requested integral cell dimensions before its pixmap is allocated".to_owned(),
            ],
        }
    }
}

fn rounded_cell_dimension(value: f32) -> BenchResult<usize> {
    let rounded = value.round();
    if (value - rounded).abs() > f32::EPSILON {
        return Err(format!("egui_ratatui requires integral cell dimensions, got {value}").into());
    }
    Ok(usize::try_from(rounded as u32)?)
}

fn consume_egui_output(
    context: &Context,
    output: FullOutput,
    terminal_texture: TextureId,
    expected_size: (u32, u32),
) -> BenchResult<Vec<u8>> {
    let FullOutput {
        shapes,
        textures_delta,
        pixels_per_point,
        ..
    } = output;
    black_box(context.tessellate(shapes, pixels_per_point));
    let (_, delta) = textures_delta
        .set
        .iter()
        .find(|(id, _)| *id == terminal_texture)
        .ok_or("egui_ratatui emitted no terminal texture delta")?;
    if delta.pos.is_some() {
        return Err("egui_ratatui emitted an unexpected partial terminal texture update".into());
    }
    let ImageData::Color(image) = &delta.image;
    let expected = [
        usize::try_from(expected_size.0)?,
        usize::try_from(expected_size.1)?,
    ];
    if image.size != expected {
        return Err(format!(
            "egui_ratatui terminal texture was {:?}, expected {expected:?}",
            image.size
        )
        .into());
    }
    Ok(image
        .pixels
        .iter()
        .flat_map(|color: &Color32| color.to_array())
        .collect())
}

fn main() -> BenchResult<()> {
    run::<EguiRatatuiAdapter>()
}
