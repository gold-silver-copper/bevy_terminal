use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::{self, Marker},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, BorderType, Borders, Clear, LineGauge, List, ListItem,
        ListState, MascotEyeColor, Padding, Paragraph, RatatuiMascot, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableState, Tabs, Wrap,
        canvas::{Canvas, Line as CanvasLine, Map, MapResolution, Points},
    },
};

use super::super::{
    ExampleState,
    support::{centered, example_area},
};

const DARK_BLUE: Color = Color::Rgb(16, 24, 48);
const LIGHT_BLUE: Color = Color::Rgb(64, 96, 192);
const LIGHT_YELLOW: Color = Color::Rgb(192, 192, 96);
const LIGHT_GREEN: Color = Color::Rgb(64, 192, 96);
const LIGHT_RED: Color = Color::Rgb(192, 96, 96);
const BLACK: Color = Color::Rgb(8, 8, 8);
const DARK_GRAY: Color = Color::Rgb(68, 68, 68);
const MID_GRAY: Color = Color::Rgb(128, 128, 128);
const LIGHT_GRAY: Color = Color::Rgb(188, 188, 188);
const WHITE: Color = Color::Rgb(238, 238, 238);

const TAB_TITLES: [&str; 5] = ["", " Recipe ", " Email ", " Traceroute ", " Weather "];

pub fn demo2(frame: &mut Frame<'_>, state: &ExampleState) {
    let area = example_area(frame, "demo2");
    frame.render_widget(Block::new().style(root_style()), area);
    let [title_bar, tab, bottom_bar] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_title_bar(frame, title_bar, state.tab);
    match state.tab {
        0 => render_about(frame, tab, state.demo2_rows[0]),
        1 => render_recipe(frame, tab, state.demo2_rows[1]),
        2 => render_email(frame, tab, state.demo2_rows[2]),
        3 => render_traceroute(frame, tab, state.demo2_rows[3]),
        4 => render_weather(frame, tab, state.demo2_rows[4]),
        _ => unreachable!("demo2 has exactly five tabs"),
    }
    render_bottom_bar(frame, bottom_bar);

    if state.toggled {
        render_destroy(frame, area, state.tick);
    }
}

fn render_title_bar(frame: &mut Frame<'_>, area: Rect, selected: usize) {
    let [title, tabs] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(43)]).areas(area);
    frame.render_widget(
        Paragraph::new("Ratatui").style(
            Style::new()
                .fg(WHITE)
                .bg(DARK_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        title,
    );
    frame.render_widget(
        Tabs::new(TAB_TITLES)
            .style(Style::new().fg(MID_GRAY).bg(DARK_BLUE))
            .highlight_style(
                Style::new()
                    .fg(WHITE)
                    .bg(DARK_BLUE)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .select(selected)
            .divider("")
            .padding("", ""),
        tabs,
    );
}

fn render_bottom_bar(frame: &mut Frame<'_>, area: Rect) {
    let keys = [
        ("H/←", "Left"),
        ("L/→", "Right"),
        ("K/↑", "Up"),
        ("J/↓", "Down"),
        ("D/Del", "Destroy"),
        ("Q/Esc", "Quit"),
    ];
    let spans = keys.into_iter().flat_map(|(key, description)| {
        [
            Span::styled(format!(" {key} "), Style::new().fg(BLACK).bg(DARK_GRAY)),
            Span::styled(
                format!(" {description} "),
                Style::new().fg(DARK_GRAY).bg(BLACK),
            ),
        ]
    });
    frame.render_widget(
        Paragraph::new(Line::from(spans.collect::<Vec<_>>()))
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::Indexed(236)).bg(Color::Indexed(232))),
        area,
    );
}

fn render_about(frame: &mut Frame<'_>, area: Rect, row: usize) {
    render_rgb_swatch(frame, area);
    let [mascot, description] =
        Layout::horizontal([Constraint::Length(34), Constraint::Min(0)]).areas(area);
    let eye = if row.is_multiple_of(2) {
        MascotEyeColor::Default
    } else {
        MascotEyeColor::Red
    };
    frame.render_widget(
        RatatuiMascot::default().set_eye(eye),
        mascot.inner(Margin::new(2, 0)),
    );

    let description = description.inner(Margin::new(2, 4));
    frame.render_widget(Clear, description);
    frame.render_widget(Block::new().style(content_style()), description);
    let description = description.inner(Margin::new(2, 1));
    frame.render_widget(
        Paragraph::new(
            "- cooking up terminal user interfaces -\n\n\
             Ratatui is a Rust crate that provides widgets (e.g. Paragraph, Table) and draws them \
             to the screen efficiently every frame.",
        )
        .style(Style::new().fg(LIGHT_GRAY).bg(DARK_BLUE))
        .block(
            Block::new()
                .title(" Ratatui ")
                .title_alignment(Alignment::Center)
                .borders(Borders::TOP)
                .border_style(Style::new().fg(LIGHT_GRAY).add_modifier(Modifier::BOLD)),
        )
        .wrap(Wrap { trim: true }),
        description,
    );
}

#[derive(Clone, Copy)]
struct Ingredient {
    quantity: &'static str,
    name: &'static str,
}

const INGREDIENTS: [Ingredient; 11] = [
    Ingredient {
        quantity: "4 tbsp",
        name: "olive oil",
    },
    Ingredient {
        quantity: "1",
        name: "onion thinly sliced",
    },
    Ingredient {
        quantity: "4",
        name: "cloves garlic\npeeled and sliced",
    },
    Ingredient {
        quantity: "1",
        name: "small bay leaf",
    },
    Ingredient {
        quantity: "1",
        name: "small eggplant cut\ninto 1/2 inch cubes",
    },
    Ingredient {
        quantity: "1",
        name: "small zucchini halved\nlengthwise and cut\ninto thin slices",
    },
    Ingredient {
        quantity: "1",
        name: "red bell pepper cut\ninto slivers",
    },
    Ingredient {
        quantity: "4",
        name: "plum tomatoes\ncoarsely chopped",
    },
    Ingredient {
        quantity: "1 tsp",
        name: "kosher salt",
    },
    Ingredient {
        quantity: "1/4 cup",
        name: "shredded fresh basil\nleaves",
    },
    Ingredient {
        quantity: "",
        name: "freshly ground black\npepper",
    },
];

fn render_recipe(frame: &mut Frame<'_>, area: Rect, selected: usize) {
    render_rgb_swatch(frame, area);
    let area = area.inner(Margin::new(2, 1));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::new()
            .title(Span::styled(
                "Ratatouille Recipe",
                Style::new().fg(WHITE).add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .style(content_style())
            .padding(Padding::new(1, 1, 2, 1)),
        area,
    );
    let scrollbar_area = Rect {
        y: area.y + 2,
        height: area.height.saturating_sub(3),
        ..area
    };
    let mut scrollbar = ScrollbarState::new(INGREDIENTS.len())
        .viewport_content_length(6)
        .position(selected);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("▐"),
        scrollbar_area,
        &mut scrollbar,
    );

    let area = area.inner(Margin::new(2, 1));
    let [recipe, ingredients] =
        Layout::horizontal([Constraint::Length(44), Constraint::Min(0)]).areas(area);
    let steps = [
        (
            "Step 1: ",
            "Over medium-low heat, add the oil to a large skillet with the onion, garlic, and bay leaf, stirring occasionally, until the onion has softened.",
        ),
        (
            "Step 2: ",
            "Add the eggplant and cook, stirring occasionally, for 8 minutes or until softened. Stir in the zucchini, red bell pepper, tomatoes, and salt, and cook until tender. Stir in the basil and pepper to taste.",
        ),
    ];
    frame.render_widget(
        Paragraph::new(
            steps
                .into_iter()
                .map(|(step, text)| {
                    Line::from(vec![
                        Span::styled(step, Style::new().fg(WHITE).add_modifier(Modifier::BOLD)),
                        Span::styled(text, Style::new().fg(Color::Gray)),
                    ])
                })
                .collect::<Vec<_>>(),
        )
        .wrap(Wrap { trim: true })
        .block(Block::new().padding(Padding::new(0, 1, 0, 0))),
        recipe,
    );
    let rows = INGREDIENTS.into_iter().map(|ingredient| {
        Row::new([ingredient.quantity, ingredient.name])
            .height(ingredient.name.lines().count() as u16)
    });
    let mut table_state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Length(7), Constraint::Length(30)])
            .header(
                Row::new(["Qty", "Ingredient"])
                    .style(Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
            )
            .style(content_style())
            .row_highlight_style(Style::new().fg(LIGHT_YELLOW)),
        ingredients,
        &mut table_state,
    );
}

struct Email {
    from: &'static str,
    subject: &'static str,
    body: &'static str,
}

const EMAILS: [Email; 5] = [
    Email {
        from: "Alice <alice@example.com>",
        subject: "Hello",
        body: "Hi Bob,\nHow are you?\n\nAlice",
    },
    Email {
        from: "Bob <bob@example.com>",
        subject: "Re: Hello",
        body: "Hi Alice,\nI'm fine, thanks!\n\nBob",
    },
    Email {
        from: "Charlie <charlie@example.com>",
        subject: "Re: Hello",
        body: "Hi Alice,\nI'm fine, thanks!\n\nCharlie",
    },
    Email {
        from: "Dave <dave@example.com>",
        subject: "Re: Hello (STOP REPLYING TO ALL)",
        body: "Hi Everyone,\nPlease stop replying to all.\n\nDave",
    },
    Email {
        from: "Eve <eve@example.com>",
        subject: "Re: Hello (STOP REPLYING TO ALL)",
        body: "Hi Everyone,\nI'm reading all your emails.\n\nEve",
    },
];

fn render_email(frame: &mut Frame<'_>, area: Rect, selected: usize) {
    render_rgb_swatch(frame, area);
    let area = area.inner(Margin::new(2, 1));
    frame.render_widget(Clear, area);
    let [inbox, email] = Layout::vertical([Constraint::Length(8), Constraint::Min(0)]).areas(area);
    let [mailboxes, messages] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inbox);
    frame.render_widget(
        Tabs::new([" Inbox ", " Sent ", " Drafts "])
            .style(Style::new().fg(MID_GRAY).bg(DARK_BLUE))
            .highlight_style(
                Style::new()
                    .fg(WHITE)
                    .bg(DARK_BLUE)
                    .add_modifier(Modifier::BOLD),
            )
            .select(0)
            .divider(""),
        mailboxes,
    );
    let items = EMAILS
        .iter()
        .map(|email| ListItem::new(Line::raw(format!("{:<29} {}", email.from, email.subject))));
    let mut list_state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items)
            .style(content_style())
            .highlight_style(Style::new().fg(LIGHT_YELLOW))
            .highlight_symbol(">>"),
        messages,
        &mut list_state,
    );
    let mut scrollbar = ScrollbarState::new(EMAILS.len()).position(selected);
    frame.render_stateful_widget(
        Scrollbar::default()
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("▐"),
        messages,
        &mut scrollbar,
    );

    let block = Block::new()
        .style(content_style())
        .padding(Padding::new(2, 2, 0, 0))
        .borders(Borders::TOP)
        .border_type(BorderType::Thick);
    let inner = block.inner(email);
    frame.render_widget(block, email);
    let selected_email = &EMAILS[selected];
    let [headers, body] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("From: ", Style::new().add_modifier(Modifier::BOLD)),
                Span::styled(selected_email.from, Style::new().fg(LIGHT_GRAY)),
            ]),
            Line::from(vec![
                Span::styled("Subject: ", Style::new().add_modifier(Modifier::BOLD)),
                Span::styled(selected_email.subject, Style::new().fg(LIGHT_GRAY)),
            ]),
            Line::styled(
                "-".repeat(usize::from(inner.width)),
                Style::new().add_modifier(Modifier::DIM),
            ),
        ])
        .style(content_style()),
        headers,
    );
    frame.render_widget(
        Paragraph::new(selected_email.body).style(content_style()),
        body,
    );
}

struct Hop {
    host: &'static str,
    address: &'static str,
    location: (f64, f64),
}

const HOPS: [Hop; 29] = [
    Hop::new("home", "127.0.0.1", (149.1, -35.3)),
    Hop::new("bad.horse", "162.252.205.130", (151.1, -33.9)),
    Hop::new("bad.horse", "162.252.205.131", (144.9, -37.8)),
    Hop::new("bad.horse", "162.252.205.132", (153.0, -27.5)),
    Hop::new("bad.horse", "162.252.205.133", (151.1, -33.9)),
    Hop::new(
        "he.rides.across.the.nation",
        "162.252.205.134",
        (115.9, -31.9),
    ),
    Hop::new("the.thoroughbred.of.sin", "162.252.205.135", (130.8, -12.4)),
    Hop::new("he.got.the.application", "162.252.205.136", (153.0, -27.5)),
    Hop::new("that.you.just.sent.in", "162.252.205.137", (138.6, -34.9)),
    Hop::new("it.needs.evaluation", "162.252.205.138", (130.8, -12.4)),
    Hop::new("so.let.the.games.begin", "162.252.205.139", (115.9, -31.9)),
    Hop::new("a.heinous.crime", "162.252.205.140", (153.0, -27.5)),
    Hop::new("a.show.of.force", "162.252.205.141", (149.1, -35.3)),
    Hop::new(
        "a.murder.would.be.nice.of.course",
        "162.252.205.142",
        (115.9, -31.9),
    ),
    Hop::new("bad.horse", "162.252.205.143", (144.9, -37.8)),
    Hop::new("bad.horse", "162.252.205.144", (130.8, -12.4)),
    Hop::new("bad.horse", "162.252.205.145", (144.9, -37.8)),
    Hop::new("he-s.bad", "162.252.205.146", (115.9, -31.9)),
    Hop::new("the.evil.league.of.evil", "162.252.205.147", (153.0, -27.5)),
    Hop::new("is.watching.so.beware", "162.252.205.148", (130.8, -12.4)),
    Hop::new(
        "the.grade.that.you.receive",
        "162.252.205.149",
        (115.9, -31.9),
    ),
    Hop::new(
        "will.be.your.last.we.swear",
        "162.252.205.150",
        (138.6, -34.9),
    ),
    Hop::new(
        "so.make.the.bad.horse.gleeful",
        "162.252.205.151",
        (151.1, -33.9),
    ),
    Hop::new(
        "or.he-ll.make.you.his.mare",
        "162.252.205.152",
        (144.9, -37.8),
    ),
    Hop::new("o_o", "162.252.205.153", (153.0, -27.5)),
    Hop::new("you-re.saddled.up", "162.252.205.154", (130.8, -12.4)),
    Hop::new("there-s.no.recourse", "162.252.205.155", (115.9, -31.9)),
    Hop::new("it-s.hi-ho.silver", "162.252.205.156", (151.1, -33.9)),
    Hop::new("signed.bad.horse", "162.252.205.157", (149.1, -35.3)),
];

impl Hop {
    const fn new(host: &'static str, address: &'static str, location: (f64, f64)) -> Self {
        Self {
            host,
            address,
            location,
        }
    }
}

fn render_traceroute(frame: &mut Frame<'_>, area: Rect, selected: usize) {
    render_rgb_swatch(frame, area);
    let area = area.inner(Margin::new(2, 1));
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(content_style()), area);
    let [left, map] =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(area);
    let [hops, ping] = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(left);

    let rows = HOPS.iter().map(|hop| Row::new([hop.host, hop.address]));
    let mut table_state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Min(16), Constraint::Length(15)])
            .header(
                Row::new(["Host", "Address"])
                    .style(Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
            )
            .row_highlight_style(Style::new().fg(LIGHT_YELLOW))
            .block(
                Block::new()
                    .padding(Padding::new(1, 1, 1, 1))
                    .title_alignment(Alignment::Center)
                    .title(Span::styled(
                        "Traceroute bad.horse",
                        Style::new().fg(WHITE).add_modifier(Modifier::BOLD),
                    )),
            ),
        hops,
        &mut table_state,
    );
    let mut scrollbar = ScrollbarState::new(HOPS.len()).position(selected);
    let scrollbar_area = Rect {
        width: hops.width.saturating_add(1),
        y: hops.y.saturating_add(3),
        height: hops.height.saturating_sub(4),
        ..hops
    };
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalLeft)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("▌"),
        scrollbar_area,
        &mut scrollbar,
    );

    let mut data = [
        8, 8, 8, 8, 7, 7, 7, 6, 6, 5, 4, 3, 3, 2, 2, 1, 1, 1, 2, 2, 3, 4, 5, 6, 7, 7, 8, 8, 8, 7,
        7, 6, 5, 4, 3, 2, 1, 1, 1, 1, 1, 2, 4, 6, 7, 8, 8, 8, 8, 6, 4, 2, 1, 1, 1, 1, 2, 2, 2, 3,
        3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7,
    ];
    let data_len = data.len();
    data.rotate_left(selected % data_len);
    frame.render_widget(
        Sparkline::default()
            .block(
                Block::new()
                    .title("Ping")
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Thick),
            )
            .data(data)
            .style(Style::new().fg(WHITE)),
        ping,
    );

    let path = HOPS.get(selected).zip(HOPS.get(selected + 1));
    frame.render_widget(
        Canvas::default()
            .background_color(DARK_BLUE)
            .block(
                Block::new()
                    .padding(Padding::new(1, 0, 1, 0))
                    .style(root_style()),
            )
            .marker(Marker::HalfBlock)
            .x_bounds([112.0, 155.0])
            .y_bounds([-46.0, -11.0])
            .paint(|context| {
                context.draw(&Map {
                    resolution: MapResolution::High,
                    color: LIGHT_GRAY,
                });
                if let Some((source, destination)) = path {
                    context.draw(&CanvasLine::new(
                        source.location.0,
                        source.location.1,
                        destination.location.0,
                        destination.location.1,
                        LIGHT_BLUE,
                    ));
                    context.draw(&Points {
                        coords: &[source.location],
                        color: LIGHT_GREEN,
                    });
                    context.draw(&Points {
                        coords: &[destination.location],
                        color: LIGHT_RED,
                    });
                }
            }),
        map,
    );
}

fn render_weather(frame: &mut Frame<'_>, area: Rect, progress: usize) {
    render_rgb_swatch(frame, area);
    let area = area.inner(Margin::new(2, 1));
    frame.render_widget(Clear, area);
    frame.render_widget(Block::new().style(content_style()), area);
    let area = area.inner(Margin::new(2, 1));
    let [main, _, gauge] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let [calendar, charts] =
        Layout::horizontal([Constraint::Length(23), Constraint::Min(0)]).areas(main);
    let [daily, seasonal] =
        Layout::vertical([Constraint::Percentage(58), Constraint::Min(0)]).areas(charts);

    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "      August 2026",
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                " Su Mo Tu We Th Fr Sa",
                Style::new().add_modifier(Modifier::ITALIC),
            ),
            Line::raw("                   1"),
            Line::raw("  2  3  4  5  6  7  8"),
            Line::raw("  9 10 11 12 13 14 15"),
            Line::styled(
                " 16 17 18 19 20 21 22",
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Line::raw(" 23 24 25 26 27 28 29"),
            Line::raw(" 30 31"),
        ])
        .block(Block::new().padding(Padding::new(0, 0, 2, 0)))
        .style(content_style()),
        calendar,
    );

    let daily_bars = [
        ("Sat", 76),
        ("Sun", 69),
        ("Mon", 65),
        ("Tue", 67),
        ("Wed", 65),
        ("Thu", 69),
        ("Fri", 73),
    ]
    .map(|(label, value)| {
        let hot = value > 70;
        Bar::default()
            .value(value)
            .text_value(format!("{value}°"))
            .style(Style::new().fg(if hot { Color::Red } else { Color::Yellow }))
            .value_style(
                Style::new()
                    .fg(if hot { Color::Gray } else { Color::DarkGray })
                    .bg(if hot { Color::Red } else { Color::Yellow })
                    .add_modifier(Modifier::BOLD),
            )
            .label(label)
    });
    frame.render_widget(
        BarChart::default()
            .data(BarGroup::default().bars(&daily_bars))
            .bar_width(3)
            .bar_gap(1),
        daily,
    );
    let seasonal_bars = [
        Bar::default().text_value("Winter 37-51").value(51),
        Bar::default().text_value("Spring 40-65").value(65),
        Bar::default().text_value("Summer 54-77").value(77),
        Bar::default()
            .text_value("Fall 41-71")
            .value(71)
            .value_style(Style::new().add_modifier(Modifier::BOLD)),
    ];
    let blue = Color::Rgb(32, 48, 96);
    frame.render_widget(
        BarChart::default()
            .block(Block::new().padding(Padding::new(0, 0, 2, 0)))
            .direction(Direction::Horizontal)
            .data(BarGroup::default().label("GPU").bars(&seasonal_bars))
            .bar_gap(1)
            .bar_style(Style::new().fg(blue))
            .value_style(Style::new().bg(blue).fg(Color::Gray)),
        seasonal,
    );

    let percent = (progress.saturating_mul(3)).min(100) as u16;
    let red = (165_u16 + percent * 9 / 10).min(255) as u8;
    let green = 220_u16.saturating_sub(percent * 7 / 5) as u8;
    frame.render_widget(
        LineGauge::default()
            .ratio(f64::from(percent) / 100.0)
            .label(if percent < 100 {
                format!("Downloading: {percent}%")
            } else {
                "Download Complete!".to_owned()
            })
            .style(Style::new().fg(Color::LightBlue))
            .filled_style(Style::new().fg(Color::Rgb(red, green, 0)))
            .unfilled_style(Style::new().fg(Color::Rgb(red / 2, green / 2, 0)))
            .filled_symbol(symbols::line::THICK_HORIZONTAL)
            .unfilled_symbol(symbols::line::THICK_HORIZONTAL),
        gauge,
    );
}

fn render_rgb_swatch(frame: &mut Frame<'_>, area: Rect) {
    let height = f32::from(area.height.max(1));
    let width = f32::from(area.width.max(1));
    let buffer = frame.buffer_mut();
    for (row_index, y) in (area.top()..area.bottom()).enumerate() {
        let value = (height - row_index as f32) / height;
        let background_value = (value - 0.5 / height).max(0.0);
        for (column_index, x) in (area.left()..area.right()).enumerate() {
            let hue = column_index as f32 * 360.0 / width;
            buffer[(x, y)]
                .set_char('▀')
                .set_fg(hsv_color(hue, value))
                .set_bg(hsv_color(hue, background_value));
        }
    }
}

fn hsv_color(hue: f32, value: f32) -> Color {
    let chroma = value * 0.88;
    let sector = hue / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let minimum = value - chroma;
    Color::Rgb(
        ((red + minimum) * 255.0) as u8,
        ((green + minimum) * 255.0) as u8,
        ((blue + minimum) * 255.0) as u8,
    )
}

fn render_destroy(frame: &mut Frame<'_>, area: Rect, tick: u64) {
    const DELAY: u64 = 8;
    const TEXT_DELAY: u64 = 18;
    let frame_count = tick.saturating_sub(DELAY);
    if frame_count == 0 || area.width == 0 || area.height < 3 {
        return;
    }

    let operations = frame_count
        .saturating_mul(frame_count)
        .saturating_mul(4)
        .min(50_000);
    {
        let mut random = 10_u64;
        let buffer = frame.buffer_mut();
        for _ in 0..operations {
            let source_x = area.left() + (next_random(&mut random) % u64::from(area.width)) as u16;
            let source_y =
                area.top() + 1 + (next_random(&mut random) % u64::from(area.height - 2)) as u16;
            let source = buffer[(source_x, source_y)].clone();
            if next_random(&mut random).is_multiple_of(100) {
                let spread = (next_random(&mut random) % 11) as i16 - 5;
                let destination_x = (source_x as i16 + spread)
                    .clamp(area.left() as i16, area.right().saturating_sub(1) as i16)
                    as u16;
                let destination = &mut buffer[(destination_x, area.top() + 1)];
                if next_random(&mut random).is_multiple_of(10) {
                    *destination = source;
                } else {
                    destination.reset();
                }
            } else {
                let destination_y = source_y.saturating_add(1).min(area.bottom() - 2);
                buffer[(source_x, destination_y)] = source;
            }
        }
    }

    let text_frame = tick.saturating_sub(TEXT_DELAY);
    if text_frame == 0 {
        return;
    }
    let intensity = (text_frame.saturating_mul(255) / 48).min(255) as u8;
    let logo = "██████      ████    ██████    ████    ██████  ██    ██  ██\n\
                ██    ██  ██    ██    ██    ██    ██    ██    ██  ██\n\
                ██████    ████████    ██    ████████    ██    ██    ██  ██\n\
                ██  ██    ██    ██    ██    ██    ██    ██    ██    ██  ██\n\
                ██    ██  ██    ██    ██    ██    ██    ██      ████    ██";
    frame.render_widget(
        Paragraph::new(logo)
            .style(Style::new().fg(Color::Rgb(intensity, 0, 0)))
            .alignment(Alignment::Center),
        centered(area, 72, 5),
    );
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn root_style() -> Style {
    Style::new().bg(DARK_BLUE)
}

fn content_style() -> Style {
    Style::new().fg(LIGHT_GRAY).bg(DARK_BLUE)
}
