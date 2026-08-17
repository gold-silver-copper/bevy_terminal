use bevy::{
    ecs::schedule::SystemSet,
    prelude::*,
    text::{FontSource, FontStyle, FontWeight, LineHeight},
};

use crate::{
    TerminalSnapshot, TerminalSurface,
    color::{TerminalTheme, dim},
    scene::{StyleFlags, TerminalCell},
};

mod batch;

pub use batch::{
    BevyTerminalPlugin, TerminalBatch, TerminalBatchOutput, TerminalBatchPresentation,
    TerminalBatchRoot, TerminalBatchStats,
};

/// Visual shape used for the terminal cursor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorStyle {
    /// A translucent rectangle covering the entire cell.
    #[default]
    Block,
    /// A two-logical-pixel bar at the cell's left edge.
    Bar,
    /// A two-logical-pixel line at the cell's bottom edge.
    Underline,
}

/// Selects the physical resolution used by the compact batch renderer.
///
/// The retained Bevy UI renderer already participates directly in Bevy's UI
/// scale handling and therefore ignores this setting.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TerminalRenderScale {
    /// Match the primary window's physical-to-logical scale factor when the
    /// terminal is presented through Bevy UI. Headless rendering uses `1.0`.
    #[default]
    Automatic,
    /// Rasterize at an explicit physical-to-logical scale factor.
    ///
    /// Values that are non-finite or less than or equal to zero fall back to
    /// `1.0`; valid values are clamped to `1.0..=8.0`. A custom UI or camera
    /// should use the same scale factor so the resulting texture maps
    /// one-to-one onto physical display pixels.
    Fixed(f32),
}

/// Configuration for converting terminal cells into Bevy-rendered geometry and text.
///
/// `cell_size` is intentionally explicit. Bevy can shape several fallback
/// fonts in one run, so there is no single font metric that is guaranteed to
/// describe every Unicode glyph. Text runs are anchored to cell coordinates to
/// prevent this from causing cumulative drift.
#[derive(Clone, Debug, Resource)]
pub struct TerminalRenderConfig {
    /// Width and height of one terminal cell in Bevy logical pixels.
    pub cell_size: Vec2,
    /// Rasterized font size in logical pixels.
    pub font_size: f32,
    /// Physical raster scale used by the compact batch renderer.
    pub render_scale: TerminalRenderScale,
    /// Bevy font source used for regular text. The generic monospace family enables system
    /// fallback.
    pub font: FontSource,
    /// Optional font source used for bold text.
    ///
    /// When absent, `font` is used with a bold weight request.
    pub bold_font: Option<FontSource>,
    /// Optional font source used for italic text.
    ///
    /// When absent, `font` is used with an italic style request.
    pub italic_font: Option<FontSource>,
    /// Optional font source used for text that is both bold and italic.
    ///
    /// When absent, the bold or italic override is reused before falling back to `font`.
    pub bold_italic_font: Option<FontSource>,
    /// Position of the terminal's top-left corner in logical pixels.
    pub origin: Vec2,
    /// Terminal color theme.
    pub theme: TerminalTheme,
    /// Cursor visual style.
    pub cursor_style: CursorStyle,
    /// Cursor blink frequency. `None` disables cursor blinking.
    pub cursor_blink_hz: Option<f32>,
    /// Slow text blink frequency.
    pub slow_blink_hz: f32,
    /// Rapid text blink frequency.
    pub rapid_blink_hz: f32,
}

impl Default for TerminalRenderConfig {
    fn default() -> Self {
        Self {
            cell_size: Vec2::new(11.0, 20.0),
            font_size: 18.0,
            render_scale: TerminalRenderScale::Automatic,
            font: FontSource::Monospace,
            bold_font: None,
            italic_font: None,
            bold_italic_font: None,
            origin: Vec2::ZERO,
            theme: TerminalTheme::default(),
            cursor_style: CursorStyle::Block,
            cursor_blink_hz: Some(1.0),
            slow_blink_hz: 1.0,
            rapid_blink_hz: 3.0,
        }
    }
}

/// Marker on the root UI node containing the rendered terminal.
#[derive(Component, Debug)]
pub struct TerminalRoot;

/// Public system set for ordering application systems around terminal syncing.
#[derive(Clone, Debug, Hash, Eq, PartialEq, SystemSet)]
pub enum TerminalSystems {
    /// Copies the latest surface state into the active renderer representation.
    Sync,
    /// Applies cursor and text blink phases.
    Blink,
}

/// Per-frame counters for diagnosing terminal synchronization and entity use.
///
/// The counters describe the most recent Bevy update. Applications and
/// benchmarks can read this resource after [`TerminalSystems::Sync`] without
/// enabling tracing or adding another dependency.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct TerminalRenderStats {
    /// Number of times the synchronization system has run.
    pub sync_frames: u64,
    /// Number of synchronization frames that returned before snapshotting.
    pub unchanged_frames: u64,
    /// Rows rebuilt during the most recent synchronization frame.
    pub changed_rows: u32,
    /// Cells cloned into retained state during the most recent sync.
    pub snapshot_cells: u32,
    /// Currently visible Bevy text primitives.
    pub active_text_primitives: u32,
    /// Currently visible Bevy UI rectangle primitives.
    pub active_solid_primitives: u32,
    /// Retained text and rectangle entities, including inactive pool capacity.
    pub pooled_primitives: u32,
    /// New primitive entities spawned during the most recent sync.
    pub spawned_primitives: u32,
}

/// Installs the renderer for one [`TerminalSurface`].
///
/// The application must also install Bevy's UI, text, time, and camera plugins
/// (normally through `DefaultPlugins`) and spawn a suitable camera.
pub struct RetainedBevyTerminalPlugin {
    surface: TerminalSurface,
    config: TerminalRenderConfig,
}

impl RetainedBevyTerminalPlugin {
    /// Creates a renderer using [`TerminalRenderConfig::default`].
    #[must_use]
    pub fn new(surface: TerminalSurface) -> Self {
        Self {
            surface,
            config: TerminalRenderConfig::default(),
        }
    }

    /// Replaces the renderer configuration.
    #[must_use]
    pub fn with_config(mut self, config: TerminalRenderConfig) -> Self {
        self.config = config;
        self
    }
}

impl Plugin for RetainedBevyTerminalPlugin {
    fn build(&self, app: &mut App) {
        self.surface
            .set_cell_size(self.config.cell_size.x, self.config.cell_size.y);

        app.insert_resource(self.surface.clone())
            .insert_resource(self.config.clone())
            .init_resource::<RenderedEntities>()
            .init_resource::<TerminalRenderStats>()
            .configure_sets(
                Update,
                (TerminalSystems::Sync, TerminalSystems::Blink).chain(),
            )
            .add_systems(Update, sync_terminal.in_set(TerminalSystems::Sync))
            .add_systems(Update, animate_blinks.in_set(TerminalSystems::Blink));
    }
}

#[derive(Default, Resource)]
struct RenderedEntities {
    root: Option<Entity>,
    cursor: Option<Entity>,
    rows: Vec<RenderedRow>,
    last_snapshot: Option<TerminalSnapshot>,
}

impl RenderedEntities {
    fn clear(&mut self, commands: &mut Commands) {
        for entity in self
            .rows
            .drain(..)
            .flat_map(RenderedRow::entities)
            .chain(self.cursor.take())
            .chain(self.root.take())
        {
            commands.entity(entity).despawn();
        }
        self.last_snapshot = None;
    }

    fn primitive_counts(&self) -> (usize, usize, usize) {
        let text = self.rows.iter().map(|row| row.text.active).sum();
        let solids = self.rows.iter().map(|row| row.solids.active).sum();
        let pooled = self
            .rows
            .iter()
            .map(|row| row.text.slots.len() + row.solids.slots.len())
            .sum();
        (text, solids, pooled)
    }
}

#[derive(Default)]
struct RenderedRow {
    text: PrimitivePool<TextPrimitive>,
    solids: PrimitivePool<SolidPrimitive>,
}

impl RenderedRow {
    fn entities(self) -> impl Iterator<Item = Entity> {
        self.text
            .slots
            .into_iter()
            .map(|slot| slot.entity)
            .chain(self.solids.slots.into_iter().map(|slot| slot.entity))
    }
}

struct PrimitivePool<T> {
    slots: Vec<PrimitiveSlot<T>>,
    active: usize,
}

impl<T> Default for PrimitivePool<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            active: 0,
        }
    }
}

struct PrimitiveSlot<T> {
    entity: Entity,
    value: T,
}

#[derive(Clone, Debug, PartialEq)]
struct TextPrimitive {
    text: String,
    start: u16,
    width: u16,
    row: u16,
    z_index: i32,
    style: ResolvedStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SolidPrimitive {
    geometry: PixelGeometry,
    color: Color,
    z_index: i32,
    blink_hz: Option<f32>,
}

#[derive(Component)]
struct TerminalBlink {
    visible: Color,
    hidden: Color,
    frequency_hz: f32,
}

#[derive(Component)]
struct TerminalBlinkVisibility {
    frequency_hz: f32,
}

#[derive(Component)]
struct TerminalCursor {
    requested: bool,
}

fn sync_terminal(
    mut commands: Commands,
    surface: Res<TerminalSurface>,
    config: Res<TerminalRenderConfig>,
    mut rendered: ResMut<RenderedEntities>,
    mut stats: ResMut<TerminalRenderStats>,
) {
    stats.sync_frames = stats.sync_frames.wrapping_add(1);
    stats.changed_rows = 0;
    stats.snapshot_cells = 0;
    stats.spawned_primitives = 0;
    if config.is_changed() {
        surface.set_cell_size(config.cell_size.x, config.cell_size.y);
    }
    if rendered.root.is_some()
        && rendered
            .last_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.revision() == surface.revision())
        && !config.is_changed()
    {
        stats.unchanged_frames = stats.unchanged_frames.wrapping_add(1);
        return;
    }

    let old_pooled = rendered.primitive_counts().2;
    let rebuild =
        rendered.root.is_none() || config.is_changed() || rendered.last_snapshot.is_none();
    let mut full_rebuild = rebuild;

    if rebuild {
        let snapshot = surface.snapshot();
        stats.snapshot_cells = u32::try_from(snapshot.cells().len()).unwrap_or(u32::MAX);
        rendered.clear(&mut commands);
        spawn_structure(&mut commands, &snapshot, &config, &mut rendered);
        stats.changed_rows = u32::from(snapshot.size().height);
        rendered.last_snapshot = Some(snapshot);
    } else {
        let mut snapshot = rendered
            .last_snapshot
            .take()
            .expect("the non-rebuild path requires retained state");
        let update = surface.update_snapshot(&mut snapshot);
        stats.snapshot_cells = u32::try_from(update.changed_cells).unwrap_or(u32::MAX);
        if update.resized {
            full_rebuild = true;
            rendered.clear(&mut commands);
            spawn_structure(&mut commands, &snapshot, &config, &mut rendered);
            stats.changed_rows = u32::from(snapshot.size().height);
        } else {
            stats.changed_rows = update_changed_rows(
                &mut commands,
                &snapshot,
                &config,
                &mut rendered,
                &update.changed_rows,
            );
            update_cursor(
                &mut commands,
                &snapshot,
                &config,
                &rendered,
                update.cursor_position_changed,
                update.cursor_visibility_changed,
            );
        }
        rendered.last_snapshot = Some(snapshot);
    }
    let (text, solids, pooled) = rendered.primitive_counts();
    stats.active_text_primitives = u32::try_from(text).unwrap_or(u32::MAX);
    stats.active_solid_primitives = u32::try_from(solids).unwrap_or(u32::MAX);
    stats.pooled_primitives = u32::try_from(pooled).unwrap_or(u32::MAX);
    stats.spawned_primitives = u32::try_from(if full_rebuild {
        pooled
    } else {
        pooled.saturating_sub(old_pooled)
    })
    .unwrap_or(u32::MAX);
}

fn spawn_structure(
    commands: &mut Commands,
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    rendered: &mut RenderedEntities,
) {
    let size = snapshot.size();
    let root = commands
        .spawn((
            TerminalRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(config.origin.x),
                top: px(config.origin.y),
                width: px(f32::from(size.width) * config.cell_size.x),
                height: px(f32::from(size.height) * config.cell_size.y),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(config.theme.background),
        ))
        .id();
    rendered.root = Some(root);
    rendered.rows = (0..size.height).map(|_| RenderedRow::default()).collect();

    for row in 0..size.height {
        rebuild_row(commands, root, row, snapshot, config, rendered);
    }

    let cursor = commands
        .spawn((
            TerminalCursor {
                requested: cursor_should_be_visible(snapshot),
            },
            cursor_node(snapshot, config),
            BackgroundColor(config.theme.cursor),
            ZIndex(i32::MAX),
            cursor_visibility(snapshot),
            ChildOf(root),
        ))
        .id();
    rendered.cursor = Some(cursor);
}

fn update_changed_rows(
    commands: &mut Commands,
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    rendered: &mut RenderedEntities,
    changed_rows: &[u16],
) -> u32 {
    let Some(root) = rendered.root else {
        return 0;
    };
    let changed_row_count = u32::try_from(changed_rows.len()).unwrap_or(u32::MAX);
    for &row in changed_rows {
        rebuild_row(commands, root, row, snapshot, config, rendered);
    }
    changed_row_count
}

fn rebuild_row(
    commands: &mut Commands,
    root: Entity,
    row: u16,
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    rendered: &mut RenderedEntities,
) {
    let row_index = usize::from(row);
    let cells = snapshot.row(row);
    let mut text = Vec::new();
    let mut solids = Vec::new();
    for run in background_runs(cells, &config.theme) {
        if run.color == config.theme.background {
            continue;
        }
        solids.push(SolidPrimitive {
            geometry: PixelGeometry {
                x: f32::from(run.start) * config.cell_size.x,
                y: f32::from(row) * config.cell_size.y,
                width: f32::from(run.width) * config.cell_size.x,
                height: config.cell_size.y,
            },
            color: run.color,
            z_index: 0,
            blink_hz: None,
        });
    }

    for run in text_runs(cells, &config.theme) {
        let z_index = foreground_z_index(row, snapshot.size().width, run.start);
        if let Some(geometry) = block_geometry(&run.text) {
            push_block(&mut solids, &run, row, config, geometry, z_index);
            push_run_decorations(&mut solids, &run, row, config, z_index);
            continue;
        }
        if let Some(mask) = quadrant_mask(&run.text) {
            push_quadrants(&mut solids, &run, row, config, mask, z_index);
            push_run_decorations(&mut solids, &run, row, config, z_index);
            continue;
        }
        if let Some(glyph) = line_glyph(&run.text) {
            push_line_glyph(&mut solids, &run, row, config, glyph, z_index, 1.0);
            push_run_decorations(&mut solids, &run, row, config, z_index);
            continue;
        }
        text.push(TextPrimitive {
            text: run.text.clone(),
            start: run.start,
            width: run.width,
            row,
            z_index,
            style: run.style.clone(),
        });
        push_run_decorations(&mut solids, &run, row, config, z_index);
    }

    let rendered_row = &mut rendered.rows[row_index];
    sync_text_pool(commands, root, config, &mut rendered_row.text, text);
    sync_solid_pool(commands, root, &mut rendered_row.solids, solids);
}

fn foreground_z_index(row: u16, width: u16, column: u16) -> i32 {
    let cell_index = u64::from(row)
        .saturating_mul(u64::from(width))
        .saturating_add(u64::from(column));
    let max_index = u64::try_from((i32::MAX - 2) / 2).expect("positive i32 fits u64");
    let cell_index = cell_index.min(max_index);
    i32::try_from(cell_index * 2 + 1).expect("cell index is clamped to i32")
}

fn push_run_decorations(
    output: &mut Vec<SolidPrimitive>,
    run: &TextRun,
    row: u16,
    config: &TerminalRenderConfig,
    foreground_z_index: i32,
) {
    if run.style.underlined {
        push_decoration(
            output,
            run,
            row,
            config,
            config.cell_size.y - 2.0,
            run.style.underline,
            foreground_z_index.saturating_add(1),
        );
    }
    if run.style.crossed_out {
        push_decoration(
            output,
            run,
            row,
            config,
            config.cell_size.y * 0.55,
            run.style.foreground,
            foreground_z_index.saturating_add(1),
        );
    }
}

fn sync_text_pool(
    commands: &mut Commands,
    root: Entity,
    config: &TerminalRenderConfig,
    pool: &mut PrimitivePool<TextPrimitive>,
    values: Vec<TextPrimitive>,
) {
    let new_active = values.len();
    for (index, value) in values.into_iter().enumerate() {
        if let Some(slot) = pool.slots.get_mut(index) {
            if index >= pool.active {
                let mut entity = commands.entity(slot.entity);
                entity.insert(Visibility::Visible);
                if let Some(frequency_hz) = slot.value.style.blink_hz(config) {
                    entity.insert(TerminalBlink {
                        visible: slot.value.style.foreground,
                        hidden: slot.value.style.background,
                        frequency_hz,
                    });
                }
            }
            update_text_primitive(commands, slot.entity, config, &slot.value, &value);
            slot.value = value;
        } else {
            let entity = spawn_text_primitive(commands, root, config, &value);
            pool.slots.push(PrimitiveSlot { entity, value });
        }
    }
    if new_active < pool.active {
        for slot in &pool.slots[new_active..pool.active] {
            commands
                .entity(slot.entity)
                .remove::<TerminalBlink>()
                .insert(Visibility::Hidden);
        }
    }
    pool.active = new_active;
}

fn spawn_text_primitive(
    commands: &mut Commands,
    root: Entity,
    config: &TerminalRenderConfig,
    value: &TextPrimitive,
) -> Entity {
    let mut entity = commands.spawn((
        Text::new(value.text.clone()),
        text_font(config, &value.style),
        TextColor(value.style.foreground),
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
        LineHeight::Px(config.cell_size.y),
        text_node(config, value),
        ZIndex(value.z_index),
        Visibility::Visible,
        ChildOf(root),
    ));
    if let Some(frequency_hz) = value.style.blink_hz(config) {
        entity.insert(TerminalBlink {
            visible: value.style.foreground,
            hidden: value.style.background,
            frequency_hz,
        });
    }
    entity.id()
}

fn update_text_primitive(
    commands: &mut Commands,
    entity: Entity,
    config: &TerminalRenderConfig,
    old: &TextPrimitive,
    new: &TextPrimitive,
) {
    if old.text != new.text {
        commands.entity(entity).insert(Text::new(new.text.clone()));
    }
    if old.style.foreground != new.style.foreground {
        commands
            .entity(entity)
            .insert(TextColor(new.style.foreground));
    }
    if old.style.bold != new.style.bold || old.style.italic != new.style.italic {
        commands
            .entity(entity)
            .insert(text_font(config, &new.style));
    }
    if old.start != new.start || old.width != new.width || old.row != new.row {
        commands.entity(entity).insert(text_node(config, new));
    }
    if old.z_index != new.z_index {
        commands.entity(entity).insert(ZIndex(new.z_index));
    }

    let old_blink = old.style.blink_hz(config);
    let new_blink = new.style.blink_hz(config);
    if old_blink != new_blink
        || old.style.foreground != new.style.foreground
        || old.style.background != new.style.background
    {
        if let Some(frequency_hz) = new_blink {
            commands.entity(entity).insert(TerminalBlink {
                visible: new.style.foreground,
                hidden: new.style.background,
                frequency_hz,
            });
        } else if old_blink.is_some() {
            commands
                .entity(entity)
                .remove::<TerminalBlink>()
                .insert(Visibility::Visible);
        }
    }
}

fn text_font(config: &TerminalRenderConfig, style: &ResolvedStyle) -> TextFont {
    let font = match (style.bold, style.italic) {
        (true, true) => config
            .bold_italic_font
            .as_ref()
            .or(config.bold_font.as_ref())
            .or(config.italic_font.as_ref()),
        (true, false) => config.bold_font.as_ref(),
        (false, true) => config.italic_font.as_ref(),
        (false, false) => None,
    }
    .unwrap_or(&config.font)
    .clone();
    TextFont {
        font,
        font_size: config.font_size.into(),
        weight: if style.bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..default()
    }
}

fn text_node(config: &TerminalRenderConfig, value: &TextPrimitive) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(f32::from(value.start) * config.cell_size.x),
        top: px(f32::from(value.row) * config.cell_size.y),
        width: px(f32::from(value.width) * config.cell_size.x),
        height: px(config.cell_size.y),
        // Terminal glyphs may have font bearings wider/taller than their cell allocation.
        // Clip fallback and emoji faces so they cannot paint over neighboring cells.
        overflow: Overflow::clip(),
        ..default()
    }
}

fn sync_solid_pool(
    commands: &mut Commands,
    root: Entity,
    pool: &mut PrimitivePool<SolidPrimitive>,
    values: Vec<SolidPrimitive>,
) {
    let new_active = values.len();
    for (index, value) in values.into_iter().enumerate() {
        if let Some(slot) = pool.slots.get_mut(index) {
            if index >= pool.active {
                let mut entity = commands.entity(slot.entity);
                entity.insert(Visibility::Visible);
                if let Some(frequency_hz) = slot.value.blink_hz {
                    entity.insert(TerminalBlinkVisibility { frequency_hz });
                }
            }
            update_solid_primitive(commands, slot.entity, slot.value, value);
            slot.value = value;
        } else {
            let entity = spawn_solid_primitive(commands, root, value);
            pool.slots.push(PrimitiveSlot { entity, value });
        }
    }
    if new_active < pool.active {
        for slot in &pool.slots[new_active..pool.active] {
            commands
                .entity(slot.entity)
                .remove::<TerminalBlinkVisibility>()
                .insert(Visibility::Hidden);
        }
    }
    pool.active = new_active;
}

fn spawn_solid_primitive(commands: &mut Commands, root: Entity, value: SolidPrimitive) -> Entity {
    let mut entity = commands.spawn((
        solid_node(value.geometry),
        BackgroundColor(value.color),
        ZIndex(value.z_index),
        Visibility::Visible,
        ChildOf(root),
    ));
    if let Some(frequency_hz) = value.blink_hz {
        entity.insert(TerminalBlinkVisibility { frequency_hz });
    }
    entity.id()
}

fn update_solid_primitive(
    commands: &mut Commands,
    entity: Entity,
    old: SolidPrimitive,
    new: SolidPrimitive,
) {
    if old.geometry != new.geometry {
        commands.entity(entity).insert(solid_node(new.geometry));
    }
    if old.color != new.color {
        commands.entity(entity).insert(BackgroundColor(new.color));
    }
    if old.z_index != new.z_index {
        commands.entity(entity).insert(ZIndex(new.z_index));
    }
    if old.blink_hz != new.blink_hz {
        if let Some(frequency_hz) = new.blink_hz {
            commands
                .entity(entity)
                .insert(TerminalBlinkVisibility { frequency_hz });
        } else {
            commands
                .entity(entity)
                .remove::<TerminalBlinkVisibility>()
                .insert(Visibility::Visible);
        }
    }
}

fn solid_node(geometry: PixelGeometry) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(geometry.x),
        top: px(geometry.y),
        width: px(geometry.width),
        height: px(geometry.height),
        ..default()
    }
}

#[derive(Clone, Copy)]
struct BlockGeometry {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PixelGeometry {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LineWeight {
    Light,
    Heavy,
    Double,
}

#[derive(Clone, Copy)]
struct LineGlyph {
    weight: LineWeight,
    directions: u8,
}

const LEFT: u8 = 1;
const RIGHT: u8 = 2;
const UP: u8 = 4;
const DOWN: u8 = 8;
const TOP_LEFT: u8 = 1;
const TOP_RIGHT: u8 = 2;
const BOTTOM_LEFT: u8 = 4;
const BOTTOM_RIGHT: u8 = 8;

fn line_glyph(symbol: &str) -> Option<LineGlyph> {
    let (weight, directions) = match symbol {
        "─" => (LineWeight::Light, LEFT | RIGHT),
        "│" => (LineWeight::Light, UP | DOWN),
        "┌" => (LineWeight::Light, RIGHT | DOWN),
        "┐" => (LineWeight::Light, LEFT | DOWN),
        "└" => (LineWeight::Light, RIGHT | UP),
        "┘" => (LineWeight::Light, LEFT | UP),
        "├" => (LineWeight::Light, RIGHT | UP | DOWN),
        "┤" => (LineWeight::Light, LEFT | UP | DOWN),
        "┬" => (LineWeight::Light, LEFT | RIGHT | DOWN),
        "┴" => (LineWeight::Light, LEFT | RIGHT | UP),
        "┼" => (LineWeight::Light, LEFT | RIGHT | UP | DOWN),
        "━" => (LineWeight::Heavy, LEFT | RIGHT),
        "┃" => (LineWeight::Heavy, UP | DOWN),
        "┏" => (LineWeight::Heavy, RIGHT | DOWN),
        "┓" => (LineWeight::Heavy, LEFT | DOWN),
        "┗" => (LineWeight::Heavy, RIGHT | UP),
        "┛" => (LineWeight::Heavy, LEFT | UP),
        "┣" => (LineWeight::Heavy, RIGHT | UP | DOWN),
        "┫" => (LineWeight::Heavy, LEFT | UP | DOWN),
        "┳" => (LineWeight::Heavy, LEFT | RIGHT | DOWN),
        "┻" => (LineWeight::Heavy, LEFT | RIGHT | UP),
        "╋" => (LineWeight::Heavy, LEFT | RIGHT | UP | DOWN),
        "═" => (LineWeight::Double, LEFT | RIGHT),
        "║" => (LineWeight::Double, UP | DOWN),
        "╔" => (LineWeight::Double, RIGHT | DOWN),
        "╗" => (LineWeight::Double, LEFT | DOWN),
        "╚" => (LineWeight::Double, RIGHT | UP),
        "╝" => (LineWeight::Double, LEFT | UP),
        "╠" => (LineWeight::Double, RIGHT | UP | DOWN),
        "╣" => (LineWeight::Double, LEFT | UP | DOWN),
        "╦" => (LineWeight::Double, LEFT | RIGHT | DOWN),
        "╩" => (LineWeight::Double, LEFT | RIGHT | UP),
        "╬" => (LineWeight::Double, LEFT | RIGHT | UP | DOWN),
        _ => return None,
    };
    Some(LineGlyph { weight, directions })
}

fn block_geometry(symbol: &str) -> Option<BlockGeometry> {
    let geometry = match symbol {
        "█" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        "▀" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 0.5,
        },
        "▄" => BlockGeometry {
            x: 0.0,
            y: 0.5,
            width: 1.0,
            height: 0.5,
        },
        "▁" => BlockGeometry {
            x: 0.0,
            y: 0.875,
            width: 1.0,
            height: 0.125,
        },
        "▂" => BlockGeometry {
            x: 0.0,
            y: 0.75,
            width: 1.0,
            height: 0.25,
        },
        "▃" => BlockGeometry {
            x: 0.0,
            y: 0.625,
            width: 1.0,
            height: 0.375,
        },
        "▅" => BlockGeometry {
            x: 0.0,
            y: 0.375,
            width: 1.0,
            height: 0.625,
        },
        "▆" => BlockGeometry {
            x: 0.0,
            y: 0.25,
            width: 1.0,
            height: 0.75,
        },
        "▇" => BlockGeometry {
            x: 0.0,
            y: 0.125,
            width: 1.0,
            height: 0.875,
        },
        "▌" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        },
        "▉" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.875,
            height: 1.0,
        },
        "▊" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.75,
            height: 1.0,
        },
        "▋" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.625,
            height: 1.0,
        },
        "▍" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.375,
            height: 1.0,
        },
        "▎" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.25,
            height: 1.0,
        },
        "▏" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 0.125,
            height: 1.0,
        },
        "▐" => BlockGeometry {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        },
        "▔" => BlockGeometry {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 0.125,
        },
        "▕" => BlockGeometry {
            x: 0.875,
            y: 0.0,
            width: 0.125,
            height: 1.0,
        },
        _ => return None,
    };
    Some(geometry)
}

fn quadrant_mask(symbol: &str) -> Option<u8> {
    let mask = match symbol {
        "▘" => TOP_LEFT,
        "▝" => TOP_RIGHT,
        "▖" => BOTTOM_LEFT,
        "▗" => BOTTOM_RIGHT,
        "▙" => TOP_LEFT | BOTTOM_LEFT | BOTTOM_RIGHT,
        "▛" => TOP_LEFT | TOP_RIGHT | BOTTOM_LEFT,
        "▜" => TOP_LEFT | TOP_RIGHT | BOTTOM_RIGHT,
        "▟" => TOP_RIGHT | BOTTOM_LEFT | BOTTOM_RIGHT,
        "▚" => TOP_LEFT | BOTTOM_RIGHT,
        "▞" => TOP_RIGHT | BOTTOM_LEFT,
        _ => return None,
    };
    Some(mask)
}

fn push_block(
    output: &mut Vec<SolidPrimitive>,
    run: &TextRun,
    row: u16,
    config: &TerminalRenderConfig,
    geometry: BlockGeometry,
    z_index: i32,
) {
    push_solid(
        output,
        run,
        row,
        config,
        PixelGeometry {
            x: geometry.x * config.cell_size.x,
            y: geometry.y * config.cell_size.y,
            width: geometry.width * config.cell_size.x,
            height: geometry.height * config.cell_size.y,
        },
        z_index,
    );
}

fn push_line_glyph(
    output: &mut Vec<SolidPrimitive>,
    run: &TextRun,
    row: u16,
    config: &TerminalRenderConfig,
    glyph: LineGlyph,
    z_index: i32,
    pixel_scale: f32,
) {
    for geometry in line_rectangles(glyph, config.cell_size, pixel_scale) {
        push_solid(output, run, row, config, geometry, z_index);
    }
}

fn push_quadrants(
    output: &mut Vec<SolidPrimitive>,
    run: &TextRun,
    row: u16,
    config: &TerminalRenderConfig,
    mask: u8,
    z_index: i32,
) {
    let half = config.cell_size * 0.5;
    for (_, x, y) in [
        (TOP_LEFT, 0.0, 0.0),
        (TOP_RIGHT, half.x, 0.0),
        (BOTTOM_LEFT, 0.0, half.y),
        (BOTTOM_RIGHT, half.x, half.y),
    ]
    .into_iter()
    .filter(|(bit, _, _)| mask & bit != 0)
    {
        push_solid(
            output,
            run,
            row,
            config,
            PixelGeometry {
                x,
                y,
                width: half.x,
                height: half.y,
            },
            z_index,
        );
    }
}

fn push_solid(
    output: &mut Vec<SolidPrimitive>,
    run: &TextRun,
    row: u16,
    config: &TerminalRenderConfig,
    geometry: PixelGeometry,
    z_index: i32,
) {
    output.push(SolidPrimitive {
        geometry: PixelGeometry {
            x: f32::from(run.start) * config.cell_size.x + geometry.x,
            y: f32::from(row) * config.cell_size.y + geometry.y,
            width: geometry.width.max(0.0),
            height: geometry.height.max(0.0),
        },
        color: run.style.foreground,
        z_index,
        blink_hz: run.style.blink_hz(config),
    });
}

fn line_rectangles(glyph: LineGlyph, cell_size: Vec2, pixel_scale: f32) -> Vec<PixelGeometry> {
    let pixel_scale = pixel_scale.max(1.0);
    let thickness = match glyph.weight {
        LineWeight::Light | LineWeight::Double => pixel_scale,
        LineWeight::Heavy => 2.0 * pixel_scale,
    };
    let offsets = match glyph.weight {
        LineWeight::Double => [-2.0 * pixel_scale, 2.0 * pixel_scale],
        LineWeight::Light | LineWeight::Heavy => [0.0, 0.0],
    };
    let offsets = if glyph.weight == LineWeight::Double {
        &offsets[..]
    } else {
        &offsets[..1]
    };
    let center = cell_size * 0.5;
    let reach = offsets.last().copied().unwrap_or(0.0).abs() + thickness * 0.5;
    let mut rectangles = Vec::with_capacity(offsets.len() * 4);

    for offset in offsets {
        if glyph.directions & LEFT != 0 {
            rectangles.push(PixelGeometry {
                x: -0.5 * pixel_scale,
                y: center.y + offset - thickness * 0.5,
                width: center.x + reach + 0.5 * pixel_scale,
                height: thickness,
            });
        }
        if glyph.directions & RIGHT != 0 {
            rectangles.push(PixelGeometry {
                x: center.x - reach,
                y: center.y + offset - thickness * 0.5,
                width: cell_size.x - center.x + reach + 0.5 * pixel_scale,
                height: thickness,
            });
        }
        if glyph.directions & UP != 0 {
            rectangles.push(PixelGeometry {
                x: center.x + offset - thickness * 0.5,
                y: -0.5 * pixel_scale,
                width: thickness,
                height: center.y + reach + 0.5 * pixel_scale,
            });
        }
        if glyph.directions & DOWN != 0 {
            rectangles.push(PixelGeometry {
                x: center.x + offset - thickness * 0.5,
                y: center.y - reach,
                width: thickness,
                height: cell_size.y - center.y + reach + 0.5 * pixel_scale,
            });
        }
    }
    rectangles
}

#[allow(clippy::too_many_arguments)]
fn push_decoration(
    output: &mut Vec<SolidPrimitive>,
    run: &TextRun,
    row: u16,
    config: &TerminalRenderConfig,
    offset_y: f32,
    color: Color,
    z_index: i32,
) {
    output.push(SolidPrimitive {
        geometry: PixelGeometry {
            x: f32::from(run.start) * config.cell_size.x,
            y: f32::from(row) * config.cell_size.y + offset_y.max(0.0),
            width: f32::from(run.width) * config.cell_size.x,
            height: 1.0,
        },
        color,
        z_index,
        blink_hz: run.style.blink_hz(config),
    });
}

/// Returns the number of columns rendered for the cell at `column`.
///
/// A wide anchor claims its declared span, clipped to the row and to the run of
/// explicit continuation cells that actually follow it, so a wide glyph can
/// never paint over a neighbor that has since been overwritten.
fn cell_span(cells: &[TerminalCell], column: usize) -> usize {
    let declared = usize::from(cells[column].columns()).min(cells.len() - column);
    let mut span = 1;
    while span < declared && cells[column + span].is_continuation() {
        span += 1;
    }
    span
}

fn update_cursor(
    commands: &mut Commands,
    snapshot: &TerminalSnapshot,
    config: &TerminalRenderConfig,
    rendered: &RenderedEntities,
    position_changed: bool,
    visibility_changed: bool,
) {
    let Some(cursor) = rendered.cursor else {
        return;
    };
    if position_changed {
        commands
            .entity(cursor)
            .insert(cursor_node(snapshot, config));
    }
    if position_changed || visibility_changed {
        commands.entity(cursor).insert((
            TerminalCursor {
                requested: cursor_should_be_visible(snapshot),
            },
            cursor_visibility(snapshot),
        ));
    }
}

fn cursor_node(snapshot: &TerminalSnapshot, config: &TerminalRenderConfig) -> Node {
    let position = snapshot.cursor_position();
    let (x_offset, y_offset, width, height) = match config.cursor_style {
        CursorStyle::Block => (0.0, 0.0, config.cell_size.x, config.cell_size.y),
        CursorStyle::Bar => (
            0.0,
            0.0,
            2.0_f32.min(config.cell_size.x),
            config.cell_size.y,
        ),
        CursorStyle::Underline => (
            0.0,
            (config.cell_size.y - 2.0).max(0.0),
            config.cell_size.x,
            2.0_f32.min(config.cell_size.y),
        ),
    };
    Node {
        position_type: PositionType::Absolute,
        left: px(f32::from(position.x) * config.cell_size.x + x_offset),
        top: px(f32::from(position.y) * config.cell_size.y + y_offset),
        width: px(width),
        height: px(height),
        ..default()
    }
}

fn cursor_visibility(snapshot: &TerminalSnapshot) -> Visibility {
    if cursor_should_be_visible(snapshot) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    }
}

fn cursor_should_be_visible(snapshot: &TerminalSnapshot) -> bool {
    let size = snapshot.size();
    let position = snapshot.cursor_position();
    snapshot.cursor_visible() && position.x < size.width && position.y < size.height
}

fn animate_blinks(
    time: Option<Res<Time>>,
    config: Res<TerminalRenderConfig>,
    mut text: Query<(&TerminalBlink, &mut TextColor)>,
    mut blink_visibility: Query<
        (&TerminalBlinkVisibility, &mut Visibility),
        Without<TerminalCursor>,
    >,
    mut cursor: Query<(&TerminalCursor, &mut Visibility)>,
) {
    let elapsed = time.as_ref().map_or(0.0, |time| time.elapsed_secs());
    for (blink, mut color) in &mut text {
        let next = if blink_hidden(elapsed, blink.frequency_hz) {
            blink.hidden
        } else {
            blink.visible
        };
        if color.0 != next {
            color.0 = next;
        }
    }
    for (blink, mut visibility) in &mut blink_visibility {
        let next = if blink_hidden(elapsed, blink.frequency_hz) {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        if *visibility != next {
            *visibility = next;
        }
    }
    for (cursor, mut visibility) in &mut cursor {
        let blink_hidden = config
            .cursor_blink_hz
            .is_some_and(|frequency| blink_hidden(elapsed, frequency));
        let next = if cursor.requested && !blink_hidden {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

fn blink_hidden(elapsed: f32, frequency_hz: f32) -> bool {
    frequency_hz.is_finite()
        && frequency_hz > 0.0
        && (elapsed * frequency_hz * 2.0).floor() as u64 % 2 == 1
}

#[derive(Clone, Debug, PartialEq)]
struct ResolvedStyle {
    foreground: Color,
    background: Color,
    underline: Color,
    bold: bool,
    italic: bool,
    underlined: bool,
    crossed_out: bool,
    slow_blink: bool,
    rapid_blink: bool,
    hidden: bool,
}

impl ResolvedStyle {
    fn new(cell: &TerminalCell, theme: &TerminalTheme) -> Self {
        let mut foreground = theme.foreground(cell.style.foreground);
        let mut background = theme.background(cell.style.background);
        if cell.style.has(StyleFlags::REVERSED) {
            std::mem::swap(&mut foreground, &mut background);
        }
        let mut underline = theme.resolve(cell.style.underline, foreground);
        if cell.style.has(StyleFlags::DIM) {
            foreground = dim(foreground, background);
            underline = dim(underline, background);
        }
        if cell.style.has(StyleFlags::HIDDEN) {
            foreground = background;
            underline = background;
        }
        Self {
            foreground,
            background,
            underline,
            bold: cell.style.has(StyleFlags::BOLD),
            italic: cell.style.has(StyleFlags::ITALIC),
            underlined: cell.style.has(StyleFlags::UNDERLINED),
            crossed_out: cell.style.has(StyleFlags::CROSSED_OUT),
            slow_blink: cell.style.has(StyleFlags::SLOW_BLINK),
            rapid_blink: cell.style.has(StyleFlags::RAPID_BLINK),
            hidden: cell.style.has(StyleFlags::HIDDEN),
        }
    }

    fn blink_hz(&self, config: &TerminalRenderConfig) -> Option<f32> {
        if self.rapid_blink {
            Some(config.rapid_blink_hz)
        } else if self.slow_blink {
            Some(config.slow_blink_hz)
        } else {
            None
        }
    }
}

#[derive(Debug, PartialEq)]
struct TextRun {
    start: u16,
    width: u16,
    text: String,
    style: ResolvedStyle,
}

fn text_runs(cells: &[TerminalCell], theme: &TerminalTheme) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut column = 0_usize;
    while column < cells.len() {
        let cell = &cells[column];
        if cell.is_continuation() {
            column += 1;
            continue;
        }

        let style = ResolvedStyle::new(cell, theme);
        let width = cell_span(cells, column);
        let start = column;
        let mut text = cell.symbol().to_owned();
        column += width;

        // A wide/forced-width grapheme is kept in an independently anchored run.
        // Unit-width cells can be batched until style or cell-width semantics change.
        if width == 1 && !uses_exact_geometry(cell.symbol()) {
            while column < cells.len() {
                let next = &cells[column];
                if next.is_continuation()
                    || next.columns() != 1
                    || uses_exact_geometry(next.symbol())
                    || ResolvedStyle::new(next, theme) != style
                {
                    break;
                }
                text.push_str(next.symbol());
                column += 1;
            }
        }

        runs.push(TextRun {
            start: start as u16,
            width: (column - start) as u16,
            text,
            style,
        });
    }
    runs
}

fn uses_exact_geometry(symbol: &str) -> bool {
    block_geometry(symbol).is_some()
        || quadrant_mask(symbol).is_some()
        || line_glyph(symbol).is_some()
}

#[derive(Debug, PartialEq)]
struct BackgroundRun {
    start: u16,
    width: u16,
    color: Color,
}

fn background_runs(cells: &[TerminalCell], theme: &TerminalTheme) -> Vec<BackgroundRun> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let color = ResolvedStyle::new(&cells[start], theme).background;
        let mut end = start + 1;
        while end < cells.len() && ResolvedStyle::new(&cells[end], theme).background == color {
            end += 1;
        }
        runs.push(BackgroundRun {
            start: start as u16,
            width: (end - start) as u16,
            color,
        });
        start = end;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{TerminalColor, TerminalStyle};

    #[test]
    fn text_runs_batch_unit_cells_but_anchor_wide_graphemes() {
        let theme = TerminalTheme::default();
        let wide = TerminalCell::wide("界", 2);
        let cells = vec![
            TerminalCell::new("A"),
            TerminalCell::new("B"),
            wide.clone(),
            TerminalCell::continuation_of(&wide),
            TerminalCell::new("C"),
        ];
        let runs = text_runs(&cells, &theme);

        assert_eq!(runs.len(), 3);
        assert_eq!(
            (runs[0].start, runs[0].width, runs[0].text.as_str()),
            (0, 2, "AB")
        );
        assert_eq!(
            (runs[1].start, runs[1].width, runs[1].text.as_str()),
            (2, 2, "界")
        );
        assert_eq!(
            (runs[2].start, runs[2].width, runs[2].text.as_str()),
            (4, 1, "C")
        );
    }

    #[test]
    fn combining_grapheme_remains_one_cell_string() {
        let theme = TerminalTheme::default();
        let cells = [TerminalCell::new("e\u{301}"), TerminalCell::new("!")];
        let runs = text_runs(&cells, &theme);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "e\u{301}!");
        assert_eq!(runs[0].width, 2);
    }

    #[test]
    fn styles_resolve_reverse_hidden_dim_and_decorations() {
        let theme = TerminalTheme::default();
        let mut cell = TerminalCell::new("X").with_style(
            TerminalStyle::new()
                .fg(TerminalColor::RED)
                .bg(TerminalColor::BLUE)
                .with(
                    StyleFlags::REVERSED
                        | StyleFlags::DIM
                        | StyleFlags::UNDERLINED
                        | StyleFlags::CROSSED_OUT
                        | StyleFlags::BOLD
                        | StyleFlags::ITALIC,
                ),
        );
        let style = ResolvedStyle::new(&cell, &theme);
        assert_eq!(style.background, theme.ansi[1]);
        assert_ne!(style.foreground, theme.ansi[4]);
        assert!(style.bold && style.italic && style.underlined && style.crossed_out);

        cell.style.flags.insert(StyleFlags::HIDDEN);
        let hidden = ResolvedStyle::new(&cell, &theme);
        assert_eq!(hidden.foreground, hidden.background);
        assert!(hidden.hidden);

        let reversed = TerminalCell::new("X").with_style(
            TerminalStyle::new()
                .fg(TerminalColor::RED)
                .bg(TerminalColor::BLUE)
                .with(StyleFlags::REVERSED | StyleFlags::UNDERLINED),
        );
        let reversed = ResolvedStyle::new(&reversed, &theme);
        assert_eq!(reversed.foreground, theme.ansi[4]);
        assert_eq!(reversed.underline, reversed.foreground);
    }

    #[test]
    fn combined_bold_italic_style_selects_the_matching_bevy_font_face() {
        let theme = TerminalTheme::default();
        let cell = TerminalCell::new("X")
            .with_style(TerminalStyle::new().with(StyleFlags::BOLD | StyleFlags::ITALIC));
        let style = ResolvedStyle::new(&cell, &theme);
        let font = text_font(&TerminalRenderConfig::default(), &style);

        assert_eq!(font.weight, FontWeight::BOLD);
        assert_eq!(font.style, FontStyle::Italic);
    }

    #[test]
    fn explicit_font_face_overrides_are_selected_by_style_flags() {
        let config = TerminalRenderConfig {
            font: FontSource::from("regular"),
            bold_font: Some(FontSource::from("bold")),
            italic_font: Some(FontSource::from("italic")),
            bold_italic_font: Some(FontSource::from("bold italic")),
            ..default()
        };
        let theme = TerminalTheme::default();
        let selected = |flags| {
            let cell = TerminalCell::new("X").with_style(TerminalStyle::new().with(flags));
            text_font(&config, &ResolvedStyle::new(&cell, &theme)).font
        };

        assert_eq!(selected(StyleFlags::NONE), FontSource::from("regular"));
        assert_eq!(selected(StyleFlags::BOLD), FontSource::from("bold"));
        assert_eq!(selected(StyleFlags::ITALIC), FontSource::from("italic"));
        assert_eq!(
            selected(StyleFlags::BOLD | StyleFlags::ITALIC),
            FontSource::from("bold italic")
        );
    }

    #[test]
    fn background_runs_cover_every_cell_exactly() {
        let theme = TerminalTheme::default();
        let mut cells = vec![TerminalCell::EMPTY; 4];
        cells[1].style.background = TerminalColor::RED;
        cells[2].style.background = TerminalColor::RED;
        let runs = background_runs(&cells, &theme);
        assert_eq!(runs.iter().map(|run| run.width).sum::<u16>(), 4);
        assert_eq!(runs[1].start, 1);
        assert_eq!(runs[1].width, 2);
        assert_eq!(runs[1].color, theme.ansi[1]);
    }

    #[test]
    fn solid_and_fractional_blocks_are_kept_as_exact_cell_runs() {
        let theme = TerminalTheme::default();
        let cells = [
            TerminalCell::new("█"),
            TerminalCell::new("█"),
            TerminalCell::new("▀"),
            TerminalCell::new("A"),
        ];
        let runs = text_runs(&cells, &theme);
        assert_eq!(runs.len(), 4);
        assert_eq!(runs[0].text, "█");
        assert_eq!(runs[1].start, 1);
        assert_eq!(runs[2].text, "▀");
        assert_eq!(runs[3].text, "A");
    }

    #[test]
    fn common_box_drawing_glyphs_use_connected_geometry() {
        let theme = TerminalTheme::default();
        let cells = [
            TerminalCell::new("┌"),
            TerminalCell::new("─"),
            TerminalCell::new("┐"),
        ];
        let runs = text_runs(&cells, &theme);
        assert_eq!(runs.len(), 3);
        assert!(runs.iter().all(|run| line_glyph(&run.text).is_some()));

        let horizontal = line_rectangles(
            line_glyph("─").expect("known line glyph"),
            Vec2::new(10.8, 20.0),
            1.0,
        );
        assert_eq!(horizontal.len(), 2);
        assert!(horizontal[0].x < 0.0);
        assert!(horizontal[1].x + horizontal[1].width > 10.8);

        let double_cross = line_rectangles(
            line_glyph("╬").expect("known line glyph"),
            Vec2::new(10.8, 20.0),
            1.0,
        );
        assert_eq!(double_cross.len(), 8);
        assert!(line_glyph("╭").is_none());
    }

    #[test]
    fn quadrant_blocks_are_isolated_and_cover_the_expected_quadrants() {
        let theme = TerminalTheme::default();
        let cells = [
            TerminalCell::new("▛"),
            TerminalCell::new("▚"),
            TerminalCell::new("A"),
        ];
        let runs = text_runs(&cells, &theme);
        assert_eq!(runs.len(), 3);
        assert_eq!(
            quadrant_mask(&runs[0].text),
            Some(TOP_LEFT | TOP_RIGHT | BOTTOM_LEFT)
        );
        assert_eq!(quadrant_mask(&runs[1].text), Some(TOP_LEFT | BOTTOM_RIGHT));
    }

    #[test]
    fn sync_reuses_primitive_entities_and_skips_unchanged_frames() {
        let surface = TerminalSurface::new(3, 1);
        let initial = [
            TerminalCell::new("A"),
            TerminalCell::new("B"),
            TerminalCell::new("C"),
        ];
        surface.begin_update().set_cells(
            initial
                .iter()
                .enumerate()
                .map(|(column, cell)| (column as u16, 0, cell)),
        );

        let mut app = App::new();
        app.add_plugins(RetainedBevyTerminalPlugin::new(surface.clone()));
        app.update();
        let initial_stats = *app.world().resource::<TerminalRenderStats>();
        assert_eq!(initial_stats.changed_rows, 1);
        assert_eq!(initial_stats.active_text_primitives, 1);
        assert_eq!(initial_stats.active_solid_primitives, 0);

        let mut styled = TerminalCell::new("B");
        styled.style.foreground = TerminalColor::RED;
        surface.begin_update().set_cell(1, 0, &styled);
        app.update();
        let style_stats = *app.world().resource::<TerminalRenderStats>();
        assert_eq!(style_stats.changed_rows, 1);
        assert_eq!(style_stats.active_text_primitives, 3);
        assert_eq!(style_stats.spawned_primitives, 2);

        styled.symbol = "Y".into();
        surface.begin_update().set_cell(1, 0, &styled);
        app.update();
        let glyph_stats = *app.world().resource::<TerminalRenderStats>();
        assert_eq!(glyph_stats.active_text_primitives, 3);
        assert_eq!(glyph_stats.spawned_primitives, 0);
        assert_eq!(glyph_stats.pooled_primitives, style_stats.pooled_primitives);

        let revision = surface.revision();
        surface.begin_update().set_cell(1, 0, &styled);
        app.update();
        let unchanged_stats = *app.world().resource::<TerminalRenderStats>();
        assert_eq!(surface.revision(), revision);
        assert_eq!(unchanged_stats.changed_rows, 0);
        assert_eq!(unchanged_stats.snapshot_cells, 0);
        assert_eq!(
            unchanged_stats.unchanged_frames,
            glyph_stats.unchanged_frames + 1
        );

        let mut block = TerminalCell::new("█");
        block.style.foreground = TerminalColor::GREEN;
        surface.begin_update().set_cell(1, 0, &block);
        app.update();
        let block_stats = *app.world().resource::<TerminalRenderStats>();
        assert_eq!(block_stats.active_text_primitives, 2);
        assert_eq!(block_stats.active_solid_primitives, 1);
    }

    #[test]
    fn inactive_blinking_pool_entries_cannot_become_visible() {
        let surface = TerminalSurface::new(2, 1);
        let plain = TerminalCell::new("A");
        let mut blinking = TerminalCell::new("B");
        blinking
            .style
            .flags
            .insert(StyleFlags::SLOW_BLINK | StyleFlags::UNDERLINED);
        surface
            .begin_update()
            .set_cells([(0, 0, &plain), (1, 0, &blinking)]);

        let mut app = App::new();
        app.add_plugins(RetainedBevyTerminalPlugin::new(surface.clone()));
        app.update();
        let (text_entity, solid_entity) = {
            let rendered = app.world().resource::<RenderedEntities>();
            (
                rendered.rows[0].text.slots[1].entity,
                rendered.rows[0].solids.slots[0].entity,
            )
        };

        let plain_b = TerminalCell::new("B");
        surface.begin_update().set_cell(1, 0, &plain_b);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(text_entity),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(solid_entity),
            Some(&Visibility::Hidden)
        );
        assert!(!app.world().entity(text_entity).contains::<TerminalBlink>());
        assert!(
            !app.world()
                .entity(solid_entity)
                .contains::<TerminalBlinkVisibility>()
        );

        surface.begin_update().set_cell(1, 0, &blinking);
        app.update();
        assert!(app.world().entity(text_entity).contains::<TerminalBlink>());
        assert!(
            app.world()
                .entity(solid_entity)
                .contains::<TerminalBlinkVisibility>()
        );
    }

    #[test]
    fn moving_cursor_across_surface_bounds_updates_visibility() {
        let surface = TerminalSurface::new(1, 1);
        surface.begin_update().set_cursor_visible(true);
        let mut app = App::new();
        app.add_plugins(RetainedBevyTerminalPlugin::new(surface.clone()));
        app.update();

        let cursor = app
            .world_mut()
            .query_filtered::<Entity, With<TerminalCursor>>()
            .single(app.world())
            .expect("one cursor entity");
        assert_eq!(
            app.world().get::<Visibility>(cursor),
            Some(&Visibility::Visible)
        );

        surface.begin_update().set_cursor_position(1, 0);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(cursor),
            Some(&Visibility::Hidden)
        );

        surface.begin_update().set_cursor_position(0, 0);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(cursor),
            Some(&Visibility::Visible)
        );
    }
}
