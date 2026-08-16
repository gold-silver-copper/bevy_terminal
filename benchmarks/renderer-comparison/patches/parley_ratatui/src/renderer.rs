use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;
use ratatui::style::{Modifier, Style};
use std::collections::VecDeque;
use std::sync::mpsc;
use unicode_width::UnicodeWidthStr;
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Brush, Color as PenikoColor, Fill};
use vello::{
    AaConfig, Glyph, RenderParams, Renderer, RendererOptions as VelloRendererOptions, Scene, wgpu,
};

use crate::color::Rgba;
use crate::color::Theme;
use crate::text::{BundledFont, FontOptions, FontStack, TextMetrics, TextStyle, TextSystem};

#[derive(Debug)]
pub enum RenderError {
    CreateRenderer(vello::Error),
    Render(vello::Error),
    ReadbackFormat(wgpu::TextureFormat),
    CreateReadback(wgpu::BufferAsyncError),
    Poll(wgpu::PollError),
    ReadbackCanceled,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateRenderer(err) => write!(f, "failed to create vello renderer: {err}"),
            Self::Render(err) => write!(f, "failed to render vello scene: {err}"),
            Self::ReadbackFormat(format) => {
                write!(f, "unsupported texture readback format: {format:?}")
            }
            Self::CreateReadback(err) => write!(f, "failed to map texture readback buffer: {err}"),
            Self::Poll(err) => write!(f, "failed to poll wgpu device for readback: {err}"),
            Self::ReadbackCanceled => write!(f, "texture readback callback was canceled"),
        }
    }
}

impl std::error::Error for RenderError {}

pub struct TextureTarget {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
}

impl TextureTarget {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        label: impl Into<Option<&'static str>>,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: label.into(),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width: width.max(1),
            height: height.max(1),
            format,
        }
    }
}

pub struct TerminalRenderer {
    text: TextSystem,
    theme: Theme,
    scene: Scene,
    cells: Vec<ResolvedCellStyle>,
}

#[derive(Clone, Copy, Debug, Default)]
struct BlinkState {
    hide_slow: bool,
    hide_rapid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedCellStyle {
    fg: Rgba,
    bg: Rgba,
    fg_color: vello::peniko::Color,
    bg_color: vello::peniko::Color,
    modifiers: Modifier,
    text_style: TextStyle,
    display_width: u16,
}

pub struct GpuRenderer {
    renderer: Renderer,
    options: GpuRendererOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuRendererOptions {
    pub antialiasing_method: AaConfig,
}

impl Default for GpuRendererOptions {
    fn default() -> Self {
        Self {
            antialiasing_method: AaConfig::Msaa8,
        }
    }
}

/// Reusable blocking texture readback state.
///
/// This keeps the staging buffer alive across frames and writes into caller-owned
/// output storage. It still waits for the GPU before returning, so interactive
/// applications should prefer [`AsyncTextureReadback`] when they can tolerate a
/// one-frame pipeline.
pub struct TextureReadback {
    buffer: Option<ReadbackBuffer>,
}

/// Pipelined texture readback state for CPU-owned image bridges.
///
/// This is useful for integrations such as the examples in this crate, where
/// Vello renders on its own `wgpu::Device` and Bevy expects CPU-side
/// `Image::data`. When Vello and the destination renderer cannot share the same
/// `wgpu::Texture` directly through their public APIs, this keeps the required
/// copy asynchronous and reuses staging buffers.
pub struct AsyncTextureReadback {
    pending: VecDeque<PendingReadback>,
    reusable: Vec<ReadbackBuffer>,
}

struct PendingReadback {
    readback: ReadbackBuffer,
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

struct ReadbackBuffer {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
}

impl GpuRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        Self::new_with_options(device, GpuRendererOptions::default())
    }

    pub fn new_with_options(
        device: &wgpu::Device,
        options: GpuRendererOptions,
    ) -> Result<Self, RenderError> {
        let renderer = Renderer::new(device, VelloRendererOptions::default())
            .map_err(RenderError::CreateRenderer)?;
        Ok(Self { renderer, options })
    }

    pub fn options(&self) -> GpuRendererOptions {
        self.options
    }

    pub fn set_antialiasing_method(&mut self, antialiasing_method: AaConfig) {
        self.options.antialiasing_method = antialiasing_method;
    }

    pub fn render_scene_to_texture_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        base_color: PenikoColor,
        scene: &Scene,
    ) -> Result<(), RenderError> {
        self.renderer
            .render_to_texture(
                device,
                queue,
                scene,
                texture_view,
                &RenderParams {
                    base_color,
                    width,
                    height,
                    antialiasing_method: self.options.antialiasing_method,
                },
            )
            .map_err(RenderError::Render)
    }

    pub fn render_to_texture(
        &mut self,
        terminal: &mut TerminalRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
    ) -> Result<(), RenderError> {
        let base_color = terminal.theme.background.to_peniko();
        let scene = terminal.build_scene(buffer, cursor, cursor_visible);
        self.render_scene_to_texture_view(
            device,
            queue,
            &target.view,
            target.width,
            target.height,
            base_color,
            scene,
        )
    }

    pub fn render_to_texture_with_elapsed(
        &mut self,
        terminal: &mut TerminalRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        elapsed_seconds: f32,
    ) -> Result<(), RenderError> {
        let base_color = terminal.theme.background.to_peniko();
        let scene =
            terminal.build_scene_with_elapsed(buffer, cursor, cursor_visible, elapsed_seconds);
        self.render_scene_to_texture_view(
            device,
            queue,
            &target.view,
            target.width,
            target.height,
            base_color,
            scene,
        )
    }

    pub fn render_to_rgba8(
        &mut self,
        terminal: &mut TerminalRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
    ) -> Result<Vec<u8>, RenderError> {
        self.render_to_texture(
            terminal,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
        )?;
        read_texture_to_rgba8(device, queue, target)
    }

    pub fn render_to_rgba8_into(
        &mut self,
        terminal: &mut TerminalRenderer,
        readback: &mut TextureReadback,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        rgba: &mut Vec<u8>,
    ) -> Result<(), RenderError> {
        self.render_to_texture(
            terminal,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
        )?;
        readback.read_texture_to_rgba8_into(device, queue, target, rgba)
    }

    pub fn render_to_rgba8_with_elapsed(
        &mut self,
        terminal: &mut TerminalRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        elapsed_seconds: f32,
    ) -> Result<Vec<u8>, RenderError> {
        self.render_to_texture_with_elapsed(
            terminal,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
            elapsed_seconds,
        )?;
        read_texture_to_rgba8(device, queue, target)
    }

    pub fn render_to_rgba8_with_elapsed_into(
        &mut self,
        terminal: &mut TerminalRenderer,
        readback: &mut TextureReadback,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        elapsed_seconds: f32,
        rgba: &mut Vec<u8>,
    ) -> Result<(), RenderError> {
        self.render_to_texture_with_elapsed(
            terminal,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
            elapsed_seconds,
        )?;
        readback.read_texture_to_rgba8_into(device, queue, target, rgba)
    }
}

impl TerminalRenderer {
    pub fn new(font: FontOptions, theme: Theme) -> Self {
        Self {
            text: TextSystem::new(font),
            theme,
            scene: Scene::new(),
            cells: Vec::new(),
        }
    }

    /// Creates a renderer from logical font options and a physical render scale.
    ///
    /// The resulting renderer draws into physical-pixel textures. Use
    /// [`Self::logical_metrics`] with the same scale for window layout.
    pub fn new_scaled(font: FontOptions, theme: Theme, render_scale: f32) -> Self {
        Self::new(font.scaled(render_scale), theme)
    }

    pub fn metrics(&self) -> TextMetrics {
        self.text.metrics()
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn logical_metrics(&self, render_scale: f32) -> TextMetrics {
        self.metrics().logical(render_scale)
    }

    pub fn register_font(&mut self, font: BundledFont) -> usize {
        self.text.register_font(font)
    }

    pub fn register_font_data(&mut self, data: &'static [u8]) -> usize {
        self.text.register_font_data(data)
    }

    pub fn register_font_family(
        &mut self,
        family_name: impl Into<String>,
        data: &'static [u8],
    ) -> usize {
        self.text.register_font_family(family_name, data)
    }

    pub fn set_font_family(&mut self, family: impl Into<String>) {
        self.text.set_family(family);
    }

    /// Replaces the renderer's font variant and fallback stack.
    ///
    /// This clears cached layouts and recomputes text metrics.
    pub fn set_font_stack(&mut self, fonts: FontStack) {
        self.text.set_font_stack(fonts);
    }

    pub fn texture_size_for_buffer(&self, buffer: &Buffer) -> (u32, u32) {
        let metrics = self.metrics();
        (
            (f32::from(buffer.area.width) * metrics.cell_width)
                .ceil()
                .max(1.0) as u32,
            (f32::from(buffer.area.height) * metrics.cell_height)
                .ceil()
                .max(1.0) as u32,
        )
    }

    pub fn build_scene(
        &mut self,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
    ) -> &Scene {
        self.build_scene_inner(buffer, cursor, cursor_visible, BlinkState::default())
    }

    pub fn build_scene_with_elapsed(
        &mut self,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        elapsed_seconds: f32,
    ) -> &Scene {
        self.build_scene_inner(
            buffer,
            cursor,
            cursor_visible,
            BlinkState::from_elapsed(elapsed_seconds),
        )
    }

    pub fn replace_scene(&mut self, scene: Scene) -> Scene {
        std::mem::replace(&mut self.scene, scene)
    }

    fn build_scene_inner(
        &mut self,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        blink: BlinkState,
    ) -> &Scene {
        self.scene.reset();

        let metrics = self.text.metrics();
        let width = f32::from(buffer.area.width) * metrics.cell_width;
        let height = f32::from(buffer.area.height) * metrics.cell_height;
        fill_rect(
            &mut self.scene,
            0.0,
            0.0,
            width,
            height,
            self.theme.background.to_peniko(),
        );

        self.resolve_buffer_cell_styles(buffer, blink);
        self.paint_backgrounds(buffer);

        let width = buffer.area.width as usize;
        for y in 0..buffer.area.height {
            let row_start = y as usize * width;
            let row = &buffer.content()[row_start..row_start + width];
            for (x, cell) in row.iter().enumerate() {
                let style = self.cells[row_start + x];
                self.paint_cell_text_and_decorations(x as u16, y, cell, style);
            }
        }

        if cursor_visible
            && let Some(position) = cursor
            && position.x < buffer.area.width
            && position.y < buffer.area.height
        {
            let x0 = snap_cell_edge(f32::from(position.x), metrics.cell_width);
            let x1 = snap_cell_edge(f32::from(position.x) + 1.0, metrics.cell_width);
            let y0 = snap_cell_edge(f32::from(position.y), metrics.cell_height);
            let y1 = snap_cell_edge(f32::from(position.y) + 1.0, metrics.cell_height);
            fill_rect(
                &mut self.scene,
                x0,
                y0,
                x1 - x0,
                y1 - y0,
                self.theme.cursor.to_peniko(),
            );
        }

        &self.scene
    }

    pub fn render_to_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
    ) -> Result<(), RenderError> {
        GpuRenderer::new(device)?.render_to_texture(
            self,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
        )
    }

    pub fn render_to_rgba8(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
    ) -> Result<Vec<u8>, RenderError> {
        GpuRenderer::new(device)?.render_to_rgba8(
            self,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
        )
    }

    pub fn render_to_rgba8_into(
        &mut self,
        readback: &mut TextureReadback,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        buffer: &Buffer,
        cursor: Option<Position>,
        cursor_visible: bool,
        rgba: &mut Vec<u8>,
    ) -> Result<(), RenderError> {
        GpuRenderer::new(device)?.render_to_rgba8_into(
            self,
            readback,
            device,
            queue,
            target,
            buffer,
            cursor,
            cursor_visible,
            rgba,
        )
    }

    fn paint_backgrounds(&mut self, buffer: &Buffer) {
        let metrics = self.text.metrics();
        let width = buffer.area.width as usize;

        for y in 0..buffer.area.height {
            let row_start = y as usize * width;
            let y0 = snap_cell_edge(f32::from(y), metrics.cell_height);
            let y1 = snap_cell_edge(f32::from(y) + 1.0, metrics.cell_height);
            let mut run_start = 0usize;
            while run_start < width {
                let color = self.cells[row_start + run_start].bg_color;
                let mut run_end = run_start + 1;
                while run_end < width && self.cells[row_start + run_end].bg_color == color {
                    run_end += 1;
                }

                let x0 = snap_cell_edge(run_start as f32, metrics.cell_width);
                let x1 = snap_cell_edge(run_end as f32, metrics.cell_width);
                fill_rect(&mut self.scene, x0, y0, x1 - x0, y1 - y0, color);
                run_start = run_end;
            }
        }
    }

    fn paint_cell_text_and_decorations(
        &mut self,
        x: u16,
        y: u16,
        cell: &Cell,
        resolved: ResolvedCellStyle,
    ) {
        let metrics = self.text.metrics();
        let x_px = f32::from(x) * metrics.cell_width;
        let y_px = f32::from(y) * metrics.cell_height;
        let cell_width = metrics.cell_width * f32::from(resolved.display_width);

        let symbol = cell.symbol();
        let draws_visible_foreground = resolved.fg != resolved.bg;
        if draws_visible_foreground && !resolved.modifiers.contains(Modifier::HIDDEN) {
            if let Some(block) = single_char(symbol).and_then(block_element_fill) {
                // Block and quadrant elements are rendered as geometric fills
                // covering the exact cell region. The font's glyphs only fill the
                // em box, not the taller line-height cell, so shaping them leaves a
                // background strip at the cell edges (visible as seams between
                // half-block "pixels"). Snapping the fills to the cell grid keeps
                // them pixel-perfect and tiling with neighbours.
                self.paint_block_element(x, y, resolved, block);
            } else if should_shape_text(symbol) {
                let layout = self.text.shape(symbol, resolved.text_style);
                paint_layout(
                    &mut self.scene,
                    &layout,
                    x_px + metrics.glyph_offset_x,
                    y_px + metrics.glyph_offset_y,
                    resolved.fg_color,
                );
            }
        }

        if draws_visible_foreground && resolved.modifiers.contains(Modifier::UNDERLINED) {
            let (line_y, thickness) = decoration_geometry(
                metrics,
                y_px,
                metrics.underline_position,
                metrics.underline_thickness,
            );
            fill_rect(
                &mut self.scene,
                x_px,
                line_y,
                cell_width,
                thickness,
                resolved.fg_color,
            );
        }
        if draws_visible_foreground && resolved.modifiers.contains(Modifier::CROSSED_OUT) {
            let (line_y, thickness) = decoration_geometry(
                metrics,
                y_px,
                metrics.strikeout_position,
                metrics.strikeout_thickness,
            );
            fill_rect(
                &mut self.scene,
                x_px,
                line_y,
                cell_width,
                thickness,
                resolved.fg_color,
            );
        }
    }

    fn paint_block_element(
        &mut self,
        x: u16,
        y: u16,
        resolved: ResolvedCellStyle,
        block: BlockElementFill,
    ) {
        let metrics = self.text.metrics();
        let column = f32::from(x);
        let row = f32::from(y);
        let span = f32::from(resolved.display_width);
        for &[fx0, fy0, fx1, fy1] in &block.rects[..block.count] {
            let x0 = snap_cell_edge(column + fx0 * span, metrics.cell_width);
            let x1 = snap_cell_edge(column + fx1 * span, metrics.cell_width);
            let y0 = snap_cell_edge(row + fy0, metrics.cell_height);
            let y1 = snap_cell_edge(row + fy1, metrics.cell_height);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            fill_rect(&mut self.scene, x0, y0, x1 - x0, y1 - y0, resolved.fg_color);
        }
    }

    fn resolve_buffer_cell_styles(&mut self, buffer: &Buffer, blink: BlinkState) {
        let width = buffer.area.width as usize;
        let height = buffer.area.height as usize;
        let cell_count = width * height;
        self.cells.clear();
        self.cells.reserve(cell_count);

        for y in 0..height {
            let row = &buffer.content()[y * width..(y + 1) * width];
            for (x, cell) in row.iter().enumerate() {
                let mut style = self.resolve_cell_style(cell.style(), cell.symbol(), blink);

                if cell.symbol() == " "
                    && cell.style().bg.is_none()
                    && x > 0
                    && display_width(row[x - 1].symbol()) > 1
                {
                    let owner_style = self.cells[y * width + x - 1];
                    style.fg = owner_style.fg;
                    style.bg = owner_style.bg;
                    style.fg_color = owner_style.fg_color;
                    style.bg_color = owner_style.bg_color;
                    style.display_width = 1;
                }

                self.cells.push(style);
            }
        }
    }

    fn resolve_cell_style(
        &self,
        style: Style,
        symbol: &str,
        blink: BlinkState,
    ) -> ResolvedCellStyle {
        let mut fg = self.theme.foreground(style);
        let mut bg = self.theme.background(style);
        if style.add_modifier.contains(Modifier::REVERSED) {
            std::mem::swap(&mut fg, &mut bg);
        }

        if style.add_modifier.contains(Modifier::HIDDEN)
            || (style.add_modifier.contains(Modifier::SLOW_BLINK) && blink.hide_slow)
            || (style.add_modifier.contains(Modifier::RAPID_BLINK) && blink.hide_rapid)
        {
            fg = bg;
        }

        ResolvedCellStyle {
            fg,
            bg,
            fg_color: fg.to_peniko(),
            bg_color: bg.to_peniko(),
            modifiers: style.add_modifier,
            text_style: TextStyle {
                bold: style.add_modifier.contains(Modifier::BOLD),
                italic: style.add_modifier.contains(Modifier::ITALIC),
            },
            display_width: display_width(symbol).clamp(1, 2),
        }
    }
}

impl TextureReadback {
    pub fn new() -> Self {
        Self { buffer: None }
    }

    pub fn read_texture_to_rgba8_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
        rgba: &mut Vec<u8>,
    ) -> Result<(), RenderError> {
        validate_readback_target(target)?;
        let readback = self
            .buffer
            .take()
            .filter(|readback| readback.matches(target))
            .unwrap_or_else(|| ReadbackBuffer::new(device, target));

        readback.copy_from_texture(device, queue, target);
        readback.map_blocking(device)?;
        readback.copy_rgba8_into(rgba);
        readback.buffer.unmap();
        self.buffer = Some(readback);
        Ok(())
    }
}

impl Default for TextureReadback {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncTextureReadback {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            reusable: Vec::new(),
        }
    }

    /// Queue a copy from `target` into a reusable staging buffer.
    ///
    /// Returns `Ok(false)` when the small readback pipeline is already full; in
    /// that case the caller can keep displaying the most recent completed frame.
    pub fn submit(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
    ) -> Result<bool, RenderError> {
        validate_readback_target(target)?;
        if self.pending.len() >= 2 {
            return Ok(false);
        }

        let readback = self
            .reusable
            .iter()
            .position(|readback| readback.matches(target))
            .map(|index| self.reusable.swap_remove(index))
            .unwrap_or_else(|| ReadbackBuffer::new(device, target));
        let receiver = readback.copy_and_map(device, queue, target);
        self.pending
            .push_back(PendingReadback { readback, receiver });
        Ok(true)
    }

    /// Poll for the oldest completed readback and copy it into `rgba`.
    ///
    /// Returns `Ok(true)` when `rgba` was updated. This method uses
    /// non-blocking device polling and does not wait for GPU completion.
    pub fn try_read_rgba8_into(
        &mut self,
        device: &wgpu::Device,
        rgba: &mut Vec<u8>,
    ) -> Result<bool, RenderError> {
        let Some(pending) = self.pending.front() else {
            return Ok(false);
        };

        match pending.receiver.try_recv() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(RenderError::CreateReadback(err)),
            Err(mpsc::TryRecvError::Empty) => {
                device
                    .poll(wgpu::PollType::Poll)
                    .map_err(RenderError::Poll)?;
                match pending.receiver.try_recv() {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => return Err(RenderError::CreateReadback(err)),
                    Err(mpsc::TryRecvError::Empty) => return Ok(false),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err(RenderError::ReadbackCanceled);
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => return Err(RenderError::ReadbackCanceled),
        }

        let pending = self.pending.pop_front().expect("pending readback");
        pending.readback.copy_rgba8_into(rgba);
        pending.readback.buffer.unmap();
        self.reusable.push(pending.readback);
        Ok(true)
    }
}

impl Default for AsyncTextureReadback {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadbackBuffer {
    fn new(device: &wgpu::Device, target: &TextureTarget) -> Self {
        let bytes_per_pixel = 4;
        let unpadded_bytes_per_row = target.width * bytes_per_pixel;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size = u64::from(padded_bytes_per_row) * u64::from(target.height);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("parley_ratatui.readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            buffer,
            width: target.width,
            height: target.height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
        }
    }

    fn matches(&self, target: &TextureTarget) -> bool {
        self.width == target.width && self.height == target.height
    }

    fn copy_and_map(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
    ) -> mpsc::Receiver<Result<(), wgpu::BufferAsyncError>> {
        self.copy_from_texture(device, queue, target);
        let slice = self.buffer.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        receiver
    }

    fn copy_from_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &TextureTarget,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("parley_ratatui.readback_encoder"),
        });
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(target.height),
                },
            },
            wgpu::Extent3d {
                width: target.width,
                height: target.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
    }

    fn map_blocking(&self, device: &wgpu::Device) -> Result<(), RenderError> {
        let slice = self.buffer.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(RenderError::Poll)?;
        receiver
            .recv()
            .map_err(|_| RenderError::ReadbackCanceled)?
            .map_err(RenderError::CreateReadback)
    }

    fn copy_rgba8_into(&self, rgba: &mut Vec<u8>) {
        let mapped = self.buffer.slice(..).get_mapped_range();
        let target_len = (self.width * self.height * 4) as usize;
        rgba.resize(target_len, 0);
        for y in 0..self.height as usize {
            let src_start = y * self.padded_bytes_per_row as usize;
            let src_end = src_start + self.unpadded_bytes_per_row as usize;
            let dst_start = y * self.unpadded_bytes_per_row as usize;
            let dst_end = dst_start + self.unpadded_bytes_per_row as usize;
            rgba[dst_start..dst_end].copy_from_slice(&mapped[src_start..src_end]);
        }
        drop(mapped);
    }
}

impl BlinkState {
    fn from_elapsed(elapsed_seconds: f32) -> Self {
        Self {
            hide_slow: blink_hidden(elapsed_seconds, 0.5),
            hide_rapid: blink_hidden(elapsed_seconds, 0.25),
        }
    }
}

fn paint_layout(
    scene: &mut Scene,
    layout: &parley::Layout<()>,
    x: f32,
    y: f32,
    color: vello::peniko::Color,
) {
    let brush = Brush::Solid(color);
    let transform = Affine::translate((f64::from(x), f64::from(y)));
    for line in layout.lines() {
        for item in line.items() {
            let parley::layout::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };

            let run = glyph_run.run();
            let font = run.font();
            let font_size = run.font_size();
            let mut x = glyph_run.offset();
            let y = glyph_run.baseline();
            scene
                .draw_glyphs(font)
                .brush(&brush)
                .hint(false)
                .transform(transform)
                .font_size(font_size)
                .normalized_coords(run.normalized_coords())
                .draw(
                    Fill::NonZero,
                    glyph_run.glyphs().map(|glyph| {
                        let gx = x + glyph.x;
                        let gy = y - glyph.y;
                        x += glyph.advance;
                        Glyph {
                            id: glyph.id as u32,
                            x: gx,
                            y: gy,
                        }
                    }),
                );
        }
    }
}

fn fill_rect(
    scene: &mut Scene,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: vello::peniko::Color,
) {
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color,
        None,
        &Rect::new(
            f64::from(x),
            f64::from(y),
            f64::from(x + width),
            f64::from(y + height),
        ),
    );
}

/// Snaps a cell-grid edge (a column or row index times the measured cell size)
/// to the physical pixel grid.
///
/// Cell backgrounds are emitted as one filled rectangle per row/run and
/// composited source-over. When [`CellQuantization::Fractional`] leaves the cell
/// size non-integer, two adjacent rectangles share a sub-pixel edge; Vello
/// antialiases each independently, so their partial coverages combine to less
/// than full opacity and the base background bleeds through as a thin seam that
/// drifts with the accumulating fractional phase. Snapping every edge to a whole
/// pixel makes adjacent cells share an identical integer boundary, so the fills
/// tile exactly with no seam. For integer cell sizes this is a no-op.
fn snap_cell_edge(index: f32, cell_size: f32) -> f32 {
    (index * cell_size).round()
}

/// Geometric coverage for a block/quadrant element, in unit cell coordinates
/// (`x` rightward, `y` downward, both in `0.0..=1.0`).
struct BlockElementFill {
    rects: [[f32; 4]; 2],
    count: usize,
}

fn single_char(symbol: &str) -> Option<char> {
    let mut chars = symbol.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

/// Maps the Unicode block and quadrant elements with exact rectangular coverage
/// (the solid blocks, eighths, halves, and quadrants in `U+2580..=U+259F`) to
/// that coverage.
///
/// Terminals draw these as filled geometry rather than font glyphs so that
/// half-block "pixels" and box fills tile the cell grid exactly; the font glyphs
/// only cover the em box and leave seams at the cell edges. The shade characters
/// `░▒▓` are dithered patterns rather than exact fills, so they are left to the
/// font shaper.
fn block_element_fill(ch: char) -> Option<BlockElementFill> {
    const E: f32 = 1.0 / 8.0;
    let solid = |rect: [f32; 4]| BlockElementFill {
        rects: [rect, [0.0; 4]],
        count: 1,
    };
    let pair = |a: [f32; 4], b: [f32; 4]| BlockElementFill {
        rects: [a, b],
        count: 2,
    };
    Some(match ch {
        '\u{2588}' => solid([0.0, 0.0, 1.0, 1.0]), // █ full block
        // Lower eighths ▁▂▃▅▆▇ and halves ▀▄
        '\u{2580}' => solid([0.0, 0.0, 1.0, 0.5]), // ▀ upper half
        '\u{2584}' => solid([0.0, 0.5, 1.0, 1.0]), // ▄ lower half
        '\u{2581}' => solid([0.0, 7.0 * E, 1.0, 1.0]),
        '\u{2582}' => solid([0.0, 6.0 * E, 1.0, 1.0]),
        '\u{2583}' => solid([0.0, 5.0 * E, 1.0, 1.0]),
        '\u{2585}' => solid([0.0, 3.0 * E, 1.0, 1.0]),
        '\u{2586}' => solid([0.0, 2.0 * E, 1.0, 1.0]),
        '\u{2587}' => solid([0.0, 1.0 * E, 1.0, 1.0]),
        '\u{2594}' => solid([0.0, 0.0, 1.0, E]), // ▔ upper eighth
        // Left eighths ▏▎▍▉▊▋ and halves ▌▐
        '\u{258C}' => solid([0.0, 0.0, 0.5, 1.0]), // ▌ left half
        '\u{2590}' => solid([0.5, 0.0, 1.0, 1.0]), // ▐ right half
        '\u{2589}' => solid([0.0, 0.0, 7.0 * E, 1.0]),
        '\u{258A}' => solid([0.0, 0.0, 6.0 * E, 1.0]),
        '\u{258B}' => solid([0.0, 0.0, 5.0 * E, 1.0]),
        '\u{258D}' => solid([0.0, 0.0, 3.0 * E, 1.0]),
        '\u{258E}' => solid([0.0, 0.0, 2.0 * E, 1.0]),
        '\u{258F}' => solid([0.0, 0.0, 1.0 * E, 1.0]),
        '\u{2595}' => solid([7.0 * E, 0.0, 1.0, 1.0]), // ▕ right eighth
        // Quadrants ▖▗▘▝ and their combinations ▙▚▛▜▞▟
        '\u{2596}' => solid([0.0, 0.5, 0.5, 1.0]), // ▖ lower left
        '\u{2597}' => solid([0.5, 0.5, 1.0, 1.0]), // ▗ lower right
        '\u{2598}' => solid([0.0, 0.0, 0.5, 0.5]), // ▘ upper left
        '\u{259D}' => solid([0.5, 0.0, 1.0, 0.5]), // ▝ upper right
        '\u{2599}' => pair([0.0, 0.0, 0.5, 1.0], [0.5, 0.5, 1.0, 1.0]), // ▙
        '\u{259A}' => pair([0.0, 0.0, 0.5, 0.5], [0.5, 0.5, 1.0, 1.0]), // ▚
        '\u{259B}' => pair([0.0, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0]), // ▛
        '\u{259C}' => pair([0.0, 0.0, 1.0, 0.5], [0.5, 0.5, 1.0, 1.0]), // ▜
        '\u{259E}' => pair([0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0]), // ▞
        '\u{259F}' => pair([0.0, 0.5, 1.0, 1.0], [0.5, 0.0, 1.0, 0.5]), // ▟
        _ => return None,
    })
}

fn should_shape_text(symbol: &str) -> bool {
    symbol.chars().any(|character| !character.is_whitespace())
}

fn display_width(symbol: &str) -> u16 {
    UnicodeWidthStr::width(symbol).max(1).min(u16::MAX as usize) as u16
}

fn decoration_geometry(
    metrics: TextMetrics,
    y_px: f32,
    position: f32,
    thickness: f32,
) -> (f32, f32) {
    let thickness = thickness.round().max(1.0);
    let y = ((y_px + metrics.baseline - position) - thickness / 2.0)
        .round()
        .min(y_px + metrics.cell_height - thickness);
    (y, thickness)
}

fn blink_hidden(elapsed_seconds: f32, half_period_seconds: f32) -> bool {
    ((elapsed_seconds / half_period_seconds).floor() as u64) % 2 == 1
}

fn read_texture_to_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &TextureTarget,
) -> Result<Vec<u8>, RenderError> {
    let mut readback = TextureReadback::new();
    let mut rgba = Vec::new();
    readback.read_texture_to_rgba8_into(device, queue, target, &mut rgba)?;
    Ok(rgba)
}

fn validate_readback_target(target: &TextureTarget) -> Result<(), RenderError> {
    if !matches!(
        target.format,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
    ) {
        return Err(RenderError::ReadbackFormat(target.format));
    }
    Ok(())
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

#[cfg(test)]
mod tests {
    use super::{block_element_fill, snap_cell_edge};

    fn covered_area(ch: char) -> Option<f32> {
        block_element_fill(ch).map(|block| {
            block.rects[..block.count]
                .iter()
                .map(|[x0, y0, x1, y1]| (x1 - x0) * (y1 - y0))
                .sum()
        })
    }

    #[test]
    fn block_elements_cover_exact_cell_fractions() {
        assert_eq!(covered_area('\u{2588}'), Some(1.0)); // █ full
        assert_eq!(covered_area('\u{2580}'), Some(0.5)); // ▀ upper half
        assert_eq!(covered_area('\u{2584}'), Some(0.5)); // ▄ lower half
        assert_eq!(covered_area('\u{258C}'), Some(0.5)); // ▌ left half
        assert_eq!(covered_area('\u{2590}'), Some(0.5)); // ▐ right half
        assert_eq!(covered_area('\u{2596}'), Some(0.25)); // ▖ one quadrant
        assert_eq!(covered_area('\u{259F}'), Some(0.75)); // ▟ three quadrants
        assert_eq!(covered_area('\u{2581}'), Some(0.125)); // ▁ lower eighth

        // ▀ specifically fills the top half so it tiles with the cell below.
        let upper_half = block_element_fill('\u{2580}').unwrap();
        assert_eq!(upper_half.count, 1);
        assert_eq!(upper_half.rects[0], [0.0, 0.0, 1.0, 0.5]);

        // Ordinary text, the dithered shades, and box drawing are left to the
        // font shaper, since they are not exact rectangular fills.
        assert!(block_element_fill('A').is_none());
        assert!(block_element_fill(' ').is_none());
        assert!(block_element_fill('\u{2591}').is_none()); // ░ light shade
        assert!(block_element_fill('\u{2592}').is_none()); // ▒ medium shade
        assert!(block_element_fill('\u{2593}').is_none()); // ▓ dark shade
        assert!(block_element_fill('\u{2500}').is_none()); // ─ box drawing
    }

    #[test]
    fn snap_cell_edge_is_exact_for_integer_cells() {
        let cell = 29.0;
        for index in 0..200 {
            assert_eq!(snap_cell_edge(index as f32, cell), index as f32 * cell);
        }
    }

    #[test]
    fn snap_cell_edge_tiles_fractional_cells_without_gaps_or_overlap() {
        // CellQuantization::Fractional leaves sub-pixel cell sizes. The snapped
        // grid must still cover every pixel exactly once: each edge lands on a
        // whole pixel, edges never go backwards, and adjacent cells abut (the
        // right edge of one cell is the left edge of the next), so no base
        // background can bleed through and no cell is painted twice.
        for &cell in &[55.875_f32, 28.898, 9.6, 17.3333, 100.4999] {
            let mut previous = snap_cell_edge(0.0, cell);
            assert_eq!(previous, 0.0);
            for index in 1..512 {
                let edge = snap_cell_edge(index as f32, cell);
                assert_eq!(edge, edge.round(), "edge must be a whole pixel");
                let span = edge - previous;
                assert!(
                    span >= cell.floor() && span <= cell.ceil(),
                    "cell {index} span {span} not within [{}, {}]",
                    cell.floor(),
                    cell.ceil(),
                );
                previous = edge;
            }
        }
    }
}
