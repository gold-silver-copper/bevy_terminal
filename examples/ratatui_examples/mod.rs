//! Deterministic Bevy ports of the runnable Ratatui 0.30.2 examples.
//!
//! The source inventory is pinned to Ratatui commit
//! `e665c36cb14752a61cd777fbd06dbef8474f2add`. Interactive input, randomness,
//! network requests, and terminal escape handling are replaced with fixed
//! fixtures so every scene can be rendered and compared reproducibly.

#![allow(dead_code)]

mod apps;
mod state;
mod support;

use bevy_grid::{BevyBackend, TerminalSurface};
use ratatui::{Frame, Terminal, backend::Backend};

pub const COLUMNS: u16 = 100;
pub const ROWS: u16 = 62;

#[derive(Clone, Copy)]
pub struct ExampleSpec {
    pub slug: &'static str,
    pub source: &'static str,
    pub adaptation: &'static str,
    render: fn(&mut Frame<'_>),
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
        "The app is frozen on a populated inbox view."
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

pub fn draw_surface(spec: &ExampleSpec) -> TerminalSurface {
    let backend = BevyBackend::new(COLUMNS, ROWS);
    let surface = backend.surface();
    let mut terminal = Terminal::new(backend).expect("the in-memory backend is infallible");
    terminal
        .draw(|frame| (spec.render)(frame))
        .expect("the in-memory backend is infallible");
    surface
}

pub fn redraw_surface(surface: &TerminalSurface, spec: &ExampleSpec) {
    let mut backend = BevyBackend::from_surface(surface.clone());
    backend
        .clear()
        .expect("the in-memory backend is infallible");
    backend.resize(COLUMNS, ROWS);
    let mut terminal = Terminal::new(backend).expect("the in-memory backend is infallible");
    terminal
        .draw(|frame| (spec.render)(frame))
        .expect("the in-memory backend is infallible");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

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
            let snapshot = draw_surface(example).snapshot();
            assert!(
                snapshot
                    .buffer()
                    .content
                    .iter()
                    .any(|cell| cell.symbol() != " "),
                "{} rendered an empty scene",
                example.slug
            );
        }
    }
}
