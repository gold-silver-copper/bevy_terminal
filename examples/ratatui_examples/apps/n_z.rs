use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        BarChart, Block, BorderType, Borders, Clear, Gauge, HighlightSpacing, List, ListItem,
        ListState, Paragraph, RatatuiLogo, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table, TableState, Wrap,
        canvas::{Canvas, Line as CanvasLine, Points},
    },
};

use super::super::support::{ACCENT, GREEN, ORANGE, PURPLE, centered, example_area, help, section};

pub fn inline(frame: &mut Frame<'_>) {
    let area = example_area(frame, "inline");
    let [history, downloads, footer] = Layout::vertical([
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(3),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("$ cargo run --example inline", Style::new().fg(GREEN)),
            Line::raw("Resolving packages..."),
            Line::raw("Starting downloads in an inline viewport"),
            Line::styled(
                "The shell history remains visible above the application.",
                Style::new().fg(Color::DarkGray),
            ),
        ])
        .block(section("Terminal history")),
        history,
    );
    let rows = Layout::vertical([Constraint::Ratio(1, 4); 4]).split(downloads);
    let items = [
        ("ratatui-core", 100, GREEN),
        ("ratatui-widgets", 84, ACCENT),
        ("bevy-ui", 61, PURPLE),
        ("unicode-data", 37, ORANGE),
    ];
    for (area, (name, progress, color)) in rows.iter().zip(items) {
        frame.render_widget(
            Gauge::default()
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .title(format!(" {name} ")),
                )
                .percent(progress)
                .gauge_style(Style::new().fg(color).bg(Color::Rgb(26, 32, 45)))
                .label(format!("{progress}%")),
            *area,
        );
    }
    frame.render_widget(
        Paragraph::new(r#"$ echo "terminal restored below inline viewport""#)
            .style(Style::new().fg(Color::LightGreen)),
        footer,
    );
}

pub fn input_form(frame: &mut Frame<'_>) {
    let area = example_area(frame, "input-form");
    let card = centered(area, 72, 32);
    let [heading, name, age, email, validation, submit, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Min(2),
    ])
    .areas(card);
    frame.render_widget(
        Paragraph::new("Create profile")
            .alignment(Alignment::Center)
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        heading,
    );
    render_field(frame, name, "Name", "Ferris Crab", false);
    render_field(frame, age, "Age", "29", false);
    render_field(frame, email, "Email", "ferris@example", true);
    frame.render_widget(
        Paragraph::new("▲ Email must contain a complete domain")
            .style(Style::new().fg(Color::LightRed)),
        validation,
    );
    let button = centered(submit, 24, 3);
    frame.render_widget(
        Paragraph::new("Submit")
            .alignment(Alignment::Center)
            .style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::new().borders(Borders::ALL)),
        button,
    );
    help(
        frame,
        footer,
        "Tab next field  •  Enter submit  •  Esc quit",
    );
}

fn render_field(frame: &mut Frame<'_>, area: Rect, label: &str, value: &str, invalid: bool) {
    let border = if invalid {
        Color::LightRed
    } else {
        Color::DarkGray
    };
    frame.render_widget(
        Paragraph::new(value).block(
            Block::new()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(border))
                .title(format!(" {label} ")),
        ),
        area,
    );
}

pub fn minimal(frame: &mut Frame<'_>) {
    let area = example_area(frame, "minimal");
    frame.render_widget(
        Paragraph::new("Hello, Ratatui!")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::LightCyan)),
        centered(area, 30, 3),
    );
}

pub fn modifiers(frame: &mut Frame<'_>) {
    let area = example_area(frame, "modifiers");
    let [title, table_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(50)]).areas(area);
    frame.render_widget(
        Paragraph::new("Note: not all terminals support all modifiers")
            .style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
        title,
    );
    let colors = [
        Color::Black,
        Color::DarkGray,
        Color::Gray,
        Color::White,
        Color::Red,
    ];
    let modifiers = [
        ("NONE", Modifier::empty()),
        ("BOLD", Modifier::BOLD),
        ("DIM", Modifier::DIM),
        ("ITALIC", Modifier::ITALIC),
        ("UNDERLINED", Modifier::UNDERLINED),
        ("SLOW_BLINK", Modifier::SLOW_BLINK),
        ("RAPID_BLINK", Modifier::RAPID_BLINK),
        ("REVERSED", Modifier::REVERSED),
        ("HIDDEN", Modifier::HIDDEN),
        ("CROSSED_OUT", Modifier::CROSSED_OUT),
    ];
    let rows = Layout::vertical([Constraint::Length(1); 50]).split(table_area);
    let cells = rows
        .iter()
        .flat_map(|row| {
            Layout::horizontal([Constraint::Percentage(20); 5])
                .split(*row)
                .to_vec()
        })
        .collect::<Vec<_>>();
    let mut index = 0;
    for background in colors {
        for foreground in colors {
            for (name, modifier) in modifiers {
                frame.render_widget(
                    Paragraph::new(format!("{name:<12}.")).style(
                        Style::new()
                            .fg(foreground)
                            .bg(background)
                            .add_modifier(modifier),
                    ),
                    cells[index],
                );
                index += 1;
            }
        }
    }
}

pub fn mouse_drawing(frame: &mut Frame<'_>) {
    let area = example_area(frame, "mouse-drawing");
    let [canvas_area, footer] =
        Layout::vertical([Constraint::Min(12), Constraint::Length(2)]).areas(area);
    let path: Vec<(f64, f64)> = (0..180)
        .map(|step| {
            let angle = f64::from(step) / 14.0;
            (
                angle.cos() * (18.0 + f64::from(step) * 0.16),
                angle.sin() * (8.0 + f64::from(step) * 0.07),
            )
        })
        .collect();
    frame.render_widget(
        Canvas::default()
            .block(section("Mouse canvas · deterministic drag gesture"))
            .marker(Marker::Braille)
            .x_bounds([-55.0, 55.0])
            .y_bounds([-24.0, 24.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &path,
                    color: Color::LightMagenta,
                });
                if let Some(&(x, y)) = path.last() {
                    ctx.print(
                        x - 5.0,
                        y + 2.0,
                        Line::styled("cursor", Style::new().fg(Color::Yellow)),
                    );
                }
            }),
        canvas_area,
    );
    help(
        frame,
        footer,
        "Upstream mouse events are replayed as a fixed spiral path",
    );
}

pub fn panic(frame: &mut Frame<'_>) {
    let area = example_area(frame, "panic");
    let card = centered(area, 78, 24);
    let [banner, report, footer] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(12),
        Constraint::Length(3),
    ])
    .areas(card);
    frame.render_widget(
        Paragraph::new("APPLICATION PANICKED")
            .alignment(Alignment::Center)
            .style(
                Style::new()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double),
            ),
        banner,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "message: intentional panic from the Ratatui example",
                Style::new().fg(Color::LightRed),
            ),
            Line::raw("location: examples/apps/panic/src/main.rs:42:9"),
            Line::raw(""),
            Line::raw("The panic hook restored the terminal before printing this report."),
            Line::raw("In the Bevy port the report is captured as a deterministic UI state."),
        ])
        .block(section("Panic hook state"))
        .wrap(Wrap { trim: false }),
        report,
    );
    help(frame, footer, "Press any key to exit");
}

pub fn popup(frame: &mut Frame<'_>) {
    let area = example_area(frame, "popup");
    let text = (1..=30)
        .map(|line| {
            Line::from(format!(
                "Background line {line:02} · popup should clear and cover this text cleanly."
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::new().fg(Color::DarkGray))
            .wrap(Wrap { trim: false }),
        area,
    );
    let popup = centered(area, 56, 15);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Popup",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::raw("This area was cleared before rendering."),
            Line::raw("No background text should show through."),
            Line::raw(""),
            Line::styled("Press p to close", Style::new().fg(Color::LightCyan)),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::new().fg(PURPLE)),
        ),
        popup,
    );
}

pub fn release_header(frame: &mut Frame<'_>) {
    let area = example_area(frame, "release-header");
    frame.buffer_mut().set_style(
        area,
        Style::new()
            .fg(Color::Rgb(246, 214, 187))
            .bg(Color::Rgb(20, 20, 50)),
    );
    let center = centered(area, 66, 22);
    let [logo_area, menus] = Layout::horizontal([Constraint::Length(36), Constraint::Length(28)])
        .spacing(2)
        .areas(center);
    let [shadow, logo, version] = Layout::vertical([
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(2),
    ])
    .flex(Flex::Center)
    .areas(logo_area);
    let rainbow = [
        Color::Rgb(156, 40, 60),
        Color::Rgb(156, 90, 40),
        Color::Rgb(156, 156, 40),
        Color::Rgb(40, 156, 80),
        Color::Rgb(40, 90, 166),
        Color::Rgb(90, 40, 166),
        Color::Rgb(156, 40, 166),
    ];
    let bands = Layout::horizontal([Constraint::Ratio(1, 7); 7]).split(shadow);
    for (band, color) in bands.iter().zip(rainbow) {
        frame.render_widget(Block::new().style(Style::new().bg(color)), *band);
    }
    frame.render_widget(RatatuiLogo::small(), shadow);
    frame.render_widget(RatatuiLogo::small(), logo);
    frame.render_widget(
        Paragraph::new(r#"v0.30.1 "Bryndza""#)
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::DarkGray)),
        version,
    );
    let [main, backends] =
        Layout::vertical([Constraint::Length(9), Constraint::Length(9)]).areas(menus);
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw("> ratatui"),
            Line::raw("> ratatui-core"),
            Line::raw("> ratatui-widgets"),
            Line::raw("> ratatui-macros"),
        ])
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::LightYellow))
                .title(" Main Courses "),
        ),
        main,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw("> ratatui-crossterm"),
            Line::raw("> ratatui-termion"),
            Line::raw("> ratatui-termina"),
            Line::raw("> ratatui-termwiz"),
        ])
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::LightYellow))
                .title(" Pairings "),
        ),
        backends,
    );
}

pub fn scrollbar(frame: &mut Frame<'_>) {
    let area = example_area(frame, "scrollbar");
    let [heading, left, right] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ])
    .areas(area);
    help(frame, heading, "Use h j k l or ◄ ▲ ▼ ► to scroll");
    let [vertical_a, vertical_b] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);
    let [horizontal_a, horizontal_b] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);
    let long_text = (0..24)
        .map(|line| {
            Line::from(format!(
                "Line {line:02}: scrollbars track content position and viewport length."
            ))
        })
        .collect::<Vec<_>>();
    let mut vertical_state = ScrollbarState::new(long_text.len()).position(7);
    frame.render_widget(
        Paragraph::new(long_text.clone())
            .block(section("Vertical with arrows"))
            .scroll((7, 0)),
        vertical_a,
    );
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        vertical_a,
        &mut vertical_state,
    );
    let mut left_state = ScrollbarState::new(long_text.len()).position(11);
    frame.render_widget(
        Paragraph::new(long_text.clone())
            .block(section("Mirrored no track"))
            .scroll((11, 0)),
        vertical_b,
    );
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalLeft)
            .begin_symbol(None)
            .track_symbol(None)
            .end_symbol(None),
        vertical_b.inner(Margin::new(0, 1)),
        &mut left_state,
    );
    render_horizontal_scroll(frame, horizontal_a, "Custom thumb", 18, "▓");
    render_horizontal_scroll(frame, horizontal_b, "Track and thumb", 33, "░");
}

fn render_horizontal_scroll(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    position: usize,
    thumb: &'static str,
) {
    let line = "Veeeeeeeeeeeeeeeery loooooooooooooooooong horizontal content ".repeat(5);
    frame.render_widget(
        Paragraph::new(line.clone())
            .block(section(title))
            .scroll((0, position as u16)),
        area,
    );
    let mut state = ScrollbarState::new(line.len()).position(position);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
            .thumb_symbol(thumb)
            .track_symbol(Some("─")),
        area.inner(Margin::new(1, 0)),
        &mut state,
    );
}

pub fn table(frame: &mut Frame<'_>) {
    let area = example_area(frame, "table");
    let [table_area, footer] =
        Layout::vertical([Constraint::Min(30), Constraint::Length(4)]).areas(area);
    let rows = [
        (
            "Ada Lovelace",
            "12 Analytical Engine Way",
            "ada@example.test",
        ),
        ("Grace Hopper", "1 Compiler Court", "grace@example.test"),
        (
            "Margaret Hamilton",
            "11 Apollo Lane",
            "margaret@example.test",
        ),
        (
            "Ferris Crab",
            "[no address or email is available for this person]",
            "",
        ),
        (
            "Barbara Liskov",
            "7 Substitution Street",
            "barbara@example.test",
        ),
        (
            "Edsger Dijkstra",
            "0 Goto Considered Harmful",
            "edsger@example.test",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, address, email))| {
        Row::new([name, address, email])
            .height(4)
            .style(Style::new().bg(if index % 2 == 0 {
                Color::Rgb(24, 32, 44)
            } else {
                Color::Rgb(31, 40, 55)
            }))
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Percentage(45),
            Constraint::Min(24),
        ],
    )
    .header(
        Row::new(["Name", "Address", "Email"]).style(
            Style::new()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .row_highlight_style(
        Style::new()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::REVERSED),
    )
    .column_highlight_style(Style::new().fg(Color::LightCyan))
    .cell_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol(" █ ")
    .highlight_spacing(HighlightSpacing::Always)
    .block(section("People"));
    let mut state = TableState::default()
        .with_selected(2)
        .with_selected_column(1);
    frame.render_stateful_widget(table, table_area, &mut state);
    let mut scroll = ScrollbarState::new(12).position(2);
    frame.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None),
        table_area.inner(Margin::new(1, 1)),
        &mut scroll,
    );
    frame.render_widget(
        Paragraph::new("↑/↓ row  ←/→ column  Shift+←/→ color")
            .alignment(Alignment::Center)
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double),
            ),
        footer,
    );
}

pub fn todo_list(frame: &mut Frame<'_>) {
    let area = example_area(frame, "todo-list");
    let [header, content, footer] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(30),
        Constraint::Length(3),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new("Ratatui Todo List")
            .alignment(Alignment::Center)
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))
            .block(Block::new().borders(Borders::BOTTOM)),
        header,
    );
    let [list_area, details] =
        Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)]).areas(content);
    let tasks = [
        ("✓", "Create Bevy backend", Color::LightGreen),
        ("✓", "Anchor wide graphemes", Color::LightGreen),
        ("•", "Port Ratatui examples", Color::LightYellow),
        (" ", "Publish crate documentation", Color::DarkGray),
        (" ", "Celebrate with cheese", Color::DarkGray),
    ]
    .map(|(mark, text, color)| {
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {mark} "), Style::new().fg(color)),
            Span::raw(text),
        ]))
    });
    let mut list_state = ListState::default().with_selected(Some(2));
    frame.render_stateful_widget(
        List::new(tasks)
            .block(section("Tasks"))
            .highlight_style(Style::new().bg(Color::Rgb(46, 55, 76)))
            .highlight_symbol("▶ "),
        list_area,
        &mut list_state,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Port Ratatui examples",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::raw("Status: in progress"),
            Line::raw("Priority: high"),
            Line::raw(""),
            Line::raw("Acceptance criteria:"),
            Line::raw("• every upstream runnable target is represented"),
            Line::raw("• fixtures are deterministic and offline"),
            Line::raw("• all frames are exported and inspected"),
        ])
        .block(section("Selected task"))
        .wrap(Wrap { trim: false }),
        details,
    );
    help(
        frame,
        footer,
        "a add  •  e edit  •  d delete  •  space toggle  •  q quit",
    );
}

pub fn tracing(frame: &mut Frame<'_>) {
    let area = example_area(frame, "tracing");
    let [stats, events, footer] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Min(26),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("events 128", Style::new().fg(ACCENT)),
            Span::raw("   "),
            Span::styled("info 91", Style::new().fg(GREEN)),
            Span::raw("   "),
            Span::styled("warn 4", Style::new().fg(ORANGE)),
            Span::raw("   "),
            Span::styled("error 1", Style::new().fg(Color::LightRed)),
        ]))
        .alignment(Alignment::Center)
        .block(section("Tracing subscriber")),
        stats,
    );
    let logs = [
        (
            "07:20:14.102",
            "INFO ",
            "app",
            "initialized terminal renderer",
        ),
        (
            "07:20:14.184",
            "DEBUG",
            "layout",
            "computed 100×48 cell grid",
        ),
        ("07:20:14.221", "INFO ", "render", "submitted Bevy UI frame"),
        (
            "07:20:14.305",
            "WARN ",
            "font",
            "fallback selected for emoji glyph",
        ),
        ("07:20:14.392", "TRACE", "backend", "flush revision=42"),
        (
            "07:20:14.510",
            "INFO ",
            "export",
            "saved deterministic snapshot",
        ),
    ]
    .map(|(time, level, target, message)| {
        let color = match level.trim() {
            "WARN" => ORANGE,
            "ERROR" => Color::LightRed,
            "DEBUG" => Color::LightBlue,
            "TRACE" => Color::DarkGray,
            _ => GREEN,
        };
        Line::from(vec![
            Span::styled(time, Style::new().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(level, Style::new().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(format!("{target:>8}"), Style::new().fg(PURPLE)),
            Span::raw("  "),
            Span::raw(message),
        ])
    });
    frame.render_widget(
        Paragraph::new(logs.to_vec()).block(section("Captured events")),
        events,
    );
    help(
        frame,
        footer,
        "The upstream tracing subscriber and writer thread are represented by fixed events",
    );
}

pub fn user_input(frame: &mut Frame<'_>) {
    let area = example_area(frame, "user-input");
    let card = centered(area, 80, 28);
    let [prompt, input, preview, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .areas(card);
    frame.render_widget(
        Paragraph::new("What should Ratatui render?")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        prompt,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("> draw a "),
            Span::styled("wide 界", Style::new().fg(ACCENT)),
            Span::raw(" character and emoji 🚀"),
            Span::styled("▌", Style::new().fg(Color::Black).bg(Color::Gray)),
        ]))
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(ACCENT))
                .title(" Input "),
        ),
        input,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Live preview",
                Style::new().fg(GREEN).add_modifier(Modifier::UNDERLINED),
            ),
            Line::raw(""),
            Line::raw("draw a wide 界 character and emoji 🚀"),
            Line::raw(""),
            Line::styled(
                "Column anchors after wide glyphs remain stable.",
                Style::new().fg(Color::DarkGray),
            ),
        ])
        .alignment(Alignment::Center)
        .block(section("Output")),
        preview,
    );
    help(frame, footer, "Enter submit  •  Ctrl+C quit");
}

pub fn volatility_surface(frame: &mut Frame<'_>) {
    let area = example_area(frame, "volatility-surface");
    let [header, surface, footer] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(32),
        Constraint::Length(3),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "VOLATILITY SURFACE",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   SPX 5,742.31   "),
            Span::styled("VIX 14.82", Style::new().fg(ORANGE)),
            Span::raw("   2026-08-16"),
        ]))
        .alignment(Alignment::Center)
        .block(Block::new().borders(Borders::BOTTOM)),
        header,
    );
    frame.render_widget(
        Canvas::default()
            .block(section("Implied volatility by strike and expiry"))
            .marker(Marker::Braille)
            .x_bounds([-75.0, 75.0])
            .y_bounds([-35.0, 35.0])
            .paint(|ctx| {
                for expiry in 0..13 {
                    let depth = f64::from(expiry);
                    let mut previous = None;
                    for strike in 0..31 {
                        let x = f64::from(strike) - 15.0;
                        let smile = x.powi(2) * 0.045 + (depth * 0.45).sin() * 1.8;
                        let screen_x = x * 3.4 + depth * 1.7 - 10.0;
                        let screen_y = smile * 1.5 + depth * 1.25 - 20.0;
                        if let Some((px, py)) = previous {
                            ctx.draw(&CanvasLine {
                                x1: px,
                                y1: py,
                                x2: screen_x,
                                y2: screen_y,
                                color: Color::Rgb(
                                    (70 + expiry * 10) as u8,
                                    (210 - expiry * 6) as u8,
                                    (150 + expiry * 7) as u8,
                                ),
                            });
                        }
                        previous = Some((screen_x, screen_y));
                    }
                }
                for strike in [-12.0_f64, -6.0, 0.0, 6.0, 12.0] {
                    ctx.draw(&CanvasLine {
                        x1: strike * 3.4 - 10.0,
                        y1: strike.powi(2) * 0.0675 - 20.0,
                        x2: strike * 3.4 + 10.4,
                        y2: strike.powi(2) * 0.0675 - 4.0,
                        color: Color::DarkGray,
                    });
                }
                ctx.print(-70.0, -31.0, Line::from("strike →"));
                ctx.print(48.0, 25.0, Line::from("expiry"));
            }),
        surface,
    );
    help(
        frame,
        footer,
        "←/→ rotate  •  ↑/↓ tilt  •  r regenerate  •  deterministic market fixture",
    );
}

pub fn weather(frame: &mut Frame<'_>) {
    let area = example_area(frame, "weather");
    let card = centered(area, 84, 34);
    let [heading, current, chart_area, footer] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(7),
        Constraint::Min(16),
        Constraint::Length(2),
    ])
    .areas(card);
    frame.render_widget(
        Paragraph::new("San Francisco · 7 day forecast")
            .alignment(Alignment::Center)
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        heading,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "☀  72°F",
                Style::new()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   Feels like 71°   Humidity 48%   Wind W 12 mph"),
        ]))
        .alignment(Alignment::Center)
        .block(section("Current")),
        current,
    );
    let temperatures = [
        ("Sun", 72),
        ("Mon", 68),
        ("Tue", 65),
        ("Wed", 69),
        ("Thu", 74),
        ("Fri", 77),
        ("Sat", 73),
    ];
    frame.render_widget(
        BarChart::default()
            .block(section("Daily high °F"))
            .data(&temperatures)
            .bar_width(6)
            .bar_gap(3)
            .bar_style(Style::new().fg(Color::LightYellow))
            .value_style(Style::new().fg(Color::Black).bg(Color::LightYellow)),
        chart_area,
    );
    help(
        frame,
        footer,
        "Random upstream temperatures replaced by a fixed forecast",
    );
}

pub fn widget_ref_container(frame: &mut Frame<'_>) {
    let area = example_area(frame, "widget-ref-container");
    let card = centered(area, 70, 28);
    let [heading, greeting, farewell, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Min(2),
    ])
    .areas(card);
    frame.render_widget(
        Paragraph::new("StackContainer<&dyn WidgetRef>")
            .alignment(Alignment::Center)
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        heading,
    );
    frame.render_widget(
        Paragraph::new("Hello from a borrowed Greeting widget")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::LightGreen))
            .block(section("Greeting")),
        greeting,
    );
    frame.render_widget(
        Paragraph::new("Goodbye from a borrowed Farewell widget")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::LightMagenta))
            .block(section("Farewell")),
        farewell,
    );
    help(
        frame,
        footer,
        "The container owns references and renders both widgets without consuming them",
    );
}
