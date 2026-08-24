//! Presents a terminal on a mesh you own — the integration style for an
//! imported model (a glTF CRT, a cockpit panel) where `TerminalWorldQuad`
//! does not apply because the geometry, UV mapping and material are yours.
//!
//! The three steps every imported-mesh integration shares:
//!
//! 1. Get hold of the mesh entity. Here the curved screen is generated in
//!    code with normalized UVs; a real project would spawn
//!    `SceneRoot(asset_server.load("crt.glb#Scene0"))` and find the screen by
//!    `Name` once the scene has loaded (see `claim_screen`).
//! 2. On `TerminalReady`, bind `TerminalTexture::image` in your own material —
//!    once, because the handle is stable for the terminal's lifetime. This
//!    example uses the terminal as both base color and emissive texture so the
//!    screen glows and can be "powered down" by fading the emissive tint.
//! 3. Observe `TerminalRemeasured` if anything you built depends on the
//!    texture size (a UV rescale, a bezel proportion). Here the mapping is
//!    normalized, so the observer only reports the change.
//!
//! Space toggles power; requires the `3d` feature:
//! `cargo run --example imported_screen --features 3d`.

#[path = "common/mod.rs"]
mod common;

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use bevy_terminal_ratatui::prelude::{
    RatatuiTerminal, TerminalPlugin, TerminalReady, TerminalRemeasured, TerminalTexture,
};

/// The entity that owns the screen mesh and its material.
#[derive(Component)]
struct Screen;

/// Power state, animated into the emissive tint.
#[derive(Resource)]
struct Power {
    on: bool,
    level: f32,
}

fn main() {
    let mut app = App::new();
    app.add_plugins((DefaultPlugins, TerminalPlugin));
    let fonts = common::fonts::load(&mut app);
    app.insert_resource(Power {
        on: true,
        level: 1.0,
    })
    .add_systems(
        Startup,
        move |mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>| {
            // Step 1: the screen mesh. Stands in for a mesh claimed from a loaded
            // scene; only the UV convention matters (0..1 across the screen face).
            let (terminal, renderer) = RatatuiTerminal::drawn(64, 20, common::draw_demo_frame);
            commands.spawn((
                Screen,
                terminal,
                renderer,
                fonts.configure(default()),
                Mesh3d(meshes.add(curved_screen(3.2, 2.4, 0.18))),
                Transform::from_xyz(0.0, 1.5, 0.0).with_rotation(Quat::from_rotation_y(-0.25)),
            ));
            commands.spawn((
                PointLight {
                    intensity: 400_000.0,
                    ..default()
                },
                Transform::from_xyz(3.0, 4.0, 4.0),
            ));
            commands.spawn((
                Camera3d::default(),
                AmbientLight {
                    brightness: 60.0,
                    ..default()
                },
                Transform::from_xyz(-1.0, 2.2, 5.5).looking_at(Vec3::new(0.0, 1.5, 0.0), Vec3::Y),
            ));
        },
    )
    // Step 2: bind the stable image handle in a material you own.
    .add_observer(
        |ready: On<TerminalReady>,
         mut commands: Commands,
         mut materials: ResMut<Assets<StandardMaterial>>,
         textures: Query<&TerminalTexture, With<Screen>>| {
            let Ok(texture) = textures.get(ready.entity) else {
                return;
            };
            info!("screen ready: {}x{} px", texture.size.x, texture.size.y);
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(texture.image.clone()),
                emissive_texture: Some(texture.image.clone()),
                emissive: LinearRgba::WHITE * 2.0,
                perceptual_roughness: 0.25,
                reflectance: 0.6,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            commands
                .entity(ready.entity)
                .insert(MeshMaterial3d(material));
        },
    )
    // Step 3: react to re-measures when your mapping depends on the size.
    .add_observer(|event: On<TerminalRemeasured>| {
        info!(
            "screen re-measured: {} -> {} px (normalized UVs need no update)",
            event.previous_size, event.size
        );
    })
    .add_systems(Update, (draw, toggle_power, animate_power))
    .run();
}

fn draw(mut screens: Query<&mut RatatuiTerminal, With<Screen>>) {
    for mut terminal in &mut screens {
        common::draw_demo(&mut terminal);
    }
}

fn toggle_power(keys: Res<ButtonInput<KeyCode>>, mut power: ResMut<Power>) {
    if keys.just_pressed(KeyCode::Space) {
        power.on = !power.on;
    }
}

/// Fades the emissive tint; the base color stays so a powered-down screen
/// still shows faint content under the room light.
fn animate_power(
    time: Res<Time>,
    mut power: ResMut<Power>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    screens: Query<&MeshMaterial3d<StandardMaterial>, With<Screen>>,
) {
    let target = if power.on { 1.0 } else { 0.0 };
    let level = power.level + (target - power.level) * (time.delta_secs() * 4.0).min(1.0);
    if (level - power.level).abs() < 1e-4 {
        return;
    }
    power.level = level;
    for material in &screens {
        if let Some(mut material) = materials.get_mut(material) {
            material.emissive = LinearRgba::WHITE * (2.0 * level);
            material.base_color =
                Color::srgb(0.2 + 0.8 * level, 0.2 + 0.8 * level, 0.25 + 0.75 * level);
        }
    }
}

/// A gently bulged screen: a `width` × `height` grid whose center is pushed
/// `bulge` units toward the viewer, with UVs running 0..1 across the face.
fn curved_screen(width: f32, height: f32, bulge: f32) -> Mesh {
    const SEGMENTS: u32 = 24;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for y in 0..=SEGMENTS {
        for x in 0..=SEGMENTS {
            let u = x as f32 / SEGMENTS as f32;
            let v = y as f32 / SEGMENTS as f32;
            let nx = u * 2.0 - 1.0;
            let ny = v * 2.0 - 1.0;
            let z = bulge * (1.0 - nx * nx) * (1.0 - ny * ny);
            positions.push([nx * width / 2.0, -ny * height / 2.0, z]);
            let normal = Vec3::new(
                2.0 * bulge * nx * (1.0 - ny * ny) / width,
                -2.0 * bulge * ny * (1.0 - nx * nx) / height,
                1.0,
            )
            .normalize();
            normals.push(normal.to_array());
            uvs.push([u, v]);
        }
    }
    let mut indices = Vec::new();
    for y in 0..SEGMENTS {
        for x in 0..SEGMENTS {
            let i = y * (SEGMENTS + 1) + x;
            indices.extend([
                i,
                i + SEGMENTS + 1,
                i + 1,
                i + 1,
                i + SEGMENTS + 1,
                i + SEGMENTS + 2,
            ]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}
