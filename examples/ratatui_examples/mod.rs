//! Interactive and deterministic Bevy ports of the runnable Ratatui examples.
//!
//! The source inventory is pinned to Ratatui commit
//! `e665c36cb14752a61cd777fbd06dbef8474f2add`. The gallery drives mutable state
//! with Bevy input, while the exporter creates canonical state and fixed local
//! data so every scene can still be compared reproducibly.

#![allow(dead_code)]

mod apps;
mod interaction;
mod state;
mod support;

use bevy_terminal_ratatui::RatatuiTerminal;
use ratatui::Frame;

pub const COLUMNS: u16 = 100;
pub const ROWS: u16 = 62;

#[allow(unused_imports)]
pub use interaction::{ExampleKey, ExampleState, KeyModifiers};

#[derive(Clone, Copy)]
pub struct ExampleSpec {
    pub slug: &'static str,
    pub source: &'static str,
    pub adaptation: &'static str,
    render: fn(&mut Frame<'_>, &ExampleState),
}

macro_rules! app {
    ($slug:literal, $function:ident, $adaptation:literal) => {
        ExampleSpec {
            slug: $slug,
            source: concat!("examples/apps/", $slug),
            adaptation: $adaptation,
            render: apps::$function,
        }
    };
}

macro_rules! state {
    ($slug:literal, $function:ident, $adaptation:literal) => {
        ExampleSpec {
            slug: concat!("state-", $slug),
            source: concat!("examples/concepts/state/src/bin/", $slug, ".rs"),
            adaptation: $adaptation,
            render: state::$function,
        }
    };
}

pub const EXAMPLES: &[ExampleSpec] = &[
    app!(
        "advanced-widget-impl",
        advanced_widget_impl,
        "Timers and interaction use a fixed representative state."
    ),
    app!(
        "async-github",
        async_github,
        "The GitHub API response is replaced by deterministic pull-request fixtures."
    ),
    app!(
        "calendar-explorer",
        calendar_explorer,
        "The selected date is fixed and the calendar is drawn without the external time crate."
    ),
    app!(
        "canvas",
        canvas,
        "Animation is frozen at a representative frame."
    ),
    app!(
        "chart",
        chart,
        "Animation is frozen at a representative frame."
    ),
    app!(
        "color-explorer",
        color_explorer,
        "The complete named and indexed palettes are rendered in a static layout."
    ),
    app!(
        "colors-rgb",
        colors_rgb,
        "The true-color animation is frozen into deterministic gradients."
    ),
    app!(
        "constraint-explorer",
        constraint_explorer,
        "Keyboard-selected constraints and flex modes are shown together."
    ),
    app!(
        "constraints",
        constraints,
        "All constraint variants are shown together instead of behind tabs."
    ),
    app!(
        "custom-widget",
        custom_widget,
        "Normal, selected, and pressed button states are shown together."
    ),
    app!(
        "demo",
        demo,
        "Animation and selection use fixed representative values."
    ),
    app!(
        "demo2",
        demo2,
        "The five tabs use deterministic local fixtures; destroy mode uses a deterministic Bevy-driven animation."
    ),
    app!(
        "flex",
        flex,
        "All flex modes are shown together instead of behind tabs."
    ),
    app!(
        "gauge",
        gauge,
        "Gauge progress is frozen at representative values."
    ),
    app!("hello-world", hello_world, "Input handling is omitted."),
    app!(
        "hyperlink",
        hyperlink,
        "Terminal OSC-8 activation is represented by its underlined visual state."
    ),
    app!(
        "inline",
        inline,
        "Download progress and inline terminal history use deterministic fixtures."
    ),
    app!(
        "input-form",
        input_form,
        "The form is frozen with populated fields and a validation message."
    ),
    app!(
        "minimal",
        minimal,
        "Terminal setup and input handling are omitted."
    ),
    app!(
        "modifiers",
        modifiers,
        "Blink phases are captured deterministically."
    ),
    app!(
        "mouse-drawing",
        mouse_drawing,
        "Mouse events are replaced by a deterministic drawn path."
    ),
    app!(
        "panic",
        panic,
        "The panic-hook state is represented as a static report."
    ),
    app!(
        "popup",
        popup,
        "The popup is captured in its visible state."
    ),
    app!("release-header", release_header, "Menu state is fixed."),
    app!(
        "scrollbar",
        scrollbar,
        "Both scroll directions use fixed offsets."
    ),
    app!(
        "table",
        table,
        "Generated rows are replaced by deterministic fixtures."
    ),
    app!(
        "todo-list",
        todo_list,
        "The selected task and edit state are fixed."
    ),
    app!(
        "tracing",
        tracing,
        "Tracing subscribers are replaced by fixed log events."
    ),
    app!(
        "user-input",
        user_input,
        "The input buffer and cursor are fixed."
    ),
    app!(
        "volatility-surface",
        volatility_surface,
        "Random market inputs and animation are replaced by a deterministic surface."
    ),
    app!(
        "weather",
        weather,
        "Random temperatures are replaced by fixed values."
    ),
    app!(
        "widget-ref-container",
        widget_ref_container,
        "The immutable widget container is rendered directly."
    ),
    state!(
        "component-trait",
        component_trait,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "immutable-consuming",
        immutable_consuming,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "immutable-function",
        immutable_function,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "immutable-shared-ref",
        immutable_shared_ref,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "mutable-function",
        mutable_function,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "mutable-widget",
        mutable_widget,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "nested-mutable-widget",
        nested_mutable_widget,
        "The nested counters are frozen after representative events."
    ),
    state!(
        "nested-stateful-widget",
        nested_stateful_widget,
        "The nested counters are frozen after representative events."
    ),
    state!(
        "refcell",
        refcell,
        "The shared counter is frozen after representative events."
    ),
    state!(
        "stateful-widget",
        stateful_widget,
        "The counter is frozen after representative increment events."
    ),
    state!(
        "widget-with-mutable-ref",
        widget_with_mutable_ref,
        "The counter is frozen after representative increment events."
    ),
];

pub fn find(slug: &str) -> Option<&'static ExampleSpec> {
    EXAMPLES.iter().find(|example| example.slug == slug)
}

/// A terminal showing `spec` in its canonical state.
pub fn draw_terminal(spec: &ExampleSpec) -> RatatuiTerminal {
    let mut terminal = RatatuiTerminal::new(COLUMNS, ROWS);
    redraw_terminal(&mut terminal, spec);
    terminal
}

pub fn redraw_terminal(terminal: &mut RatatuiTerminal, spec: &ExampleSpec) {
    redraw_interactive_terminal(terminal, spec, &ExampleState::canonical(spec.slug));
}

pub fn redraw_interactive_terminal(
    terminal: &mut RatatuiTerminal,
    spec: &ExampleSpec,
    state: &ExampleState,
) {
    terminal.draw(|frame| {
        (spec.render)(frame, state);
        interaction::render_help(frame, spec, state);
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use bevy_terminal_ratatui::prelude::StyleFlags;

    use super::*;

    #[test]
    fn catalog_covers_every_upstream_runnable_target() {
        assert_eq!(EXAMPLES.len(), 43);
        assert_eq!(
            EXAMPLES
                .iter()
                .filter(|example| example.source.starts_with("examples/apps/"))
                .count(),
            32
        );
        assert_eq!(
            EXAMPLES
                .iter()
                .filter(|example| example.source.starts_with("examples/concepts/state/"))
                .count(),
            11
        );
        assert_eq!(
            EXAMPLES
                .iter()
                .map(|example| example.slug)
                .collect::<BTreeSet<_>>()
                .len(),
            EXAMPLES.len()
        );
    }

    #[test]
    fn every_port_renders_nonempty_terminal_content() {
        for example in EXAMPLES {
            let terminal = draw_terminal(example);
            assert!(
                terminal
                    .grid()
                    .cells()
                    .iter()
                    .any(|cell| cell.symbol() != " "),
                "{} rendered an empty scene",
                example.slug
            );
        }
    }

    #[test]
    fn every_interactive_control_changes_its_scene() {
        for example in EXAMPLES {
            let mut state = ExampleState::new(example.slug);
            let mut terminal = draw_terminal(example);
            redraw_interactive_terminal(&mut terminal, example, &state);
            let before = terminal.grid().clone();
            let exercised = match example.slug {
                "async-github" => press(&mut state, ExampleKey::Down),
                "calendar-explorer" => press(&mut state, ExampleKey::Right),
                "canvas" => press(&mut state, ExampleKey::Enter),
                "constraint-explorer" => press(&mut state, ExampleKey::Up),
                "constraints" => press(&mut state, ExampleKey::Right),
                "custom-widget" => press(&mut state, ExampleKey::Char(' ')),
                "demo" => press(&mut state, ExampleKey::Right),
                "demo2" => press(&mut state, ExampleKey::Right),
                "flex" => press(&mut state, ExampleKey::Right),
                "input-form" => press(&mut state, ExampleKey::Backspace),
                "mouse-drawing" => state.pointer(
                    50,
                    31,
                    ratatui::layout::Size::new(COLUMNS, ROWS),
                    true,
                    true,
                ),
                "panic" => press(&mut state, ExampleKey::Char('p')),
                "popup" => press(&mut state, ExampleKey::Char('p')),
                "scrollbar" => press(&mut state, ExampleKey::Down),
                "table" => press(&mut state, ExampleKey::Down),
                "todo-list" => press(&mut state, ExampleKey::Enter),
                "user-input" => press(&mut state, ExampleKey::Char('x')),
                "volatility-surface" => press(&mut state, ExampleKey::Right),
                "advanced-widget-impl"
                | "chart"
                | "colors-rgb"
                | "gauge"
                | "inline"
                | "modifiers"
                | "tracing" => state.tick(),
                slug if slug.starts_with("state-") => state.tick(),
                _ => false,
            };
            if !exercised {
                continue;
            }
            redraw_interactive_terminal(&mut terminal, example, &state);
            assert_ne!(
                before.cells(),
                terminal.grid().cells(),
                "{} accepted input but did not visibly redraw",
                example.slug
            );
        }
    }

    fn press(state: &mut ExampleState, key: ExampleKey) -> bool {
        state.handle_key(key, KeyModifiers::default()).redraw
    }

    #[test]
    fn contextual_help_renders_global_and_local_bindings() {
        let example = find("table").expect("table is in the catalog");
        let mut state = ExampleState::new(example.slug);
        state.help_visible = true;
        let mut terminal = draw_terminal(example);
        redraw_interactive_terminal(&mut terminal, example, &state);
        let symbols = terminal
            .grid()
            .cells()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(symbols.contains("PageDown"));
        assert!(symbols.contains("Shift+Left/Right"));
    }

    #[test]
    fn modifiers_example_contains_a_combined_bold_italic_face() {
        let example = find("modifiers").expect("modifiers is in the catalog");
        let terminal = draw_terminal(example);
        let combined = terminal
            .grid()
            .cells()
            .iter()
            .filter(|cell| cell.style.has(StyleFlags::BOLD | StyleFlags::ITALIC))
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert_eq!(combined, "Bold Italic");
    }

    #[test]
    fn demo2_renders_all_five_upstream_tabs_as_distinct_scenes() {
        let example = find("demo2").expect("demo2 is in the catalog");
        let landmarks = [
            "Ratatui is a Rust crate",
            "olive oil",
            "Alice <alice@example.com>",
            "Traceroute bad.horse",
            "August 2026",
        ];
        let mut rendered = BTreeSet::new();

        for (tab, landmark) in landmarks.into_iter().enumerate() {
            let mut state = ExampleState::new(example.slug);
            state.tab = tab;
            let mut terminal = draw_terminal(example);
            redraw_interactive_terminal(&mut terminal, example, &state);
            let text = terminal
                .grid()
                .cells()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                text.contains(landmark),
                "demo2 tab {tab} did not render {landmark:?}"
            );
            rendered.insert(text);
        }

        assert_eq!(rendered.len(), landmarks.len());
    }

    #[test]
    fn demo2_destroy_mode_replaces_the_scene_with_the_logo() {
        let example = find("demo2").expect("demo2 is in the catalog");
        let mut state = ExampleState::new(example.slug);
        state.toggled = true;
        state.tick = 30;
        let mut terminal = draw_terminal(example);
        redraw_interactive_terminal(&mut terminal, example, &state);
        let red_logo_cells = terminal
            .grid()
                .cells()
                .iter()
            .filter(|cell| {
                cell.symbol() == "█"
                    && matches!(cell.style.foreground, bevy_terminal_ratatui::prelude::TerminalColor::Rgb(red, 0, 0) if red > 0)
            })
            .count();

        assert!(red_logo_cells > 50);
    }
}
