//! Application-owned 3D presentation: bind the terminal image to an ordinary
//! material and adjust a unit rectangle to the measured aspect ratio.
//! Run with `cargo run --example world_quad`; press Space to resize the grid.

#[path = "common/mod.rs"]
mod common;

use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins((DefaultPlugins, RatatuiPlugin));
    let fonts = common::fonts::load(&mut app);
    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>| {
            commands.spawn((
                RatatuiTerminal::new(60, 18),
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

fn draw(
    keys: Res<ButtonInput<KeyCode>>,
    mut terminal: Single<&mut RatatuiTerminal>,
    mut wide: Local<bool>,
) {
    if keys.just_pressed(KeyCode::Space) {
        *wide = !*wide;
        let columns = if *wide { 90 } else { 60 };
        terminal.resize_grid(columns, 18);
    }
    common::draw_demo(&mut terminal);
}

fn present(
    mut screens: Query<
        (
            &TerminalTexture,
            &MeshMaterial3d<StandardMaterial>,
            &mut Transform,
        ),
        Changed<TerminalTexture>,
    >,
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
        if existing.base_color_texture.as_ref() != Some(texture.image()) {
            materials
                .get_mut(&material.0)
                .expect("material exists")
                .base_color_texture = Some(texture.image().clone());
        }
    }
}
