//! Displays two independently updated Ratatui terminals in one Bevy UI scene.
//! The window is resizable: each terminal refits its grid to its half of the
//! window at the renderer's measured cell size.

#[path = "common/fonts.rs"]
mod fonts;
#[path = "common/app.rs"]
mod presentation;

use std::time::Duration;

use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowResolution},
};
use bevy_terminal_ratatui::prelude::{
    RatatuiPlugin, RatatuiTerminal, TerminalRenderConfig, TerminalSizing, TerminalSystems,
    TerminalTexture,
};
use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};

/// Which of the two terminals an entity is.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// The revision counter both terminals display, advanced by a timer.
#[derive(Resource)]
struct Ticker {
    tick: u64,
    timer: Timer,
}

const ROW_LEFT: f32 = 24.0;
const ROW_TOP: f32 = 56.0;
const ROW_GAP: f32 = 52.0;
const ROW_BOTTOM_MARGIN: f32 = 24.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy_terminal_ratatui · multiple independent terminals".into(),
            resolution: WindowResolution::new(940, 430),
            ..default()
        }),
        ..default()
    }));
    let fonts = fonts::load(&mut app);
    let config = fonts.configure(TerminalRenderConfig {
        sizing: TerminalSizing::FitCellWidth(Vec2::new(10.0, 18.0)),
        ..default()
    });
    app.insert_resource(fonts)
        .add_plugins(RatatuiPlugin)
        .add_plugins((presentation::presentation, presentation::window_scale))
        .insert_resource(Ticker {
            tick: 0,
            timer: Timer::new(Duration::from_millis(250), TimerMode::Repeating),
        })
        .insert_resource(PendingRight(Some(config.clone())))
        .add_systems(Startup, setup)
        .add_systems(Startup, move |mut commands: Commands| {
            // Both terminals live in one flex row; the left one exists from the
            // first frame, the right one is added to the row later.
            commands
                .spawn((
                    TerminalRow,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(ROW_LEFT),
                        top: px(ROW_TOP),
                        column_gap: px(ROW_GAP),
                        ..default()
                    },
                ))
                .with_children(|row| {
                    row.spawn(row_terminal(Side::Left, 0, config.clone()));
                });
        })
        .add_systems(
            Update,
            (spawn_right_terminal, fit_to_window, update_terminals).before(TerminalSystems::Sync),
        )
        .run();
    Ok(())
}

/// The flex row containing both terminals.
#[derive(Component)]
struct TerminalRow;

/// A terminal for one side of the row, drawn at `tick`.
fn row_terminal(side: Side, tick: u64, config: TerminalRenderConfig) -> impl Bundle {
    let mut terminal = match side {
        Side::Left => RatatuiTerminal::new(42, 16),
        Side::Right => RatatuiTerminal::new(34, 12),
    };
    draw(&mut terminal, side, tick);
    (
        terminal,
        side,
        config,
        ImageNode::default(),
        Node::default(),
    )
}

/// The right terminal is spawned a moment after startup to show that terminals
/// can be added while the app is running and participate in UI layout.
#[derive(Resource)]
struct PendingRight(Option<TerminalRenderConfig>);

fn spawn_right_terminal(
    mut commands: Commands,
    time: Res<Time>,
    ticker: Res<Ticker>,
    mut pending: ResMut<PendingRight>,
    row: Query<Entity, With<TerminalRow>>,
) {
    if time.elapsed_secs() > 0.75
        && let Ok(row) = row.single()
        && let Some(config) = pending.0.take()
    {
        commands.entity(row).with_children(|row| {
            row.spawn(row_terminal(Side::Right, ticker.tick, config));
        });
    }
}

fn setup(mut commands: Commands, fonts: Res<fonts::ExampleFonts>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::new("Two Ratatui terminals · two textures · one Bevy app"),
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

/// Each terminal takes half of the window width (minus margins and the gap)
/// and the height below the heading, at its own measured cell size.
fn fit_to_window(
    ticker: Res<Ticker>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut terminals: Query<(&mut RatatuiTerminal, &TerminalTexture, &Side)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.resolution.size();
    let half = Vec2::new(
        ((size.x - ROW_LEFT * 2.0 - ROW_GAP) / 2.0).max(1.0),
        (size.y - ROW_TOP - ROW_BOTTOM_MARGIN).max(1.0),
    );
    for (mut terminal, texture, &side) in &mut terminals {
        if presentation::fit_grid(&mut terminal, texture, half) {
            draw(&mut terminal, side, ticker.tick);
        }
    }
}

fn update_terminals(
    time: Res<Time>,
    mut ticker: ResMut<Ticker>,
    mut terminals: Query<(&mut RatatuiTerminal, &Side)>,
) {
    if !ticker.timer.tick(time.delta()).just_finished() {
        return;
    }
    ticker.tick = ticker.tick.wrapping_add(1);
    for (mut terminal, &side) in &mut terminals {
        draw(&mut terminal, side, ticker.tick);
    }
}

fn draw(terminal: &mut RatatuiTerminal, side: Side, tick: u64) {
    match side {
        Side::Left => draw_left(terminal, tick),
        Side::Right => draw_right(terminal, tick),
    }
}

fn draw_left(terminal: &mut RatatuiTerminal, tick: u64) {
    let Ok(size) = terminal.size();
    terminal.draw(|frame| {
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
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(format!(" {} × {} ", size.width, size.height)),
            ),
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
            Paragraph::new("This terminal updates without touching the terminal on the right.")
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(Block::new().borders(Borders::ALL)),
            message,
        );
    });
}

fn draw_right(terminal: &mut RatatuiTerminal, tick: u64) {
    let Ok(size) = terminal.size();
    terminal.draw(|frame| {
        let events = (0..8).rev().map(|offset| {
            let revision = tick.saturating_sub(offset);
            ListItem::new(format!("event {revision:04} · right terminal only"))
        });
        frame.render_widget(
            List::new(events)
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .title(format!(" RIGHT · {} × {} ", size.width, size.height)),
                )
                .style(Style::new().fg(Color::LightYellow)),
            frame.area(),
        );
    });
}
