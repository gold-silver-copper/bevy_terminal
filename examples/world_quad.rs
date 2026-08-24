//! Presents a live Ratatui terminal on a world-space quad in a 3D scene.
//!
//! `TerminalWorldQuad` builds the mesh and material for you and keeps the
//! quad's aspect ratio in step with the measured texture, including when the
//! grid is resized (press `Space`). The `TerminalRemeasured` observer shows
//! the hook custom presentation (an imported screen mesh, say) would use.
//!
//! Requires the `3d` feature: `cargo run --example world_quad --features 3d`.

#[path = "common/mod.rs"]
mod common;

use bevy::prelude::*;
use bevy_terminal_ratatui::RatatuiTerminal;
use bevy_terminal_ratatui::prelude::{
    TerminalPlugin, TerminalReady, TerminalRemeasured, TerminalTexture, TerminalWorldQuad,
};

#[derive(Resource)]
struct Screen {
    terminal: RatatuiTerminal,
    wide: bool,
}

fn main() {
    // The first presented frame already shows the demo scene.
    let (terminal, renderer) = RatatuiTerminal::drawn(60, 18, common::draw_demo_frame);
    let mut app = App::new();
    app.add_plugins((DefaultPlugins, TerminalPlugin));
    let fonts = common::fonts::load(&mut app);
    app.insert_resource(Screen {
        terminal,
        wide: false,
    })
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn((
            renderer.clone(),
            fonts.configure(default()),
            TerminalWorldQuad::new(3.0),
            Transform::from_xyz(0.0, 1.5, 0.0).with_rotation(Quat::from_rotation_y(-0.35)),
        ));
        commands.spawn((
            Camera3d::default(),
            Transform::from_xyz(-1.5, 2.0, 6.0).looking_at(Vec3::new(0.0, 1.5, 0.0), Vec3::Y),
        ));
    })
    .add_observer(
        |ready: On<TerminalReady>, textures: Query<&TerminalTexture>| {
            let texture = textures.get(ready.entity).unwrap();
            info!("terminal ready: {}x{} px", texture.size.x, texture.size.y);
        },
    )
    .add_observer(|event: On<TerminalRemeasured>| {
        info!(
            "terminal re-measured: {} -> {} px",
            event.previous_size, event.size
        );
    })
    .add_systems(Update, (draw, toggle_size))
    .run();
}

fn draw(mut screen: ResMut<Screen>) {
    common::draw_demo(&mut screen.terminal);
}

fn toggle_size(keys: Res<ButtonInput<KeyCode>>, mut screen: ResMut<Screen>) {
    if keys.just_pressed(KeyCode::Space) {
        screen.wide = !screen.wide;
        let columns = if screen.wide { 90 } else { 60 };
        screen.terminal.resize_grid(columns, 18);
    }
}
