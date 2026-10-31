//! The settings window.
//!
//! A small immediate-mode UI drawn with the same software renderer as the
//! overlay: every frame lays the controls out, draws them and keeps their
//! rectangles for hit testing. No GUI toolkit, so Snapt stays small.

use std::{num::NonZeroU32, rc::Rc};

use anyhow::{Context, Result};
use softbuffer::Surface;
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect as SkRect, Stroke, Transform};
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
    icon,
    paint::{fill_round, rgb},
    platform,
    settings::{ColorFormat, CopyAs, Feature, Format, ReplaceMode, Settings, Shortcut},
    tray::hotkey_label,
};

/// What the user changed. The app applies it and redraws the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Language(Option<Lang>),
    Autostart,
    ReplaceSystem,
    /// Whether Snapt asks before taking over from the system tool or just does it.
    ReplaceMode(ReplaceMode),
    /// Gives PrtSc, Win+Shift+S and the screenshot links back to the system tool.
    RestoreSystem,
    /// The shortcut field started (true) or stopped (false) listening for keys.
    Recording(bool),
    Hotkey(Shortcut, String),
    ResetHotkey(Shortcut),
    ColorFormat(ColorFormat),
    Feature(Feature),
    ChangeFolder,
    OpenFolder,
    Format(Format),
    CopyAs(CopyAs),
    CopyOnSelect,
    WindowOpensEditor,
    QuickSave,
    SaveOnCopy,
    OpenAfterSave,
    AutoUpdate,
    CheckUpdates,
    InstallUpdate,
    Website,
    Close,
}

/// The pages of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Shortcuts,
    Features,
    Editor,
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
    ReplaceMode(ReplaceMode),
    RestoreSystem,
    Hotkey(Shortcut),
    ResetHotkey(Shortcut),
    ColorFormat(ColorFormat),
    Feature(Feature),
    Tab(Tab),
    ChangeFolder,
    OpenFolder,
    Format(Format),
    CopyAs(CopyAs),
    CopyOnSelect,
    WindowOpensEditor,
    QuickSave,
    SaveOnCopy,
    OpenAfterSave,
    AutoUpdate,
    CheckUpdates,
    InstallUpdate,
    Website,
    LangMenu,
}

/// A message under the shortcut field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    ShortcutTaken,
    ShortcutInvalid,
    ReplaceFailed,
}

const WIDTH: f64 = 400.0;
/// Windows has one more row, for asking before replacing the Snipping Tool.
const HEIGHT: f64 = if platform::REPLACE_MODES {
    HEIGHT_WITH_MODES
} else {
    720.0
};
const HEIGHT_WITH_MODES: f64 = 760.0;
const MARGIN: f32 = 20.0;
/// The height of one settings row, and of the controls in it.
const ROW: f32 = 40.0;
const H: f32 = 28.0;
const MENU_W: f32 = 150.0;

const BG: u32 = 0x1C1C1F;
const LINE: u32 = 0x2C2C32;
const MENU: u32 = 0x26262B;
const CONTROL: u32 = 0x2E2E35;
const CONTROL_HOVER: u32 = 0x3A3A43;
const TEXT: u32 = 0xF4F4F6;
const MUTED: u32 = 0x9A9AA3;
const ACCENT: u32 = 0x8B5CF6;
const ACCENT_TEXT: u32 = 0xA78BFA;
const ERROR: u32 = 0xF87171;

pub struct SettingsWindow {
    // Declared before the context so the surface drops first.
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    cursor: (f32, f32),
    hover: Option<Id>,
    modifiers: ModifiersState,
    /// The shortcut field listening for keys.
    recording: Option<Shortcut>,
    menu_open: bool,
    tab: Tab,
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
            .with_window_icon(
                winit::window::Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE).ok(),
            )
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
            recording: None,
            menu_open: false,
            tab: Tab::General,
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
            WindowEvent::Focused(false) if self.recording.is_some() => {
                self.recording = None;
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
        self.recording = None;
        self.menu_open = false;
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
        let hover = self.hover;
        if std::mem::take(&mut self.menu_open) {
            // A click while the language list is open picks a language or just closes it.
            self.window.request_redraw();
            return match hover? {
                Id::Lang(lang) => Some(Action::Language(lang)),
                _ => None,
            };
        }
        let id = hover?;
        self.window.request_redraw();
        if !matches!(id, Id::Hotkey(_)) {
            self.recording = None;
        }
        Some(match id {
            Id::Lang(lang) => Action::Language(lang),
            Id::Autostart => Action::Autostart,
            Id::ReplaceSystem => Action::ReplaceSystem,
            Id::ReplaceMode(mode) => Action::ReplaceMode(mode),
            Id::RestoreSystem => Action::RestoreSystem,
            Id::Hotkey(which) => {
                self.recording = (self.recording != Some(which)).then_some(which);
                self.notice = None;
                Action::Recording(self.recording.is_some())
            }
            Id::ResetHotkey(which) => {
                self.notice = None;
                Action::ResetHotkey(which)
            }
            Id::ColorFormat(f) => Action::ColorFormat(f),
            Id::Feature(f) => Action::Feature(f),
            Id::Tab(tab) => {
                self.tab = tab;
                if self.recording.take().is_some() {
                    return Some(Action::Recording(false));
                }
                return None;
            }
            Id::ChangeFolder => Action::ChangeFolder,
            Id::OpenFolder => Action::OpenFolder,
            Id::Format(f) => Action::Format(f),
            Id::CopyAs(c) => Action::CopyAs(c),
            Id::CopyOnSelect => Action::CopyOnSelect,
            Id::WindowOpensEditor => Action::WindowOpensEditor,
            Id::QuickSave => Action::QuickSave,
            Id::SaveOnCopy => Action::SaveOnCopy,
            Id::OpenAfterSave => Action::OpenAfterSave,
            Id::AutoUpdate => Action::AutoUpdate,
            Id::CheckUpdates => Action::CheckUpdates,
            Id::InstallUpdate => Action::InstallUpdate,
            Id::Website => Action::Website,
            Id::LangMenu => {
                self.menu_open = true;
                return None;
            }
        })
    }

    fn key(&mut self, event: KeyEvent) -> Option<Action> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return None;
        };
        let Some(which) = self.recording else {
            if code == KeyCode::Escape && self.menu_open {
                self.menu_open = false;
                self.window.request_redraw();
                return None;
            }
            return matches!(code, KeyCode::Escape).then(|| self.close());
        };
        if code == KeyCode::Escape {
            self.recording = None;
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
        self.recording = None;
        self.window.request_redraw();
        // Backspace or Delete alone clears the shortcut; capture always keeps one.
        let clear = matches!(code, KeyCode::Backspace | KeyCode::Delete);
        if clear && self.modifiers.is_empty() && which != Shortcut::Capture {
            return Some(Action::Hotkey(which, String::new()));
        }
        match hotkey_from(self.modifiers, code) {
            Some(hotkey) => Some(Action::Hotkey(which, hotkey)),
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
        build(
            &mut ui,
            settings,
            update,
            self.tab,
            self.recording,
            self.notice,
            self.menu_open,
        );
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
    tab: Tab,
    recording: Option<Shortcut>,
    notice: Option<Notice>,
    menu_open: bool,
) {
    let left = MARGIN;
    let right = WIDTH as f32 - MARGIN;
    let mut y = 12.0;

    // The page tabs
    let tabs = [
        (Tab::General, t(T::General)),
        (Tab::Shortcuts, t(T::Shortcuts)),
        (Tab::Features, t(T::Features)),
        (Tab::Editor, t(T::Editor)),
    ];
    let seg_w = (right - left) / tabs.len() as f32;
    for (i, (id, name)) in tabs.iter().enumerate() {
        let x = left + i as f32 * seg_w;
        let last = i == tabs.len() - 1;
        ui.segment(Id::Tab(*id), name, x, y, seg_w, tab == *id, i == 0, last);
    }
    y += H + 6.0;
    y = ui.divider(y);

    let (menu_x, menu_y) = (right - MENU_W, y + 6.0);
    let menu_open = menu_open && tab == Tab::General;
    match tab {
        Tab::General => {
            y = general(ui, settings, notice, menu_open, platform::REPLACE_MODES, y);
        }
        Tab::Shortcuts => y = shortcuts(ui, settings, recording, notice, y),
        Tab::Features => {
            for (feature, name) in [
                (Feature::Recognize, t(T::Recognize)),
                (Feature::Scroll, t(T::Scroll)),
                (Feature::Record, t(T::Record)),
                (Feature::Pin, t(T::Pin)),
                (Feature::Search, t(T::Search)),
                (Feature::Print, t(T::Print)),
                (Feature::Picker, t(T::PickColor)),
            ] {
                let on = settings.is_enabled(feature);
                ui.switch(Id::Feature(feature), name, y, on);
                y += ROW;
            }
        }
        Tab::Editor => y = editor(ui, settings, y),
    }
    debug_assert!(y <= HEIGHT as f32 - 52.0, "the page runs into the footer");
    let y = ui.divider(HEIGHT as f32 - 53.0);

    // Footer: version, update status and links
    let (status, color) = match update {
        UpdateStatus::Idle => (format!("Snapt {}", env!("CARGO_PKG_VERSION")), MUTED),
        UpdateStatus::Checking => (t(T::Checking).into(), MUTED),
        UpdateStatus::UpToDate => (t(T::UpToDate).into(), MUTED),
        UpdateStatus::Available(v) => (t(T::UpdateAvailable).replace("{}", v), TEXT),
        UpdateStatus::Downloading => (t(T::Downloading).into(), MUTED),
        UpdateStatus::Failed => (t(T::UpdateFailed).into(), ERROR),
    };
    ui.label(&status, left, y + 9.0, 12.0, color);
    let site_w = font::measure(t(T::Website), 12.0).0;
    let mut x = right - site_w;
    ui.link(Id::Website, t(T::Website), x, y + 9.0, 12.0);
    match update {
        UpdateStatus::Available(_) => {
            let w = font::measure(t(T::InstallUpdate), 12.0).0 + 24.0;
            x -= w + 14.0;
            ui.button(Id::InstallUpdate, t(T::InstallUpdate), x, y + 2.0, true);
        }
        UpdateStatus::Checking | UpdateStatus::Downloading => {}
        _ => {
            x -= font::measure(t(T::CheckNow), 12.0).0 + 14.0;
            ui.link(Id::CheckUpdates, t(T::CheckNow), x, y + 9.0, 12.0);
        }
    }

    // The open language list goes over everything, and takes clicks first.
    if menu_open {
        let mut options = vec![(t(T::SystemLanguage), None)];
        options.extend(Lang::ALL.iter().map(|l| (l.name(), Some(*l))));
        let top = menu_y + 32.0;
        let item_h = 28.0;
        let h = item_h * options.len() as f32 + 8.0;
        let mut hits = std::mem::take(&mut ui.hits);
        ui.fill(
            SkRect::from_xywh(menu_x - 1.0, top - 1.0, MENU_W + 2.0, h + 2.0),
            7.0,
            LINE,
        );
        ui.fill(SkRect::from_xywh(menu_x, top, MENU_W, h), 6.0, MENU);
        for (i, (name, lang)) in options.iter().enumerate() {
            let iy = top + 4.0 + i as f32 * item_h;
            let hovered = ui.hit(Id::Lang(*lang), menu_x + 4.0, iy, MENU_W - 8.0, item_h);
            if hovered {
                ui.fill(
                    SkRect::from_xywh(menu_x + 4.0, iy, MENU_W - 8.0, item_h),
                    4.0,
                    CONTROL_HOVER,
                );
            }
            let color = if settings.language == *lang {
                ACCENT_TEXT
            } else {
                TEXT
            };
            ui.label(name, menu_x + 12.0, iy + 7.0, 13.0, color);
        }
        // Anywhere else closes the list.
        ui.hits.append(&mut hits);
    }
}

/// The editor page: a switch per drawing tool; returns where it ends.
fn editor(ui: &mut Ui, settings: &Settings, mut y: f32) -> f32 {
    let (left, right) = (MARGIN, WIDTH as f32 - MARGIN);
    for (feature, name) in [
        (Feature::Pen, t(T::Pen)),
        (Feature::Line, t(T::Line)),
        (Feature::Arrow, t(T::Arrow)),
        (Feature::Rect, t(T::Rectangle)),
        (Feature::Ellipse, t(T::Ellipse)),
        (Feature::Marker, t(T::Marker)),
        (Feature::Text, t(T::Text)),
        (Feature::Number, t(T::Number)),
        (Feature::Pixelate, t(T::Pixelate)),
        (Feature::Blur, t(T::Blur)),
        (Feature::Spotlight, t(T::Spotlight)),
        (Feature::Crop, t(T::Crop)),
    ] {
        ui.switch(Id::Feature(feature), name, y, settings.is_enabled(feature));
        y += ROW;
    }
    ui.wrapped(t(T::EditorHint), left, y + 4.0, right - left, 12.0, MUTED);
    y + 34.0
}

/// The general page; returns where it ends.
fn general(
    ui: &mut Ui,
    settings: &Settings,
    notice: Option<Notice>,
    menu_open: bool,
    replace_modes: bool,
    mut y: f32,
) -> f32 {
    let left = MARGIN;
    let right = WIDTH as f32 - MARGIN;
    // Language, start at login, updates
    ui.label(t(T::Language), left, y + 12.0, 13.0, TEXT);
    let lang_name = match settings.language {
        Some(lang) => lang.name(),
        None => t(T::SystemLanguage),
    };
    let menu_x = right - MENU_W;
    let menu_y = y + 6.0;
    ui.select(Id::LangMenu, lang_name, menu_x, menu_y, MENU_W, menu_open);
    y += ROW;
    ui.switch(Id::Autostart, t(T::StartAtLogin), y, settings.autostart);
    y += ROW;
    ui.switch(Id::AutoUpdate, t(T::AutoUpdate), y, settings.auto_update);
    y += ROW;
    y = ui.divider(y);

    ui.label(t(T::ColorFormat), left, y + 12.0, 13.0, TEXT);
    let seg_w = 60.0;
    let mut x = right - 2.0 * seg_w;
    for (i, (name, format)) in [("HEX", ColorFormat::Hex), ("RGB", ColorFormat::Rgb)]
        .iter()
        .enumerate()
    {
        let selected = settings.color_format == *format;
        let id = Id::ColorFormat(*format);
        ui.segment(id, name, x, y + 6.0, seg_w, selected, i == 0, i == 1);
        x += seg_w;
    }
    y += ROW;
    ui.switch(
        Id::ReplaceSystem,
        t(T::ReplaceSystem),
        y,
        settings.replace_system.is_on(),
    );
    y += ROW;
    if replace_modes {
        // Asking or not only matters while the switch is on, but the row stays
        // put so the page doesn't jump; picking a mode turns the switch on.
        ui.label(t(T::ReplaceWhen), left, y + 12.0, 13.0, TEXT);
        let choices = [
            (t(T::ReplaceAsk), ReplaceMode::Ask),
            (t(T::ReplaceAuto), ReplaceMode::Auto),
        ];
        let seg_w = choices
            .iter()
            .map(|(name, _)| font::measure(name, 12.0).0 + 24.0)
            .fold(60.0, f32::max);
        let mut x = right - 2.0 * seg_w;
        for (i, (name, mode)) in choices.iter().enumerate() {
            let selected = settings.replace_system == *mode;
            let id = Id::ReplaceMode(*mode);
            ui.segment(id, name, x, y + 6.0, seg_w, selected, i == 0, i == 1);
            x += seg_w;
        }
        y += ROW;
    }
    y -= 4.0;
    let (hint, color) = match notice {
        Some(Notice::ReplaceFailed) => (t(T::ReplaceFailed), ERROR),
        _ => (t(platform::replace_system_hint()), MUTED),
    };
    ui.wrapped(hint, left, y, right - left, 12.0, color);
    y += 34.0;
    if replace_modes {
        // Works whatever the switch says, e.g. after an old version left PrtSc off.
        ui.link(Id::RestoreSystem, t(T::RestoreSystem), left, y, 12.0);
        y += 22.0;
    }
    y = ui.divider(y);

    // Saving
    ui.label(t(T::Folder), left, y + 12.0, 13.0, TEXT);
    let open_w = font::measure(t(T::Open), 12.0).0 + 20.0;
    let path_w = 210.0;
    let path_x = right - open_w - 6.0 - path_w;
    let path = elide_start(&settings.folder.display().to_string(), 12.0, path_w - 20.0);
    ui.field_left(Id::ChangeFolder, &path, path_x, y + 6.0, path_w);
    ui.button(Id::OpenFolder, t(T::Open), right - open_w, y + 6.0, false);
    y += ROW;
    ui.label(t(T::Format), left, y + 12.0, 13.0, TEXT);
    let seg_w = 60.0;
    let mut x = right - 2.0 * seg_w;
    for (i, (name, format)) in [("PNG", Format::Png), ("JPEG", Format::Jpeg)]
        .iter()
        .enumerate()
    {
        let selected = settings.format == *format;
        ui.segment(
            Id::Format(*format),
            name,
            x,
            y + 6.0,
            seg_w,
            selected,
            i == 0,
            i == 1,
        );
        x += seg_w;
    }
    y += ROW;
    ui.label(t(T::CopyAs), left, y + 12.0, 13.0, TEXT);
    let choices = [
        (t(T::CopyImage), CopyAs::Image),
        (t(T::CopyFile), CopyAs::File),
    ];
    let seg_w = choices
        .iter()
        .map(|(name, _)| font::measure(name, 12.0).0 + 24.0)
        .fold(60.0, f32::max);
    let mut x = right - 2.0 * seg_w;
    for (i, (name, copy_as)) in choices.iter().enumerate() {
        let selected = settings.copy_as == *copy_as;
        let id = Id::CopyAs(*copy_as);
        ui.segment(id, name, x, y + 6.0, seg_w, selected, i == 0, i == 1);
        x += seg_w;
    }
    y += ROW;
    let instant = settings.copy_on_select;
    ui.switch(Id::CopyOnSelect, t(T::CopyOnSelect), y, instant);
    y += ROW;
    // Only matters while copying on select is on, but the row stays put so
    // the page doesn't jump.
    let editor = settings.window_opens_editor;
    ui.switch(Id::WindowOpensEditor, t(T::WindowOpensEditor), y, editor);
    y += ROW;
    ui.switch(Id::QuickSave, t(T::QuickSave), y, settings.quick_save);
    y += ROW;
    ui.switch(Id::SaveOnCopy, t(T::SaveOnCopy), y, settings.save_on_copy);
    y += ROW;
    let open = settings.open_after_save;
    ui.switch(Id::OpenAfterSave, t(T::OpenAfterSave), y, open);
    y + ROW
}

/// The shortcuts page: every action with its keys; returns where it ends.
fn shortcuts(
    ui: &mut Ui,
    settings: &Settings,
    recording: Option<Shortcut>,
    notice: Option<Notice>,
    mut y: f32,
) -> f32 {
    let left = MARGIN;
    let right = WIDTH as f32 - MARGIN;
    for which in Shortcut::ALL {
        let label = match which {
            Shortcut::Capture => t(T::TakeScreenshot),
            Shortcut::Pick => t(T::PickColor),
            Shortcut::FullScreen => t(T::FullScreen),
            Shortcut::RepeatArea => t(T::RepeatArea),
            Shortcut::PinLast => t(T::PinLast),
            Shortcut::ScrollPage => t(T::ScrollPage),
            Shortcut::Recognize => t(T::Recognize),
            Shortcut::Scroll => t(T::Scroll),
            Shortcut::Record => t(T::Record),
            Shortcut::Pin => t(T::Pin),
            Shortcut::Copy => t(T::Copy),
            Shortcut::Save => t(T::Save),
        };
        ui.label(label, left, y + 10.0, 13.0, TEXT);
        let active = recording == Some(which);
        let keys = settings.shortcut(which);
        let field = if active {
            t(T::PressShortcut).to_string()
        } else if keys.is_empty() {
            "—".to_string()
        } else {
            hotkey_label(keys)
        };
        let field_w = 130.0;
        ui.field(
            Id::Hotkey(which),
            &field,
            right - field_w,
            y + 4.0,
            field_w,
            active,
        );
        if keys != which.default_keys() && !active {
            let w = font::measure(t(T::Reset), 12.0).0;
            let x = right - field_w - 10.0 - w;
            ui.link(Id::ResetHotkey(which), t(T::Reset), x, y + 11.0, 12.0);
        }
        y += ROW - 4.0;
    }
    let (hint, color) = match notice {
        Some(Notice::ShortcutTaken) => (t(T::ShortcutTaken), ERROR),
        Some(Notice::ShortcutInvalid) => (t(T::ShortcutInvalid), ERROR),
        _ => (t(T::ShortcutsHint), MUTED),
    };
    ui.wrapped(hint, left, y + 4.0, right - left, 12.0, color);
    y + 34.0
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

    /// A thin line between groups; returns where the next group starts.
    fn divider(&mut self, y: f32) -> f32 {
        self.fill(
            SkRect::from_xywh(MARGIN, y + 6.0, WIDTH as f32 - 2.0 * MARGIN, 1.0),
            0.0,
            LINE,
        );
        y + 13.0
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
        let hovered = self.hit(id, x, y, w, H);
        let color = if selected {
            ACCENT
        } else if hovered {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        // Square inner edges by overdrawing rounded pieces from the neighbors' side.
        self.fill(SkRect::from_xywh(x, y, w, H), 6.0, color);
        if !first {
            self.fill(SkRect::from_xywh(x, y, 8.0, H), 0.0, color);
        }
        if !last {
            self.fill(SkRect::from_xywh(x + w - 8.0, y, 8.0, H), 0.0, color);
        }
        let tw = font::measure(text, 12.0).0;
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 12.0, TEXT);
    }

    /// A row with `text` on the left and a switch on the right; the whole row toggles.
    fn switch(&mut self, id: Id, text: &str, y: f32, on: bool) {
        let right = WIDTH as f32 - MARGIN;
        let hovered = self.hit(id, MARGIN, y + 4.0, right - MARGIN, ROW - 8.0);
        self.label(text, MARGIN, y + 12.0, 13.0, TEXT);
        let x = right - 34.0;
        let track = if on {
            ACCENT
        } else if hovered {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        self.fill(SkRect::from_xywh(x, y + 10.0, 34.0, 20.0), 10.0, track);
        let knob_x = if on { x + 16.0 } else { x + 2.0 };
        self.fill(SkRect::from_xywh(knob_x, y + 12.0, 16.0, 16.0), 8.0, TEXT);
    }

    /// A drop-down field showing the current choice.
    fn select(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32, open: bool) {
        let hovered = self.hit(id, x, y, w, H);
        let color = if hovered || open {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 6.0, color);
        self.label(text, x + 10.0, y + 7.0, 12.0, TEXT);
        self.chevron(x + w - 16.0, y + H / 2.0);
    }

    fn chevron(&mut self, cx: f32, cy: f32) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        pb.move_to((cx - 4.0) * s, (cy - 2.0) * s);
        pb.line_to(cx * s, (cy + 2.0) * s);
        pb.line_to((cx + 4.0) * s, (cy - 2.0) * s);
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        let [r, g, b] = rgb(MUTED);
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: 1.5 * s,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn field(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32, active: bool) {
        let hovered = self.hit(id, x, y, w, H);
        if active {
            self.fill(
                SkRect::from_xywh(x - 1.5, y - 1.5, w + 3.0, H + 3.0),
                7.0,
                ACCENT,
            );
        }
        let color = if hovered && !active {
            CONTROL_HOVER
        } else {
            CONTROL
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 6.0, color);
        let tw = font::measure(text, 12.0).0;
        let text_color = if active { MUTED } else { TEXT };
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 12.0, text_color);
    }

    /// A clickable field with left-aligned, muted text.
    fn field_left(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32) {
        let hovered = self.hit(id, x, y, w, H);
        let color = if hovered { CONTROL_HOVER } else { CONTROL };
        self.fill(SkRect::from_xywh(x, y, w, H), 6.0, color);
        self.label(text, x + 10.0, y + 7.0, 12.0, MUTED);
    }

    fn button(&mut self, id: Id, text: &str, x: f32, y: f32, primary: bool) {
        let tw = font::measure(text, 12.0).0;
        let w = tw + if primary { 24.0 } else { 20.0 };
        let hovered = self.hit(id, x, y, w, H);
        let color = match (primary, hovered) {
            (true, false) => ACCENT,
            (true, true) => 0x9D74F8,
            (false, false) => CONTROL,
            (false, true) => CONTROL_HOVER,
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 6.0, color);
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 12.0, TEXT);
    }

    fn link(&mut self, id: Id, text: &str, x: f32, y: f32, size: f32) {
        let (w, h) = font::measure(text, size);
        let hovered = self.hit(id, x, y, w, h);
        let color = if hovered { 0xC4B5FD } else { ACCENT_TEXT };
        self.label(text, x, y, size, color);
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
            let _lang = crate::snapshot::language(lang);
            let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
            let mut ui = Ui {
                pixmap: &mut pixmap,
                scale: 1.0,
                hover: None,
                hits: Vec::new(),
            };
            let update = UpdateStatus::Available("9.9.9".into());
            for tab in [Tab::General, Tab::Shortcuts, Tab::Features, Tab::Editor] {
                build(
                    &mut ui,
                    &Settings::default(),
                    &update,
                    tab,
                    None,
                    None,
                    true,
                );
            }
            for (id, r) in &ui.hits {
                assert!(
                    r.left() >= MARGIN - 0.5
                        && r.right() <= WIDTH as f32 - MARGIN + 0.5
                        && r.bottom() <= HEIGHT as f32 - 8.0,
                    "{id:?} {r:?} in {lang:?}"
                );
            }
        }
    }

    /// The Windows row for asking or replacing must fit too, though these tests run elsewhere.
    #[test]
    fn the_replace_modes_fit_in_every_language() {
        for lang in Lang::ALL {
            let _lang = crate::snapshot::language(lang);
            let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT_WITH_MODES as u32).unwrap();
            let mut ui = Ui {
                pixmap: &mut pixmap,
                scale: 1.0,
                hover: None,
                hits: Vec::new(),
            };
            let top = ui.divider(12.0 + H + 6.0);
            let settings = Settings::default();
            let end = general(&mut ui, &settings, None, false, true, top);
            assert!(
                end <= HEIGHT_WITH_MODES as f32 - 52.0,
                "ends at {end} in {lang:?}"
            );
            // The label must not run under the two choices.
            let label = font::measure(t(T::ReplaceWhen), 13.0).0;
            let first = ui
                .hits
                .iter()
                .find(|(id, _)| *id == Id::ReplaceMode(ReplaceMode::Ask))
                .map(|(_, r)| r.left())
                .unwrap();
            assert!(MARGIN + label + 12.0 <= first, "{lang:?}");
            for (id, r) in &ui.hits {
                assert!(
                    r.left() >= MARGIN - 0.5 && r.right() <= WIDTH as f32 - MARGIN + 0.5,
                    "{id:?} {r:?} in {lang:?}"
                );
            }
        }
    }

    // The hint under "Replace system screenshots" differs between systems
    // and desktops, so the reference is the one for Linux outside GNOME.
    #[cfg(target_os = "linux")]
    #[test]
    fn settings_window_snapshot() {
        if crate::platform::replace_system_hint() != T::ReplaceHintLinux {
            eprintln!("skipped: the reference is for desktops other than GNOME");
            return;
        }
        // Fixed values, so the picture doesn't depend on the OS or the user.
        let mut settings = Settings {
            folder: "/home/user/Pictures/Snapt".into(),
            ..Settings::default()
        };
        // The overlay keys say Cmd on macOS; fix them all to the Linux ones.
        for which in Shortcut::ALL {
            *settings.shortcut_mut(which) = which.default_keys().replace("Cmd", "Ctrl");
        }
        *settings.shortcut_mut(Shortcut::Capture) = "Ctrl+Shift+X".into();
        for (lang, name, tab) in [
            (Lang::En, "settings-en", Tab::General),
            (Lang::Ru, "settings-ru", Tab::General),
            (Lang::En, "settings-shortcuts-en", Tab::Shortcuts),
            (Lang::Ru, "settings-features-ru", Tab::Features),
            (Lang::Uk, "settings-editor-uk", Tab::Editor),
        ] {
            let _lang = crate::snapshot::language(lang);
            let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
            let mut ui = Ui {
                pixmap: &mut pixmap,
                scale: 1.0,
                hover: None,
                hits: Vec::new(),
            };
            ui.fill(
                SkRect::from_xywh(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
                0.0,
                BG,
            );
            // Not `Idle`, which shows the version and would change every release.
            build(
                &mut ui,
                &settings,
                &UpdateStatus::UpToDate,
                tab,
                None,
                None,
                false,
            );
            let image =
                image::RgbaImage::from_raw(WIDTH as u32, HEIGHT as u32, pixmap.take()).unwrap();
            crate::snapshot::assert_matches(name, &image);
        }
    }
}
