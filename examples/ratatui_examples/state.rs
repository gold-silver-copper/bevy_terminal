use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
};

use super::{
    ExampleState,
    support::{ACCENT, centered, example_area, help},
};

macro_rules! state_example {
    ($function:ident, $title:literal, $pattern:literal, $count:literal) => {
        pub fn $function(frame: &mut Frame<'_>, state: &ExampleState) {
            render_pattern(frame, $title, $pattern, state.value as u16, None);
        }
    };
}

state_example!(
    component_trait,
    "state/component-trait",
    "Application components own state and expose a render method.",
    7
);
state_example!(
    immutable_consuming,
    "state/immutable-consuming",
    "Widget consumes an immutable Counter value.",
    3
);
state_example!(
    immutable_function,
    "state/immutable-function",
    "A free function renders an immutable state reference.",
    4
);
state_example!(
    immutable_shared_ref,
    "state/immutable-shared-ref",
    "Widget is implemented for &Counter.",
    8
);
state_example!(
    mutable_function,
    "state/mutable-function",
    "A render function receives mutable state.",
    5
);
state_example!(
    mutable_widget,
    "state/mutable-widget",
    "Widget is implemented for &mut Counter.",
    6
);
state_example!(
    refcell,
    "state/refcell",
    "Interior mutability shares a counter through RefCell.",
    9
);
state_example!(
    stateful_widget,
    "state/stateful-widget",
    "StatefulWidget separates rendering behavior from CounterState.",
    11
);
state_example!(
    widget_with_mutable_ref,
    "state/widget-with-mutable-ref",
    "A widget value contains a mutable state reference.",
    12
);

pub fn nested_mutable_widget(frame: &mut Frame<'_>, state: &ExampleState) {
    render_pattern(
        frame,
        "state/nested-mutable-widget",
        "The parent Widget delegates to child widgets implemented for mutable references.",
        state.value as u16,
        Some(state.secondary as u16),
    );
}

pub fn nested_stateful_widget(frame: &mut Frame<'_>, state: &ExampleState) {
    render_pattern(
        frame,
        "state/nested-stateful-widget",
        "The parent StatefulWidget composes independently owned child states.",
        state.value as u16,
        Some(state.secondary as u16),
    );
}

fn render_pattern(
    frame: &mut Frame<'_>,
    title: &str,
    pattern: &str,
    count: u16,
    second_count: Option<u16>,
) {
    let area = example_area(frame, title);
    let card = centered(area, 68, if second_count.is_some() { 21 } else { 16 });
    let [heading, description, counter, second, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(if second_count.is_some() { 5 } else { 0 }),
        Constraint::Min(1),
    ])
    .areas(card);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("State pattern: ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                title.trim_start_matches("state/"),
                Style::new()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]))
        .alignment(Alignment::Center)
        .block(Block::new().borders(Borders::BOTTOM)),
        heading,
    );
    frame.render_widget(
        Paragraph::new(pattern)
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true }),
        description,
    );
    render_counter(frame, counter, "primary counter", count, Color::LightGreen);
    if let Some(second_count) = second_count {
        render_counter(
            frame,
            second,
            "nested counter",
            second_count,
            Color::LightMagenta,
        );
    }
    help(
        frame,
        footer,
        "All state-pattern examples intentionally have similar pixels; their upstream distinction is ownership and API shape.",
    );
}

fn render_counter(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    label: &str,
    count: u16,
    color: Color,
) {
    frame.render_widget(
        Gauge::default()
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(format!(" {label} ")),
            )
            .gauge_style(Style::new().fg(color).bg(Color::Rgb(28, 35, 48)))
            .percent((count * 7).min(100))
            .label(format!("count = {count}")),
        area,
    );
}
