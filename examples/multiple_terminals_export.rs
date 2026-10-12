//! Headless visual QA for multiple independently rendered terminal textures.

#[path = "common/app.rs"]
mod app;
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
use bevy_terminal_ratatui::prelude::{
    CursorConfig, RasterConfig, RatatuiPlugin, RatatuiTerminal, TerminalRenderConfig,
    TerminalSizing, TerminalSystems,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text as RatatuiText},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};

const WIDTH: u32 = 900;
const HEIGHT: u32 = 360;
const EXPORT_FRAMES: u32 = 10;

/// Which of the two terminals an entity is.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let config = fonts.configure(TerminalRenderConfig {
        sizing: TerminalSizing::FitCellWidth(Vec2::new(10.0, 18.0)),
        raster: RasterConfig {
            scale: 1.0,
            ..default()
        },
        cursor: CursorConfig {
            blink_hz: None,
            ..default()
        },
        ..default()
    });
    app.insert_resource(fonts)
        .add_plugins((export_plugin, RatatuiPlugin, app::presentation))
        .add_systems(Startup, move |mut commands: Commands| {
            for (side, columns, rows, origin) in [
                (Side::Left, 37, 12, Vec2::new(20.0, 62.0)),
                (Side::Right, 32, 10, Vec2::new(450.0, 62.0)),
            ] {
                let mut terminal = RatatuiTerminal::new(columns, rows);
                draw_terminal(&mut terminal, side, 0);
                commands.spawn(app::ui_terminal((terminal, side), config.clone(), origin));
            }
        })
        .add_systems(Startup, setup_export)
        .add_systems(Update, mutate_for_qa.before(TerminalSystems::Sync))
        .add_systems(Update, stop_after_export)
        .run();

    export_threads.finish();
    Ok(())
}

fn setup_export(
    mut commands: Commands,
    fonts: Res<fonts::ExampleFonts>,
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

fn mutate_for_qa(mut terminals: Query<(&mut RatatuiTerminal, &Side)>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame != 5 {
        return;
    }

    for (mut terminal, &side) in &mut terminals {
        if side == Side::Right {
            terminal.resize_grid(36, 12);
        }
        draw_terminal(&mut terminal, side, 5);
    }
}

fn draw_terminal(terminal: &mut RatatuiTerminal, side: Side, revision: u64) {
    match side {
        Side::Left => draw_left(terminal, revision),
        Side::Right => draw_right(terminal, revision),
    }
}

fn draw_left(terminal: &mut RatatuiTerminal, revision: u64) {
    terminal.draw(|frame| {
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
                Line::from("This terminal keeps its 37 × 12 grid."),
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
    });
}

fn draw_right(terminal: &mut RatatuiTerminal, revision: u64) {
    terminal.draw(|frame| {
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
    });
}

fn stop_after_export(mut frame: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    if *frame >= EXPORT_FRAMES {
        exit.write(AppExit::Success);
    }
}
