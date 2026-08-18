//! Headless GPU tests: render a terminal and read the texture back.
//!
//! These need a GPU (Metal/Vulkan/DX12) and are ignored by default; run them
//! with `cargo test -p bevy_terminal --test gpu_readback -- --ignored`.

use std::sync::{Arc, Mutex};

use bevy::{
    app::ScheduleRunnerPlugin,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        settings::RenderCreation,
    },
    winit::WinitPlugin,
};
use bevy_terminal::prelude::*;

#[derive(Resource, Clone)]
struct Captured(Arc<Mutex<Option<Capture>>>);

type Capture = (Vec<u8>, UVec2);

/// Renders `surface` headlessly with `config` and returns the texture's RGBA8
/// bytes (sRGB-encoded, straight alpha) and its size.
fn render_headless(surface: TerminalSurface, config: TerminalRenderConfig) -> (Vec<u8>, UVec2) {
    let captured = Captured(Arc::new(Mutex::new(None)));
    let sink = captured.clone();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::default()),
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins((
        ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(1)),
        TerminalPlugin,
    ))
    .insert_resource(captured)
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn((Terminal::new(surface.clone()), config.clone()));
    })
    .add_observer(
        |ready: On<TerminalReady>, mut commands: Commands, textures: Query<&TerminalTexture>| {
            let texture = textures.get(ready.entity).unwrap();
            commands
                .spawn(Readback::texture(texture.image.clone()))
                .observe(
                    |done: On<ReadbackComplete>,
                     sink: Res<Captured>,
                     textures: Query<&TerminalTexture>,
                     mut frames: Local<u32>,
                     mut exit: MessageWriter<AppExit>| {
                        // Skip the first readbacks: the scene reaches the GPU a couple of
                        // frames after the texture exists.
                        *frames += 1;
                        if *frames < 6 {
                            return;
                        }
                        let size = textures.single().map(|t| t.size).unwrap_or_default();
                        *sink.0.lock().unwrap() = Some((done.data.clone(), size));
                        exit.write(AppExit::Success);
                    },
                );
        },
    );
    app.run();
    let captured = sink.0.lock().unwrap().take();
    captured.expect("a readback completed")
}

fn texel(data: &[u8], size: UVec2, x: u32, y: u32) -> [u8; 4] {
    // Readback rows may be padded to 256 bytes.
    let unpadded = size.x as usize * 4;
    let stride = data.len() / size.y as usize;
    assert!(stride >= unpadded);
    let start = y as usize * stride + x as usize * 4;
    [
        data[start],
        data[start + 1],
        data[start + 2],
        data[start + 3],
    ]
}

#[test]
#[ignore = "requires a GPU"]
fn translucent_background_and_srgb_texture() {
    let surface = TerminalSurface::new((4, 2));
    surface.update(|u| {
        u.set_cell((0, 0), &TerminalCell::new("█"));
        u.set_cell(
            (1, 0),
            &TerminalCell::new("█")
                .with_style(TerminalStyle::new().fg(TerminalColor::Rgb(255, 0, 0))),
        );
    });
    let theme = TerminalTheme {
        background: Color::srgba(0.0, 0.0, 0.0, 0.5),
        ..default()
    };
    let config = TerminalRenderConfig {
        cell_size: Vec2::new(10.0, 20.0).into(),
        font_size: FontSizing::Px(18.0),
        theme,
        cursor: CursorConfig {
            blink_hz: None,
            ..default()
        },
        blink: BlinkConfig::NONE,
        ..default()
    };
    let (data, size) = render_headless(surface, config);
    assert_eq!(size, UVec2::new(40, 40));
    // An empty cell shows the translucent background.
    let empty = texel(&data, size, 35, 30);
    assert!(
        (empty[3] as i32 - 128).abs() <= 2,
        "empty cell alpha {empty:?}"
    );
    // A full block is opaque, and red stays red through the sRGB encoding.
    let block = texel(&data, size, 5, 10);
    assert_eq!(block[3], 255, "block alpha {block:?}");
    let red = texel(&data, size, 15, 10);
    assert!(
        red[0] > 240 && red[1] < 16 && red[2] < 16 && red[3] == 255,
        "red block {red:?}"
    );
}
