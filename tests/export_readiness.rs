//! Exercise the examples' export buffer synchronization with real GPU assets.
#[path = "../examples/common/export_gpu.rs"]
mod export_gpu;

use bevy::{
    app::ScheduleRunnerPlugin,
    asset::RenderAssetUsages,
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin,
        render_asset::RenderAssets,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        settings::RenderCreation,
        texture::GpuImage,
    },
    winit::WinitPlugin,
};
use bevy_image_export::{GpuImageExportSource, ImageExportPlugin, ImageExportSource};
use std::sync::{Arc, Mutex};

#[derive(Resource, Clone, Default)]
struct Observed(Arc<Mutex<Vec<UVec2>>>);

#[derive(Resource)]
struct KeepSource {
    _handle: Handle<ImageExportSource>,
}

#[test]
#[ignore = "requires a GPU"]
fn export_buffers_follow_delayed_images_and_resizes() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
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
        ScheduleRunnerPlugin::default(),
        ImageExportPlugin::default(),
    ));
    export_gpu::install(&mut app);
    let observed = Observed::default();
    app.sub_app_mut(RenderApp)
        .insert_resource(observed.clone())
        .add_systems(
            Render,
            (|sources: Res<RenderAssets<GpuImageExportSource>>,
              images: Res<RenderAssets<GpuImage>>,
              observed: Res<Observed>| {
                let mut samples = observed.0.lock().unwrap();
                if sources.iter().next().is_none() {
                    samples.push(UVec2::ZERO);
                }
                for (_, source) in sources.iter() {
                    let image = images.get(&source.source_handle).unwrap();
                    assert_eq!(source.source_size, image.texture.size());
                    assert_eq!(
                        source.buffer.size(),
                        u64::from(source.padded_bytes_per_row)
                            * u64::from(source.source_size.height)
                    );
                    samples.push(UVec2::new(
                        source.source_size.width,
                        source.source_size.height,
                    ));
                }
            })
            .after(export_gpu::refresh_buffers),
        );
    let image = app.world().resource::<Assets<Image>>().reserve_handle();
    let source = app
        .world_mut()
        .resource_mut::<Assets<ImageExportSource>>()
        .add(image.clone());
    app.insert_resource(KeepSource { _handle: source });
    app.add_systems(
        Update,
        move |mut frame: Local<u32>,
              mut images: ResMut<Assets<Image>>,
              mut exit: MessageWriter<AppExit>| {
            *frame += 1;
            if *frame == 5 {
                images
                    .insert(
                        image.id(),
                        Image::new_fill(
                            Extent3d {
                                width: 4,
                                height: 4,
                                depth_or_array_layers: 1,
                            },
                            TextureDimension::D2,
                            &[0, 0, 0, 255],
                            TextureFormat::Rgba8UnormSrgb,
                            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
                        ),
                    )
                    .unwrap();
            }
            if *frame == 12 {
                images.get_mut(&image).unwrap().resize(Extent3d {
                    width: 8,
                    height: 4,
                    depth_or_array_layers: 1,
                });
            }
            if *frame == 18 {
                images.get_mut(&image).unwrap().resize(Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                });
            }
            if *frame == 30 {
                exit.write(AppExit::Success);
            }
        },
    );
    app.run();
    let samples = observed.0.lock().unwrap();
    assert!(samples.contains(&UVec2::ZERO), "missing images must wait");
    assert!(samples.contains(&UVec2::new(4, 4)));
    assert!(samples.contains(&UVec2::new(8, 4)));
    assert!(samples.contains(&UVec2::new(2, 2)));
}
