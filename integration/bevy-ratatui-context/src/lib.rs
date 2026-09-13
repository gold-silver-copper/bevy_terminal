//! Compile and runtime fixture for bevy_ratatui's forthcoming windowed migration.
use bevy::{app::PluginGroupBuilder, prelude::*};
use bevy_ratatui::{RatatuiPlugins, context::TerminalContext};
use bevy_terminal_ratatui::prelude::*;

/// Resource-owned context with the actual consumer's required dereference target.
#[derive(Resource, Deref, DerefMut)]
pub struct WindowedContext(ratatui::Terminal<RatatuiBackend>);

impl TerminalContext<RatatuiBackend> for WindowedContext {
    fn init() -> Result<Self> {
        Ok(Self(ratatui::Terminal::new(RatatuiBackend::new(80, 24))?))
    }

    fn restore() -> Result<()> {
        Ok(())
    }

    fn configure_plugin_group(
        _group: &RatatuiPlugins,
        builder: PluginGroupBuilder,
    ) -> PluginGroupBuilder {
        builder.add(TerminalPlugin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_drives_a_stable_measured_texture_without_cpu_image_copying() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::text::TextPlugin,
        ))
        .init_asset::<Image>()
        .add_plugins(WindowedContext::configure_plugin_group(
            &RatatuiPlugins::default(),
            PluginGroupBuilder::start::<RatatuiPlugins>(),
        ));
        let font = Font::from_bytes(
            include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
                .to_vec(),
        );
        let font = app.world_mut().resource_mut::<Assets<Font>>().add(font);
        let mut context = WindowedContext::init().unwrap();
        context
            .draw(|frame| frame.render_widget("context", frame.area()))
            .unwrap();
        let surface = context.backend().surface();
        let entity = app
            .world_mut()
            .spawn((
                TerminalRenderer::new(surface.clone()),
                TerminalRenderConfig {
                    font: FontFaces::regular(font),
                    ..default()
                },
                ImageNode::default(),
            ))
            .id();
        app.insert_resource(context);
        for _ in 0..4 {
            app.update();
        }
        let initial = app
            .world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .unwrap()
            .clone();
        assert_eq!(surface.snapshot().row_text(0).trim_end(), "context");
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&initial.image)
                .unwrap()
                .data
                .is_none()
        );
        {
            let mut context = app.world_mut().resource_mut::<WindowedContext>();
            context.backend_mut().resize(40, 12);
            context.autoresize().unwrap();
            context
                .draw(|frame| frame.render_widget("resized", frame.area()))
                .unwrap();
        }
        app.update();
        let output = app
            .world()
            .get::<TerminalTexture>(entity)
            .unwrap()
            .measured()
            .unwrap();
        assert_eq!(output.image, initial.image);
        assert_ne!(output.size, initial.size);
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().image,
            initial.image
        );
        let pixels = output.size;
        app.world_mut()
            .resource_mut::<WindowedContext>()
            .backend_mut()
            .set_pixel_size(pixels);
        assert_eq!(surface.snapshot().row_text(0).trim_end(), "resized");
        assert!(app.world_mut().despawn(entity));
        WindowedContext::restore().unwrap();
    }
}
