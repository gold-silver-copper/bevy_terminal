//! Interactive Bevy gallery for every runnable Ratatui example port.
//!
//! Run the full gallery with `cargo run --example ratatui_examples`. An
//! optional slug chooses the starting example.

#[path = "ratatui_examples/mod.rs"]
mod catalog;
#[path = "common/fonts.rs"]
mod fonts;

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseWheel,
    },
    prelude::*,
    window::{CursorMoved, PrimaryWindow, WindowResizeConstraints, WindowResolution},
};
use bevy_terminal_ratatui::{
    Presentation, RatatuiBackend, RatatuiTerminalExt, Terminal as TerminalEntity, TerminalPlugin,
    TerminalRenderConfig, TerminalSurface, TerminalSystems,
};
use ratatui::{Terminal, layout::Size};

const CELL_WIDTH: f32 = 10.0;
const CELL_HEIGHT: f32 = 18.0;
const MARGIN: f32 = 20.0;
const MIN_COLUMNS: u16 = 64;
const MIN_ROWS: u16 = 24;

fn main() {
    let Some(start_index) = selected_example_index() else {
        return;
    };
    let gallery = Gallery::new(start_index);
    let surface = gallery.surface();
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(CELL_WIDTH, CELL_HEIGHT),
        ..default()
    };
    let width = f32::from(catalog::COLUMNS).mul_add(CELL_WIDTH, MARGIN * 2.0);
    let height = f32::from(catalog::ROWS).mul_add(CELL_HEIGHT, MARGIN * 2.0);

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: window_title(start_index),
            resolution: WindowResolution::new(width as u32, height as u32),
            resizable: true,
            resize_constraints: WindowResizeConstraints {
                min_width: window_width(MIN_COLUMNS),
                min_height: window_height(MIN_ROWS),
                ..default()
            },
            ..default()
        }),
        ..default()
    }));
    let fonts = fonts::load(&mut app);
    let config = fonts.configure(config);
    app.add_plugins(TerminalPlugin)
        .insert_resource(gallery)
        .insert_resource(AnimationClock(Timer::from_seconds(
            0.1,
            TimerMode::Repeating,
        )))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(
                TerminalEntity::new(surface.clone())
                    .with_config(config.clone())
                    .with_presentation(Presentation::Ui {
                        origin: Vec2::splat(MARGIN),
                    }),
            );
        })
        .add_systems(
            Update,
            (
                resize_gallery_to_window,
                keyboard_input,
                pointer_input,
                animate_current,
            )
                .chain()
                .before(TerminalSystems::Sync),
        )
        .run();
}

#[derive(Resource)]
struct Gallery {
    index: usize,
    states: Vec<catalog::ExampleState>,
    terminal: Terminal<RatatuiBackend>,
}

impl Gallery {
    fn new(index: usize) -> Self {
        let backend = RatatuiBackend::new(catalog::COLUMNS, catalog::ROWS);
        let terminal = Terminal::new(backend).expect("the in-memory backend is infallible");
        let mut gallery = Self {
            index,
            states: catalog::EXAMPLES
                .iter()
                .map(|spec| catalog::ExampleState::new(spec.slug))
                .collect(),
            terminal,
        };
        gallery.redraw();
        gallery
    }

    fn state_mut(&mut self) -> &mut catalog::ExampleState {
        &mut self.states[self.index]
    }

    fn surface(&self) -> TerminalSurface {
        self.terminal.backend().surface()
    }

    fn size(&self) -> Size {
        self.terminal
            .size()
            .expect("the in-memory backend is infallible")
    }

    fn resize(&mut self, size: Size) -> bool {
        if self.size() == size {
            return false;
        }
        self.terminal.resize_grid(size.width, size.height);
        self.redraw();
        true
    }

    fn redraw(&mut self) {
        let index = self.index;
        catalog::redraw_interactive_terminal(
            &mut self.terminal,
            &catalog::EXAMPLES[index],
            &self.states[index],
        );
    }

    fn next(&mut self) {
        self.state_mut().help_visible = false;
        self.index = (self.index + 1) % catalog::EXAMPLES.len();
    }

    fn previous(&mut self) {
        self.state_mut().help_visible = false;
        self.index = (self.index + catalog::EXAMPLES.len() - 1) % catalog::EXAMPLES.len();
    }
}

#[derive(Resource)]
struct AnimationClock(Timer);

fn resize_gallery_to_window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut gallery: ResMut<Gallery>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    gallery.resize(terminal_grid_size(Vec2::new(
        window.width(),
        window.height(),
    )));
}

fn keyboard_input(
    mut messages: MessageReader<KeyboardInput>,
    physical_keys: Res<ButtonInput<KeyCode>>,
    mut gallery: ResMut<Gallery>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    for message in messages.read() {
        if message.state != ButtonState::Pressed {
            continue;
        }
        let shift = physical_keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let control = physical_keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let mut redraw = false;
        match &message.logical_key {
            Key::PageDown => {
                gallery.next();
                redraw = true;
            }
            Key::PageUp => {
                gallery.previous();
                redraw = true;
            }
            Key::F6 if shift => {
                gallery.previous();
                redraw = true;
            }
            Key::F6 => {
                gallery.next();
                redraw = true;
            }
            Key::F1 => {
                let state = gallery.state_mut();
                state.help_visible = !state.help_visible;
                redraw = true;
            }
            Key::F2 => {
                gallery.state_mut().reset();
                redraw = true;
            }
            Key::F10 => {
                exit.write(AppExit::Success);
                continue;
            }
            _ => {
                let modifiers = catalog::KeyModifiers { control, shift };
                for key in example_keys(message, shift) {
                    let outcome = gallery.state_mut().handle_key(key, modifiers);
                    redraw |= outcome.redraw;
                    if outcome.quit {
                        exit.write(AppExit::Success);
                    }
                }
            }
        }
        if redraw {
            gallery.redraw();
            update_window_title(&mut windows, gallery.index);
        }
    }
}

fn example_keys(message: &KeyboardInput, shift: bool) -> Vec<catalog::ExampleKey> {
    let special = match message.logical_key {
        Key::ArrowUp => Some(catalog::ExampleKey::Up),
        Key::ArrowDown => Some(catalog::ExampleKey::Down),
        Key::ArrowLeft => Some(catalog::ExampleKey::Left),
        Key::ArrowRight => Some(catalog::ExampleKey::Right),
        Key::Home => Some(catalog::ExampleKey::Home),
        Key::End => Some(catalog::ExampleKey::End),
        Key::Enter => Some(catalog::ExampleKey::Enter),
        Key::Escape => Some(catalog::ExampleKey::Escape),
        Key::Tab if shift => Some(catalog::ExampleKey::BackTab),
        Key::Tab => Some(catalog::ExampleKey::Tab),
        Key::Backspace => Some(catalog::ExampleKey::Backspace),
        Key::Delete => Some(catalog::ExampleKey::Delete),
        Key::Space => Some(catalog::ExampleKey::Char(' ')),
        _ => None,
    };
    if let Some(special) = special {
        return vec![special];
    }
    let Key::Character(value) = &message.logical_key else {
        return Vec::new();
    };
    value.chars().map(catalog::ExampleKey::Char).collect()
}

fn pointer_input(
    mut cursor_messages: MessageReader<CursorMoved>,
    mut wheel_messages: MessageReader<MouseWheel>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut gallery: ResMut<Gallery>,
) {
    let pressed = mouse_buttons.pressed(MouseButton::Left);
    let terminal_size = gallery.size();
    let mut redraw = false;
    if mouse_buttons.just_pressed(MouseButton::Left)
        && let Ok(window) = windows.single()
        && let Some(position) = window.cursor_position()
        && let Some((column, row)) = terminal_position(position, terminal_size)
    {
        redraw |= gallery
            .state_mut()
            .pointer(column, row, terminal_size, true, true);
    }
    for message in cursor_messages.read() {
        if let Some((column, row)) = terminal_position(message.position, terminal_size) {
            redraw |= gallery
                .state_mut()
                .pointer(column, row, terminal_size, pressed, false);
        }
    }
    for message in wheel_messages.read() {
        let lines = (-message.y.round() as i32).clamp(-8, 8);
        redraw |= gallery.state_mut().scroll(lines);
    }
    if redraw {
        gallery.redraw();
    }
}

fn animate_current(
    time: Res<Time>,
    mut clock: ResMut<AnimationClock>,
    mut gallery: ResMut<Gallery>,
) {
    if !clock.0.tick(time.delta()).just_finished() {
        return;
    }
    if gallery.state_mut().tick() {
        gallery.redraw();
    }
}

fn terminal_position(position: Vec2, terminal_size: Size) -> Option<(u16, u16)> {
    let x = position.x - MARGIN;
    let y = position.y - MARGIN;
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let column = (x / CELL_WIDTH).floor() as u16;
    let row = (y / CELL_HEIGHT).floor() as u16;
    (column < terminal_size.width && row < terminal_size.height).then_some((column, row))
}

fn terminal_grid_size(window_size: Vec2) -> Size {
    let columns = ((window_size.x - MARGIN * 2.0) / CELL_WIDTH)
        .floor()
        .clamp(f32::from(MIN_COLUMNS), f32::from(u16::MAX)) as u16;
    let rows = ((window_size.y - MARGIN * 2.0) / CELL_HEIGHT)
        .floor()
        .clamp(f32::from(MIN_ROWS), f32::from(u16::MAX)) as u16;
    Size::new(columns, rows)
}

fn window_width(columns: u16) -> f32 {
    f32::from(columns).mul_add(CELL_WIDTH, MARGIN * 2.0)
}

fn window_height(rows: u16) -> f32 {
    f32::from(rows).mul_add(CELL_HEIGHT, MARGIN * 2.0)
}

fn update_window_title(windows: &mut Query<&mut Window, With<PrimaryWindow>>, index: usize) {
    if let Ok(mut window) = windows.single_mut() {
        window.title = window_title(index);
    }
}

fn window_title(index: usize) -> String {
    format!(
        "bevy_terminal_ratatui · {}/{} · {} · PageUp/PageDown examples · F1 help",
        index + 1,
        catalog::EXAMPLES.len(),
        catalog::EXAMPLES[index].slug,
    )
}

fn selected_example_index() -> Option<usize> {
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
        "port: {}\nsource: {}\ndeterministic export: {}\ncontrols: {}\ngallery: PageUp/PageDown switch · F1 help · F2 reset · F10 quit",
        spec.slug,
        spec.source,
        spec.adaptation,
        catalog::ExampleState::new(spec.slug).controls(),
    );
    Some(
        catalog::EXAMPLES
            .iter()
            .position(|candidate| candidate.slug == spec.slug)
            .expect("the selected spec belongs to the static catalog"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_navigation_wraps_and_preserves_state() {
        let mut gallery = Gallery::new(catalog::EXAMPLES.len() - 1);
        gallery.state_mut().tick = 99;
        gallery.next();
        assert_eq!(gallery.index, 0);
        gallery.previous();
        assert_eq!(gallery.index, catalog::EXAMPLES.len() - 1);
        assert_eq!(gallery.states[gallery.index].tick, 99);
    }

    #[test]
    fn live_redraw_submits_one_surface_revision_without_a_clear_frame() {
        let mut gallery = Gallery::new(0);
        let surface = gallery.surface();
        let before = surface.revision();
        assert!(gallery.state_mut().tick());
        gallery.redraw();
        assert_eq!(surface.revision(), before + 1);
        assert!(
            surface
                .snapshot()
                .cells()
                .iter()
                .any(|cell| cell.symbol() != " ")
        );
    }

    #[test]
    fn window_coordinates_map_to_terminal_cells() {
        let size = Size::new(catalog::COLUMNS, catalog::ROWS);
        assert_eq!(terminal_position(Vec2::splat(MARGIN), size), Some((0, 0)));
        assert_eq!(
            terminal_position(
                Vec2::new(MARGIN + CELL_WIDTH * 99.5, MARGIN + CELL_HEIGHT * 61.5),
                size
            ),
            Some((99, 61))
        );
        assert_eq!(terminal_position(Vec2::ZERO, size), None);
        assert_eq!(
            terminal_position(Vec2::new(MARGIN + CELL_WIDTH * 100.5, MARGIN), size),
            None
        );
    }

    #[test]
    fn window_dimensions_map_to_a_bounded_terminal_grid() {
        assert_eq!(
            terminal_grid_size(Vec2::new(
                window_width(catalog::COLUMNS),
                window_height(catalog::ROWS),
            )),
            Size::new(catalog::COLUMNS, catalog::ROWS)
        );
        assert_eq!(terminal_grid_size(Vec2::ZERO), Size::new(64, 24));
        assert_eq!(
            terminal_grid_size(Vec2::new(window_width(123), window_height(71))),
            Size::new(123, 71)
        );
    }

    #[test]
    fn every_gallery_scene_survives_small_and_large_window_grids() {
        for index in 0..catalog::EXAMPLES.len() {
            let mut gallery = Gallery::new(index);
            for size in [Size::new(MIN_COLUMNS, MIN_ROWS), Size::new(132, 76)] {
                assert!(gallery.resize(size));
                assert_eq!(gallery.size(), size);
                assert!(
                    gallery
                        .surface()
                        .snapshot()
                        .cells()
                        .iter()
                        .any(|cell| cell.symbol() != " "),
                    "{} rendered an empty {size:?} scene",
                    catalog::EXAMPLES[index].slug,
                );
            }
        }
    }

    #[test]
    fn mouse_drawing_renders_a_persistent_point_and_live_cursor() {
        let index = catalog::EXAMPLES
            .iter()
            .position(|example| example.slug == "mouse-drawing")
            .expect("the mouse drawing example is in the catalog");
        let mut gallery = Gallery::new(index);
        let size = gallery.size();

        assert!(gallery.state_mut().pointer(20, 12, size, true, true));
        assert!(gallery.state_mut().pointer(21, 12, size, false, false));
        gallery.redraw();

        let snapshot = gallery.surface().snapshot();
        assert_eq!(snapshot[(20, 12)].symbol(), ratatui::symbols::block::FULL);
        assert_eq!(snapshot[(21, 12)].symbol(), "╳");
    }
}
