//! World-space presentation: [`TerminalWorldQuad`] shows a terminal on an
//! unlit 3D rectangle whose aspect ratio follows the measured texture.

use bevy::{asset::Assets, mesh::Mesh3d, pbr::MeshMaterial3d, prelude::*};

use super::{TerminalSystems, TerminalTexture};

/// Presents a [`super::Terminal`] on a world-space rectangle.
///
/// Add it next to the `Terminal` component (with a `Transform` to place it).
/// The plugin inserts a `Mesh3d` sized `height` tall and as wide as the
/// texture's aspect ratio dictates, plus an unlit, alpha-blended
/// `StandardMaterial` bound to the terminal image. The mesh is rebuilt whenever
/// the texture is (re)measured or this component changes; the material is
/// created once because the image handle is stable.
///
/// Entities with their own mesh (an imported screen model, say) do not need
/// this: bind [`TerminalTexture::image`] to your material once and observe
/// [`super::TerminalRemeasured`] if the mapping depends on the size.
///
/// Requires the `3d` feature.
#[derive(Clone, Copy, Debug, PartialEq, Component)]
#[require(Transform, Visibility)]
pub struct TerminalWorldQuad {
    /// Height of the quad in world units; width follows the texture aspect.
    pub height: f32,
    /// Whether the material ignores scene lighting (default `true`, which
    /// reproduces the terminal colors exactly).
    pub unlit: bool,
    /// Alpha mode of the material (default `AlphaMode::Blend`, so a
    /// transparent theme background shows the scene behind the quad).
    pub alpha_mode: AlphaMode,
}

impl Default for TerminalWorldQuad {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl TerminalWorldQuad {
    /// A quad `height` world units tall.
    #[must_use]
    pub const fn new(height: f32) -> Self {
        Self {
            height,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
        }
    }

    /// Size of the quad in world units for a texture of `size` pixels.
    #[must_use]
    pub fn size_for(&self, size: UVec2) -> Vec2 {
        let size = size.max(UVec2::ONE).as_vec2();
        Vec2::new(self.height * size.x / size.y, self.height)
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        sync_world_quads.after(TerminalSystems::Sync).run_if(
            resource_exists::<Assets<Mesh>>.and_then(resource_exists::<Assets<StandardMaterial>>),
        ),
    );
}

type QuadChanged = Or<(Changed<TerminalWorldQuad>, Changed<TerminalTexture>)>;
type QuadItem<'w> = (
    Entity,
    &'w TerminalWorldQuad,
    &'w TerminalTexture,
    Option<&'w Mesh3d>,
    Option<&'w MeshMaterial3d<StandardMaterial>>,
);

fn sync_world_quads(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    quads: Query<QuadItem<'_>, QuadChanged>,
) {
    for (entity, quad, texture, mesh, material) in &quads {
        let size = quad.size_for(texture.size);
        let rectangle = Mesh::from(Rectangle::from_size(size));
        match mesh.filter(|mesh| meshes.contains(&mesh.0)) {
            Some(mesh) => {
                if let Some(mut existing) = meshes.get_mut(&mesh.0) {
                    *existing = rectangle;
                }
            }
            None => {
                commands
                    .entity(entity)
                    .insert(Mesh3d(meshes.add(rectangle)));
            }
        }
        let material_handle = match material {
            Some(material) if materials.contains(&material.0) => material.0.clone(),
            _ => {
                let handle = materials.add(StandardMaterial::default());
                commands
                    .entity(entity)
                    .insert(MeshMaterial3d(handle.clone()));
                handle
            }
        };
        let Some(mut material) = materials.get_mut(&material_handle) else {
            continue;
        };
        material.base_color_texture = Some(texture.image.clone());
        material.unlit = quad.unlit;
        material.alpha_mode = quad.alpha_mode;
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::primitives::MeshAabb;

    use super::*;
    use crate::{render::Terminal, surface::TerminalSurface};

    #[test]
    fn quad_follows_the_texture_aspect_and_keeps_its_material() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
            super::super::TerminalPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>();
        let surface = TerminalSurface::new((4, 2));
        let entity = app
            .world_mut()
            .spawn((Terminal::new(surface.clone()), TerminalWorldQuad::new(2.0)))
            .id();
        app.update();
        app.update();
        let world = app.world();
        let texture = world.get::<TerminalTexture>(entity).unwrap();
        let material = world
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .unwrap()
            .clone();
        let standard = world
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap();
        assert_eq!(standard.base_color_texture, Some(texture.image.clone()));
        assert!(standard.unlit);
        let mesh = world.get::<Mesh3d>(entity).unwrap().clone();
        let expected = TerminalWorldQuad::new(2.0).size_for(texture.size);
        assert_eq!(expected.y, 2.0);
        let aabb = world
            .resource::<Assets<Mesh>>()
            .get(&mesh)
            .unwrap()
            .compute_aabb()
            .unwrap();
        assert_eq!(aabb.half_extents.truncate() * 2.0, expected);

        surface.update(|update| {
            update.resize((8, 2));
        });
        app.update();
        let world = app.world();
        let doubled = TerminalWorldQuad::new(2.0)
            .size_for(world.get::<TerminalTexture>(entity).unwrap().size);
        assert!(
            (doubled.x - expected.x * 2.0).abs() < 1e-3,
            "{doubled:?} vs {expected:?}"
        );
        assert_eq!(
            world.get::<Mesh3d>(entity).unwrap().0,
            mesh.0,
            "mesh handle reused"
        );
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0,
            material.0
        );
        let aabb = world
            .resource::<Assets<Mesh>>()
            .get(&mesh)
            .unwrap()
            .compute_aabb()
            .unwrap();
        assert!((aabb.half_extents.x * 2.0 - doubled.x).abs() < 1e-3);
    }
}
