use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Position, Rect, Size},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use super::{ExampleSpec, support::centered};

const CALENDAR_MONTH_LENGTHS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MOUSE_COLORS: [Color; 6] = [
    Color::LightMagenta,
    Color::LightCyan,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightRed,
    Color::White,
];

pub(super) fn mouse_color(index: usize) -> Color {
    MOUSE_COLORS[index % MOUSE_COLORS.len()]
}

/// A backend-independent key understood by the interactive example ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExampleKey {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    BackTab,
    Backspace,
    Delete,
}

/// Keyboard modifiers supplied by Bevy's input system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub control: bool,
    pub shift: bool,
}

/// The result of delivering input to the current example.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputOutcome {
    pub redraw: bool,
    pub quit: bool,
}

impl InputOutcome {
    const REDRAW: Self = Self {
        redraw: true,
        quit: false,
    };

    const QUIT: Self = Self {
        redraw: false,
        quit: true,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanicState {
    Ready,
    HookDisabled,
    PanicCaptured,
    ErrorCaptured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

/// Mutable state shared by the interactive versions of the example ports.
///
/// Fields are deliberately generic because the catalog keeps one instance per
/// example. This makes switching examples cheap and preserves each example's
/// state while it is not visible.
#[derive(Clone, Debug)]
pub struct ExampleState {
    pub slug: &'static str,
    pub tick: u64,
    pub selected: Option<usize>,
    pub secondary: usize,
    pub tab: usize,
    pub value: i32,
    pub spacing: u16,
    pub offset_x: i32,
    pub offset_y: i32,
    pub marker: usize,
    pub toggled: bool,
    pub palette: usize,
    pub fields: [String; 3],
    pub focus: usize,
    pub submitted: bool,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor: usize,
    pub messages: Vec<String>,
    pub demo2_rows: [usize; 5],
    pub todo_done: Vec<bool>,
    pub mouse_points: Vec<(u16, u16, usize)>,
    pub mouse_position: Option<(u16, u16)>,
    pub mouse_color: usize,
    pub constraint_values: Vec<i32>,
    pub constraint_kinds: Vec<usize>,
    pub panic_state: PanicState,
    pub help_visible: bool,
    pub notice: String,
}

impl ExampleState {
    #[must_use]
    pub fn new(slug: &'static str) -> Self {
        let mut state = Self {
            slug,
            tick: 0,
            selected: Some(0),
            secondary: 0,
            tab: 0,
            value: 0,
            spacing: 1,
            offset_x: 0,
            offset_y: 0,
            marker: 0,
            toggled: false,
            palette: 0,
            fields: [String::new(), String::new(), String::new()],
            focus: 0,
            submitted: false,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor: 0,
            messages: Vec::new(),
            demo2_rows: [0; 5],
            todo_done: vec![true, true, false, false, false],
            mouse_points: Vec::new(),
            mouse_position: None,
            mouse_color: 0,
            constraint_values: vec![12, 12, 25, 8],
            constraint_kinds: vec![2, 2, 3, 0],
            panic_state: PanicState::Ready,
            help_visible: false,
            notice: String::new(),
        };

        match slug {
            "calendar-explorer" => state.value = 227,
            "custom-widget" => state.selected = Some(1),
            "flex" => {
                state.tab = 4;
                state.selected = Some(4);
            }
            "input-form" => {
                state.fields = [
                    "Ferris Crab".to_owned(),
                    "29".to_owned(),
                    "ferris@example".to_owned(),
                ];
            }
            "popup" => state.toggled = true,
            "scrollbar" => {
                state.offset_x = 18;
                state.offset_y = 7;
                state.secondary = 11;
            }
            "table" => {
                state.selected = Some(2);
                state.secondary = 1;
            }
            "todo-list" => state.selected = Some(2),
            "user-input" => {
                state.input_mode = InputMode::Editing;
                state.input = "draw a wide 界 character and emoji 🚀".to_owned();
                state.cursor = state.input.chars().count();
            }
            slug if slug.starts_with("state-") => {
                state.value = match slug {
                    "state-component-trait" => 7,
                    "state-immutable-consuming" => 3,
                    "state-immutable-function" => 4,
                    "state-immutable-shared-ref" => 8,
                    "state-mutable-function" => 5,
                    "state-mutable-widget" => 6,
                    "state-refcell" => 9,
                    "state-stateful-widget" => 11,
                    "state-widget-with-mutable-ref" => 12,
                    "state-nested-stateful-widget" => 10,
                    _ => 7,
                };
                if slug == "state-nested-mutable-widget" {
                    state.secondary = 12;
                } else if slug == "state-nested-stateful-widget" {
                    state.secondary = 4;
                }
            }
            _ => {}
        }
        state
    }

    /// State used by deterministic image exports and snapshot tests.
    #[must_use]
    pub fn canonical(slug: &'static str) -> Self {
        let mut state = Self::new(slug);
        match slug {
            "gauge" => state.tick = 25,
            "mouse-drawing" => {
                state.mouse_points = (0..180)
                    .map(|step| {
                        let angle = f64::from(step) / 14.0;
                        let x = (50.0 + angle.cos() * (8.0 + f64::from(step) * 0.12))
                            .round()
                            .clamp(1.0, 98.0) as u16;
                        let y = (31.0 + angle.sin() * (4.0 + f64::from(step) * 0.05))
                            .round()
                            .clamp(1.0, 60.0) as u16;
                        (x, y, step as usize / 30)
                    })
                    .collect();
                state.mouse_position = Some((50, 31));
            }
            "panic" => state.panic_state = PanicState::PanicCaptured,
            _ => {}
        }
        state
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.slug);
    }

    /// Advance time-driven examples. Returns whether a redraw is useful.
    pub fn tick(&mut self) -> bool {
        let animated = matches!(
            self.slug,
            "advanced-widget-impl"
                | "chart"
                | "colors-rgb"
                | "demo"
                | "demo2"
                | "gauge"
                | "inline"
                | "modifiers"
                | "tracing"
                | "volatility-surface"
        ) || self.slug.starts_with("state-");
        let active = animated
            && !(self.slug == "volatility-surface" && self.toggled)
            && (self.slug != "demo2" || self.toggled);
        if active {
            self.tick = self.tick.wrapping_add(1);
            if self.slug.starts_with("state-") {
                self.value = (self.value + 1) % 100;
                if self.slug.starts_with("state-nested-") {
                    self.secondary = (self.secondary + 1) % 100;
                }
            }
        }
        active
    }

    #[must_use]
    pub fn controls(&self) -> &'static str {
        match self.slug {
            "async-github" => "j/k or Up/Down: scroll pull requests",
            "calendar-explorer" => {
                "h/j/k/l or arrows: day/week  •  n/p or Tab/Shift+Tab: month  •  s: style"
            }
            "canvas" => "h/j/k/l or arrows: move  •  Enter: marker  •  drag: draw",
            "constraint-explorer" => {
                "Left/Right: select  •  Up/Down: edit  •  1–6: type  •  a/x: add/delete  •  +/-: spacing"
            }
            "constraints" => "h/j/k/l or arrows: tab/item  •  Home/End: first/last",
            "custom-widget" => "h/l or Left/Right: select  •  Space/left click: toggle",
            "demo" => "h/l or Left/Right: tabs  •  j/k or Up/Down: selection  •  t: chart",
            "demo2" => {
                "h/l or Left/Right/Tab: tabs  •  j/k or Up/Down: tab action  •  d/Delete: destroy"
            }
            "flex" => {
                "h/l or Left/Right: flex mode  •  j/k or Up/Down: row  •  +/-: spacing  •  Home/End"
            }
            "gauge" => "Space/Enter: restart animation",
            "input-form" => {
                "Tab/Shift+Tab: field  •  type/Backspace: edit  •  Up/Down: age  •  Enter: submit  •  Esc: cancel"
            }
            "mouse-drawing" => "left-drag: draw  •  Space: change color  •  c: clear",
            "panic" => "p: capture demonstration panic  •  e: capture error  •  h: disable hook",
            "popup" => "p: toggle popup",
            "scrollbar" => "h/j/k/l or arrows: scroll  •  mouse wheel: vertical scroll",
            "table" => {
                "j/k or Up/Down: row  •  h/l or Left/Right: column  •  Shift+Left/Right: color"
            }
            "todo-list" => {
                "j/k or Up/Down: select  •  h: clear selection  •  l/Right/Enter: toggle  •  Home/End"
            }
            "user-input" => {
                "Normal: e edit, q quit  •  Editing: type, arrows, Backspace, Enter submit, Esc normal"
            }
            "volatility-surface" => {
                "hjkl/arrows: rotate  •  z/x: zoom  •  p: palette  •  Space: pause  •  Ctrl+R: reset"
            }
            "advanced-widget-impl"
            | "chart"
            | "colors-rgb"
            | "inline"
            | "modifiers"
            | "tracing" => "Animation advances automatically  •  q: quit gallery",
            slug if slug.starts_with("state-") => {
                "Counter advances on every gallery tick, matching the upstream render-mutation loop"
            }
            _ => "This example has no mutable controls  •  q or F10: quit gallery",
        }
    }

    pub fn handle_key(&mut self, key: ExampleKey, modifiers: KeyModifiers) -> InputOutcome {
        if self.help_visible {
            if matches!(key, ExampleKey::Escape | ExampleKey::Char('?')) {
                self.help_visible = false;
                return InputOutcome::REDRAW;
            }
            return InputOutcome::default();
        }

        match self.slug {
            "async-github" => match key {
                ExampleKey::Down | ExampleKey::Char('j') => self.select_next(6),
                ExampleKey::Up | ExampleKey::Char('k') => self.select_previous(6),
                _ => return self.default_key(key),
            },
            "calendar-explorer" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => self.value -= 1,
                ExampleKey::Right | ExampleKey::Char('l') => self.value += 1,
                ExampleKey::Down | ExampleKey::Char('j') => self.value += 7,
                ExampleKey::Up | ExampleKey::Char('k') => self.value -= 7,
                ExampleKey::Tab | ExampleKey::Char('n') => self.move_calendar_month(1),
                ExampleKey::BackTab | ExampleKey::Char('p') => self.move_calendar_month(-1),
                ExampleKey::Char('s') => self.palette = (self.palette + 1) % 3,
                _ => return self.default_key(key),
            },
            "canvas" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => self.offset_x -= 1,
                ExampleKey::Right | ExampleKey::Char('l') => self.offset_x += 1,
                ExampleKey::Up | ExampleKey::Char('k') => self.offset_y -= 1,
                ExampleKey::Down | ExampleKey::Char('j') => self.offset_y += 1,
                ExampleKey::Enter => self.marker = (self.marker + 1) % 4,
                _ => return self.default_key(key),
            },
            "constraint-explorer" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => {
                    self.selected = Some(self.selected.unwrap_or(0).saturating_sub(1));
                }
                ExampleKey::Right | ExampleKey::Char('l') => {
                    let last = self.constraint_values.len().saturating_sub(1);
                    self.selected = Some((self.selected.unwrap_or(0) + 1).min(last));
                }
                ExampleKey::Up | ExampleKey::Char('k') => self.edit_constraint(1),
                ExampleKey::Down | ExampleKey::Char('j') => self.edit_constraint(-1),
                ExampleKey::Char(kind @ '1'..='6') => {
                    if let Some(selected) = self.selected
                        && let Some(value) = self.constraint_kinds.get_mut(selected)
                    {
                        *value = usize::from(kind as u8 - b'1');
                    }
                }
                ExampleKey::Char('+') => self.spacing = self.spacing.saturating_add(1).min(10),
                ExampleKey::Char('-') => self.spacing = self.spacing.saturating_sub(1),
                ExampleKey::Char('a') => {
                    if self.constraint_values.len() < 8 {
                        let at = self.selected.unwrap_or(0).min(self.constraint_values.len());
                        self.constraint_values.insert(at, 10);
                        self.constraint_kinds.insert(at, 2);
                    }
                }
                ExampleKey::Char('x') | ExampleKey::Delete => {
                    if self.constraint_values.len() > 1 {
                        let at = self
                            .selected
                            .unwrap_or(0)
                            .min(self.constraint_values.len() - 1);
                        self.constraint_values.remove(at);
                        self.constraint_kinds.remove(at);
                        self.selected = Some(at.min(self.constraint_values.len() - 1));
                    }
                }
                _ => return self.default_key(key),
            },
            "constraints" => match key {
                ExampleKey::Right | ExampleKey::Char('l') => self.tab = (self.tab + 1) % 6,
                ExampleKey::Left | ExampleKey::Char('h') => self.tab = (self.tab + 5) % 6,
                ExampleKey::Down | ExampleKey::Char('j') => self.select_next(3),
                ExampleKey::Up | ExampleKey::Char('k') => self.select_previous(3),
                ExampleKey::Home | ExampleKey::Char('g') => self.selected = Some(0),
                ExampleKey::End | ExampleKey::Char('G') => self.selected = Some(2),
                _ => return self.default_key(key),
            },
            "custom-widget" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => self.select_previous(3),
                ExampleKey::Right | ExampleKey::Char('l') => self.select_next(3),
                ExampleKey::Char(' ') | ExampleKey::Enter => self.toggled = !self.toggled,
                _ => return self.default_key(key),
            },
            "demo" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => self.tab = (self.tab + 2) % 3,
                ExampleKey::Right | ExampleKey::Char('l') => self.tab = (self.tab + 1) % 3,
                ExampleKey::Down | ExampleKey::Char('j') => self.select_next(4),
                ExampleKey::Up | ExampleKey::Char('k') => self.select_previous(4),
                ExampleKey::Char('t') => self.toggled = !self.toggled,
                _ => return self.default_key(key),
            },
            "demo2" => match key {
                ExampleKey::Escape => return InputOutcome::QUIT,
                ExampleKey::Left | ExampleKey::Char('h') => {
                    self.tab = self.tab.saturating_sub(1);
                }
                ExampleKey::Right | ExampleKey::Char('l') | ExampleKey::Tab => {
                    self.tab = (self.tab + 1).min(4);
                }
                ExampleKey::Down | ExampleKey::Char('j') => match self.tab {
                    0 => {
                        self.demo2_rows[0] = self.demo2_rows[0].saturating_add(1);
                    }
                    1 => self.demo2_rows[1] = (self.demo2_rows[1] + 1) % 11,
                    2 => self.demo2_rows[2] = (self.demo2_rows[2] + 1) % 5,
                    3 => self.demo2_rows[3] = (self.demo2_rows[3] + 1) % 29,
                    4 => {
                        self.demo2_rows[4] = self.demo2_rows[4].saturating_add(1);
                    }
                    _ => unreachable!("demo2 has exactly five tabs"),
                },
                ExampleKey::Up | ExampleKey::Char('k') => match self.tab {
                    0 | 4 => {
                        self.demo2_rows[self.tab] = self.demo2_rows[self.tab].saturating_sub(1);
                    }
                    1 => self.demo2_rows[1] = (self.demo2_rows[1] + 10) % 11,
                    2 => self.demo2_rows[2] = (self.demo2_rows[2] + 4) % 5,
                    3 => self.demo2_rows[3] = (self.demo2_rows[3] + 28) % 29,
                    _ => unreachable!("demo2 has exactly five tabs"),
                },
                ExampleKey::Char('d') | ExampleKey::Delete => {
                    self.toggled = true;
                    self.tick = 0;
                }
                _ => return self.default_key(key),
            },
            "flex" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => self.tab = (self.tab + 6) % 7,
                ExampleKey::Right | ExampleKey::Char('l') => self.tab = (self.tab + 1) % 7,
                ExampleKey::Down | ExampleKey::Char('j') => self.select_next(7),
                ExampleKey::Up | ExampleKey::Char('k') => self.select_previous(7),
                ExampleKey::Home | ExampleKey::Char('g') => self.selected = Some(0),
                ExampleKey::End | ExampleKey::Char('G') => self.selected = Some(6),
                ExampleKey::Char('+') => self.spacing = self.spacing.saturating_add(1).min(12),
                ExampleKey::Char('-') => self.spacing = self.spacing.saturating_sub(1),
                _ => return self.default_key(key),
            },
            "gauge" => match key {
                ExampleKey::Char(' ') | ExampleKey::Enter => self.tick = 0,
                _ => return self.default_key(key),
            },
            "input-form" => return self.handle_form_key(key),
            "mouse-drawing" => match key {
                ExampleKey::Char(' ') => self.mouse_color = (self.mouse_color + 1) % 6,
                ExampleKey::Char('c') => self.mouse_points.clear(),
                _ => return self.default_key(key),
            },
            "panic" => match key {
                ExampleKey::Char('p') => self.panic_state = PanicState::PanicCaptured,
                ExampleKey::Char('e') => self.panic_state = PanicState::ErrorCaptured,
                ExampleKey::Char('h') => self.panic_state = PanicState::HookDisabled,
                _ => return self.default_key(key),
            },
            "popup" => match key {
                ExampleKey::Char('p') => self.toggled = !self.toggled,
                _ => return self.default_key(key),
            },
            "scrollbar" => match key {
                ExampleKey::Down | ExampleKey::Char('j') => self.offset_y += 1,
                ExampleKey::Up | ExampleKey::Char('k') => self.offset_y -= 1,
                ExampleKey::Right | ExampleKey::Char('l') => self.offset_x += 1,
                ExampleKey::Left | ExampleKey::Char('h') => self.offset_x -= 1,
                _ => return self.default_key(key),
            },
            "table" => match key {
                ExampleKey::Down | ExampleKey::Char('j') => self.select_wrapped(6, 1),
                ExampleKey::Up | ExampleKey::Char('k') => self.select_wrapped(6, -1),
                ExampleKey::Right | ExampleKey::Char('l') if modifiers.shift => {
                    self.palette = (self.palette + 1) % 5;
                }
                ExampleKey::Left | ExampleKey::Char('h') if modifiers.shift => {
                    self.palette = (self.palette + 4) % 5;
                }
                ExampleKey::Right | ExampleKey::Char('l') => {
                    self.secondary = (self.secondary + 1) % 3
                }
                ExampleKey::Left | ExampleKey::Char('h') => {
                    self.secondary = (self.secondary + 2) % 3
                }
                _ => return self.default_key(key),
            },
            "todo-list" => match key {
                ExampleKey::Left | ExampleKey::Char('h') => self.selected = None,
                ExampleKey::Down | ExampleKey::Char('j') => self.select_next(5),
                ExampleKey::Up | ExampleKey::Char('k') => self.select_previous(5),
                ExampleKey::Home | ExampleKey::Char('g') => self.selected = Some(0),
                ExampleKey::End | ExampleKey::Char('G') => self.selected = Some(4),
                ExampleKey::Right | ExampleKey::Char('l') | ExampleKey::Enter => {
                    if let Some(index) = self.selected {
                        self.todo_done[index] = !self.todo_done[index];
                    }
                }
                _ => return self.default_key(key),
            },
            "user-input" => return self.handle_user_input_key(key),
            "volatility-surface" => match key {
                ExampleKey::Up | ExampleKey::Char('k') => self.offset_y += 1,
                ExampleKey::Down | ExampleKey::Char('j') => self.offset_y -= 1,
                ExampleKey::Left | ExampleKey::Char('h') => self.offset_x += 1,
                ExampleKey::Right | ExampleKey::Char('l') => self.offset_x -= 1,
                ExampleKey::Char('z') => self.value = (self.value + 1).min(12),
                ExampleKey::Char('x') => self.value = (self.value - 1).max(-8),
                ExampleKey::Char('p') => self.palette = (self.palette + 1) % 4,
                ExampleKey::Char(' ') => self.toggled = !self.toggled,
                ExampleKey::Char('r') if modifiers.control => self.reset(),
                _ => return self.default_key(key),
            },
            _ => return self.default_key(key),
        }
        InputOutcome::REDRAW
    }

    fn default_key(&self, key: ExampleKey) -> InputOutcome {
        if matches!(key, ExampleKey::Char('q')) {
            InputOutcome::QUIT
        } else {
            InputOutcome::default()
        }
    }

    fn handle_form_key(&mut self, key: ExampleKey) -> InputOutcome {
        match key {
            ExampleKey::Escape => {
                self.submitted = false;
                self.notice = "Form cancelled — F2 restores the fixture".to_owned();
            }
            ExampleKey::Enter => {
                self.submitted = true;
                self.notice = if self.fields[2].contains('.') && self.fields[2].contains('@') {
                    "Profile submitted successfully".to_owned()
                } else {
                    "Email must contain @ and a complete domain".to_owned()
                };
            }
            ExampleKey::Tab => self.focus = (self.focus + 1) % self.fields.len(),
            ExampleKey::BackTab => {
                self.focus = (self.focus + self.fields.len() - 1) % self.fields.len()
            }
            ExampleKey::Backspace => {
                self.fields[self.focus].pop();
                self.submitted = false;
            }
            ExampleKey::Up if self.focus == 1 => self.change_age(1),
            ExampleKey::Down if self.focus == 1 => self.change_age(-1),
            ExampleKey::Char(character) if !character.is_control() => {
                if self.focus != 1 || character.is_ascii_digit() {
                    self.fields[self.focus].push(character);
                    self.submitted = false;
                }
            }
            _ => return InputOutcome::default(),
        }
        InputOutcome::REDRAW
    }

    fn handle_user_input_key(&mut self, key: ExampleKey) -> InputOutcome {
        match self.input_mode {
            InputMode::Normal => match key {
                ExampleKey::Char('e') => self.input_mode = InputMode::Editing,
                ExampleKey::Char('q') => return InputOutcome::QUIT,
                _ => return InputOutcome::default(),
            },
            InputMode::Editing => match key {
                ExampleKey::Escape => self.input_mode = InputMode::Normal,
                ExampleKey::Enter => {
                    if !self.input.is_empty() {
                        self.messages.push(self.input.clone());
                        self.input.clear();
                        self.cursor = 0;
                    }
                }
                ExampleKey::Left => self.cursor = self.cursor.saturating_sub(1),
                ExampleKey::Right => {
                    self.cursor = (self.cursor + 1).min(self.input.chars().count())
                }
                ExampleKey::Backspace => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        remove_char(&mut self.input, self.cursor);
                    }
                }
                ExampleKey::Delete => {
                    if self.cursor < self.input.chars().count() {
                        remove_char(&mut self.input, self.cursor);
                    }
                }
                ExampleKey::Char(character) if !character.is_control() => {
                    insert_char(&mut self.input, self.cursor, character);
                    self.cursor += 1;
                }
                _ => return InputOutcome::default(),
            },
        }
        InputOutcome::REDRAW
    }

    fn select_next(&mut self, length: usize) {
        if length == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            Some(index) => (index + 1).min(length.saturating_sub(1)),
            None => 0,
        });
    }

    fn select_previous(&mut self, length: usize) {
        if length == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            Some(index) => index.saturating_sub(1),
            None => length.saturating_sub(1),
        });
    }

    fn edit_constraint(&mut self, delta: i32) {
        if let Some(value) = self
            .selected
            .and_then(|index| self.constraint_values.get_mut(index))
        {
            *value = (*value + delta).max(0);
        }
    }

    fn select_wrapped(&mut self, length: usize, delta: isize) {
        if length == 0 {
            self.selected = None;
            return;
        }
        let current = self.selected.unwrap_or_default() % length;
        self.selected = Some((current as isize + delta).rem_euclid(length as isize) as usize);
    }

    fn move_calendar_month(&mut self, delta: i32) {
        let mut day_of_year = self.value.rem_euclid(365);
        let mut month = 0_usize;
        while day_of_year >= CALENDAR_MONTH_LENGTHS[month] {
            day_of_year -= CALENDAR_MONTH_LENGTHS[month];
            month += 1;
        }
        let day = day_of_year + 1;
        let target_month = (month as i32 + delta).rem_euclid(12) as usize;
        let target_day = day.min(CALENDAR_MONTH_LENGTHS[target_month]);
        self.value = CALENDAR_MONTH_LENGTHS[..target_month].iter().sum::<i32>() + target_day - 1;
    }

    fn change_age(&mut self, delta: i32) {
        let age = self.fields[1].parse::<i32>().unwrap_or_default();
        self.fields[1] = (age + delta).clamp(0, 150).to_string();
    }

    pub fn scroll(&mut self, lines: i32) -> bool {
        if lines == 0 {
            return false;
        }
        if matches!(
            self.slug,
            "scrollbar" | "async-github" | "table" | "todo-list"
        ) {
            if lines > 0 {
                for _ in 0..lines {
                    let _ = self.handle_key(ExampleKey::Down, KeyModifiers::default());
                }
            } else {
                for _ in lines..0 {
                    let _ = self.handle_key(ExampleKey::Up, KeyModifiers::default());
                }
            }
            true
        } else {
            false
        }
    }

    /// Handle a pointer expressed in terminal-cell coordinates.
    pub fn pointer(
        &mut self,
        column: u16,
        row: u16,
        terminal_size: Size,
        pressed: bool,
        activate: bool,
    ) -> bool {
        match self.slug {
            "mouse-drawing" => {
                let position = (column, row);
                let drawing_area = example_inner(terminal_size);
                if !drawing_area.contains(Position::new(column, row)) {
                    let changed = self.mouse_position.take().is_some();
                    return changed;
                }
                let cursor_changed = self.mouse_position != Some(position);
                self.mouse_position = Some(position);
                if activate {
                    self.mouse_points
                        .push((column, row, self.mouse_color % MOUSE_COLORS.len()));
                    return true;
                }
                if pressed && cursor_changed {
                    let start = self
                        .mouse_points
                        .last()
                        .map_or(position, |&(x, y, _)| (x, y));
                    append_mouse_line(
                        &mut self.mouse_points,
                        start,
                        position,
                        self.mouse_color % MOUSE_COLORS.len(),
                    );
                    return true;
                }
                cursor_changed
            }
            "canvas" if pressed => {
                let area = example_inner(terminal_size);
                self.offset_x =
                    i32::from(column).saturating_sub(i32::from(area.x + area.width / 2));
                self.offset_y = i32::from(row).saturating_sub(i32::from(area.y + area.height / 2));
                true
            }
            "custom-widget" => {
                let Some(index) = custom_widget_buttons(terminal_size)
                    .iter()
                    .position(|area| area.contains(Position::new(column, row)))
                else {
                    return false;
                };
                let changed = self.selected != Some(index);
                self.selected = Some(index);
                if activate {
                    self.toggled = !self.toggled;
                }
                changed || activate
            }
            _ => false,
        }
    }
}

fn example_inner(size: Size) -> Rect {
    Rect::new(
        1,
        1,
        size.width.saturating_sub(2),
        size.height.saturating_sub(2),
    )
}

fn custom_widget_buttons(size: Size) -> Vec<Rect> {
    let card = centered(example_inner(size), 76, 20);
    let [_title, buttons, _description] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(8),
        Constraint::Min(3),
    ])
    .areas(card);
    Layout::horizontal([Constraint::Ratio(1, 3); 3])
        .spacing(3)
        .split(buttons)
        .to_vec()
}

fn append_mouse_line(
    points: &mut Vec<(u16, u16, usize)>,
    start: (u16, u16),
    end: (u16, u16),
    color: usize,
) {
    let (mut x0, mut y0) = (i32::from(start.0), i32::from(start.1));
    let (x1, y1) = (i32::from(end.0), i32::from(end.1));
    let dx = (x1 - x0).abs();
    let step_x = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let step_y = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        points.push((x0 as u16, y0 as u16, color));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let doubled_error = error * 2;
        if doubled_error >= dy {
            error += dy;
            x0 += step_x;
        }
        if doubled_error <= dx {
            error += dx;
            y0 += step_y;
        }
    }
}

fn insert_char(value: &mut String, char_index: usize, character: char) {
    let byte_index = value
        .char_indices()
        .nth(char_index)
        .map_or(value.len(), |(index, _)| index);
    value.insert(byte_index, character);
}

fn remove_char(value: &mut String, char_index: usize) {
    let start = value.char_indices().nth(char_index).map(|(index, _)| index);
    let end = value
        .char_indices()
        .nth(char_index + 1)
        .map_or(value.len(), |(index, _)| index);
    if let Some(start) = start {
        value.replace_range(start..end, "");
    }
}

pub fn render_help(frame: &mut Frame<'_>, spec: &ExampleSpec, state: &ExampleState) {
    if !state.help_visible {
        return;
    }
    let area = centered(frame.area(), 84, 22);
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::new().fg(Color::LightCyan))
        .title(" Gallery controls ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [title, global, local, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(7),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                spec.slug,
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  ({}/{})",
                index_of(spec) + 1,
                super::EXAMPLES.len()
            )),
        ]))
        .alignment(Alignment::Center),
        title,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw("PageDown / F6       next example"),
            Line::raw("PageUp / Shift+F6   previous example"),
            Line::raw("F2                  reset current example"),
            Line::raw("F1                  toggle this help"),
            Line::raw("F10                 quit gallery"),
        ])
        .alignment(Alignment::Center),
        global,
    );
    frame.render_widget(
        Paragraph::new(state.controls())
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::LightGreen))
            .wrap(Wrap { trim: true }),
        local,
    );
    frame.render_widget(
        Paragraph::new("Press F1 or Esc to close")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::DarkGray)),
        footer,
    );
}

fn index_of(spec: &ExampleSpec) -> usize {
    super::EXAMPLES
        .iter()
        .position(|candidate| candidate.slug == spec.slug)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_editing_uses_character_not_byte_offsets() {
        let mut state = ExampleState::new("user-input");
        state.input = "a界🚀".to_owned();
        state.cursor = 2;
        state.handle_key(ExampleKey::Backspace, KeyModifiers::default());
        assert_eq!(state.input, "a🚀");
        assert_eq!(state.cursor, 1);
        state.handle_key(ExampleKey::Char('é'), KeyModifiers::default());
        assert_eq!(state.input, "aé🚀");
    }

    #[test]
    fn every_example_has_contextual_controls() {
        for spec in super::super::EXAMPLES {
            assert!(!ExampleState::new(spec.slug).controls().is_empty());
        }
    }

    #[test]
    fn form_navigation_validation_and_submission_work() {
        let mut state = ExampleState::new("input-form");
        state.handle_key(ExampleKey::BackTab, KeyModifiers::default());
        assert_eq!(state.focus, 2);
        state.fields[2] = "ferris@example.test".to_owned();
        state.handle_key(ExampleKey::Enter, KeyModifiers::default());
        assert!(state.submitted);
        assert_eq!(state.notice, "Profile submitted successfully");
    }

    #[test]
    fn table_shift_arrows_change_color_without_changing_column() {
        let mut state = ExampleState::new("table");
        let column = state.secondary;
        state.handle_key(
            ExampleKey::Right,
            KeyModifiers {
                shift: true,
                control: false,
            },
        );
        assert_eq!(state.secondary, column);
        assert_eq!(state.palette, 1);
    }

    #[test]
    fn demo2_preserves_independent_tab_state_and_starts_destroy_mode() {
        let mut state = ExampleState::new("demo2");
        state.handle_key(ExampleKey::Down, KeyModifiers::default());
        assert_eq!(state.demo2_rows[0], 1);
        state.handle_key(ExampleKey::Right, KeyModifiers::default());
        state.handle_key(ExampleKey::Down, KeyModifiers::default());
        assert_eq!(state.demo2_rows, [1, 1, 0, 0, 0]);
        state.handle_key(ExampleKey::Delete, KeyModifiers::default());
        assert!(state.toggled);
        assert!(state.tick());
    }

    #[test]
    fn mouse_cursor_click_and_drag_follow_terminal_cells() {
        let mut state = ExampleState::new("mouse-drawing");
        let size = Size::new(100, 62);

        assert!(state.pointer(10, 10, size, false, false));
        assert_eq!(state.mouse_position, Some((10, 10)));
        assert!(state.mouse_points.is_empty());

        assert!(state.pointer(10, 10, size, true, true));
        assert_eq!(state.mouse_points, [(10, 10, 0)]);
        assert!(state.pointer(14, 12, size, true, false));
        assert!(state.mouse_points.len() > 2);
        assert_eq!(state.mouse_points.last(), Some(&(14, 12, 0)));

        state.handle_key(ExampleKey::Char(' '), KeyModifiers::default());
        assert!(state.pointer(16, 12, size, true, false));
        assert_eq!(state.mouse_points.last(), Some(&(16, 12, 1)));
        assert_eq!(state.mouse_points.first(), Some(&(10, 10, 0)));

        assert!(state.pointer(0, 0, size, false, false));
        assert_eq!(state.mouse_position, None);
    }

    #[test]
    fn calendar_month_navigation_clamps_to_the_target_month() {
        let mut state = ExampleState::new("calendar-explorer");
        state.value = 30; // January 31
        state.handle_key(ExampleKey::Char('n'), KeyModifiers::default());
        assert_eq!(state.value, 58); // February 28
        state.handle_key(ExampleKey::Char('p'), KeyModifiers::default());
        assert_eq!(state.value, 27); // January 28
    }

    #[test]
    fn table_row_navigation_wraps_like_the_upstream_example() {
        let mut state = ExampleState::new("table");
        state.selected = Some(5);
        state.handle_key(ExampleKey::Down, KeyModifiers::default());
        assert_eq!(state.selected, Some(0));
        state.handle_key(ExampleKey::Up, KeyModifiers::default());
        assert_eq!(state.selected, Some(5));
    }

    #[test]
    fn custom_widget_pointer_hover_selects_and_click_activates() {
        let mut state = ExampleState::new("custom-widget");
        let size = Size::new(100, 62);
        assert!(state.pointer(80, 30, size, false, false));
        assert_eq!(state.selected, Some(2));
        assert!(!state.toggled);
        assert!(state.pointer(80, 30, size, true, true));
        assert!(state.toggled);
    }
}
