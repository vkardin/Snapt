//! The settings window.
//!
//! A small immediate-mode UI drawn with the same software renderer as the
//! overlay: every frame lays the controls out, draws them and keeps their
//! rectangles for hit testing. No GUI toolkit, so Snapt stays small.

use std::{num::NonZeroU32, rc::Rc};

use anyhow::{Context, Result};
use softbuffer::Surface;
use tiny_skia::{Pixmap, Rect as SkRect};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{CursorIcon, Window, WindowId},
};

use crate::{
    font,
    i18n::{Lang, T, t},
    paint::fill_round,
    platform,
    settings::{Format, Settings},
    tray::hotkey_label,
};

/// What the user changed. The app applies it and redraws the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Language(Option<Lang>),
    Autostart,
    ReplaceSystem,
    /// The shortcut field started (true) or stopped (false) listening for keys.
    Recording(bool),
    Hotkey(String),
    ResetHotkey,
    ChangeFolder,
    OpenFolder,
    Format(Format),
    QuickSave,
    AutoUpdate,
    CheckUpdates,
    InstallUpdate,
    Website,
    Close,
}

/// Where the update check stands, shown under the update switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available(String),
    Downloading,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Id {
    Lang(Option<Lang>),
    Autostart,
    ReplaceSystem,
    Hotkey,
    ResetHotkey,
    ChangeFolder,
    OpenFolder,
    Format(Format),
    QuickSave,
    AutoUpdate,
    CheckUpdates,
    InstallUpdate,
    Website,
    Done,
}

/// A message under the shortcut field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    ShortcutTaken,
    ShortcutInvalid,
    ReplaceFailed,
}

const WIDTH: f64 = 560.0;
const HEIGHT: f64 = 712.0;

const BG: u32 = 0x18181B;
const CARD: u32 = 0x222227;
const CONTROL: u32 = 0x2E2E35;
const CONTROL_HOVER: u32 = 0x3A3A43;
const TEXT: u32 = 0xF4F4F6;
const MUTED: u32 = 0xA1A1AA;
const ACCENT: u32 = 0x8B5CF6;
const ERROR: u32 = 0xF87171;

pub struct SettingsWindow {
    // Declared before the context so the surface drops first.
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    cursor: (f32, f32),
    hover: Option<Id>,
    modifiers: ModifiersState,
    recording: bool,
    pub notice: Option<Notice>,
    hits: Vec<(Id, SkRect)>,
}

impl SettingsWindow {
    pub fn open(event_loop: &ActiveEventLoop) -> Result<Self> {
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;
        let attributes = Window::default_attributes()
            .with_title(t(T::SettingsTitle))
            .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
            .with_resizable(false)
            .with_visible(false);
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the settings window")?,
        );
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("failed to create a drawing surface: {e}"))?;
        Ok(Self {
            surface,
            window,
            _context: context,
            cursor: (-1.0, -1.0),
            hover: None,
            modifiers: ModifiersState::empty(),
            recording: false,
            notice: None,
            hits: Vec::new(),
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Shows the window, drawn for `settings`, in front of everything.
    pub fn show(&mut self, settings: &Settings, update: &UpdateStatus) {
        self.window.set_title(t(T::SettingsTitle));
        self.redraw(settings, update);
        self.window.set_visible(true);
        self.window.set_minimized(false);
        self.window.focus_window();
    }

    /// Follows a language change.
    pub fn retitle(&self) {
        self.window.set_title(t(T::SettingsTitle));
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Handles an event; returns what the user changed, if anything.
    pub fn window_event(
        &mut self,
        event: WindowEvent,
        settings: &Settings,
        update: &UpdateStatus,
    ) -> Option<Action> {
        match event {
            WindowEvent::CloseRequested => return Some(self.close()),
            WindowEvent::RedrawRequested => self.redraw(settings, update),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => self.cursor_moved(position),
            WindowEvent::CursorLeft { .. } => {
                self.cursor = (-1.0, -1.0);
                self.set_hover(None);
            }
            WindowEvent::Focused(false) if self.recording => {
                self.recording = false;
                self.window.request_redraw();
                return Some(Action::Recording(false));
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => return self.click(),
            WindowEvent::KeyboardInput { event, .. } => return self.key(event),
            _ => {}
        }
        None
    }

    fn close(&mut self) -> Action {
        self.recording = false;
        self.notice = None;
        self.window.set_visible(false);
        Action::Close
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let s = self.window.scale_factor();
        self.cursor = ((position.x / s) as f32, (position.y / s) as f32);
        let hover = self
            .hits
            .iter()
            .find(|(_, r)| {
                let (x, y) = self.cursor;
                x >= r.left() && x < r.right() && y >= r.top() && y < r.bottom()
            })
            .map(|(id, _)| *id);
        self.set_hover(hover);
    }

    fn set_hover(&mut self, hover: Option<Id>) {
        if hover != self.hover {
            self.hover = hover;
            let icon = if hover.is_some() {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            };
            self.window.set_cursor(icon);
            self.window.request_redraw();
        }
    }

    fn click(&mut self) -> Option<Action> {
        let id = self.hover?;
        self.window.request_redraw();
        if self.recording && id != Id::Hotkey {
            self.recording = false;
        }
        Some(match id {
            Id::Lang(lang) => Action::Language(lang),
            Id::Autostart => Action::Autostart,
            Id::ReplaceSystem => Action::ReplaceSystem,
            Id::Hotkey => {
                self.recording = !self.recording;
                self.notice = None;
                Action::Recording(self.recording)
            }
            Id::ResetHotkey => {
                self.notice = None;
                Action::ResetHotkey
            }
            Id::ChangeFolder => Action::ChangeFolder,
            Id::OpenFolder => Action::OpenFolder,
            Id::Format(f) => Action::Format(f),
            Id::QuickSave => Action::QuickSave,
            Id::AutoUpdate => Action::AutoUpdate,
            Id::CheckUpdates => Action::CheckUpdates,
            Id::InstallUpdate => Action::InstallUpdate,
            Id::Website => Action::Website,
            Id::Done => self.close(),
        })
    }

    fn key(&mut self, event: KeyEvent) -> Option<Action> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return None;
        };
        if !self.recording {
            return matches!(code, KeyCode::Escape).then(|| self.close());
        }
        if code == KeyCode::Escape {
            self.recording = false;
            self.window.request_redraw();
            return Some(Action::Recording(false));
        }
        // Wait for the key that comes with the modifiers.
        let modifier_only = matches!(
            code,
            KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight
        );
        if modifier_only {
            return None;
        }
        self.recording = false;
        self.window.request_redraw();
        match hotkey_from(self.modifiers, code) {
            Some(hotkey) => Some(Action::Hotkey(hotkey)),
            None => {
                self.notice = Some(Notice::ShortcutInvalid);
                Some(Action::Recording(false))
            }
        }
    }

    fn redraw(&mut self, settings: &Settings, update: &UpdateStatus) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if let Err(err) = self.surface.resize(w, h) {
            log::error!("failed to resize the settings window: {err}");
            return;
        }
        let Some(mut pixmap) = Pixmap::new(size.width, size.height) else {
            return;
        };
        let scale = self.window.scale_factor() as f32;
        let mut ui = Ui {
            pixmap: &mut pixmap,
            scale,
            hover: self.hover,
            hits: Vec::new(),
        };
        ui.fill(
            SkRect::from_xywh(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
            0.0,
            BG,
        );
        build(&mut ui, settings, update, self.recording, self.notice);
        self.hits = ui.hits;

        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(err) => {
                log::error!("failed to draw the settings window: {err}");
                return;
            }
        };
        for (dst, src) in buffer.iter_mut().zip(pixmap.data().chunks_exact(4)) {
            *dst = (src[0] as u32) << 16 | (src[1] as u32) << 8 | src[2] as u32;
        }
        if let Err(err) = buffer.present() {
            log::error!("failed to present the settings window: {err}");
        }
    }
}

/// Builds `Ctrl+Shift+S`-style text from a key press, or `None` when the key
/// needs a modifier and has none.
fn hotkey_from(modifiers: ModifiersState, code: KeyCode) -> Option<String> {
    let name = format!("{code:?}");
    let key = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .unwrap_or(&name)
        .to_string();
    let standalone = matches!(
        code,
        KeyCode::PrintScreen | KeyCode::Pause | KeyCode::Insert
    ) || (key.len() >= 2
        && key.starts_with('F')
        && key[1..].chars().all(|c| c.is_ascii_digit()));
    let mut parts = Vec::new();
    if modifiers.control_key() {
        parts.push("Ctrl");
    }
    if modifiers.alt_key() {
        parts.push("Alt");
    }
    if modifiers.shift_key() {
        parts.push("Shift");
    }
    if modifiers.super_key() {
        parts.push(if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        });
    }
    if parts.is_empty() && !standalone {
        return None;
    }
    let hotkey = parts
        .into_iter()
        .chain([key.as_str()])
        .collect::<Vec<_>>()
        .join("+");
    hotkey
        .parse::<global_hotkey::hotkey::HotKey>()
        .is_ok()
        .then_some(hotkey)
}

/// Lays out and draws the whole window. Units are logical pixels.
fn build(
    ui: &mut Ui,
    settings: &Settings,
    update: &UpdateStatus,
    recording: bool,
    notice: Option<Notice>,
) {
    let margin = 24.0;
    let label_x = margin + 16.0;
    let control_x = 190.0;
    let right = WIDTH as f32 - margin - 16.0;

    // General
    let mut y = ui.section(t(T::General), margin, 16.0, 188.0);
    ui.label(t(T::Language), label_x, y + 8.0, 14.0, TEXT);
    let mut options = vec![(t(T::SystemLanguage), None)];
    options.extend(Lang::ALL.iter().map(|l| (l.name(), Some(*l))));
    let mut x = control_x;
    for (i, (name, lang)) in options.iter().enumerate() {
        let w = font::measure(name, 13.0).0 + 22.0;
        let selected = settings.language == *lang;
        ui.segment(
            Id::Lang(*lang),
            name,
            x,
            y,
            w,
            selected,
            i == 0,
            i == options.len() - 1,
        );
        x += w;
    }
    y += 48.0;
    ui.switch(
        Id::Autostart,
        t(T::StartAtLogin),
        label_x,
        y,
        settings.autostart,
    );
    y += 40.0;
    ui.switch(
        Id::AutoUpdate,
        t(T::AutoUpdate),
        label_x,
        y,
        settings.auto_update,
    );
    y += 36.0;
    let status_x = label_x + 46.0;
    let (status, color) = match update {
        UpdateStatus::Idle => (format!("Snapt {}", env!("CARGO_PKG_VERSION")), MUTED),
        UpdateStatus::Checking => (t(T::Checking).into(), MUTED),
        UpdateStatus::UpToDate => (t(T::UpToDate).into(), MUTED),
        UpdateStatus::Available(v) => (t(T::UpdateAvailable).replace("{}", v), TEXT),
        UpdateStatus::Downloading => (t(T::Downloading).into(), MUTED),
        UpdateStatus::Failed => (t(T::UpdateFailed).into(), ERROR),
    };
    match update {
        UpdateStatus::Available(_) => {
            ui.button(Id::InstallUpdate, t(T::InstallUpdate), status_x, y, true)
        }
        UpdateStatus::Checking | UpdateStatus::Downloading => {}
        _ => ui.button(Id::CheckUpdates, t(T::CheckNow), status_x, y, false),
    }
    let button_w = match update {
        UpdateStatus::Available(_) => font::measure(t(T::InstallUpdate), 13.0).0 + 40.0 + 12.0,
        UpdateStatus::Checking | UpdateStatus::Downloading => 0.0,
        _ => font::measure(t(T::CheckNow), 13.0).0 + 24.0 + 12.0,
    };
    ui.label(&status, status_x + button_w, y + 9.0, 13.0, color);

    // Capture
    y = ui.section(t(T::Capture), margin, 244.0, 172.0);
    ui.label(t(T::Shortcut), label_x, y + 8.0, 14.0, TEXT);
    let field = if recording {
        t(T::PressShortcut).to_string()
    } else {
        hotkey_label(&settings.hotkey)
    };
    let field_w = 190.0;
    ui.field(Id::Hotkey, &field, control_x, y, field_w, recording);
    ui.button(
        Id::ResetHotkey,
        t(T::Reset),
        control_x + field_w + 8.0,
        y,
        false,
    );
    y += 38.0;
    let (hint, color) = match notice {
        Some(Notice::ShortcutTaken) => (t(T::ShortcutTaken), ERROR),
        Some(Notice::ShortcutInvalid) => (t(T::ShortcutInvalid), ERROR),
        _ => (t(T::ShortcutHint), MUTED),
    };
    ui.label(hint, control_x, y, 12.0, color);
    y += 30.0;
    ui.switch(
        Id::ReplaceSystem,
        t(T::ReplaceSystem),
        label_x,
        y,
        settings.replace_system,
    );
    y += 30.0;
    let (hint, color) = match notice {
        Some(Notice::ReplaceFailed) => (t(T::ReplaceFailed), ERROR),
        _ => (t(platform::replace_system_hint()), MUTED),
    };
    ui.wrapped(hint, label_x + 46.0, y, right - label_x - 46.0, 12.0, color);

    // Saving
    y = ui.section(t(T::Saving), margin, 456.0, 160.0);
    ui.label(t(T::Folder), label_x, y + 8.0, 14.0, TEXT);
    let open_w = font::measure(t(T::Open), 13.0).0 + 24.0;
    let change_w = font::measure(t(T::Change), 13.0).0 + 24.0;
    let path_w = right - control_x - open_w - change_w - 16.0;
    let path = elide_start(&settings.folder.display().to_string(), 13.0, path_w - 20.0);
    ui.fill(SkRect::from_xywh(control_x, y, path_w, 32.0), 6.0, CONTROL);
    ui.label(&path, control_x + 10.0, y + 8.0, 13.0, MUTED);
    let x = control_x + path_w + 8.0;
    ui.button(Id::ChangeFolder, t(T::Change), x, y, false);
    ui.button(Id::OpenFolder, t(T::Open), x + change_w + 8.0, y, false);
    y += 44.0;
    ui.label(t(T::Format), label_x, y + 8.0, 14.0, TEXT);
    let mut x = control_x;
    for (i, (name, format)) in [("PNG", Format::Png), ("JPEG", Format::Jpeg)]
        .iter()
        .enumerate()
    {
        let w = 72.0;
        let selected = settings.format == *format;
        ui.segment(Id::Format(*format), name, x, y, w, selected, i == 0, i == 1);
        x += w;
    }
    y += 44.0;
    ui.switch(
        Id::QuickSave,
        t(T::QuickSave),
        label_x,
        y,
        settings.quick_save,
    );
    ui.label(t(T::QuickSaveHint), label_x + 46.0, y + 26.0, 12.0, MUTED);

    // Footer
    let footer_y = HEIGHT as f32 - 52.0;
    let version = format!("Snapt {}", env!("CARGO_PKG_VERSION"));
    ui.label(&version, margin, footer_y + 9.0, 13.0, MUTED);
    let link_x = margin + font::measure(&version, 13.0).0 + 16.0;
    ui.link(Id::Website, t(T::Website), link_x, footer_y + 9.0);
    let done_w = font::measure(t(T::Done), 13.0).0 + 40.0;
    ui.button(
        Id::Done,
        t(T::Done),
        WIDTH as f32 - margin - done_w,
        footer_y,
        true,
    );
}

/// Cuts the start off `text` until it fits in `width`, as `…/Pictures/Snapt`.
fn elide_start(text: &str, size: f32, width: f32) -> String {
    if font::measure(text, size).0 <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for start in 1..chars.len() {
        let candidate: String = std::iter::once('…')
            .chain(chars[start..].iter().copied())
            .collect();
        if font::measure(&candidate, size).0 <= width {
            return candidate;
        }
    }
    "…".into()
}

/// Drawing helpers in logical pixels; remembers clickable areas.
struct Ui<'a> {
    pixmap: &'a mut Pixmap,
    scale: f32,
    hover: Option<Id>,
    hits: Vec<(Id, SkRect)>,
}

impl Ui<'_> {
    fn rect(&self, r: Option<SkRect>) -> Option<SkRect> {
        let r = r?;
        let s = self.scale;
        SkRect::from_xywh(r.x() * s, r.y() * s, r.width() * s, r.height() * s)
    }

    fn fill(&mut self, r: Option<SkRect>, radius: f32, color: u32) {
        let r = self.rect(r);
        fill_round(self.pixmap, r, radius * self.scale, color, 1.0);
    }

    fn label(&mut self, text: &str, x: f32, y: f32, size: f32, color: u32) {
        let s = self.scale;
        font::draw(self.pixmap, text, x * s, y * s, size * s, color, 1.0);
    }

    /// Text broken into lines that fit `width`.
    fn wrapped(&mut self, text: &str, x: f32, y: f32, width: f32, size: f32, color: u32) {
        let mut line = String::new();
        let mut y = y;
        for word in text.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if font::measure(&candidate, size).0 > width && !line.is_empty() {
                self.label(&line, x, y, size, color);
                y += font::line_height(size);
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        self.label(&line, x, y, size, color);
    }

    fn hit(&mut self, id: Id, x: f32, y: f32, w: f32, h: f32) -> bool {
        if let Some(r) = SkRect::from_xywh(x, y, w, h) {
            self.hits.push((id, r));
        }
        self.hover == Some(id)
    }

    /// A card `height` tall with its title above, at `y`. Returns where the
    /// first row inside goes.
    fn section(&mut self, title: &str, x: f32, y: f32, height: f32) -> f32 {
        self.label(title, x + 2.0, y, 12.0, ACCENT);
        let top = y + 22.0;
        self.fill(
            SkRect::from_xywh(x, top, WIDTH as f32 - 2.0 * x, height),
            10.0,
            CARD,
        );
        top + 14.0
    }

    #[allow(clippy::too_many_arguments)]
    fn segment(
        &mut self,
        id: Id,
        text: &str,
        x: f32,
        y: f32,
        w: f32,
        selected: bool,
        first: bool,
        last: bool,
    ) {
        let hovered = self.hit(id, x, y, w, 32.0);
        let color = if selected {
            ACCENT
        } else if hovered {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        // Square inner edges by overdrawing rounded pieces from the neighbors' side.
        self.fill(SkRect::from_xywh(x, y, w, 32.0), 6.0, color);
        if !first {
            self.fill(SkRect::from_xywh(x, y, 8.0, 32.0), 0.0, color);
        }
        if !last {
            self.fill(SkRect::from_xywh(x + w - 8.0, y, 8.0, 32.0), 0.0, color);
            self.fill(SkRect::from_xywh(x + w - 0.5, y + 6.0, 1.0, 20.0), 0.0, BG);
        }
        let tw = font::measure(text, 13.0).0;
        self.label(text, x + (w - tw) / 2.0, y + 8.0, 13.0, TEXT);
    }

    fn switch(&mut self, id: Id, text: &str, x: f32, y: f32, on: bool) {
        let tw = font::measure(text, 14.0).0;
        let hovered = self.hit(id, x, y, 46.0 + tw, 24.0);
        let track = if on {
            ACCENT
        } else if hovered {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        self.fill(SkRect::from_xywh(x, y + 2.0, 36.0, 20.0), 10.0, track);
        let knob_x = if on { x + 18.0 } else { x + 2.0 };
        self.fill(SkRect::from_xywh(knob_x, y + 4.0, 16.0, 16.0), 8.0, TEXT);
        self.label(text, x + 46.0, y + 3.0, 14.0, TEXT);
    }

    fn field(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32, active: bool) {
        let hovered = self.hit(id, x, y, w, 32.0);
        if active {
            self.fill(
                SkRect::from_xywh(x - 2.0, y - 2.0, w + 4.0, 36.0),
                8.0,
                ACCENT,
            );
        }
        let color = if hovered && !active {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        self.fill(SkRect::from_xywh(x, y, w, 32.0), 6.0, color);
        let tw = font::measure(text, 14.0).0;
        let text_color = if active { MUTED } else { TEXT };
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 14.0, text_color);
    }

    fn button(&mut self, id: Id, text: &str, x: f32, y: f32, primary: bool) {
        let w = font::measure(text, 13.0).0 + if primary { 40.0 } else { 24.0 };
        let hovered = self.hit(id, x, y, w, 32.0);
        let color = match (primary, hovered) {
            (true, false) => ACCENT,
            (true, true) => 0x9D74F8,
            (false, false) => CONTROL,
            (false, true) => CONTROL_HOVER,
        };
        self.fill(SkRect::from_xywh(x, y, w, 32.0), 6.0, color);
        let tw = font::measure(text, 13.0).0;
        self.label(text, x + (w - tw) / 2.0, y + 8.0, 13.0, TEXT);
    }

    fn link(&mut self, id: Id, text: &str, x: f32, y: f32) {
        let (w, h) = font::measure(text, 13.0);
        let hovered = self.hit(id, x, y, w, h);
        self.label(text, x, y, 13.0, if hovered { 0xC4B5FD } else { 0xA78BFA });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_presses_become_hotkeys() {
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(
            hotkey_from(ctrl_shift, KeyCode::KeyS).as_deref(),
            Some("Ctrl+Shift+S")
        );
        assert_eq!(
            hotkey_from(ModifiersState::ALT, KeyCode::Digit9).as_deref(),
            Some("Alt+9")
        );
        assert_eq!(
            hotkey_from(ModifiersState::empty(), KeyCode::PrintScreen).as_deref(),
            Some("PrintScreen")
        );
        assert_eq!(
            hotkey_from(ModifiersState::empty(), KeyCode::F8).as_deref(),
            Some("F8")
        );
        // A bare letter would swallow normal typing.
        assert_eq!(hotkey_from(ModifiersState::empty(), KeyCode::KeyS), None);
    }

    #[test]
    fn long_paths_are_cut_from_the_start() {
        let path = "/home/someone/a/very/long/path/to/Pictures/Snapt";
        let short = elide_start(path, 13.0, 120.0);
        assert!(short.starts_with('…') && short.ends_with("Snapt"));
        assert!(font::measure(&short, 13.0).0 <= 120.0);
    }

    #[test]
    fn every_language_lays_out_inside_the_window() {
        for lang in Lang::ALL {
            crate::i18n::set(lang);
            let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
            let mut ui = Ui {
                pixmap: &mut pixmap,
                scale: 1.0,
                hover: None,
                hits: Vec::new(),
            };
            let update = UpdateStatus::Available("9.9.9".into());
            build(&mut ui, &Settings::default(), &update, false, None);
            for (id, r) in &ui.hits {
                assert!(
                    r.right() <= WIDTH as f32 - 20.0 && r.bottom() <= HEIGHT as f32,
                    "{id:?} {r:?} in {lang:?}"
                );
            }
        }
        crate::i18n::set(Lang::En);
    }
}
