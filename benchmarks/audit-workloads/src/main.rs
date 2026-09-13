//! Reproducible CPU/heap workloads, separate from production crate lints/dependencies.
use bevy::prelude::*;
use bevy_terminal::prelude::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};

struct CountingAllocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

fn allocated(bytes: usize) {
    ALLOCATIONS.fetch_add(1, Relaxed);
    BYTES.fetch_add(bytes as u64, Relaxed);
    let live = LIVE.fetch_add(bytes as u64, Relaxed) + bytes as u64;
    PEAK.fetch_max(live, Relaxed);
}

// SAFETY: All allocations and layouts are forwarded unchanged to System.
// Accounting uses only atomics and never allocates or dereferences user memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            allocated(layout.size());
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            allocated(layout.size());
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as u64, Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Relaxed);
            allocated(size);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measure(name: &str, steps: usize, mut work: impl FnMut(usize) -> u64) {
    ALLOCATIONS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    let live = LIVE.load(Relaxed);
    PEAK.store(live, Relaxed);
    let start = Instant::now();
    let mut misses = 0;
    for step in 0..steps {
        misses += std::hint::black_box(work(step));
    }
    let elapsed = start.elapsed().as_nanos();
    let allocations = ALLOCATIONS.load(Relaxed);
    let bytes = BYTES.load(Relaxed);
    let peak = PEAK.load(Relaxed).saturating_sub(live);
    println!("{name}\t{steps}\t{elapsed}\t{allocations}\t{bytes}\t{peak}\t{misses}");
}

fn scroll(name: &str, symbol: &str, changing: bool) {
    let surface = TerminalSurface::new((80, 24));
    let a = TerminalCell::new(symbol);
    let b = TerminalCell::new(&format!("b{}", &symbol[1..]));
    surface.update(|u| {
        for y in 0..24 {
            for x in 0..80 {
                u.set_cell((x, y), if changing && y % 2 != 0 { &b } else { &a });
            }
        }
    });
    measure(name, 200, |step| {
        surface.update(|u| {
            u.scroll_up(0..24, 1);
            for x in 0..80 {
                u.set_cell((x, 23), if changing && step % 2 == 0 { &b } else { &a });
            }
        });
        0
    });
}

fn app(terminals: usize) -> (App, Vec<(Entity, TerminalSurface)>, Vec<Handle<Font>>) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
    ))
    .init_asset::<Image>()
    .add_plugins(TerminalPlugin);
    let bytes = include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
    let fonts: Vec<_> = (0..16)
        .map(|_| {
            app.world_mut()
                .resource_mut::<Assets<Font>>()
                .add(Font::from_bytes(bytes.to_vec()))
        })
        .collect();
    let surfaces = (0..terminals)
        .map(|_| {
            let surface = TerminalSurface::new((40, 12));
            let entity = app
                .world_mut()
                .spawn((
                    TerminalRenderer::new(surface.clone()),
                    TerminalRenderConfig {
                        font: FontFaces::regular(fonts[0].clone()),
                        sizing: TerminalSizing::Fixed {
                            cell_size: Vec2::new(8.0, 12.0),
                            font_size: 8.0,
                        },
                        ..default()
                    },
                ))
                .id();
            (entity, surface)
        })
        .collect();
    for _ in 0..5 {
        app.update();
    }
    (app, surfaces, fonts)
}

fn main() {
    println!(
        "workload\tsteps\tns\tallocations\tallocated_bytes\tpeak_extra_live_bytes\tshape_misses"
    );
    scroll("scroll_equal_ascii", "a", false);
    scroll(
        "scroll_equal_heap",
        "a\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
        false,
    );
    scroll(
        "scroll_changing_heap",
        "a\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
        true,
    );
    {
        let (mut app, surfaces, _fonts) = app(32);
        let image_bytes: usize = app
            .world()
            .resource::<Assets<Image>>()
            .iter()
            .map(|(_, image)| image.data.as_ref().map_or(0, Vec::len))
            .sum();
        eprintln!("idle_32 CPU image bytes: {image_bytes}");
        assert!(surfaces.iter().all(|(entity, _)| {
            app.world()
                .get::<TerminalTexture>(*entity)
                .unwrap()
                .measured()
                .is_some()
        }));
        measure("idle_32_fonts_16", 200, |_| {
            app.update();
            0
        });
    }
    let (mut app, surfaces, fonts) = app(1);
    let (entity, surface) = &surfaces[0];
    measure("resize_font_scale_churn", 48, |step| {
        surface.update(|u| {
            u.resize((if step % 2 == 0 { 40 } else { 80 }, 12));
            u.set_cell((0, 0), &TerminalCell::new("A"));
        });
        let mut config = app
            .world_mut()
            .get_mut::<TerminalRenderConfig>(*entity)
            .unwrap();
        config.font = FontFaces::regular(fonts[step % 2].clone());
        config.raster.scale = if step % 3 == 0 { 2.0 } else { 1.0 };
        app.update();
        u64::from(
            app.world()
                .get::<TerminalStats>(*entity)
                .unwrap()
                .shape_misses,
        )
    });
    surface.update(|u| {
        u.resize((40, 12));
    });
    measure("unicode_cache_saturation", 12, |step| {
        surface.update(|u| {
            for y in 0..12 {
                for x in 0..40 {
                    let index = step * 480 + usize::from(y) * 40 + usize::from(x);
                    let symbol = char::from_u32(0x400 + index as u32).unwrap().to_string();
                    u.set_cell((x, y), &TerminalCell::new(&symbol));
                }
            }
        });
        app.update();
        u64::from(
            app.world()
                .get::<TerminalStats>(*entity)
                .unwrap()
                .shape_misses,
        )
    });
    measure("ascii_after_saturation", 20, |step| {
        surface.update(|u| {
            for y in 0..12 {
                for x in 0..40 {
                    let symbol =
                        char::from(b'!' + ((usize::from(x) + step) % 16) as u8).to_string();
                    u.set_cell((x, y), &TerminalCell::new(&symbol));
                }
            }
        });
        app.update();
        u64::from(
            app.world()
                .get::<TerminalStats>(*entity)
                .unwrap()
                .shape_misses,
        )
    });
}
