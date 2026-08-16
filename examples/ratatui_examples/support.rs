use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn example_area(frame: &mut Frame<'_>, title: &str) -> Rect {
    let outer = frame.area();
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::DarkGray))
        .title(Line::from(vec![
            Span::styled(" Ratatui 0.30.2 · ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                title,
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    inner
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(vertical);
    horizontal
}

pub fn section<'a>(title: &'a str) -> Block<'a> {
    Block::new()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::DarkGray))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(Color::Yellow),
        ))
}

pub fn help(frame: &mut Frame<'_>, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::DarkGray)),
        area,
    );
}

pub const ACCENT: Color = Color::Rgb(82, 196, 232);
pub const GREEN: Color = Color::Rgb(92, 200, 140);
pub const ORANGE: Color = Color::Rgb(245, 166, 35);
pub const PURPLE: Color = Color::Rgb(190, 120, 240);
