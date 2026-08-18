//! Displays two independently updated Ratatui terminals in one Bevy UI scene.
//! The window is resizable: each terminal refits its grid to its half of the
//! window at the renderer's measured cell size.

#[path = "common/fonts.rs"]
mod fonts;

use std::time::Duration;

use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowResolution},
};
use bevy_terminal_ratatui::prelude::{
    TerminalPlugin, TerminalRenderConfig, TerminalSystems, TerminalTexture,
};
use bevy_terminal_ratatui::{RatatuiBackend, RatatuiTerminalExt, TerminalRenderer};
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
}

const ROW_LEFT: f32 = 24.0;
const ROW_TOP: f32 = 56.0;
const ROW_GAP: f32 = 52.0;
const ROW_BOTTOM_MARGIN: f32 = 24.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminals = IndependentTerminals {
        left: Terminal::new(RatatuiBackend::new(42, 16))?,
        right: Terminal::new(RatatuiBackend::new(34, 12))?,
        tick: 0,
        timer: Timer::new(Duration::from_millis(250), TimerMode::Repeating),
    };
    redraw(&mut terminals)?;
    let left_surface = terminals.left.backend().surface();
    let right_surface = terminals.right.backend().surface();

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
        cell_size: Vec2::new(10.0, 18.0).into(),
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
                    row.spawn((
                        TerminalRenderer::new(left_surface.clone()),
                        config.clone(),
                        ImageNode::default(),
                        Node::default(),
                    ));
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

/// The right terminal is spawned a moment after startup to show that terminals
/// can be added while the app is running and participate in UI layout.
#[derive(Resource)]
struct PendingRight {
    surface: Option<bevy_terminal_ratatui::prelude::TerminalSurface>,
    config: TerminalRenderConfig,
}

fn spawn_right_terminal(
    mut commands: Commands,
    time: Res<Time>,
    mut pending: ResMut<PendingRight>,
    row: Query<Entity, With<TerminalRow>>,
) {
    if time.elapsed_secs() > 0.75
        && let Some(surface) = pending.surface.take()
        && let Ok(row) = row.single()
    {
        commands.entity(row).with_children(|row| {
            row.spawn((
                TerminalRenderer::new(surface),
                pending.config.clone(),
                ImageNode::default(),
                Node::default(),
            ));
        });
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

/// Each terminal takes half of the window width (minus margins and the gap)
/// and the height below the heading, at its own measured cell size.
fn fit_to_window(
    mut terminals: ResMut<IndependentTerminals>,
    windows: Query<&Window, With<PrimaryWindow>>,
    textures: Query<(&TerminalRenderer, &TerminalTexture)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = window.resolution.size();
    let half = Vec2::new(
        ((size.x - ROW_LEFT * 2.0 - ROW_GAP) / 2.0).max(1.0),
        (size.y - ROW_TOP - ROW_BOTTOM_MARGIN).max(1.0),
    );
    let mut changed = false;
    let IndependentTerminals { left, right, .. } = &mut *terminals;
    for (renderer, texture) in &textures {
        for terminal in [&mut *left, &mut *right] {
            if renderer
                .surface()
                .shares_state_with(&terminal.backend().surface())
            {
                changed |= terminal.fit_to(texture, half);
            }
        }
    }
    if changed {
        redraw(&mut terminals).expect("the in-memory backend is infallible");
    }
}

fn update_terminals(time: Res<Time>, mut terminals: ResMut<IndependentTerminals>) {
    if !terminals.timer.tick(time.delta()).just_finished() {
        return;
    }
    terminals.tick = terminals.tick.wrapping_add(1);
    redraw(&mut terminals).expect("the in-memory backend is infallible");
}

fn redraw(terminals: &mut IndependentTerminals) -> Result<(), Box<dyn std::error::Error>> {
    let tick = terminals.tick;
    let left_size = terminals.left.size()?;
    let right_size = terminals.right.size()?;
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
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(format!(" {} × {} ", left_size.width, left_size.height)),
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
            Paragraph::new("This terminal updates without touching the surface on the right.")
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(Block::new().borders(Borders::ALL)),
            message,
        );
    })?;

    terminals.right.draw(|frame| {
        let events = (0..8).rev().map(|offset| {
            let revision = tick.saturating_sub(offset);
            ListItem::new(format!("event {revision:04} · right surface only"))
        });
        frame.render_widget(
            List::new(events)
                .block(Block::new().borders(Borders::ALL).title(format!(
                    " RIGHT · {} × {} ",
                    right_size.width, right_size.height
                )))
                .style(Style::new().fg(Color::LightYellow)),
            frame.area(),
        );
    })?;
    Ok(())
}
