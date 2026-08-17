use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, BarChart, Block, BorderType, Borders, Cell, Chart, Dataset, Gauge, GraphType, List,
        ListItem, ListState, Paragraph, Row, Sparkline, Table, TableState, Tabs, Wrap,
        canvas::{Canvas, Circle, Line as CanvasLine, Points},
    },
};

use super::super::{
    ExampleState,
    support::{ACCENT, GREEN, ORANGE, PURPLE, centered, example_area, help, section},
};

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn advanced_widget_impl(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "advanced-widget-impl");
    let [heading, body, footer] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(12),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new("Hello from an advanced custom widget")
            .alignment(Alignment::Center)
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))
            .block(section("Greeting")),
        heading,
    );
    let [timer, boxed, aligned] = Layout::horizontal([
        Constraint::Percentage(32),
        Constraint::Percentage(36),
        Constraint::Percentage(32),
    ])
    .areas(body);
    frame.render_widget(
        Gauge::default()
            .block(section("Timer widget"))
            .gauge_style(
                Style::new()
                    .fg(Color::LightGreen)
                    .bg(Color::Rgb(25, 35, 45)),
            )
            .percent(((state.tick + 72) % 101) as u16)
            .label(format!(
                "{:.1} / 10.0 seconds",
                ((state.tick + 72) % 101) as f64 / 10.0
            )),
        timer,
    );
    let boxed_block = section("Box<dyn WidgetRef>");
    let inner = boxed_block.inner(boxed);
    frame.render_widget(boxed_block, boxed);
    let squares = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .flex(Flex::Center)
        .split(inner);
    frame.render_widget(Block::new().style(Style::new().bg(Color::Red)), squares[0]);
    frame.render_widget(Block::new().style(Style::new().bg(Color::Blue)), squares[1]);
    let aligned_block = section("&mut Widget");
    let inner = aligned_block.inner(aligned);
    frame.render_widget(aligned_block, aligned);
    let [square] = Layout::horizontal([Constraint::Length(10)])
        .flex(Flex::End)
        .areas(inner);
    let [square] = Layout::vertical([Constraint::Length(8)])
        .flex(Flex::Center)
        .areas(square);
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .style(Style::new().fg(Color::Yellow).bg(Color::Rgb(70, 40, 10))),
        square,
    );
    help(
        frame,
        footer,
        "Static snapshot of the upstream ownership and widget-reference demonstration",
    );
}

pub fn async_github(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "async-github");
    let [status, table_area, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "ratatui/ratatui",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  open pull requests  "),
            Span::styled("● synchronized", Style::new().fg(Color::LightGreen)),
        ]))
        .alignment(Alignment::Center)
        .block(Block::new().borders(Borders::BOTTOM)),
        status,
    );
    let rows = [
        ("#2581", "Unleash the rats v0.30.2", "ratatui-bot", "merged"),
        ("#2574", "Improve table row highlighting", "orhun", "review"),
        (
            "#2568",
            "Document custom backend semantics",
            "joshka",
            "ready",
        ),
        (
            "#2559",
            "Fix wide grapheme diff behavior",
            "fdehau",
            "draft",
        ),
        ("#2547", "Add layout flex examples", "ratatui", "ready"),
        (
            "#2531",
            "Clarify canvas marker resolution",
            "maintainer",
            "review",
        ),
    ];
    let rows = rows.map(|(number, title, author, state)| {
        let color = match state {
            "merged" => PURPLE,
            "ready" => GREEN,
            "draft" => Color::DarkGray,
            _ => ORANGE,
        };
        Row::new([
            Cell::from(number).style(Style::new().fg(Color::DarkGray)),
            Cell::from(title),
            Cell::from(author).style(Style::new().fg(Color::LightCyan)),
            Cell::from(state).style(Style::new().fg(color)),
        ])
    });
    let mut table_state = TableState::default().with_selected(state.selected.unwrap_or(0));
    frame.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Min(32),
                Constraint::Length(16),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(["PR", "Title", "Author", "State"])
                .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .bottom_margin(1),
        )
        .block(section("Deterministic API fixture"))
        .column_spacing(2)
        .row_highlight_style(Style::new().bg(Color::Rgb(45, 55, 80)))
        .highlight_symbol("▶ "),
        table_area,
        &mut table_state,
    );
    help(
        frame,
        footer,
        "The upstream Octocrab request is replaced by fixed PR data for offline render QA",
    );
}

pub fn calendar_explorer(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "calendar-explorer");
    let [header, calendars, legend] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(20),
        Constraint::Length(3),
    ])
    .areas(area);
    let month_lengths = [31_i32, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut day_of_year = state.value.rem_euclid(365);
    let mut selected_month = 0_usize;
    while day_of_year >= month_lengths[selected_month] {
        day_of_year -= month_lengths[selected_month];
        selected_month += 1;
    }
    let selected_day = usize::try_from(day_of_year + 1).unwrap_or(1);
    let selection_color = [ACCENT, Color::LightGreen, Color::LightMagenta][state.palette % 3];
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "2026",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  ←  {} {selected_day}  →  ",
                MONTH_NAMES[selected_month]
            )),
            Span::styled(
                "selected",
                Style::new().fg(Color::Black).bg(selection_color),
            ),
        ]))
        .alignment(Alignment::Center),
        header,
    );
    let month_rows = Layout::vertical([Constraint::Ratio(1, 3); 3]).split(calendars);
    let month_areas = month_rows
        .iter()
        .flat_map(|row| {
            Layout::horizontal([Constraint::Ratio(1, 4); 4])
                .split(*row)
                .to_vec()
        })
        .collect::<Vec<_>>();
    let months = [
        ("January 2026", 4, 31, None),
        ("February 2026", 0, 28, None),
        ("March 2026", 0, 31, None),
        ("April 2026", 3, 30, None),
        ("May 2026", 5, 31, None),
        ("June 2026", 1, 30, None),
        ("July 2026", 3, 31, None),
        ("August 2026", 6, 31, Some(16)),
        ("September 2026", 2, 30, None),
        ("October 2026", 4, 31, None),
        ("November 2026", 0, 30, None),
        ("December 2026", 2, 31, None),
    ];
    for (index, (area, (title, offset, days, _))) in month_areas.into_iter().zip(months).enumerate()
    {
        let selected = (index == selected_month).then_some(selected_day);
        render_month(frame, area, title, offset, days, selected, selection_color);
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {selected_day:>2} "),
                Style::new().fg(Color::Black).bg(selection_color),
            ),
            Span::raw(" today/selection    "),
            Span::styled(" 23 ", Style::new().fg(Color::Black).bg(Color::LightYellow)),
            Span::raw(" event    ←/→ change day"),
        ]))
        .alignment(Alignment::Center),
        legend,
    );
}

fn render_month(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    weekday_offset: usize,
    days: usize,
    selected: Option<usize>,
    selection_color: Color,
) {
    let mut lines = vec![Line::styled(
        " Su Mo Tu We Th Fr Sa",
        Style::new().fg(Color::DarkGray),
    )];
    let mut cells = vec!["  ".to_string(); weekday_offset];
    cells.extend((1..=days).map(|day| format!("{day:>2}")));
    for week in cells.chunks(7) {
        let mut spans = vec![Span::raw(" ")];
        for value in week {
            let day = value.trim().parse::<usize>().ok();
            let style = if day == selected {
                Style::new()
                    .fg(Color::Black)
                    .bg(selection_color)
                    .add_modifier(Modifier::BOLD)
            } else if day == Some(23) {
                Style::new().fg(Color::Black).bg(Color::LightYellow)
            } else {
                Style::default()
            };
            spans.push(Span::styled(format!("{value} "), style));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(section(title)),
        area,
    );
}

pub fn canvas(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "canvas");
    let [canvas_area, footer] =
        Layout::vertical([Constraint::Min(12), Constraint::Length(2)]).areas(area);
    let orbit: Vec<(f64, f64)> = (0..72)
        .map(|step| {
            let angle = f64::from(step) * std::f64::consts::TAU / 72.0;
            (
                angle.cos() * 48.0 + f64::from(state.offset_x),
                angle.sin() * 18.0 - f64::from(state.offset_y),
            )
        })
        .collect();
    frame.render_widget(
        Canvas::default()
            .block(section("Canvas primitives · Braille marker"))
            .marker(
                [
                    Marker::Braille,
                    Marker::Dot,
                    Marker::Block,
                    Marker::HalfBlock,
                ][state.marker % 4],
            )
            .x_bounds([-100.0, 100.0])
            .y_bounds([-45.0, 45.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &orbit,
                    color: Color::LightCyan,
                });
                ctx.draw(&Circle {
                    x: f64::from(state.offset_x),
                    y: -f64::from(state.offset_y),
                    radius: 13.0,
                    color: Color::Yellow,
                });
                ctx.draw(&CanvasLine {
                    x1: -90.0 + f64::from(state.offset_x),
                    y1: -32.0 - f64::from(state.offset_y),
                    x2: 88.0 + f64::from(state.offset_x),
                    y2: 30.0 - f64::from(state.offset_y),
                    color: Color::LightGreen,
                });
                ctx.print(
                    -12.0 + f64::from(state.offset_x),
                    1.0 - f64::from(state.offset_y),
                    Line::styled("Ratatui", Style::new().fg(PURPLE)),
                );
            }),
        canvas_area,
    );
    help(
        frame,
        footer,
        "Canvas points, circles, lines, labels, and Braille rasterization",
    );
}

pub fn chart(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "chart");
    let quadrants = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(quadrants[0]);
    let bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(quadrants[1]);
    let wave: Vec<(f64, f64)> = (0..80)
        .map(|x| {
            let x = f64::from(x) / 8.0;
            (x, (x + state.tick as f64 * 0.08).sin() * 3.0)
        })
        .collect();
    render_chart_widget(
        frame,
        top[0],
        "Animated line",
        &wave,
        Marker::Braille,
        GraphType::Line,
    );
    render_bar_chart(frame, top[1]);
    let scatter = [
        (0.5, 1.0),
        (1.2, 4.0),
        (2.1, 2.4),
        (3.2, 6.1),
        (4.0, 4.2),
        (5.3, 7.5),
        (6.4, 5.8),
        (7.7, 8.7),
        (8.8, 6.4),
        (9.4, 9.0),
    ];
    render_chart_widget(
        frame,
        bottom[0],
        "Scatter",
        &scatter,
        Marker::Dot,
        GraphType::Scatter,
    );
    let trend = [
        (0.0, 2.0),
        (2.0, 3.4),
        (4.0, 3.1),
        (6.0, 6.8),
        (8.0, 7.3),
        (10.0, 9.2),
    ];
    render_chart_widget(
        frame,
        bottom[1],
        "Line chart",
        &trend,
        Marker::Braille,
        GraphType::Line,
    );
}

fn render_chart_widget(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    data: &[(f64, f64)],
    marker: Marker,
    graph_type: GraphType,
) {
    let datasets = vec![
        Dataset::default()
            .name("series")
            .marker(marker)
            .graph_type(graph_type)
            .style(Style::new().fg(ACCENT))
            .data(data),
    ];
    let chart = Chart::new(datasets)
        .block(section(title))
        .x_axis(Axis::default().bounds([0.0, 10.0]).labels(["0", "5", "10"]))
        .y_axis(
            Axis::default()
                .bounds([-4.0, 10.0])
                .labels(["-4", "3", "10"]),
        );
    frame.render_widget(chart, area);
}

fn render_bar_chart(frame: &mut Frame<'_>, area: Rect) {
    let data = [
        ("A", 4),
        ("B", 7),
        ("C", 12),
        ("D", 6),
        ("E", 15),
        ("F", 10),
    ];
    frame.render_widget(
        BarChart::default()
            .block(section("Bar chart"))
            .data(&data)
            .bar_width(4)
            .bar_gap(2)
            .bar_style(Style::new().fg(GREEN))
            .value_style(Style::new().fg(Color::Black).bg(GREEN)),
        area,
    );
}

pub fn color_explorer(frame: &mut Frame<'_>, _state: &ExampleState) {
    let area = example_area(frame, "color-explorer");
    let [named, indexed, grayscale] = Layout::vertical([
        Constraint::Length(11),
        Constraint::Min(20),
        Constraint::Length(7),
    ])
    .areas(area);
    let colors = [
        ("Black", Color::Black),
        ("Red", Color::Red),
        ("Green", Color::Green),
        ("Yellow", Color::Yellow),
        ("Blue", Color::Blue),
        ("Magenta", Color::Magenta),
        ("Cyan", Color::Cyan),
        ("Gray", Color::Gray),
        ("DarkGray", Color::DarkGray),
        ("LightRed", Color::LightRed),
        ("LightGreen", Color::LightGreen),
        ("LightYellow", Color::LightYellow),
        ("LightBlue", Color::LightBlue),
        ("LightMagenta", Color::LightMagenta),
        ("LightCyan", Color::LightCyan),
        ("White", Color::White),
    ];
    let named_block = section("Named colors · foreground and background");
    let inner = named_block.inner(named);
    frame.render_widget(named_block, named);
    let cells = Layout::horizontal([Constraint::Ratio(1, 8); 8]).split(inner);
    for (index, (name, color)) in colors.iter().enumerate() {
        let column = index % 8;
        let row = index / 8;
        let rect = Rect::new(
            cells[column].x,
            inner.y + row as u16 * 4,
            cells[column].width,
            3,
        );
        let foreground = if matches!(
            color,
            Color::Black | Color::Blue | Color::Red | Color::Magenta | Color::DarkGray
        ) {
            Color::White
        } else {
            Color::Black
        };
        frame.render_widget(
            Paragraph::new(*name)
                .alignment(Alignment::Center)
                .style(Style::new().fg(foreground).bg(*color)),
            rect,
        );
    }
    let indexed_block = section("ANSI indexed 16–231");
    let inner = indexed_block.inner(indexed);
    frame.render_widget(indexed_block, indexed);
    for row in 0..6_u16 {
        for column in 0..36_u16 {
            let index = 16 + row * 36 + column;
            let x = inner.x + column * inner.width / 36;
            let next_x = inner.x + (column + 1) * inner.width / 36;
            frame.render_widget(
                Block::new().style(Style::new().bg(Color::Indexed(index as u8))),
                Rect::new(x, inner.y + row * 3, next_x.saturating_sub(x).max(1), 3),
            );
        }
    }
    let grayscale_block = section("Indexed grayscale 232–255");
    let inner = grayscale_block.inner(grayscale);
    frame.render_widget(grayscale_block, grayscale);
    for column in 0..24_u16 {
        let x = inner.x + column * inner.width / 24;
        let next_x = inner.x + (column + 1) * inner.width / 24;
        frame.render_widget(
            Block::new().style(Style::new().bg(Color::Indexed((232 + column) as u8))),
            Rect::new(x, inner.y, next_x.saturating_sub(x).max(1), inner.height),
        );
    }
}

pub fn colors_rgb(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "colors-rgb");
    let [header, gradient, swatches, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new("24-bit RGB color · deterministic hue/lightness field")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
        header,
    );
    for y in 0..gradient.height {
        for x in 0..gradient.width {
            let phase = (state.tick % 256) as u32;
            let red = ((u32::from(x) * 255 / u32::from(gradient.width.max(1)) + phase) % 256) as u8;
            let green =
                ((u32::from(y) * 255 / u32::from(gradient.height.max(1)) + phase / 2) % 256) as u8;
            let blue = 255_u8.saturating_sub(((u16::from(red) + u16::from(green)) / 2) as u8);
            frame.render_widget(
                Block::new().style(Style::new().bg(Color::Rgb(red, green, blue))),
                Rect::new(gradient.x + x, gradient.y + y, 1, 1),
            );
        }
    }
    let swatch_areas = Layout::horizontal([Constraint::Ratio(1, 6); 6]).split(swatches);
    let values = [
        ("#ff5f5f", Color::Rgb(255, 95, 95)),
        ("#ffaf5f", Color::Rgb(255, 175, 95)),
        ("#ffff87", Color::Rgb(255, 255, 135)),
        ("#5fd7af", Color::Rgb(95, 215, 175)),
        ("#5fafff", Color::Rgb(95, 175, 255)),
        ("#d787ff", Color::Rgb(215, 135, 255)),
    ];
    for (area, (label, color)) in swatch_areas.iter().zip(values) {
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(Style::new().fg(Color::Black).bg(color))
                .block(Block::new().borders(Borders::ALL)),
            *area,
        );
    }
    help(
        frame,
        footer,
        "Every cell in the field is a Ratatui true-color background",
    );
}

pub fn constraint_explorer(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "constraint-explorer");
    let [controls, demos] =
        Layout::horizontal([Constraint::Length(28), Constraint::Min(50)]).areas(area);
    let selected = state.selected.unwrap_or(0);
    let kind_names = ["Min", "Max", "Length", "Percentage", "Ratio", "Fill"];
    let mut control_lines = vec![
        Line::styled(
            "User constraints",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
    ];
    for (index, (value, kind)) in state
        .constraint_values
        .iter()
        .zip(&state.constraint_kinds)
        .enumerate()
    {
        let prefix = if index == selected { "▶" } else { " " };
        control_lines.push(Line::styled(
            format!("{prefix} {}({value})", kind_names[*kind % kind_names.len()]),
            if index == selected {
                Style::new().fg(Color::Black).bg(ACCENT)
            } else {
                Style::default()
            },
        ));
    }
    control_lines.extend([
        Line::raw(""),
        Line::styled(
            format!("Spacing: {}", state.spacing),
            Style::new().fg(ACCENT),
        ),
        Line::raw("←/→ select  ↑/↓ edit"),
        Line::raw("1–6 type  a/x add/delete"),
        Line::raw("+/- spacing"),
    ]);
    frame.render_widget(
        Paragraph::new(control_lines).block(section("Controls")),
        controls,
    );
    let constraints = state
        .constraint_values
        .iter()
        .zip(&state.constraint_kinds)
        .map(|(&value, &kind)| constraint_from(kind, value))
        .collect::<Vec<_>>();
    let rows = Layout::vertical([Constraint::Length(8); 5]).split(demos);
    render_constraint_row(
        frame,
        rows[0],
        "Start",
        Flex::Start,
        &constraints,
        state.spacing,
        selected,
    );
    render_constraint_row(
        frame,
        rows[1],
        "Center",
        Flex::Center,
        &constraints,
        state.spacing,
        selected,
    );
    render_constraint_row(
        frame,
        rows[2],
        "End",
        Flex::End,
        &constraints,
        state.spacing,
        selected,
    );
    render_constraint_row(
        frame,
        rows[3],
        "SpaceBetween",
        Flex::SpaceBetween,
        &constraints,
        state.spacing,
        selected,
    );
    render_constraint_row(
        frame,
        rows[4],
        "SpaceAround",
        Flex::SpaceAround,
        &constraints,
        state.spacing,
        selected,
    );
}

fn constraint_from(kind: usize, value: i32) -> Constraint {
    let value = value.max(0) as u16;
    match kind % 6 {
        0 => Constraint::Min(value),
        1 => Constraint::Max(value),
        2 => Constraint::Length(value),
        3 => Constraint::Percentage(value.min(100)),
        4 => Constraint::Ratio(u32::from(value.max(1)), 100),
        _ => Constraint::Fill(value.max(1)),
    }
}

fn render_constraint_row(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    flex: Flex,
    constraints: &[Constraint],
    spacing: u16,
    selected: usize,
) {
    let block = section(label);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::horizontal(constraints.iter().copied())
        .flex(flex)
        .spacing(spacing)
        .split(inner);
    let colors = [Color::Red, Color::Yellow, Color::Green, Color::Blue];
    for (index, part) in parts.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(format!("C{}", index + 1))
                .alignment(Alignment::Center)
                .style(
                    Style::new()
                        .fg(Color::Black)
                        .bg(colors[index % colors.len()])
                        .add_modifier(if index == selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            *part,
        );
    }
}

pub fn constraints(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "constraints");
    let [tabs, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(20),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Tabs::new(["Length", "Percentage", "Ratio", "Fill", "Min", "Max"])
            .select(state.tab)
            .highlight_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .divider("│")
            .block(Block::new().borders(Borders::BOTTOM)),
        tabs,
    );
    let rows = Layout::vertical([Constraint::Ratio(1, 6); 6]).split(body);
    let examples = [
        (
            "Length",
            [
                Constraint::Length(15),
                Constraint::Length(25),
                Constraint::Fill(1),
            ],
        ),
        (
            "Percentage",
            [
                Constraint::Percentage(20),
                Constraint::Percentage(35),
                Constraint::Percentage(45),
            ],
        ),
        (
            "Ratio",
            [
                Constraint::Ratio(1, 4),
                Constraint::Ratio(1, 2),
                Constraint::Ratio(1, 4),
            ],
        ),
        (
            "Fill",
            [
                Constraint::Fill(1),
                Constraint::Fill(2),
                Constraint::Fill(3),
            ],
        ),
        (
            "Min",
            [Constraint::Min(12), Constraint::Min(20), Constraint::Min(8)],
        ),
        (
            "Max",
            [
                Constraint::Max(12),
                Constraint::Max(20),
                Constraint::Fill(1),
            ],
        ),
    ];
    for (row_index, (row, (label, constraints))) in rows.iter().zip(examples).enumerate() {
        let [name, demo] =
            Layout::horizontal([Constraint::Length(14), Constraint::Min(10)]).areas(*row);
        let label_style = if row_index == state.tab {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Right)
                .style(label_style),
            name,
        );
        let parts = Layout::horizontal(constraints).spacing(1).split(demo);
        for (index, part) in parts.iter().enumerate() {
            let color = [Color::Red, Color::Green, Color::Blue][index];
            frame.render_widget(
                Paragraph::new(if state.selected == Some(index) {
                    format!("▶{index}")
                } else {
                    format!(" {index}")
                })
                .alignment(Alignment::Center)
                .style(Style::new().fg(Color::Black).bg(color)),
                *part,
            );
        }
    }
    help(
        frame,
        footer,
        "All upstream constraint tabs captured in one deterministic comparison frame",
    );
}

pub fn custom_widget(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "custom-widget");
    let card = centered(area, 76, 20);
    let [title, buttons, description] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(8),
        Constraint::Min(3),
    ])
    .areas(card);
    frame.render_widget(
        Paragraph::new("A button implemented directly against Buffer")
            .alignment(Alignment::Center)
            .style(Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        title,
    );
    let button_areas = Layout::horizontal([Constraint::Ratio(1, 3); 3])
        .spacing(3)
        .split(buttons);
    let labels = ["Red", "Green", "Blue"];
    for (index, (area, label)) in button_areas.iter().zip(labels).enumerate() {
        let is_selected = state.selected == Some(index);
        let is_active = is_selected && state.toggled;
        let style = if is_active {
            Style::new()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::new().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::new().fg(Color::White).bg(Color::DarkGray)
        };
        let border_type = if is_active {
            BorderType::Thick
        } else if is_selected {
            BorderType::Double
        } else {
            BorderType::Plain
        };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Center)
                .style(style)
                .block(Block::new().borders(Borders::ALL).border_type(border_type)),
            *area,
        );
    }
    frame.render_widget(
        Paragraph::new(
            "Use the keyboard or click a button. The selected button can be toggled active.",
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true }),
        description,
    );
}

pub fn demo(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "demo");
    let [tabs, main, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(30),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Tabs::new(["Graphs", "Lists & Tables", "Text"])
            .select(state.tab)
            .highlight_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .block(Block::new().borders(Borders::BOTTOM)),
        tabs,
    );
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(main);
    let left_rows = Layout::vertical([
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Min(12),
    ])
    .split(left);
    frame.render_widget(
        Gauge::default()
            .block(section("Gauge 1"))
            .percent(((state.tick + 66) % 101) as u16)
            .gauge_style(
                Style::new()
                    .fg(Color::LightMagenta)
                    .bg(Color::Rgb(35, 30, 48)),
            ),
        left_rows[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(section("Gauge 2"))
            .ratio(((state.tick + 42) % 101) as f64 / 100.0)
            .gauge_style(
                Style::new()
                    .fg(Color::LightGreen)
                    .bg(Color::Rgb(25, 40, 35)),
            ),
        left_rows[1],
    );
    let spark = [
        1, 3, 2, 5, 8, 4, 7, 11, 9, 13, 8, 12, 15, 10, 16, 14, 18, 17,
    ];
    frame.render_widget(
        Sparkline::default()
            .block(section("Sparkline"))
            .data(spark)
            .style(Style::new().fg(ACCENT)),
        left_rows[2],
    );
    let right_rows =
        Layout::vertical([Constraint::Percentage(56), Constraint::Percentage(44)]).split(right);
    let sine: Vec<(f64, f64)> = (0..80)
        .map(|x| {
            let x = f64::from(x) / 8.0;
            (x, (x + state.tick as f64 * 0.08).sin() * 5.0)
        })
        .collect();
    if state.toggled {
        frame.render_widget(
            Paragraph::new("Chart hidden — press t to restore")
                .alignment(Alignment::Center)
                .block(section("Chart")),
            right_rows[0],
        );
    } else {
        render_chart_widget(
            frame,
            right_rows[0],
            "Chart",
            &sine,
            Marker::Braille,
            GraphType::Line,
        );
    }
    let items = [
        "Event: connected",
        "Event: loaded widgets",
        "Event: rendered Bevy UI",
        "Event: awaiting input",
    ]
    .map(ListItem::new);
    let mut list_state = ListState::default().with_selected(state.selected);
    frame.render_stateful_widget(
        List::new(items)
            .block(section("Events"))
            .highlight_style(Style::new().bg(Color::Blue))
            .highlight_symbol("▶ "),
        right_rows[1],
        &mut list_state,
    );
    help(frame, footer, "h/l tabs  •  j/k select  •  t toggle chart");
}

pub fn flex(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "flex");
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(34),
        Constraint::Length(2),
    ])
    .areas(area);
    frame.render_widget(
        Tabs::new([
            "Legacy",
            "Start",
            "Center",
            "End",
            "SpaceBetween",
            "SpaceAround",
            "SpaceEvenly",
        ])
        .select(state.tab)
        .highlight_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        header,
    );
    let rows = Layout::vertical([Constraint::Ratio(1, 7); 7]).split(body);
    let modes = [
        ("Legacy", Flex::Legacy),
        ("Start", Flex::Start),
        ("Center", Flex::Center),
        ("End", Flex::End),
        ("SpaceBetween", Flex::SpaceBetween),
        ("SpaceAround", Flex::SpaceAround),
        ("SpaceEvenly", Flex::SpaceEvenly),
    ];
    for (index, (area, (label, mode))) in rows.iter().zip(modes).enumerate() {
        render_flex_row(
            frame,
            *area,
            label,
            mode,
            state.spacing,
            state.selected == Some(index),
        );
    }
    help(
        frame,
        footer,
        "Each row uses the same three fixed-length constraints with a different Flex mode",
    );
}

fn render_flex_row(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    flex: Flex,
    spacing: u16,
    selected: bool,
) {
    let [name, demo] =
        Layout::horizontal([Constraint::Length(16), Constraint::Min(20)]).areas(area);
    frame.render_widget(
        Paragraph::new(if selected {
            format!("▶ {label}")
        } else {
            label.to_owned()
        })
        .alignment(Alignment::Right)
        .style(if selected {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }),
        name,
    );
    let parts = Layout::horizontal([Constraint::Length(10); 3])
        .flex(flex)
        .spacing(spacing)
        .split(demo);
    for (index, part) in parts.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(format!(" item {} ", index + 1))
                .alignment(Alignment::Center)
                .style(
                    Style::new()
                        .fg(Color::Black)
                        .bg([Color::Red, Color::Green, Color::Blue][index]),
                ),
            *part,
        );
    }
}

pub fn gauge(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "gauge");
    let card = centered(area, 80, 34);
    let rows = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Min(2),
    ])
    .split(card);
    let progress = (state.tick % 101) as u16;
    frame.render_widget(
        Gauge::default()
            .block(section("Default gauge"))
            .percent(progress)
            .label(format!("{progress}%")),
        rows[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(section("Unicode gauge"))
            .percent((progress + 27) % 101)
            .use_unicode(true)
            .gauge_style(Style::new().fg(GREEN).bg(Color::Rgb(25, 38, 35))),
        rows[1],
    );
    frame.render_widget(
        Gauge::default()
            .block(section("Styled gauge"))
            .ratio(f64::from((progress + 53) % 101) / 100.0)
            .gauge_style(
                Style::new()
                    .fg(PURPLE)
                    .bg(Color::Rgb(42, 28, 52))
                    .add_modifier(Modifier::ITALIC),
            )
            .label(format!("throughput {}%", (progress + 53) % 101)),
        rows[2],
    );
    frame.render_widget(
        ratatui::widgets::LineGauge::default()
            .block(section("Line gauge"))
            .ratio(f64::from((progress + 39) % 101) / 100.0)
            .filled_style(Style::new().fg(ORANGE).add_modifier(Modifier::BOLD))
            .unfilled_style(Style::new().fg(Color::DarkGray)),
        rows[3],
    );
    help(frame, rows[4], "Space/Enter restarts the animation");
}

pub fn hello_world(frame: &mut Frame<'_>, _state: &ExampleState) {
    let area = example_area(frame, "hello-world");
    let card = centered(area, 54, 9);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Hello World!",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::styled("Press q to quit", Style::new().fg(Color::DarkGray)),
        ])
        .alignment(Alignment::Center)
        .block(
            Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        ),
        card,
    );
}

pub fn hyperlink(frame: &mut Frame<'_>, _state: &ExampleState) {
    let area = example_area(frame, "hyperlink");
    let card = centered(area, 72, 13);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Hyperlink widget",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::from(vec![
                Span::raw("Project website: "),
                Span::styled(
                    "https://ratatui.rs",
                    Style::new()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::UNDERLINED),
                ),
            ]),
            Line::raw(""),
            Line::styled(
                "Bevy UI has no terminal OSC-8 target; this port preserves the visual affordance.",
                Style::new().fg(Color::DarkGray),
            ),
        ])
        .alignment(Alignment::Center)
        .block(section("OSC-8 visual port")),
        card,
    );
}
