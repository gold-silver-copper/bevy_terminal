use bevy_terminal::bevy::math::Vec2;
use bevy_terminal::prelude::{TerminalSurface, TerminalTexture};

pub fn measure(surface: TerminalSurface, cell_size: Vec2) -> TerminalTexture {
    measure_at_scale(surface, cell_size, 1.0)
}

pub fn measure_at_scale(surface: TerminalSurface, cell_size: Vec2, scale: f32) -> TerminalTexture {
    use bevy_terminal::bevy::prelude::*;
    use bevy_terminal::prelude::*;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
    ))
    .init_asset::<Image>()
    .add_plugins(TerminalPlugin);
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf").to_vec(),
        ));
    let entity = app
        .world_mut()
        .spawn((
            TerminalRenderer::new(surface),
            TerminalRenderConfig {
                font: FontFaces::regular(font),
                raster: RasterConfig { scale, ..default() },
                sizing: TerminalSizing::Fixed {
                    cell_size,
                    font_size: 12.0,
                },
                ..default()
            },
        ))
        .id();
    for _ in 0..4 {
        app.update();
    }
    let output = app.world().get::<TerminalTexture>(entity).unwrap().clone();
    assert!(output.measured().is_some());
    output
}
