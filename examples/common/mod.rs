use bevy_grid::{BevyBackend, TerminalSurface};
use ratatui::{
    Terminal,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};

pub const COLUMNS: u16 = 72;
pub const ROWS: u16 = 22;

pub fn demo_surface() -> TerminalSurface {
    let backend = BevyBackend::new(COLUMNS, ROWS);
    let surface = backend.surface();
    let mut terminal = Terminal::new(backend).expect("the in-memory backend is infallible");
    terminal
        .draw(|frame| {
            let [header, body, footer] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .areas(frame.area());

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        " bevy_grid ",
                        Style::new()
                            .fg(Color::Black)
                            .bg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  Ratatui rendered by Bevy UI + text",
                        Style::new().fg(Color::LightYellow),
                    ),
                ]))
                .block(Block::new().borders(Borders::ALL).border_style(Color::Cyan)),
                header,
            );

            let [left, right] =
                Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                    .areas(body);

            let unicode = vec![
                Line::from(Span::styled(
                    "Unicode cell geometry",
                    Style::new()
                        .fg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                )),
                Line::from(""),
                Line::from("ASCII:  0123456789 | columns stay fixed"),
                Line::from("CJK:    汉字 日本語 한글 | after wide cells"),
                Line::from("Marks:  e\u{301}  A\u{30a}  n\u{303} | combining sequences"),
                Line::from("Emoji:  🙂 🚀 | anchored double-width runs"),
                Line::from("Blocks: ████████ ▓▓▒▒░░  ▀▄▌▐"),
                Line::from("Braille: ⣿⣷⣯⣟⡿⢿⣻⣽⣼⣧"),
                Line::from("Lines:   ┌────┬────┐ ╔════╦════╗"),
                Line::from("         │    │    │ ║    ║    ║"),
                Line::from("         └────┴────┘ ╚════╩════╝"),
            ];
            frame.render_widget(
                Paragraph::new(unicode)
                    .block(Block::new().title(" grid ").borders(Borders::ALL))
                    .wrap(Wrap { trim: false }),
                left,
            );

            let [styles, gauge] =
                Layout::vertical([Constraint::Min(9), Constraint::Length(4)]).areas(right);
            let styled = vec![
                Line::from(Span::styled(
                    "Colors + modifiers",
                    Style::new()
                        .fg(Color::Indexed(213))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled(" bold ", Style::new().add_modifier(Modifier::BOLD)),
                    Span::styled(" italic ", Style::new().add_modifier(Modifier::ITALIC)),
                ]),
                Line::from(vec![
                    Span::styled(
                        " underline ",
                        Style::new().add_modifier(Modifier::UNDERLINED),
                    ),
                    Span::styled(
                        " crossed ",
                        Style::new().add_modifier(Modifier::CROSSED_OUT),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        " reversed ",
                        Style::new()
                            .fg(Color::LightBlue)
                            .bg(Color::Rgb(50, 18, 80))
                            .add_modifier(Modifier::REVERSED),
                    ),
                    Span::styled(
                        " dim ",
                        Style::new().fg(Color::White).add_modifier(Modifier::DIM),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(" 256 ", Style::new().fg(Color::Indexed(202))),
                    Span::styled(
                        " truecolor ",
                        Style::new()
                            .fg(Color::Rgb(20, 230, 180))
                            .bg(Color::Rgb(28, 38, 66)),
                    ),
                ]),
                Line::from(Span::styled(
                    " slow blink   rapid blink ",
                    Style::new()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::SLOW_BLINK),
                )),
            ];
            frame.render_widget(
                Paragraph::new(styled).block(Block::new().title(" style ").borders(Borders::ALL)),
                styles,
            );
            frame.render_widget(
                Gauge::default()
                    .block(
                        Block::new()
                            .title(" exact background rectangles ")
                            .borders(Borders::ALL),
                    )
                    .gauge_style(
                        Style::new()
                            .fg(Color::Rgb(90, 210, 170))
                            .bg(Color::Rgb(25, 35, 55)),
                    )
                    .percent(63),
                gauge,
            );

            frame.render_widget(
                Paragraph::new(" Cursor →  Resize with backend.resize + terminal.autoresize ")
                    .style(Style::new().fg(Color::Black).bg(Color::LightYellow))
                    .block(Block::new().borders(Borders::ALL)),
                footer,
            );
            frame.set_cursor_position((11, footer.y + 1));
        })
        .expect("the in-memory backend is infallible");
    surface
}
