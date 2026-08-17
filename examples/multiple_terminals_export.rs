//! Headless visual QA for multiple independently rendered terminal textures.

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
    BevyTerminalPlugin, RatatuiBackend, TerminalRenderConfig, TerminalRenderScale, TerminalSystems,
};
use ratatui::{
    Terminal,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text as RatatuiText},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};

const WIDTH: u32 = 900;
const HEIGHT: u32 = 360;
const EXPORT_FRAMES: u32 = 10;

#[derive(Resource)]
struct QaTerminals {
    left: Terminal<RatatuiBackend>,
    right: Terminal<RatatuiBackend>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminals = QaTerminals {
        left: Terminal::new(RatatuiBackend::new(37, 12))?,
        right: Terminal::new(RatatuiBackend::new(32, 10))?,
    };
    draw_terminals(&mut terminals, 0)?;
    let left_surface = terminals.left.backend().surface();
    let right_surface = terminals.right.backend().surface();
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
    let fonts = fonts::load(&mut app);
    let left_config = fonts.configure(TerminalRenderConfig {
        cell_size: Vec2::new(10.0, 18.0),
        font_size: 16.0,
        render_scale: TerminalRenderScale::Fixed(1.0),
        origin: Vec2::new(20.0, 62.0),
        cursor_blink_hz: None,
        ..default()
    });
    let right_config = fonts.configure(TerminalRenderConfig {
        cell_size: Vec2::new(10.0, 18.0),
        font_size: 16.0,
        render_scale: TerminalRenderScale::Fixed(1.0),
        origin: Vec2::new(450.0, 62.0),
        cursor_blink_hz: None,
        ..default()
    });
    app.insert_resource(fonts)
        .add_plugins((
            export_plugin,
            BevyTerminalPlugin::new(left_surface).with_config(left_config),
            BevyTerminalPlugin::new(right_surface).with_config(right_config),
        ))
        .insert_resource(terminals)
        .add_systems(Startup, setup_export)
        .add_systems(Update, mutate_for_qa.before(TerminalSystems::Sync))
        .add_systems(Update, stop_after_export)
        .run();

    export_threads.finish();
    Ok(())
}

fn setup_export(
    mut commands: Commands,
    fonts: Res<fonts::JetBrainsMonoFonts>,
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
            label: Some("bevy_terminal_ratatui multiple-terminal QA"),
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
            clear_color: ClearColorConfig::Custom(bevy::prelude::Color::srgb(0.018, 0.024, 0.038)),
            ..default()
        },
    ));
    commands.spawn((
        Text::new("Multiple independent terminal textures"),
        fonts.text_font(24.0),
        TextColor(bevy::prelude::Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: px(20.0),
            top: px(16.0),
            ..default()
        },
    ));
    commands.spawn((
        ImageExport(export_sources.add(output)),
        ImageExportSettings {
            output_dir: "target/multiple-terminals-qa".into(),
            extension: "png".into(),
        },
    ));
}

fn mutate_for_qa(mut terminals: ResMut<QaTerminals>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame != 5 {
        return;
    }

    terminals.right.backend_mut().resize(36, 12);
    terminals
        .right
        .autoresize()
        .expect("the in-memory backend is infallible");
    draw_terminals(&mut terminals, 5).expect("the in-memory backend is infallible");
}

fn draw_terminals(
    terminals: &mut QaTerminals,
    revision: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    terminals.left.draw(|frame| {
        let area = frame.area();
        frame.render_widget(
            Paragraph::new(RatatuiText::from(vec![
                Line::from(Span::styled(
                    "LEFT TEXTURE",
                    Style::new()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("revision {revision}")),
                Line::default(),
                Line::from("This surface keeps its 37 × 12 grid."),
            ]))
            .block(Block::new().borders(Borders::ALL).title(" terminal A ")),
            area,
        );
        let gauge_area = ratatui::layout::Rect::new(3, 8, area.width.saturating_sub(6), 3);
        frame.render_widget(
            Gauge::default()
                .block(Block::new().borders(Borders::ALL))
                .gauge_style(Style::new().fg(Color::LightGreen))
                .percent(if revision == 0 { 24 } else { 72 }),
            gauge_area,
        );
    })?;

    terminals.right.draw(|frame| {
        let items = [
            ListItem::new("independent texture handle"),
            ListItem::new("independent dirty rows"),
            ListItem::new("independent resize"),
            ListItem::new(if revision == 0 {
                "waiting at 32 × 10"
            } else {
                "resized to 36 × 12"
            }),
        ];
        frame.render_widget(
            List::new(items)
                .style(Style::new().fg(Color::LightYellow))
                .block(Block::new().borders(Borders::ALL).title(" terminal B ")),
            frame.area(),
        );
    })?;
    Ok(())
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
