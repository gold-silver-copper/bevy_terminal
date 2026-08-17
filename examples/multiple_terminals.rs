//! Displays two independently updated Ratatui terminals in one Bevy UI scene.

#[path = "common/fonts.rs"]
mod fonts;

use std::time::Duration;

use bevy::{prelude::*, window::WindowResolution};
use bevy_terminal_ratatui::{
    Presentation, RatatuiBackend, RatatuiTerminalExt, Terminal as TerminalEntity, TerminalPlugin,
    TerminalRenderConfig, TerminalSystems,
};
use ratatui::{
    Terminal,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};

#[derive(Resource)]
struct IndependentTerminals {
    left: Terminal<RatatuiBackend>,
    right: Terminal<RatatuiBackend>,
    tick: u64,
    timer: Timer,
    right_resized: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminals = IndependentTerminals {
        left: Terminal::new(RatatuiBackend::new(42, 16))?,
        right: Terminal::new(RatatuiBackend::new(34, 12))?,
        tick: 0,
        timer: Timer::new(Duration::from_millis(250), TimerMode::Repeating),
        right_resized: false,
    };
    redraw(&mut terminals)?;
    let left_surface = terminals.left.backend().surface();
    let right_surface = terminals.right.backend().surface();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy_terminal_ratatui · multiple independent terminals".into(),
            resolution: WindowResolution::new(940, 430),
            resizable: false,
            ..default()
        }),
        ..default()
    }));
    let fonts = fonts::load(&mut app);
    let config = fonts.configure(TerminalRenderConfig {
        cell_size: Vec2::new(10.0, 18.0),
        ..default()
    });
    app.insert_resource(fonts)
        .add_plugins(TerminalPlugin)
        .insert_resource(terminals)
        .insert_resource(PendingRight {
            surface: Some(right_surface),
            config: config.clone(),
        })
        .add_systems(Startup, setup)
        .add_systems(Startup, move |mut commands: Commands| {
            // The left terminal exists from the first frame.
            commands.spawn(
                TerminalEntity::new(left_surface.clone())
                    .with_config(config.clone())
                    .with_presentation(Presentation::Ui {
                        origin: Vec2::new(24.0, 56.0),
                    }),
            );
        })
        .add_systems(
            Update,
            (spawn_right_terminal, update_terminals).before(TerminalSystems::Sync),
        )
        .run();
    Ok(())
}

/// The right terminal is spawned a moment after startup to show that terminals
/// can be added while the app is running.
#[derive(Resource)]
struct PendingRight {
    surface: Option<bevy_terminal_ratatui::TerminalSurface>,
    config: TerminalRenderConfig,
}

fn spawn_right_terminal(
    mut commands: Commands,
    time: Res<Time>,
    mut pending: ResMut<PendingRight>,
) {
    if time.elapsed_secs() > 0.75
        && let Some(surface) = pending.surface.take()
    {
        commands.spawn(
            TerminalEntity::new(surface)
                .with_config(pending.config.clone())
                .with_presentation(Presentation::Ui {
                    origin: Vec2::new(496.0, 56.0),
                }),
        );
    }
}

fn setup(mut commands: Commands, fonts: Res<fonts::ExampleFonts>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new("Two Ratatui surfaces · two textures · one Bevy app"),
        fonts.text_font(22.0),
        TextColor(bevy::prelude::Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: px(24.0),
            top: px(14.0),
            ..default()
        },
    ));
}

fn update_terminals(time: Res<Time>, mut terminals: ResMut<IndependentTerminals>) {
    if !terminals.timer.tick(time.delta()).just_finished() {
        return;
    }
    terminals.tick = terminals.tick.wrapping_add(1);
    if terminals.tick == 8 && !terminals.right_resized {
        terminals.right.resize_grid(38, 14);
        terminals.right_resized = true;
    }
    redraw(&mut terminals).expect("the in-memory backend is infallible");
}

fn redraw(terminals: &mut IndependentTerminals) -> Result<(), Box<dyn std::error::Error>> {
    let tick = terminals.tick;
    terminals.left.draw(|frame| {
        let [heading, gauge, message] = Layout::vertical([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(4),
        ])
        .areas(frame.area());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "LEFT",
                    Style::new()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("  revision {tick}")),
            ]))
            .alignment(Alignment::Center)
            .block(Block::new().borders(Borders::ALL).title(" 42 × 16 ")),
            heading,
        );
        frame.render_widget(
            Gauge::default()
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .title(" independent counter "),
                )
                .gauge_style(Style::new().fg(Color::LightGreen))
                .percent((tick % 101) as u16),
            gauge,
        );
        frame.render_widget(
            Paragraph::new("This terminal updates without touching the surface on the right.")
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(Block::new().borders(Borders::ALL)),
            message,
        );
    })?;

    let resized = terminals.right_resized;
    terminals.right.draw(|frame| {
        let events = (0..8).rev().map(|offset| {
            let revision = tick.saturating_sub(offset);
            ListItem::new(format!("event {revision:04} · right surface only"))
        });
        frame.render_widget(
            List::new(events)
                .block(Block::new().borders(Borders::ALL).title(if resized {
                    " RIGHT · resized to 38 × 14 "
                } else {
                    " RIGHT · 34 × 12 "
                }))
                .style(Style::new().fg(Color::LightYellow)),
            frame.area(),
        );
    })?;
    Ok(())
}
