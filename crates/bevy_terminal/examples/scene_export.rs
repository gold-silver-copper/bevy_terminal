//! Exports the direct-scene example's renderer-owned textures to PNG files.

mod common;

use bevy::{
    app::ScheduleRunnerPlugin,
    asset::RenderAssetUsages,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
    winit::WinitPlugin,
};
use bevy_terminal::prelude::*;

#[derive(Resource, Default)]
struct ExportsFinished(u32);

#[derive(Resource)]
struct Surfaces {
    main: TerminalSurface,
    status: TerminalSurface,
}

fn main() {
    let main = common::scene_surface();
    let status = common::status_surface();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(
        std::time::Duration::from_millis(1),
    ));
    let config = common::configure_fonts(
        &mut app,
        TerminalRenderConfig {
            sizing: TerminalSizing::FitCellWidth(common::CELL_SIZE),
            raster: RasterConfig {
                scale: TerminalRenderScale::Fixed(1.0),
                ..default()
            },
            cursor: CursorConfig {
                blink_hz: None,
                ..default()
            },
            ..default()
        },
    );
    app.add_plugins(TerminalPlugin)
        .insert_resource(Surfaces {
            main: main.clone(),
            status: status.clone(),
        })
        .add_systems(Startup, move |mut commands: Commands| {
            for surface in [&main, &status] {
                commands.spawn((TerminalRenderer::new(surface.clone()), config.clone()));
            }
        })
        .init_resource::<ExportsFinished>()
        .add_observer(export_when_ready)
        .add_systems(Update, |time: Res<Time>| {
            assert!(time.elapsed_secs() < 60.0, "terminal export timed out");
        });
    app.run();
}

/// These two scenes are static after readiness and contain opaque pixels.
/// New target images contain only zero bytes. A correctly sized readback with
/// nonzero alpha therefore proves that the initial scene reached the GPU;
/// asset preparation may take any number of frames. This check is specific to
/// this example, not a completion signal for arbitrary changing terminals.
fn export_when_ready(
    ready: On<TerminalReady>,
    surfaces: Res<Surfaces>,
    terminals: Query<(&TerminalRenderer, &TerminalTexture)>,
    mut commands: Commands,
) {
    let (terminal, texture) = terminals.get(ready.entity).unwrap();
    let name = if terminal.surface().shares_state_with(&surfaces.main) {
        "scene"
    } else if terminal.surface().shares_state_with(&surfaces.status) {
        "status"
    } else {
        return;
    };
    let size = texture.measured().unwrap().size();
    let row_bytes = size.x as usize * 4;
    let stride = row_bytes.next_multiple_of(256);
    commands
        .spawn(Readback::texture(texture.image.clone()))
        .observe(
            move |done: On<ReadbackComplete>,
                  mut commands: Commands,
                  mut saved: Local<bool>,
                  mut finished: ResMut<ExportsFinished>,
                  mut exit: MessageWriter<AppExit>| {
                if *saved || done.data.len() != stride * size.y as usize {
                    return;
                }
                let pixels: Vec<u8> = done
                    .data
                    .chunks_exact(stride)
                    .flat_map(|row| row[..row_bytes].iter().copied())
                    .collect();
                if !pixels.chunks_exact(4).any(|rgba| rgba[3] != 0) {
                    return;
                }
                let image = Image::new(
                    Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    pixels,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::MAIN_WORLD,
                );
                let directory = format!("target/bevy-terminal-qa/{name}");
                std::fs::create_dir_all(&directory).expect("create export directory");
                image
                    .try_into_dynamic()
                    .expect("RGBA8 image")
                    .save(format!("{directory}/00000.png"))
                    .expect("save PNG");
                *saved = true;
                commands.entity(done.entity).despawn();
                finished.0 += 1;
                if finished.0 == 2 {
                    exit.write(AppExit::Success);
                }
            },
        );
}
