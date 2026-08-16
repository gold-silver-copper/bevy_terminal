//! Opens one deterministic port from Ratatui's upstream example catalog.
//!
//! Run with a slug, for example: cargo run --example ratatui_examples -- chart

#[path = "ratatui_examples/mod.rs"]
mod catalog;

use bevy::{prelude::*, window::WindowResolution};
use bevy_grid::{BevyGridPlugin, TerminalRenderConfig};

const CELL_WIDTH: f32 = 9.6;
const CELL_HEIGHT: f32 = 18.0;
const MARGIN: f32 = 20.0;

fn main() {
    let Some(spec) = selected_example() else {
        return;
    };
    let surface = catalog::draw_surface(spec);
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(CELL_WIDTH, CELL_HEIGHT),
        font_size: 16.0,
        origin: Vec2::splat(MARGIN),
        ..default()
    };
    let width = f32::from(catalog::COLUMNS).mul_add(CELL_WIDTH, MARGIN * 2.0);
    let height = f32::from(catalog::ROWS).mul_add(CELL_HEIGHT, MARGIN * 2.0);

    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("bevy_grid · Ratatui example · {}", spec.slug),
                    resolution: WindowResolution::new(width as u32, height as u32),
                    resizable: false,
                    ..default()
                }),
                ..default()
            }),
            BevyGridPlugin::new(surface).with_config(config),
        ))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        })
        .run();
}

fn selected_example() -> Option<&'static catalog::ExampleSpec> {
    let argument = std::env::args().nth(1);
    if argument.as_deref() == Some("--list") {
        for spec in catalog::EXAMPLES {
            println!("{}", spec.slug);
        }
        return None;
    }
    let slug = argument.as_deref().unwrap_or(catalog::EXAMPLES[0].slug);
    let Some(spec) = catalog::find(slug) else {
        eprintln!("unknown Ratatui example: {slug}");
        eprintln!(
            "run with --list to see all {} ports",
            catalog::EXAMPLES.len()
        );
        return None;
    };
    println!(
        "port: {}\nsource: {}\nadaptation: {}",
        spec.slug, spec.source, spec.adaptation
    );
    Some(spec)
}
