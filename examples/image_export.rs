//! Exports deterministic render-QA frames with `bevy_image_export`.

mod common;

use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::{
            Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        },
    },
    window::WindowResolution,
};
use bevy_grid::{
    BevyBackend, BevyGridBatchPlugin, TerminalRenderConfig, TerminalRenderScale, TerminalSurface,
    TerminalSystems,
};
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};

const WIDTH: u32 = 832;
const HEIGHT: u32 = 480;
const EXPORT_FRAMES: u32 = 12;

fn main() {
    let surface = common::demo_surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0),
        font_size: 18.0,
        render_scale: TerminalRenderScale::Fixed(1.0),
        origin: Vec2::new(20.0, 20.0),
        cursor_blink_hz: None,
        ..default()
    };
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();

    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: WindowResolution::new(WIDTH, HEIGHT)
                            .with_scale_factor_override(1.0),
                        visible: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                }),
            export_plugin,
            BevyGridBatchPlugin::new(surface).with_config(config),
        ))
        .add_systems(Startup, setup_export)
        .add_systems(Update, resize_for_qa.before(TerminalSystems::Sync))
        .add_systems(Update, stop_after_export)
        .run();

    export_threads.finish();
}

fn setup_export(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    let size = Extent3d {
        width: WIDTH,
        height: HEIGHT,
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("bevy_grid render QA"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let output = images.add(image);

    commands.spawn((
        Camera2d,
        IsDefaultUiCamera,
        RenderTarget::Image(output.clone().into()),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.025, 0.03, 0.045)),
            ..default()
        },
    ));
    commands.spawn((
        ImageExport(export_sources.add(output)),
        ImageExportSettings {
            output_dir: "target/render-qa".into(),
            extension: "png".into(),
        },
    ));
}

fn resize_for_qa(surface: Res<TerminalSurface>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame == 8 {
        BevyBackend::from_surface(surface.as_ref().clone()).resize(60, 18);
    }
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
