//! Application-owned 3D presentation: bind the terminal image to an ordinary
//! material and adjust a unit rectangle to the measured aspect ratio.
//! Run with `cargo run --example world_quad`; press Space to resize the grid.

#[path = "common/mod.rs"]
mod common;

use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;

#[derive(Resource)]
struct Screen {
    terminal: RatatuiTerminal,
    wide: bool,
}

fn main() {
    let mut terminal = RatatuiTerminal::new(60, 18);
    terminal.draw(common::draw_demo_frame);
    let (terminal, renderer) = terminal.with_renderer();
    let mut app = App::new();
    app.add_plugins((DefaultPlugins, TerminalPlugin));
    let fonts = common::fonts::load(&mut app);
    app.insert_resource(Screen {
        terminal,
        wide: false,
    })
    .add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>| {
            commands.spawn((
                renderer.clone(),
                fonts.configure(default()),
                Mesh3d(meshes.add(Rectangle::default())),
                MeshMaterial3d(materials.add(StandardMaterial {
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })),
                Transform::from_xyz(0.0, 1.5, 0.0).with_rotation(Quat::from_rotation_y(-0.35)),
            ));
            commands.spawn((
                Camera3d::default(),
                Transform::from_xyz(-1.5, 2.0, 6.0).looking_at(Vec3::new(0.0, 1.5, 0.0), Vec3::Y),
            ));
        },
    )
    .add_systems(Update, draw.before(TerminalSystems::Sync))
    .add_systems(Update, present.after(TerminalSystems::Sync))
    .run();
}

fn draw(keys: Res<ButtonInput<KeyCode>>, mut screen: ResMut<Screen>) {
    if keys.just_pressed(KeyCode::Space) {
        screen.wide = !screen.wide;
        let columns = if screen.wide { 90 } else { 60 };
        screen.terminal.resize_grid(columns, 18);
    }
    common::draw_demo(&mut screen.terminal);
}

fn present(
    mut screens: Query<(
        &TerminalTexture,
        &MeshMaterial3d<StandardMaterial>,
        &mut Transform,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (texture, material, mut transform) in &mut screens {
        let Some(geometry) = texture.measured() else {
            continue;
        };
        let size = geometry.logical_size();
        let scale = Vec3::new(3.0 * size.x / size.y, 3.0, 1.0);
        if transform.scale != scale {
            transform.scale = scale;
        }
        let Some(existing) = materials.get(&material.0) else {
            continue;
        };
        if existing.base_color_texture.as_ref() != Some(&texture.image) {
            materials
                .get_mut(&material.0)
                .expect("material exists")
                .base_color_texture = Some(texture.image.clone());
        }
    }
}
