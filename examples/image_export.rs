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
use bevy_image_export::{ImageExport, ImageExportPlugin, ImageExportSettings, ImageExportSource};
use bevy_terminal_ratatui::TerminalRenderer;
use bevy_terminal_ratatui::prelude::{
    CursorConfig, RasterConfig, TerminalPlugin, TerminalRenderConfig, TerminalRenderScale,
    TerminalSystems,
};

/// Canvas large enough for the 72×22 scene at the measured Iosevka cell
/// (11 px columns → 22 px font → 27 px line box), plus margins.
const WIDTH: u32 = 832;
const HEIGHT: u32 = 640;
const EXPORT_FRAMES: u32 = 12;

fn main() {
    let surface = common::demo_surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(11.0, 20.0).into(),
        raster: RasterConfig {
            scale: TerminalRenderScale::Fixed(1.0),
            ..default()
        },
        cursor: CursorConfig {
            blink_hz: None,
            ..default()
        },
        ..default()
    };
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();

    let mut app = App::new();
    app.add_plugins(
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
    );
    let fonts = common::fonts::load(&mut app);
    let config = fonts.configure(config);
    app.add_plugins((export_plugin, TerminalPlugin))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(common::app::ui_terminal(
                TerminalRenderer::new(surface.clone()),
                config.clone(),
                Vec2::new(20.0, 20.0),
            ));
        })
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
            label: Some("bevy_terminal_ratatui render QA"),
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

fn resize_for_qa(terminals: Query<&TerminalRenderer>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame == 8 {
        let terminal = terminals
            .single()
            .expect("the example creates exactly one terminal");
        terminal.surface().update(|update| {
            update.resize((60, 18));
        });
    }
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
