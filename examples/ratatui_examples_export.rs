//! Exports every deterministic Ratatui example port for visual regression QA.

#[path = "ratatui_examples/mod.rs"]
mod catalog;
#[path = "common/fonts.rs"]
mod fonts;

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
use bevy_terminal_ratatui::{
    BevyTerminalPlugin, TerminalBatch, TerminalRenderConfig, TerminalRenderScale, TerminalSystems,
};

const CELL_WIDTH: f32 = 10.0;
const CELL_HEIGHT: f32 = 18.0;
const MARGIN: f32 = 20.0;
const FRAMES_PER_EXAMPLE: u8 = 4;

#[derive(Resource)]
struct CaptureSequence {
    example_index: usize,
    frames_on_example: u8,
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--list") {
        for spec in catalog::EXAMPLES {
            println!("{}", spec.slug);
        }
        return;
    }

    let surface = catalog::draw_surface(&catalog::EXAMPLES[0]);
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(CELL_WIDTH, CELL_HEIGHT),
        font_size: 16.0,
        render_scale: TerminalRenderScale::Fixed(1.0),
        origin: Vec2::splat(MARGIN),
        cursor_blink_hz: None,
        slow_blink_hz: 0.0,
        rapid_blink_hz: 0.0,
        ..default()
    };
    let export_plugin = ImageExportPlugin::default();
    let export_threads = export_plugin.threads.clone();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(image_width(), image_height())
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
    let fonts = fonts::load(&mut app);
    app.insert_resource(CaptureSequence {
        example_index: 0,
        frames_on_example: 0,
    })
    .add_plugins((
        export_plugin,
        BevyTerminalPlugin::new(surface).with_config(fonts.configure(config)),
    ))
    .add_systems(Startup, setup_export)
    .add_systems(Update, advance_capture.before(TerminalSystems::Sync))
    .run();

    export_threads.finish();
}

fn image_width() -> u32 {
    f32::from(catalog::COLUMNS)
        .mul_add(CELL_WIDTH, MARGIN * 2.0)
        .round() as u32
}

fn image_height() -> u32 {
    f32::from(catalog::ROWS)
        .mul_add(CELL_HEIGHT, MARGIN * 2.0)
        .round() as u32
}

fn setup_export(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut export_sources: ResMut<Assets<ImageExportSource>>,
) {
    let size = Extent3d {
        width: image_width(),
        height: image_height(),
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("Ratatui examples render QA"),
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
            clear_color: ClearColorConfig::Custom(Color::srgb(0.018, 0.022, 0.032)),
            ..default()
        },
    ));
    commands.spawn((
        ImageExport(export_sources.add(output)),
        ImageExportSettings {
            output_dir: output_dir(catalog::EXAMPLES[0].slug),
            extension: "png".into(),
        },
    ));
}

fn advance_capture(
    terminals: Query<&TerminalBatch>,
    mut sequence: ResMut<CaptureSequence>,
    mut settings: Query<&mut ImageExportSettings>,
    mut exit: MessageWriter<AppExit>,
) {
    sequence.frames_on_example += 1;
    if sequence.frames_on_example < FRAMES_PER_EXAMPLE {
        return;
    }

    sequence.example_index += 1;
    sequence.frames_on_example = 0;
    let Some(spec) = catalog::EXAMPLES.get(sequence.example_index) else {
        exit.write(AppExit::Success);
        return;
    };

    let terminal = terminals
        .single()
        .expect("the exporter creates exactly one terminal");
    catalog::redraw_surface(terminal.surface(), spec);
    for mut settings in &mut settings {
        settings.output_dir = output_dir(spec.slug);
    }
}

fn output_dir(slug: &str) -> String {
    format!("target/ratatui-examples/{slug}")
}
