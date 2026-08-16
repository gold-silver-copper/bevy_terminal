//! Shared protocol and headless Bevy driver for Ratatui renderer benchmarks.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::{
    app::SubApps,
    asset::RenderAssetUsages,
    camera::RenderTarget,
    prelude::*,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages},
        renderer::{RenderAdapterInfo, RenderDevice},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use fontdb::{Database, Family, Query};
use ratatui::{
    Frame,
    buffer::Buffer,
    style::{Color as RatatuiColor, Modifier, Style},
};
use serde::{Deserialize, Serialize};

/// Result type used across separately compiled adapters.
pub type BenchResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Wire-format version emitted by every adapter process.
pub const SCHEMA_VERSION: u32 = 1;

/// A canonical workload applied to an entire Ratatui frame.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Workload {
    /// A byte-identical dashboard after the first frame.
    Static,
    /// A dashboard where only a counter and progress marker move.
    Sparse,
    /// Every cell changes its printable ASCII glyph.
    DenseAscii,
    /// Every cell changes true-color foreground/background and modifiers.
    DenseStyled,
    /// Terminal-specific Unicode: wide, combining, emoji, box and braille.
    Unicode,
}

impl Workload {
    /// Stable command-line spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Sparse => "sparse",
            Self::DenseAscii => "dense_ascii",
            Self::DenseStyled => "dense_styled",
            Self::Unicode => "unicode",
        }
    }
}

impl std::str::FromStr for Workload {
    type Err = ParseWorkloadError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "static" => Ok(Self::Static),
            "sparse" => Ok(Self::Sparse),
            "dense_ascii" | "dense-ascii" => Ok(Self::DenseAscii),
            "dense_styled" | "dense-styled" => Ok(Self::DenseStyled),
            "unicode" => Ok(Self::Unicode),
            _ => Err(ParseWorkloadError(value.to_owned())),
        }
    }
}

/// Error returned for an unknown workload name.
#[derive(Debug)]
pub struct ParseWorkloadError(String);

impl fmt::Display for ParseWorkloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown workload {:?}", self.0)
    }
}

impl Error for ParseWorkloadError {}

/// Shared runtime configuration accepted by every adapter binary.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchConfig {
    /// Terminal columns.
    pub cols: u16,
    /// Terminal rows.
    pub rows: u16,
    /// Requested logical cell width.
    pub cell_width: f32,
    /// Requested logical cell height.
    pub cell_height: f32,
    /// Requested font size.
    pub font_size: u32,
    /// Frames discarded before sampling.
    pub warmup_frames: u32,
    /// Frames retained as samples.
    pub measured_frames: u32,
    /// Canonical cell workload.
    pub workload: Workload,
    /// Wait for all work submitted to Bevy's WGPU device every frame.
    pub gpu_sync: bool,
    /// Optional PNG path written after all timed frames.
    #[serde(skip)]
    pub capture_path: Option<PathBuf>,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 40,
            cell_width: 10.0,
            cell_height: 20.0,
            font_size: 16,
            warmup_frames: 30,
            measured_frames: 120,
            workload: Workload::DenseAscii,
            gpu_sync: true,
            capture_path: None,
        }
    }
}

impl BenchConfig {
    /// Parse the dependency-free CLI shared by all adapter executables.
    ///
    /// # Errors
    ///
    /// Returns an error when a flag is unknown, a value is missing or malformed,
    /// or the requested dimensions and frame count are invalid.
    pub fn from_args() -> BenchResult<Self> {
        let mut config = Self::default();
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            let mut value = || -> BenchResult<String> {
                args.next()
                    .ok_or_else(|| format!("missing value after {flag}").into())
            };
            match flag.as_str() {
                "--cols" => config.cols = value()?.parse()?,
                "--rows" => config.rows = value()?.parse()?,
                "--cell-width" => config.cell_width = value()?.parse()?,
                "--cell-height" => config.cell_height = value()?.parse()?,
                "--font-size" => config.font_size = value()?.parse()?,
                "--warmup" => config.warmup_frames = value()?.parse()?,
                "--frames" => config.measured_frames = value()?.parse()?,
                "--workload" => config.workload = value()?.parse()?,
                "--gpu-sync" => config.gpu_sync = parse_bool(&value()?)?,
                "--capture" => config.capture_path = Some(value()?.into()),
                "--help" | "-h" => {
                    println!(
                        "--cols N --rows N --cell-width PX --cell-height PX \
                         --font-size PX --warmup N --frames N \
                         --workload static|sparse|dense_ascii|dense_styled|unicode \
                         --gpu-sync true|false [--capture PATH]"
                    );
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument {flag}").into()),
            }
        }
        if config.cols == 0 || config.rows == 0 {
            return Err("--cols and --rows must both be non-zero".into());
        }
        if config.measured_frames == 0 {
            return Err("--frames must be non-zero".into());
        }
        if !config.cell_width.is_finite()
            || !config.cell_height.is_finite()
            || config.cell_width <= 0.0
            || config.cell_height <= 0.0
        {
            return Err("cell dimensions must be finite and positive".into());
        }
        Ok(config)
    }

    /// Requested offscreen target width.
    #[must_use]
    pub fn target_width(&self) -> u32 {
        positive_rounded_u32(f32::from(self.cols) * self.cell_width)
    }

    /// Requested offscreen target height.
    #[must_use]
    pub fn target_height(&self) -> u32 {
        positive_rounded_u32(f32::from(self.rows) * self.cell_height)
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn positive_rounded_u32(value: f32) -> u32 {
    // The caller validates that cell dimensions are finite and positive. Clamp
    // here as a defensive measure for multiplication overflow or future callers.
    value.round().clamp(1.0, u32::MAX as f32) as u32
}

fn parse_bool(value: &str) -> BenchResult<bool> {
    match value {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("expected boolean, got {value:?}").into()),
    }
}

/// Static identity and scope supplied by an adapter.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AdapterMetadata {
    /// Registry identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Renderer crate version or local revision.
    pub renderer_version: String,
    /// Bevy version in this adapter process.
    pub bevy_version: String,
    /// Ratatui version in this adapter process.
    pub ratatui_version: String,
    /// Concise description of the measured render path.
    pub render_path: String,
    /// Qualification required when interpreting comparisons.
    pub notes: Vec<String>,
}

/// Timing phases measured directly by one adapter call.
#[derive(Clone, Copy, Debug, Default)]
pub struct AdapterFrame {
    /// Canonical workload closure, Ratatui diff, and backend draw.
    pub draw_ns: u64,
    /// CPU renderer preparation after Ratatui draw.
    pub prepare_ns: u64,
    /// Texture conversion, upload, or GPU command submission at the call site.
    pub submit_ns: u64,
}

/// A renderer implementation plugged into the shared Bevy frame driver.
pub trait RendererAdapter: Sized {
    /// Construct CPU-side state before Bevy plugins are finalized.
    ///
    /// # Errors
    ///
    /// Returns an error when the renderer cannot create its CPU-side state.
    fn new(config: &BenchConfig) -> BenchResult<Self>;

    /// Add renderer-specific plugins and systems.
    ///
    /// # Errors
    ///
    /// Returns an error when renderer plugins cannot be configured.
    fn configure_app(&mut self, _app: &mut App, _config: &BenchConfig) -> BenchResult<()> {
        Ok(())
    }

    /// Create renderer resources after Bevy's renderer device exists.
    ///
    /// # Errors
    ///
    /// Returns an error when renderer resources cannot be initialized.
    fn initialize(&mut self, world: &mut World, config: &BenchConfig) -> BenchResult<()>;

    /// Whether delayed Bevy-side materialization has completed.
    ///
    /// # Errors
    ///
    /// Returns an error when readiness cannot be determined.
    fn ready(&mut self, _world: &mut World) -> BenchResult<bool> {
        Ok(true)
    }

    /// Render one canonical frame. Work performed by Bevy systems belongs in `SubApps::update`.
    ///
    /// # Errors
    ///
    /// Returns an error when the adapter cannot render or submit the frame.
    fn render_frame(
        &mut self,
        world: &mut World,
        config: &BenchConfig,
        frame_index: u64,
    ) -> BenchResult<AdapterFrame>;

    /// Actual renderer-owned texture or output dimensions.
    fn output_size(&self, config: &BenchConfig) -> (u32, u32) {
        (config.target_width(), config.target_height())
    }

    /// Read the final renderer-owned output as tightly packed RGBA8 after timed frames.
    ///
    /// # Errors
    ///
    /// Returns an error when the renderer cannot read its completed output.
    fn capture_rgba(
        &mut self,
        _sub_apps: &mut SubApps,
        _config: &BenchConfig,
    ) -> BenchResult<Vec<u8>> {
        Err("this adapter does not implement capture_rgba".into())
    }

    /// Static adapter identity and measurement scope.
    fn metadata(&self) -> AdapterMetadata;
}

/// One retained sample in nanoseconds.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FrameSample {
    /// Zero-based sample index after warmup.
    pub frame: u32,
    /// Adapter draw phase.
    pub draw_ns: u64,
    /// Adapter preparation phase.
    pub prepare_ns: u64,
    /// Adapter upload/submission phase.
    pub submit_ns: u64,
    /// Complete Bevy main-world and render-world update.
    pub bevy_update_ns: u64,
    /// Explicit WGPU completion wait after the Bevy update.
    pub gpu_wait_ns: u64,
    /// Whole measured frame, including all phases above.
    pub total_ns: u64,
}

/// Distribution summary for one phase.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Distribution {
    /// Arithmetic mean.
    pub mean_ns: f64,
    /// Population standard deviation.
    pub stddev_ns: f64,
    /// Minimum.
    pub min_ns: u64,
    /// Median.
    pub p50_ns: u64,
    /// 95th percentile, nearest-rank.
    pub p95_ns: u64,
    /// 99th percentile, nearest-rank.
    pub p99_ns: u64,
    /// Maximum.
    pub max_ns: u64,
}

/// Process and graphics device information attached to a result.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MachineInfo {
    /// Rust target architecture.
    pub arch: String,
    /// Rust target operating system.
    pub os: String,
    /// WGPU adapter name.
    pub gpu_name: String,
    /// WGPU backend.
    pub gpu_backend: String,
    /// WGPU device category.
    pub gpu_device_type: String,
    /// Selected common system font description.
    pub font: String,
}

/// Complete result printed as one JSON object by an adapter process.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchReport {
    /// Wire schema version.
    pub schema_version: u32,
    /// Renderer identity.
    pub adapter: AdapterMetadata,
    /// Requested benchmark configuration.
    pub config: BenchConfig,
    /// Actual output width.
    pub output_width: u32,
    /// Actual output height.
    pub output_height: u32,
    /// Host/device identity.
    pub machine: MachineInfo,
    /// Raw retained samples.
    pub samples: Vec<FrameSample>,
    /// Per-phase distributions, using the sample field names.
    pub summary: BTreeMap<String, Distribution>,
}

/// Common font bytes loaded once from the host's preferred monospace family.
#[derive(Clone)]
pub struct FontFixture {
    bytes: Arc<Vec<u8>>,
    description: String,
}

impl FontFixture {
    /// Discover and load the host's preferred monospace font.
    ///
    /// # Errors
    ///
    /// Returns an error when no usable index-zero monospace face can be found or
    /// its bytes cannot be copied.
    pub fn system_monospace() -> BenchResult<Self> {
        let mut database = Database::new();
        database.load_system_fonts();
        let id = database
            .query(&Query {
                families: &[Family::Monospace],
                ..Query::default()
            })
            .filter(|id| database.face(*id).is_some_and(|face| face.index == 0))
            .or_else(|| {
                database
                    .faces()
                    .find(|face| face.monospaced && face.index == 0)
                    .map(|face| face.id)
            })
            .ok_or("no system monospace font was found")?;
        let description = database.face(id).map_or_else(
            || "system monospace".to_owned(),
            |face| face.post_script_name.clone(),
        );
        let bytes = database
            .with_face_data(id, |data, _face_index| data.to_vec())
            .ok_or("selected system monospace font has no copyable bytes")?;
        Ok(Self {
            bytes: Arc::new(bytes),
            description,
        })
    }

    /// Owned bytes suitable for renderers that retain their allocation.
    #[must_use]
    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.as_ref().clone()
    }

    /// Shared byte slice for immediate parsing.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// Font name reported with benchmark results.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Execute one adapter and print exactly one JSON report to stdout.
///
/// # Errors
///
/// Returns an error when configuration, renderer initialization, a frame, GPU
/// synchronization, or JSON serialization fails.
pub fn run<A: RendererAdapter>() -> BenchResult<()> {
    let config = BenchConfig::from_args()?;
    let font = FontFixture::system_monospace()?;
    let mut adapter = A::new(&config)?;
    let mut app = headless_bevy_app();
    app.insert_resource(SharedFontFixture(font.clone()));
    adapter.configure_app(&mut app, &config)?;
    app.finish();
    app.cleanup();
    let mut sub_apps = std::mem::take(app.sub_apps_mut());
    adapter.initialize(sub_apps.main.world_mut(), &config)?;

    wait_until_ready(&mut adapter, &mut sub_apps, &config)?;

    let total_frames = config
        .warmup_frames
        .checked_add(config.measured_frames)
        .ok_or("frame count overflow")?;
    let mut samples = Vec::with_capacity(config.measured_frames as usize);
    for frame in 0..total_frames {
        let frame_start = Instant::now();
        let phases = adapter.render_frame(sub_apps.main.world_mut(), &config, u64::from(frame))?;

        let update_start = Instant::now();
        sub_apps.update();
        let bevy_update_ns = nanos(update_start.elapsed());

        let wait_start = Instant::now();
        if config.gpu_sync {
            wait_for_bevy_gpu(sub_apps.main.world())?;
        }
        let gpu_wait_ns = nanos(wait_start.elapsed());
        let total_ns = nanos(frame_start.elapsed());

        if frame >= config.warmup_frames {
            samples.push(FrameSample {
                frame: frame - config.warmup_frames,
                draw_ns: phases.draw_ns,
                prepare_ns: phases.prepare_ns,
                submit_ns: phases.submit_ns,
                bevy_update_ns,
                gpu_wait_ns,
                total_ns,
            });
        }
    }

    let (output_width, output_height) = adapter.output_size(&config);
    if let Some(path) = config.capture_path.clone() {
        wait_for_bevy_gpu(sub_apps.main.world())?;
        let rgba = adapter.capture_rgba(&mut sub_apps, &config)?;
        let expected_len = output_width as usize * output_height as usize * 4;
        if rgba.len() != expected_len {
            return Err(format!(
                "capture contained {} bytes, expected {expected_len} for {output_width}x{output_height} RGBA8",
                rgba.len()
            )
            .into());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image::save_buffer_with_format(
            path,
            &rgba,
            output_width,
            output_height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
    }
    let report = BenchReport {
        schema_version: SCHEMA_VERSION,
        adapter: adapter.metadata(),
        config,
        output_width,
        output_height,
        machine: machine_info(sub_apps.main.world(), font.description()),
        summary: summarize(&samples),
        samples,
    };
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

/// Read a render-world Bevy image into tightly packed RGBA8 pixels.
///
/// The image must include [`TextureUsages::COPY_SRC`]. BGRA images are converted to RGBA.
///
/// # Errors
///
/// Returns an error when the render image is unavailable, its format is unsupported, or WGPU
/// readback fails.
pub fn read_bevy_image_rgba(sub_apps: &mut SubApps, image: Handle<Image>) -> BenchResult<Vec<u8>> {
    const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;

    let descriptor = sub_apps
        .main
        .world()
        .resource::<Assets<Image>>()
        .get(&image)
        .ok_or("requested Bevy image asset is unavailable")?
        .texture_descriptor
        .clone();
    let width = descriptor.size.width;
    let height = descriptor.size.height;
    let format = descriptor.format;
    if !matches!(
        format,
        TextureFormat::Rgba8Unorm
            | TextureFormat::Rgba8UnormSrgb
            | TextureFormat::Bgra8Unorm
            | TextureFormat::Bgra8UnormSrgb
    ) {
        return Err(format!("unsupported capture texture format {format:?}").into());
    }
    if !descriptor.usage.contains(TextureUsages::COPY_SRC) {
        return Err("capture texture is missing COPY_SRC usage".into());
    }

    let unpadded_bytes_per_row = width * 4;
    let align = COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
    let entity = sub_apps
        .main
        .world_mut()
        .spawn(Readback::texture(image))
        .observe(move |event: On<ReadbackComplete>| {
            let _ = sender.try_send(event.to_vec());
        })
        .id();
    let mut padded = None;
    for _ in 0..120 {
        sub_apps.update();
        wait_for_bevy_gpu(sub_apps.main.world())?;
        if let Ok(data) = receiver.try_recv() {
            padded = Some(data);
            break;
        }
    }
    sub_apps.main.world_mut().despawn(entity);
    let padded = padded.ok_or("Bevy GPU capture did not complete within 120 frames")?;
    let mut rgba = vec![0; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let source_start = row * padded_bytes_per_row as usize;
        let target_start = row * unpadded_bytes_per_row as usize;
        rgba[target_start..target_start + unpadded_bytes_per_row as usize].copy_from_slice(
            padded
                .get(source_start..source_start + unpadded_bytes_per_row as usize)
                .ok_or("Bevy GPU capture returned a truncated padded row")?,
        );
    }
    if matches!(
        format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    Ok(rgba)
}

/// Convert tightly packed linear RGBA8 pixels to sRGB in place, preserving alpha.
///
/// Any trailing bytes that do not form a complete pixel are left unchanged.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn linear_rgba8_to_srgb(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        for channel in &mut pixel[..3] {
            let linear = f32::from(*channel) / 255.0;
            let srgb = if linear <= 0.003_130_8 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            *channel = (srgb * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn headless_bevy_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>(),
    );
    app
}

fn wait_until_ready<A: RendererAdapter>(
    adapter: &mut A,
    sub_apps: &mut SubApps,
    config: &BenchConfig,
) -> BenchResult<()> {
    const MAX_READY_FRAMES: u32 = 120;
    for _ in 0..MAX_READY_FRAMES {
        if adapter.ready(sub_apps.main.world_mut())? {
            return Ok(());
        }
        sub_apps.update();
        if config.gpu_sync {
            wait_for_bevy_gpu(sub_apps.main.world())?;
        }
    }
    Err(format!("adapter did not become ready within {MAX_READY_FRAMES} Bevy frames").into())
}

fn wait_for_bevy_gpu(world: &World) -> BenchResult<()> {
    world
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;
    Ok(())
}

fn machine_info(world: &World, font: &str) -> MachineInfo {
    let adapter = world.get_resource::<RenderAdapterInfo>();
    MachineInfo {
        arch: std::env::consts::ARCH.to_owned(),
        os: std::env::consts::OS.to_owned(),
        gpu_name: adapter.map_or_else(|| "unknown".to_owned(), |info| info.name.clone()),
        gpu_backend: adapter.map_or_else(
            || "unknown".to_owned(),
            |info| format!("{:?}", info.backend),
        ),
        gpu_device_type: adapter.map_or_else(
            || "unknown".to_owned(),
            |info| format!("{:?}", info.device_type),
        ),
        font: font.to_owned(),
    }
}

fn summarize(samples: &[FrameSample]) -> BTreeMap<String, Distribution> {
    type SampleField = fn(&FrameSample) -> u64;
    let fields: [(&str, SampleField); 6] = [
        ("draw_ns", |sample| sample.draw_ns),
        ("prepare_ns", |sample| sample.prepare_ns),
        ("submit_ns", |sample| sample.submit_ns),
        ("bevy_update_ns", |sample| sample.bevy_update_ns),
        ("gpu_wait_ns", |sample| sample.gpu_wait_ns),
        ("total_ns", |sample| sample.total_ns),
    ];
    fields
        .into_iter()
        .map(|(name, field)| {
            let values = samples.iter().map(field).collect::<Vec<_>>();
            (name.to_owned(), distribution(&values))
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn distribution(values: &[u64]) -> Distribution {
    // Floating point is intentional for aggregate statistics; raw samples stay u64.
    assert!(!values.is_empty(), "measured frame count is validated");
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mean_ns = sorted.iter().map(|value| *value as f64).sum::<f64>() / sorted.len() as f64;
    let variance = sorted
        .iter()
        .map(|value| {
            let delta = *value as f64 - mean_ns;
            delta * delta
        })
        .sum::<f64>()
        / sorted.len() as f64;
    Distribution {
        mean_ns,
        stddev_ns: variance.sqrt(),
        min_ns: sorted[0],
        p50_ns: percentile(&sorted, 50),
        p95_ns: percentile(&sorted, 95),
        p99_ns: percentile(&sorted, 99),
        max_ns: sorted[sorted.len() - 1],
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Measure a closure in monotonic wall-clock nanoseconds.
pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, u64) {
    let start = Instant::now();
    let output = operation();
    (output, nanos(start.elapsed()))
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// Resource through which adapters receive the exact same system font bytes.
#[derive(Resource, Clone)]
pub struct SharedFontFixture(pub FontFixture);

/// Add an offscreen UI camera at the benchmark's requested pixel resolution.
pub fn spawn_offscreen_ui_target(world: &mut World, config: &BenchConfig) -> Handle<Image> {
    let size = Extent3d {
        width: config.target_width(),
        height: config.target_height(),
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_uninit(
        size,
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    world.spawn((
        Camera2d,
        IsDefaultUiCamera,
        RenderTarget::Image(handle.clone().into()),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
    ));
    handle
}

/// Render the selected deterministic workload into a Ratatui frame.
pub fn render_workload(frame: &mut Frame<'_>, workload: Workload, frame_index: u64) {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    match workload {
        Workload::Static => render_dashboard(buffer, frame_index, false),
        Workload::Sparse => render_dashboard(buffer, frame_index, true),
        Workload::DenseAscii => render_dense_ascii(buffer, frame_index),
        Workload::DenseStyled => render_dense_styled(buffer, frame_index),
        Workload::Unicode => render_unicode(buffer, frame_index),
    }
    debug_assert_eq!(buffer.area, area);
}

fn render_dashboard(buffer: &mut Buffer, frame_index: u64, animate: bool) {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let border = Style::default().fg(RatatuiColor::Cyan);
    let body = Style::default()
        .fg(RatatuiColor::Gray)
        .bg(RatatuiColor::Rgb(8, 12, 20));
    for y in 0..height {
        for x in 0..width {
            buffer[(x, y)].set_symbol(" ").set_style(body);
        }
    }
    if width >= 2 && height >= 2 {
        for x in 1..width - 1 {
            buffer[(x, 0)].set_symbol("─").set_style(border);
            buffer[(x, height - 1)].set_symbol("─").set_style(border);
        }
        for y in 1..height - 1 {
            buffer[(0, y)].set_symbol("│").set_style(border);
            buffer[(width - 1, y)].set_symbol("│").set_style(border);
        }
        buffer[(0, 0)].set_symbol("┌").set_style(border);
        buffer[(width - 1, 0)].set_symbol("┐").set_style(border);
        buffer[(0, height - 1)].set_symbol("└").set_style(border);
        buffer[(width - 1, height - 1)]
            .set_symbol("┘")
            .set_style(border);
    }

    if height > 2 && width > 4 {
        buffer.set_stringn(
            2,
            1,
            "Ratatui renderer comparison",
            usize::from(width.saturating_sub(4)),
            Style::default()
                .fg(RatatuiColor::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }
    for y in 3..height.saturating_sub(2) {
        let label = format!("worker-{y:02}  load ");
        buffer.set_stringn(
            2,
            y,
            label,
            usize::from(width.saturating_sub(4)),
            Style::default().fg(RatatuiColor::White),
        );
        let bar_start = 19_u16.min(width.saturating_sub(2));
        let bar_width = width.saturating_sub(bar_start + 3);
        for x in 0..bar_width {
            let filled = (u64::from(x) + u64::from(y) * 3) % 17 < 10;
            buffer[(bar_start + x, y)]
                .set_symbol(if filled { "█" } else { "░" })
                .set_fg(if filled {
                    RatatuiColor::Green
                } else {
                    RatatuiColor::DarkGray
                });
        }
    }

    if animate && width > 16 && height > 2 {
        let counter = format!("{:010}", frame_index % 10_000_000_000);
        buffer.set_stringn(
            width.saturating_sub(12),
            1,
            counter,
            10,
            Style::default().fg(RatatuiColor::LightMagenta),
        );
        let x =
            2 + u16::try_from(frame_index % u64::from(width.saturating_sub(4).max(1))).unwrap_or(0);
        let y = height.saturating_sub(2);
        buffer[(x.min(width.saturating_sub(2)), y)]
            .set_symbol("◆")
            .set_fg(RatatuiColor::LightYellow);
    }
}

fn render_dense_ascii(buffer: &mut Buffer, frame_index: u64) {
    const PRINTABLE: &[u8] = b"!#$%&()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_abcdefghijklmnopqrstuvwxyz{|}~";
    let width = buffer.area.width;
    let height = buffer.area.height;
    for y in 0..height {
        for x in 0..width {
            let index =
                (u64::from(x) * 17 + u64::from(y) * 31 + frame_index) % PRINTABLE.len() as u64;
            let printable_index = usize::try_from(index).expect("index is modulo slice length");
            let character = char::from(PRINTABLE[printable_index]);
            let frame_phase = u32::try_from(frame_index & 0xff).expect("value is masked");
            let shade = u8::try_from((u32::from(x) * 3 + u32::from(y) * 5 + frame_phase) & 0xff)
                .expect("value is masked");
            buffer[(x, y)]
                .set_char(character)
                .set_fg(RatatuiColor::Rgb(
                    255_u8.saturating_sub(shade / 2),
                    shade,
                    180,
                ))
                .set_bg(RatatuiColor::Rgb(4, 7, 12));
        }
    }
}

fn render_dense_styled(buffer: &mut Buffer, frame_index: u64) {
    const SYMBOLS: [&str; 8] = ["█", "▓", "▒", "░", "▀", "▄", "▌", "▐"];
    let width = buffer.area.width;
    let height = buffer.area.height;
    for y in 0..height {
        for x in 0..width {
            let seed = u64::from(x) * 0x9e37 + u64::from(y) * 0x85eb + frame_index * 0xc2b2;
            let r = u8::try_from(seed & 0xff).expect("value is masked");
            let g = u8::try_from((seed >> 8) & 0xff).expect("value is masked");
            let b = u8::try_from((seed >> 16) & 0xff).expect("value is masked");
            let mut style = Style::default()
                .fg(RatatuiColor::Rgb(r, g, b))
                .bg(RatatuiColor::Rgb(b / 5, r / 5, g / 5));
            if seed & 1 != 0 {
                style = style.add_modifier(Modifier::BOLD);
            }
            if seed & 2 != 0 {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if seed & 4 != 0 {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            let symbol_index = usize::try_from((seed >> 3) % SYMBOLS.len() as u64)
                .expect("index is modulo slice length");
            buffer[(x, y)]
                .set_symbol(SYMBOLS[symbol_index])
                .set_style(style);
        }
    }
}

fn render_unicode(buffer: &mut Buffer, frame_index: u64) {
    const ROWS: [&str; 8] = [
        "ASCII terminal 0123456789 ┌─┬─┐ ├─┼─┤ └─┴─┘",
        "wide: 日本語 中文 한국어 カタカナ 终端渲染",
        "combining: e\u{301} a\u{308} o\u{302} n\u{303} Z\u{335}",
        "emoji: 😀 🚀 ✨ 🔥 👩\u{200d}💻 🧑🏽\u{200d}🚀 🏳️\u{200d}🌈",
        "blocks: █ ▉ ▊ ▋ ▌ ▍ ▎ ▏ ▀ ▄ ▖ ▗ ▘ ▙",
        "braille: ⠀ ⠁ ⠃ ⠇ ⡇ ⣇ ⣧ ⣷ ⣿ ⢀ ⢠ ⢰",
        "double: ╔══╦══╗ ║  ║  ║ ╠══╬══╣ ╚══╩══╝",
        "scripts: العربية नमस्ते עברית Ελληνικά Кириллица",
    ];
    let width = buffer.area.width;
    let height = buffer.area.height;
    let frame_phase =
        usize::try_from(frame_index % ROWS.len() as u64).expect("phase is modulo row count");
    for y in 0..height {
        for x in 0..width {
            buffer[(x, y)]
                .set_symbol(" ")
                .set_bg(RatatuiColor::Rgb(6, 9, 16));
        }
        let row = ROWS[(usize::from(y) + frame_phase) % ROWS.len()];
        let color = hue_color(u64::from(y) * 29 + frame_index);
        buffer.set_stringn(
            0,
            y,
            row,
            usize::from(width),
            Style::default().fg(color).bg(RatatuiColor::Rgb(6, 9, 16)),
        );
    }
}

fn hue_color(seed: u64) -> RatatuiColor {
    let phase = u8::try_from(seed % 6).expect("phase is modulo six");
    let offset = u8::try_from((seed * 37) & 0xff).expect("value is masked");
    let low = 64_u8.saturating_add(offset / 3);
    match phase {
        0 => RatatuiColor::Rgb(255, low, 96),
        1 => RatatuiColor::Rgb(220, 255, low),
        2 => RatatuiColor::Rgb(low, 255, 128),
        3 => RatatuiColor::Rgb(low, 220, 255),
        4 => RatatuiColor::Rgb(128, low, 255),
        _ => RatatuiColor::Rgb(255, 96, low),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn every_workload_renders_at_tiny_and_normal_sizes() {
        for (cols, rows) in [(1, 1), (2, 2), (80, 24)] {
            for workload in [
                Workload::Static,
                Workload::Sparse,
                Workload::DenseAscii,
                Workload::DenseStyled,
                Workload::Unicode,
            ] {
                let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
                terminal
                    .draw(|frame| render_workload(frame, workload, 17))
                    .unwrap();
            }
        }
    }

    #[test]
    fn static_is_stable_and_sparse_changes() {
        let render = |workload, frame_index| {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| render_workload(frame, workload, frame_index))
                .unwrap();
            terminal.backend().buffer().clone()
        };
        assert_eq!(render(Workload::Static, 1), render(Workload::Static, 99));
        assert_ne!(render(Workload::Sparse, 1), render(Workload::Sparse, 2));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&[1, 2, 3, 4], 50), 2);
        assert_eq!(percentile(&[1, 2, 3, 4], 95), 4);
    }

    #[test]
    fn linear_capture_conversion_preserves_alpha_and_trailing_bytes() {
        let mut rgba = [0, 55, 255, 17, 9];
        linear_rgba8_to_srgb(&mut rgba);
        assert_eq!(rgba, [0, 128, 255, 17, 9]);
    }
}
