//! The settings window.
//!
//! A small immediate-mode UI drawn with the same software renderer as the
//! overlay: every frame lays the controls out, draws them and keeps their
//! rectangles for hit testing. No GUI toolkit, so Snapt stays small.
//!
//! Sections sit in a sidebar on the left; the page on the right scrolls when
//! it doesn't fit. Typing anywhere searches every section at once.

use std::{num::NonZeroU32, rc::Rc};

use anyhow::{Context, Result};
use softbuffer::Surface;
use tiny_skia::{
    FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Rect as SkRect,
    Stroke, Transform,
};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{CursorIcon, Window, WindowId},
};

use crate::{
    font,
    i18n::{Lang, T, t},
    icon,
    overlay::{Button, Tool, draw_icon},
    paint::{fill_round, rgb},
    platform,
    settings::{ColorFormat, CopyAs, Feature, Format, ReplaceMode, Settings, Shortcut, Theme},
    tray::hotkey_label,
};

/// What the user changed. The app applies it and redraws the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Language(Option<Lang>),
    Theme(Theme),
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
    FileName(String),
    History(usize),
    CopyAs(CopyAs),
    CopyOnSelect,
    WindowOpensEditor,
    CaptureCursor,
    QuickSave,
    SaveOnCopy,
    OpenAfterSave,
    AutoUpdate,
    CheckUpdates,
    InstallUpdate,
    /// Puts every setting of a section back to its default.
    Reset(Section),
    /// Saves every setting to a file the user picks.
    ExportSettings,
    /// Replaces every setting with those in a file the user picks.
    ImportSettings,
    /// Opens a web page.
    Link(&'static str),
    Close,
}

/// The sections in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Capture,
    Saving,
    Shortcuts,
    Features,
    Editor,
    System,
    About,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::General,
        Section::Capture,
        Section::Saving,
        Section::Shortcuts,
        Section::Features,
        Section::Editor,
        Section::System,
        Section::About,
    ];

    fn title(self) -> &'static str {
        t(match self {
            Section::General => T::General,
            Section::Capture => T::SecCapture,
            Section::Saving => T::SecSaving,
            Section::Shortcuts => T::Shortcuts,
            Section::Features => T::Features,
            Section::Editor => T::Editor,
            Section::System => T::SecSystem,
            Section::About => T::SecAbout,
        })
    }

    fn subtitle(self) -> &'static str {
        t(match self {
            Section::General => T::SubGeneral,
            Section::Capture => T::SubCapture,
            Section::Saving => T::SubSaving,
            Section::Shortcuts => T::ShortcutsHint,
            Section::Features => T::SubFeatures,
            Section::Editor => T::EditorHint,
            Section::System => T::SubSystem,
            Section::About => T::Description,
        })
    }

    /// Sections whose settings can be put back to the defaults in one go.
    fn resettable(self) -> bool {
        !matches!(self, Section::System | Section::About)
    }
}

/// The extra actions on the editor toolbar, as the Features section lists them.
pub const FEATURES: [Feature; 7] = [
    Feature::Recognize,
    Feature::Scroll,
    Feature::Record,
    Feature::Pin,
    Feature::Search,
    Feature::Print,
    Feature::Picker,
];

/// The drawing tools, as the Editor section shows them.
pub const TOOLS: [Feature; 12] = [
    Feature::Pen,
    Feature::Line,
    Feature::Arrow,
    Feature::Rect,
    Feature::Ellipse,
    Feature::Marker,
    Feature::Text,
    Feature::Number,
    Feature::Pixelate,
    Feature::Blur,
    Feature::Spotlight,
    Feature::Crop,
];

const LEVELER: &str = "https://vkardin.github.io/leveler/";
const VIDEO_GLOW: &str = "https://vkardin.github.io/video-glow/";
const SOURCE: &str = "https://github.com/vkardin/snapt";

/// Where the update check stands, shown on the About page.
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
    LangMenu,
    Theme(Theme),
    Autostart,
    ReplaceSystem,
    ReplaceMode(ReplaceMode),
    RestoreSystem,
    Hotkey(Shortcut),
    ResetHotkey(Shortcut),
    ColorFormat(ColorFormat),
    Feature(Feature),
    Section(Section),
    Search,
    ClearSearch,
    ChangeFolder,
    OpenFolder,
    Format(Format),
    FileName,
    History(usize),
    CopyAs(CopyAs),
    CopyOnSelect,
    WindowOpensEditor,
    CaptureCursor,
    QuickSave,
    SaveOnCopy,
    OpenAfterSave,
    AutoUpdate,
    CheckUpdates,
    InstallUpdate,
    Reset(Section),
    Link(&'static str),
    ExportSettings,
    ImportSettings,
}

/// A message shown in the window after something went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    ShortcutTaken,
    ShortcutInvalid,
    ReplaceFailed,
    ImportFailed,
}

/// The text field taking the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Search,
    FileName,
}

const WIDTH: f64 = 700.0;
const HEIGHT: f64 = 520.0;
const SIDEBAR: f32 = 200.0;
/// The page's left and right edges.
const LEFT: f32 = SIDEBAR + 28.0;
const RIGHT: f32 = WIDTH as f32 - 28.0;
/// The title and subtitle above the page stay put while it scrolls.
const HEADER: f32 = 76.0;
/// The height of a settings row without a hint, and of the controls in it.
const ROW: f32 = 44.0;
const H: f32 = 28.0;
const MENU_W: f32 = 150.0;
const HISTORY: [usize; 4] = [0, 5, 10, 20];

/// The colors of one theme, from the Snapt website.
#[derive(Debug, Clone, Copy)]
struct Palette {
    bg: u32,
    side: u32,
    line: u32,
    control: u32,
    control_hover: u32,
    /// The track of a switch that is off.
    track: u32,
    text: u32,
    muted: u32,
    accent: u32,
    accent_hover: u32,
    accent_text: u32,
    link_hover: u32,
    /// The selected sidebar item and turned-on tiles.
    pill: u32,
    error: u32,
}

const DARK: Palette = Palette {
    bg: 0x17132B,
    side: 0x0F0C1D,
    line: 0x2A2445,
    control: 0x231D3D,
    control_hover: 0x30284F,
    track: 0x3A3358,
    text: 0xECE9F7,
    muted: 0xA49DBD,
    accent: 0x8B5CF6,
    accent_hover: 0x9D74F8,
    accent_text: 0xA78BFA,
    link_hover: 0xC4B5FD,
    pill: 0x2B2150,
    error: 0xF87171,
};

const LIGHT: Palette = Palette {
    bg: 0xFFFFFF,
    side: 0xF6F4FE,
    line: 0xE6E1F5,
    control: 0xF1EDFC,
    control_hover: 0xE5DEFA,
    track: 0xD6D0E8,
    text: 0x1C1530,
    muted: 0x5D5675,
    accent: 0x7C3AED,
    accent_hover: 0x6D28D9,
    accent_text: 0x7C3AED,
    link_hover: 0x5B21B6,
    pill: 0xEDE7FD,
    error: 0xDC2626,
};

/// Everything a frame shows besides the settings themselves.
#[derive(Debug, Clone)]
struct View {
    section: Section,
    scroll: f32,
    recording: Option<Shortcut>,
    notice: Option<Notice>,
    menu_open: bool,
    query: String,
    focus: Option<Focus>,
    /// The file name as it is being typed.
    file_name: Option<String>,
    /// A section waiting for a second click on its reset link.
    confirm_reset: Option<Section>,
    dark: bool,
}

impl View {
    fn new() -> Self {
        Self {
            section: Section::General,
            scroll: 0.0,
            recording: None,
            notice: None,
            menu_open: false,
            query: String::new(),
            focus: None,
            file_name: None,
            confirm_reset: None,
            dark: true,
        }
    }
}

pub struct SettingsWindow {
    // Declared before the context so the surface drops first.
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    cursor: (f32, f32),
    hover: Option<Id>,
    modifiers: ModifiersState,
    view: View,
    /// The system is in dark mode; followed while the theme is `System`.
    system_dark: bool,
    /// The theme last handed to the window's title bar.
    title_theme: Option<Option<Theme>>,
    /// How tall the page is, to know how far it scrolls.
    page_height: f32,
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
        let system_dark = window.theme() != Some(winit::window::Theme::Light);
        Ok(Self {
            surface,
            window,
            _context: context,
            cursor: (-1.0, -1.0),
            hover: None,
            modifiers: ModifiersState::empty(),
            view: View::new(),
            system_dark,
            title_theme: None,
            page_height: 0.0,
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

    /// Handles an event; returns what the user changed, in order.
    pub fn window_event(
        &mut self,
        event: WindowEvent,
        settings: &Settings,
        update: &UpdateStatus,
    ) -> Vec<Action> {
        match event {
            WindowEvent::CloseRequested => return vec![self.close()],
            WindowEvent::RedrawRequested => self.redraw(settings, update),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.window.request_redraw();
            }
            WindowEvent::ThemeChanged(theme) => {
                self.system_dark = theme != winit::window::Theme::Light;
                self.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => self.cursor_moved(position),
            WindowEvent::CursorLeft { .. } => {
                self.cursor = (-1.0, -1.0);
                self.set_hover(None);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    MouseScrollDelta::PixelDelta(p) => (p.y / self.window.scale_factor()) as f32,
                };
                self.scroll_by(-dy);
            }
            WindowEvent::Focused(false) => {
                let mut actions = self.blur();
                if self.view.recording.take().is_some() {
                    actions.push(Action::Recording(false));
                }
                self.window.request_redraw();
                return actions;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => return self.click(),
            WindowEvent::KeyboardInput { event, .. } => return self.key(event, settings),
            _ => {}
        }
        Vec::new()
    }

    fn close(&mut self) -> Action {
        self.view.recording = None;
        self.view.menu_open = false;
        self.view.focus = None;
        self.view.file_name = None;
        self.view.confirm_reset = None;
        self.notice = None;
        self.window.set_visible(false);
        Action::Close
    }

    fn scroll_by(&mut self, dy: f32) {
        let max = (self.page_height - (HEIGHT as f32 - HEADER)).max(0.0);
        let scroll = (self.view.scroll + dy).clamp(0.0, max);
        if scroll != self.view.scroll {
            self.view.scroll = scroll;
            self.view.menu_open = false;
            self.window.request_redraw();
        }
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
            let icon = match hover {
                Some(Id::Search | Id::FileName) => CursorIcon::Text,
                Some(_) => CursorIcon::Pointer,
                None => CursorIcon::Default,
            };
            self.window.set_cursor(icon);
            self.window.request_redraw();
        }
    }

    /// Lets go of the text field; a typed file name is applied.
    fn blur(&mut self) -> Vec<Action> {
        let focus = self.view.focus.take();
        match self.view.file_name.take() {
            Some(name) if focus == Some(Focus::FileName) => vec![Action::FileName(name)],
            _ => Vec::new(),
        }
    }

    fn click(&mut self) -> Vec<Action> {
        let hover = self.hover;
        self.window.request_redraw();
        if std::mem::take(&mut self.view.menu_open) {
            // A click while the language list is open picks a language or just closes it.
            return match hover {
                Some(Id::Lang(lang)) => vec![Action::Language(lang)],
                _ => Vec::new(),
            };
        }
        let keeps_focus = match self.view.focus {
            Some(Focus::Search) => matches!(hover, Some(Id::Search | Id::ClearSearch)),
            Some(Focus::FileName) => hover == Some(Id::FileName),
            None => true,
        };
        let mut actions = if keeps_focus { Vec::new() } else { self.blur() };
        if !matches!(hover, Some(Id::Reset(_))) {
            self.view.confirm_reset = None;
        }
        let Some(id) = hover else {
            return actions;
        };
        if !matches!(id, Id::Hotkey(_)) && self.view.recording.take().is_some() {
            actions.push(Action::Recording(false));
        }
        let action = match id {
            Id::Lang(lang) => Action::Language(lang),
            Id::LangMenu => {
                self.view.menu_open = true;
                return actions;
            }
            Id::Theme(theme) => Action::Theme(theme),
            Id::Autostart => Action::Autostart,
            Id::ReplaceSystem => Action::ReplaceSystem,
            Id::ReplaceMode(mode) => Action::ReplaceMode(mode),
            Id::RestoreSystem => Action::RestoreSystem,
            Id::Hotkey(which) => {
                let listening = self.view.recording != Some(which);
                self.view.recording = listening.then_some(which);
                self.notice = None;
                Action::Recording(listening)
            }
            Id::ResetHotkey(which) => {
                self.notice = None;
                Action::ResetHotkey(which)
            }
            Id::ColorFormat(f) => Action::ColorFormat(f),
            Id::Feature(f) => Action::Feature(f),
            Id::Section(section) => {
                self.view.section = section;
                self.view.scroll = 0.0;
                self.view.query.clear();
                return actions;
            }
            Id::Search => {
                self.view.focus = Some(Focus::Search);
                return actions;
            }
            Id::ClearSearch => {
                self.view.query.clear();
                self.view.scroll = 0.0;
                return actions;
            }
            Id::FileName => {
                if self.view.focus != Some(Focus::FileName) {
                    self.view.focus = Some(Focus::FileName);
                    self.view.file_name = None;
                }
                return actions;
            }
            Id::ChangeFolder => Action::ChangeFolder,
            Id::OpenFolder => Action::OpenFolder,
            Id::Format(f) => Action::Format(f),
            Id::History(n) => Action::History(n),
            Id::CopyAs(c) => Action::CopyAs(c),
            Id::CopyOnSelect => Action::CopyOnSelect,
            Id::WindowOpensEditor => Action::WindowOpensEditor,
            Id::CaptureCursor => Action::CaptureCursor,
            Id::QuickSave => Action::QuickSave,
            Id::SaveOnCopy => Action::SaveOnCopy,
            Id::OpenAfterSave => Action::OpenAfterSave,
            Id::AutoUpdate => Action::AutoUpdate,
            Id::CheckUpdates => Action::CheckUpdates,
            Id::InstallUpdate => Action::InstallUpdate,
            Id::Reset(section) => {
                if self.view.confirm_reset.take() != Some(section) {
                    self.view.confirm_reset = Some(section);
                    return actions;
                }
                self.notice = None;
                Action::Reset(section)
            }
            Id::Link(url) => Action::Link(url),
            Id::ExportSettings => Action::ExportSettings,
            Id::ImportSettings => Action::ImportSettings,
        };
        actions.push(action);
        actions
    }

    fn key(&mut self, event: KeyEvent, settings: &Settings) -> Vec<Action> {
        if event.state != ElementState::Pressed {
            return Vec::new();
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return Vec::new();
        };
        if let Some(which) = self.view.recording {
            return self.record(which, code);
        }
        if self.view.menu_open {
            if code == KeyCode::Escape {
                self.view.menu_open = false;
                self.window.request_redraw();
            }
            return Vec::new();
        }
        let command = self.modifiers.control_key() || self.modifiers.super_key();
        let typed: String = event
            .text
            .as_deref()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        match self.view.focus {
            None => {
                match code {
                    KeyCode::Escape => return vec![self.close()],
                    KeyCode::PageDown => self.scroll_by(HEIGHT as f32 - HEADER - 40.0),
                    KeyCode::PageUp => self.scroll_by(-(HEIGHT as f32 - HEADER - 40.0)),
                    KeyCode::ArrowDown => self.scroll_by(40.0),
                    KeyCode::ArrowUp => self.scroll_by(-40.0),
                    KeyCode::Home => self.scroll_by(-self.page_height),
                    KeyCode::End => self.scroll_by(self.page_height),
                    // Typing anywhere starts a search.
                    _ if !typed.trim().is_empty() && !command && !self.modifiers.alt_key() => {
                        self.view.focus = Some(Focus::Search);
                        self.view.query.push_str(&typed);
                        self.view.scroll = 0.0;
                        self.window.request_redraw();
                    }
                    _ => {}
                }
                Vec::new()
            }
            Some(focus) => self.edit(focus, code, &typed, command, &settings.file_name),
        }
    }

    /// Types into the focused text field.
    fn edit(
        &mut self,
        focus: Focus,
        code: KeyCode,
        typed: &str,
        command: bool,
        file_name: &str,
    ) -> Vec<Action> {
        self.window.request_redraw();
        if focus == Focus::FileName && self.view.file_name.is_none() {
            self.view.file_name = Some(file_name.to_string());
        }
        let text = match focus {
            Focus::Search => &mut self.view.query,
            Focus::FileName => self.view.file_name.get_or_insert_with(String::new),
        };
        match code {
            KeyCode::Escape => {
                if focus == Focus::Search && !text.is_empty() {
                    text.clear();
                } else {
                    self.view.focus = None;
                    self.view.file_name = None;
                }
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Tab => return self.blur(),
            KeyCode::Backspace if command => text.clear(),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::KeyV if command => {
                let pasted = arboard::Clipboard::new()
                    .and_then(|mut c| c.get_text())
                    .unwrap_or_default();
                text.extend(pasted.chars().filter(|c| !c.is_control()));
            }
            KeyCode::KeyA | KeyCode::KeyX if command => text.clear(),
            _ if !command && !typed.is_empty() => text.push_str(typed),
            _ => {}
        }
        if focus == Focus::Search {
            self.view.scroll = 0.0;
        }
        Vec::new()
    }

    /// Turns a key press into the shortcut being recorded.
    fn record(&mut self, which: Shortcut, code: KeyCode) -> Vec<Action> {
        if code == KeyCode::Escape {
            self.view.recording = None;
            self.window.request_redraw();
            return vec![Action::Recording(false)];
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
            return Vec::new();
        }
        self.view.recording = None;
        self.window.request_redraw();
        // Backspace or Delete alone clears the shortcut; capture always keeps one.
        let clear = matches!(code, KeyCode::Backspace | KeyCode::Delete);
        if clear && self.modifiers.is_empty() && which != Shortcut::Capture {
            return vec![Action::Hotkey(which, String::new())];
        }
        match hotkey_from(self.modifiers, code) {
            Some(hotkey) => vec![Action::Hotkey(which, hotkey)],
            None => {
                self.notice = Some(Notice::ShortcutInvalid);
                vec![Action::Recording(false)]
            }
        }
    }

    fn redraw(&mut self, settings: &Settings, update: &UpdateStatus) {
        // The title bar follows the theme where the system lets it.
        let wanted = match settings.theme {
            Theme::System => None,
            theme => Some(theme),
        };
        if self.title_theme != Some(wanted) {
            self.title_theme = Some(wanted);
            self.window.set_theme(wanted.map(|theme| match theme {
                Theme::Light => winit::window::Theme::Light,
                _ => winit::window::Theme::Dark,
            }));
        }
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
        self.view.dark = match settings.theme {
            Theme::System => self.system_dark,
            Theme::Light => false,
            Theme::Dark => true,
        };
        self.view.notice = self.notice;
        let scale = self.window.scale_factor() as f32;
        let mut ui = Ui::new(&mut pixmap, scale, self.hover, self.view.dark);
        self.page_height = build(&mut ui, settings, update, &self.view);
        self.hits = ui.hits;
        // A page that got shorter, e.g. while searching, can't stay scrolled past its end.
        let max = (self.page_height - (HEIGHT as f32 - HEADER)).max(0.0);
        if self.view.scroll > max {
            self.view.scroll = max;
            self.window.request_redraw();
        }

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

/// Lays out and draws the whole window; returns how tall the page is.
/// Units are logical pixels.
fn build(ui: &mut Ui, settings: &Settings, update: &UpdateStatus, view: &View) -> f32 {
    let p = ui.p;
    ui.fill(
        SkRect::from_xywh(0.0, 0.0, WIDTH as f32, HEIGHT as f32),
        0.0,
        p.bg,
    );

    // The page first: the header and the sidebar are drawn over what scrolls away.
    let top = HEADER - view.scroll;
    let query = view.query.trim().to_lowercase();
    let mut y = top;
    if query.is_empty() {
        let mut page = Page::new(ui, y, None, None);
        section(&mut page, view.section, settings, update, view);
        y = page.y;
        if view.section.resettable() {
            let confirm = view.confirm_reset == Some(view.section);
            let text = t(if confirm {
                T::ResetConfirm
            } else {
                T::ResetSection
            });
            let w = font::measure(text, 12.0).0;
            // Rows end in a line of their own; the tiles don't.
            if view.section == Section::Editor {
                ui.divider(y + 4.0);
            }
            ui.link(
                Id::Reset(view.section),
                text,
                RIGHT - w,
                y + 22.0,
                12.0,
                confirm,
            );
            y += 48.0;
        }
    } else {
        let mut found = false;
        for s in Section::ALL {
            let mut page = Page::new(ui, y, Some(query.clone()), Some(s));
            section(&mut page, s, settings, update, view);
            found |= page.shown > 0;
            y = page.y;
        }
        if !found {
            ui.label(t(T::NothingFound), LEFT, y + 8.0, 13.0, p.muted);
            y += 40.0;
        }
    }
    let height = y + 16.0 - top;
    // Nothing under the header can be clicked.
    ui.hits.retain(|(_, r)| r.bottom() > HEADER + 1.0);
    for (_, r) in &mut ui.hits {
        if r.top() < HEADER {
            *r = SkRect::from_ltrb(r.left(), HEADER, r.right(), r.bottom()).unwrap_or(*r);
        }
    }

    // Header
    ui.fill(
        SkRect::from_xywh(SIDEBAR, 0.0, WIDTH as f32 - SIDEBAR, HEADER),
        0.0,
        p.bg,
    );
    let (title, subtitle) = if query.is_empty() {
        (view.section.title(), view.section.subtitle())
    } else {
        (t(T::SearchSettings), "")
    };
    ui.label(title, LEFT, 20.0, 19.0, p.text);
    let subtitle = elide_end(subtitle, 12.0, RIGHT - LEFT);
    ui.label(&subtitle, LEFT, 48.0, 12.0, p.muted);
    if view.scroll > 0.0 {
        ui.fill(
            SkRect::from_xywh(SIDEBAR, HEADER - 1.0, WIDTH as f32 - SIDEBAR, 1.0),
            0.0,
            p.line,
        );
    }

    // Scrollbar
    let visible = HEIGHT as f32 - HEADER;
    if height > visible {
        let track = visible - 12.0;
        let thumb = (track * visible / height).max(30.0);
        let at = HEADER + 6.0 + (track - thumb) * view.scroll / (height - visible);
        ui.fill(
            SkRect::from_xywh(WIDTH as f32 - 9.0, at, 4.0, thumb),
            2.0,
            p.track,
        );
    }

    sidebar(ui, view, query.is_empty());

    // The open language list goes over everything, and takes clicks first.
    if view.menu_open
        && let Some(anchor) = ui.anchor
    {
        let mut options = vec![(t(T::SystemLanguage), None)];
        options.extend(Lang::ALL.iter().map(|l| (l.name(), Some(*l))));
        let item_h = 28.0;
        let h = item_h * options.len() as f32 + 8.0;
        let (x, top) = (anchor.left(), anchor.bottom() + 4.0);
        let mut hits = std::mem::take(&mut ui.hits);
        ui.fill(
            SkRect::from_xywh(x - 1.0, top - 1.0, MENU_W + 2.0, h + 2.0),
            7.0,
            p.line,
        );
        ui.fill(SkRect::from_xywh(x, top, MENU_W, h), 6.0, p.side);
        for (i, (name, lang)) in options.iter().enumerate() {
            let iy = top + 4.0 + i as f32 * item_h;
            let hovered = ui.hit(Id::Lang(*lang), x + 4.0, iy, MENU_W - 8.0, item_h);
            if hovered {
                ui.fill(
                    SkRect::from_xywh(x + 4.0, iy, MENU_W - 8.0, item_h),
                    4.0,
                    p.control_hover,
                );
            }
            let color = if settings.language == *lang {
                p.accent_text
            } else {
                p.text
            };
            ui.label(name, x + 12.0, iy + 7.0, 13.0, color);
        }
        // Anywhere else closes the list.
        ui.hits.append(&mut hits);
    }
    height
}

/// The sidebar: logo, search and the sections.
fn sidebar(ui: &mut Ui, view: &View, browsing: bool) {
    let p = ui.p;
    ui.fill(
        SkRect::from_xywh(0.0, 0.0, SIDEBAR, HEIGHT as f32),
        0.0,
        p.side,
    );
    ui.fill(
        SkRect::from_xywh(SIDEBAR - 1.0, 0.0, 1.0, HEIGHT as f32),
        0.0,
        p.line,
    );
    ui.logo(18.0, 16.0, 26.0);
    ui.label("Snapt", 52.0, 20.0, 17.0, p.text);

    // Search
    let (x, y, w) = (14.0, 56.0, SIDEBAR - 28.0);
    let focused = view.focus == Some(Focus::Search);
    let hovered = ui.hit(Id::Search, x, y, w, 30.0);
    if focused {
        ui.fill(
            SkRect::from_xywh(x - 1.5, y - 1.5, w + 3.0, 33.0),
            8.0,
            p.accent,
        );
    }
    let color = if hovered && !focused {
        p.control_hover
    } else {
        p.control
    };
    ui.fill(SkRect::from_xywh(x, y, w, 30.0), 7.0, color);
    ui.glyph(Glyph::Search, x + 9.0, y + 7.0, 16.0, p.muted);
    if view.query.is_empty() {
        ui.label(t(T::SearchSettings), x + 32.0, y + 8.0, 12.0, p.muted);
        if focused {
            ui.caret(x + 32.0, y + 7.0);
        }
    } else {
        let text = elide_start(&view.query, 12.0, w - 58.0);
        ui.label(&text, x + 32.0, y + 8.0, 12.0, p.text);
        if focused {
            ui.caret(x + 33.0 + font::measure(&text, 12.0).0, y + 7.0);
        }
        let hovered = ui.hit(Id::ClearSearch, x + w - 26.0, y + 3.0, 24.0, 24.0);
        let color = if hovered { p.text } else { p.muted };
        ui.glyph(Glyph::Clear, x + w - 21.0, y + 8.0, 14.0, color);
    }

    // Sections
    let mut y = 100.0;
    for s in Section::ALL {
        if s == Section::About {
            y = HEIGHT as f32 - 14.0 - 34.0;
            ui.fill(
                SkRect::from_xywh(14.0, y - 8.0, SIDEBAR - 28.0, 1.0),
                0.0,
                p.line,
            );
        }
        let selected = browsing && view.section == s;
        let hovered = ui.hit(Id::Section(s), 10.0, y, SIDEBAR - 20.0, 34.0);
        if selected {
            ui.fill(
                SkRect::from_xywh(10.0, y, SIDEBAR - 20.0, 34.0),
                8.0,
                p.pill,
            );
        } else if hovered {
            ui.fill(
                SkRect::from_xywh(10.0, y, SIDEBAR - 20.0, 34.0),
                8.0,
                p.control,
            );
        }
        let ink = if selected { p.accent_text } else { p.muted };
        ui.section_icon(s, 22.0, y + 9.0, ink);
        ui.label(s.title(), 50.0, y + 10.0, 13.0, p.text);
        y += 36.0;
    }
}

/// Draws the rows of `section`.
fn section(page: &mut Page, s: Section, settings: &Settings, update: &UpdateStatus, view: &View) {
    match s {
        Section::General => general(page, settings, view),
        Section::Capture => capture(page, settings),
        Section::Saving => saving(page, settings, view),
        Section::Shortcuts => shortcuts(page, settings, view),
        Section::Features => features(page, settings),
        Section::Editor => editor(page, settings),
        Section::System => system(page, settings, view, platform::REPLACE_MODES),
        Section::About => about(page, settings, update),
    }
}

fn general(page: &mut Page, settings: &Settings, view: &View) {
    if page.matches(&[t(T::Language), "language", "язык", "мова"]) {
        let name = match settings.language {
            Some(lang) => lang.name(),
            None => t(T::SystemLanguage),
        };
        let y = page.row(t(T::Language), None);
        let x = RIGHT - MENU_W;
        page.ui
            .select(Id::LangMenu, name, x, y + 8.0, MENU_W, view.menu_open);
        page.ui.anchor = SkRect::from_xywh(x, y + 8.0, MENU_W, H);
    }
    page.choice(
        t(T::Theme),
        None,
        &[
            (
                t(T::ThemeSystem),
                Id::Theme(Theme::System),
                settings.theme == Theme::System,
            ),
            (
                t(T::ThemeLight),
                Id::Theme(Theme::Light),
                settings.theme == Theme::Light,
            ),
            (
                t(T::ThemeDark),
                Id::Theme(Theme::Dark),
                settings.theme == Theme::Dark,
            ),
        ],
    );
    page.switch(
        Id::Autostart,
        t(T::StartAtLogin),
        None,
        settings.autostart,
        true,
    );
    let (hint, failed) = match view.notice {
        Some(Notice::ImportFailed) => (t(T::ImportFailed), true),
        _ => (t(T::HintSettingsFile), false),
    };
    let texts = [
        t(T::SettingsFile),
        hint,
        "export",
        "import",
        "экспорт",
        "импорт",
        "експорт",
        "імпорт",
    ];
    if page.matches(&texts) {
        let import_w = font::measure(t(T::Import), 12.0).0 + 20.0;
        let export_w = font::measure(t(T::Export), 12.0).0 + 20.0;
        let hint_w = RIGHT - LEFT - import_w - export_w - 18.0;
        let p = page.ui.p;
        let tint = if failed { p.error } else { p.muted };
        let y = page.row_tinted(t(T::SettingsFile), Some(hint), 0.0, hint_w, tint);
        let x = RIGHT - import_w;
        page.ui
            .button(Id::ImportSettings, t(T::Import), x, y + 8.0, false);
        let x = x - 6.0 - export_w;
        page.ui
            .button(Id::ExportSettings, t(T::Export), x, y + 8.0, false);
    }
}

fn capture(page: &mut Page, settings: &Settings) {
    let on = settings.copy_on_select;
    page.switch(
        Id::CopyOnSelect,
        t(T::CopyOnSelect),
        Some(t(T::HintCopyOnSelect)),
        on,
        true,
    );
    // Only matters while copying on select is on.
    let editor = settings.window_opens_editor;
    let hint = Some(t(T::HintWindowOpensEditor));
    page.switch(
        Id::WindowOpensEditor,
        t(T::WindowOpensEditor),
        hint,
        editor,
        on,
    );
    let hint = Some(t(T::HintCaptureCursor));
    let pointer = settings.capture_cursor;
    page.switch(Id::CaptureCursor, t(T::CaptureCursor), hint, pointer, true);
    page.choice(
        t(T::ColorFormat),
        Some(t(T::HintColorFormat)),
        &[
            (
                "HEX",
                Id::ColorFormat(ColorFormat::Hex),
                settings.color_format == ColorFormat::Hex,
            ),
            (
                "RGB",
                Id::ColorFormat(ColorFormat::Rgb),
                settings.color_format == ColorFormat::Rgb,
            ),
        ],
    );
}

fn saving(page: &mut Page, settings: &Settings, view: &View) {
    page.group(t(T::Save));
    if page.matches(&[t(T::Folder)]) {
        let y = page.row(t(T::Folder), None);
        let open_w = font::measure(t(T::Open), 12.0).0 + 20.0;
        let path_w = 230.0;
        let path_x = RIGHT - open_w - 6.0 - path_w;
        let path = elide_start(&settings.folder.display().to_string(), 12.0, path_w - 20.0);
        page.ui
            .field_left(Id::ChangeFolder, &path, path_x, y + 8.0, path_w);
        page.ui
            .button(Id::OpenFolder, t(T::Open), RIGHT - open_w, y + 8.0, false);
    }
    page.choice(
        t(T::Format),
        None,
        &[
            (
                "PNG",
                Id::Format(Format::Png),
                settings.format == Format::Png,
            ),
            (
                "JPEG",
                Id::Format(Format::Jpeg),
                settings.format == Format::Jpeg,
            ),
        ],
    );
    if page.matches(&[t(T::FileName), t(T::HintFileName)]) {
        let focused = view.focus == Some(Focus::FileName);
        let typed = view.file_name.as_deref().filter(|_| focused);
        let template = typed.unwrap_or(&settings.file_name);
        let example = crate::output::file_name(chrono::Local::now(), template, settings.format);
        let hint = format!(
            "{}\n{}",
            t(T::Example).replace("{}", &example),
            t(T::HintFileName)
        );
        let y = page.row(t(T::FileName), Some(&hint));
        let w = 230.0;
        page.ui
            .text_field(Id::FileName, template, RIGHT - w, y + 8.0, w, focused);
    }
    let hint = Some(t(T::HintQuickSave));
    page.switch(
        Id::QuickSave,
        t(T::QuickSave),
        hint,
        settings.quick_save,
        true,
    );
    let hint = Some(t(T::HintSaveOnCopy));
    page.switch(
        Id::SaveOnCopy,
        t(T::SaveOnCopy),
        hint,
        settings.save_on_copy,
        true,
    );
    let open = settings.open_after_save;
    page.switch(Id::OpenAfterSave, t(T::OpenAfterSave), None, open, true);

    page.group(t(T::Copy));
    page.choice(
        t(T::CopyAs),
        Some(t(T::HintCopyAs)),
        &[
            (
                t(T::CopyImage),
                Id::CopyAs(CopyAs::Image),
                settings.copy_as == CopyAs::Image,
            ),
            (
                t(T::CopyFile),
                Id::CopyAs(CopyAs::File),
                settings.copy_as == CopyAs::File,
            ),
        ],
    );
    let names = ["", "5", "10", "20"];
    let choices: Vec<(&str, Id, bool)> = HISTORY
        .iter()
        .zip(names)
        .map(|(n, name)| {
            let name = if *n == 0 { t(T::Off) } else { name };
            (name, Id::History(*n), settings.history == *n)
        })
        .collect();
    page.choice(t(T::HistoryLimit), None, &choices);
}

fn shortcuts(page: &mut Page, settings: &Settings, view: &View) {
    for global in [true, false] {
        page.group(t(if global {
            T::GroupAnywhere
        } else {
            T::GroupInEditor
        }));
        for which in Shortcut::ALL
            .into_iter()
            .filter(|w| w.is_global() == global)
        {
            let label = shortcut_label(which);
            let keys = settings.shortcut(which);
            if !page.matches(&[label, keys]) {
                continue;
            }
            let y = page.row(label, None);
            let active = view.recording == Some(which);
            let field = if active {
                t(T::PressShortcut).to_string()
            } else if keys.is_empty() {
                "—".to_string()
            } else {
                hotkey_label(keys)
            };
            let field_w = 150.0;
            let x = RIGHT - field_w;
            page.ui
                .field(Id::Hotkey(which), &field, x, y + 8.0, field_w, active);
            if keys != which.default_keys() && !active {
                let w = font::measure(t(T::Reset), 12.0).0;
                let x = x - 12.0 - w;
                page.ui.link(
                    Id::ResetHotkey(which),
                    t(T::Reset),
                    x,
                    y + 15.0,
                    12.0,
                    false,
                );
            }
        }
    }
    let message = match view.notice {
        Some(Notice::ShortcutTaken) => Some(t(T::ShortcutTaken)),
        Some(Notice::ShortcutInvalid) => Some(t(T::ShortcutInvalid)),
        _ => None,
    };
    if let Some(message) = message
        && !page.searching()
    {
        let p = page.ui.p;
        page.y += page
            .ui
            .wrapped(message, LEFT, page.y + 6.0, RIGHT - LEFT, 12.0, p.error)
            + 12.0;
    }
}

fn shortcut_label(which: Shortcut) -> &'static str {
    t(match which {
        Shortcut::Capture => T::TakeScreenshot,
        Shortcut::Pick => T::PickColor,
        Shortcut::FullScreen => T::FullScreen,
        Shortcut::RepeatArea => T::RepeatArea,
        Shortcut::PinLast => T::PinLast,
        Shortcut::ScrollPage => T::ScrollPage,
        Shortcut::Recognize => T::Recognize,
        Shortcut::Scroll => T::Scroll,
        Shortcut::Record => T::Record,
        Shortcut::Pin => T::Pin,
        Shortcut::Copy => T::Copy,
        Shortcut::Save => T::Save,
    })
}

/// The name, toolbar icon and hint of an extra action.
fn feature_info(feature: Feature) -> (&'static str, Button, &'static str) {
    let (name, button, hint) = match feature {
        Feature::Recognize => (T::Recognize, Button::Recognize, T::HintRecognize),
        Feature::Scroll => (T::Scroll, Button::Scroll, T::HintScroll),
        Feature::Record => (T::Record, Button::Record, T::HintRecord),
        Feature::Pin => (T::Pin, Button::Pin, T::HintPin),
        Feature::Search => (T::Search, Button::Search, T::HintSearch),
        Feature::Print => (T::Print, Button::Print, T::HintPrint),
        _ => (T::PickColor, Button::Eyedropper, T::HintPicker),
    };
    (t(name), button, t(hint))
}

/// The name and toolbar icon of a drawing tool.
fn tool_info(feature: Feature) -> (&'static str, Button) {
    let (name, button) = match feature {
        Feature::Pen => (T::Pen, Button::Tool(Tool::Pen)),
        Feature::Line => (T::Line, Button::Tool(Tool::Line)),
        Feature::Arrow => (T::Arrow, Button::Tool(Tool::Arrow)),
        Feature::Rect => (T::Rectangle, Button::Tool(Tool::Rect)),
        Feature::Ellipse => (T::Ellipse, Button::Tool(Tool::Ellipse)),
        Feature::Marker => (T::Marker, Button::Tool(Tool::Marker)),
        Feature::Text => (T::Text, Button::Tool(Tool::Text)),
        Feature::Number => (T::Number, Button::Tool(Tool::Number)),
        Feature::Pixelate => (T::Pixelate, Button::Tool(Tool::Pixelate)),
        Feature::Blur => (T::Blur, Button::Tool(Tool::Blur)),
        Feature::Spotlight => (T::Spotlight, Button::Tool(Tool::Spotlight)),
        _ => (T::Crop, Button::Crop),
    };
    (t(name), button)
}

fn features(page: &mut Page, settings: &Settings) {
    for feature in FEATURES {
        let (name, button, hint) = feature_info(feature);
        if !page.matches(&[name, hint]) {
            continue;
        }
        let on = settings.is_enabled(feature);
        let y = page.y;
        page.switch_row(Id::Feature(feature), name, Some(hint), on, true, 30.0);
        let ink = if on {
            page.ui.p.accent_text
        } else {
            page.ui.p.muted
        };
        page.ui.tool_icon(button, LEFT + 1.0, y + 12.0, 18.0, ink);
    }
}

fn editor(page: &mut Page, settings: &Settings) {
    let tools: Vec<Feature> = TOOLS
        .into_iter()
        .filter(|f| page.matches_quietly(&[tool_info(*f).0]))
        .collect();
    if tools.is_empty() {
        return;
    }
    if page.searching() {
        page.reveal();
    }
    let (columns, gap, tile_h) = (4, 10.0, 70.0);
    let tile_w = (RIGHT - LEFT - gap * (columns - 1) as f32) / columns as f32;
    let top = page.y + 8.0;
    for (i, feature) in tools.iter().enumerate() {
        let x = LEFT + (i % columns) as f32 * (tile_w + gap);
        let y = top + (i / columns) as f32 * (tile_h + gap);
        let (name, button) = tool_info(*feature);
        page.ui.tile(
            Id::Feature(*feature),
            name,
            button,
            x,
            y,
            tile_w,
            tile_h,
            settings.is_enabled(*feature),
        );
    }
    let rows = tools.len().div_ceil(columns);
    page.y = top + rows as f32 * (tile_h + gap) + 4.0;
}

fn system(page: &mut Page, settings: &Settings, view: &View, replace_modes: bool) {
    let (hint, error) = match view.notice {
        Some(Notice::ReplaceFailed) => (t(T::ReplaceFailed), true),
        _ => (t(platform::replace_system_hint()), false),
    };
    let on = settings.replace_system.is_on();
    if page.matches(&[t(T::ReplaceSystem), hint, "prtsc", "print screen"]) {
        let y = page.y;
        page.switch_row(
            Id::ReplaceSystem,
            t(T::ReplaceSystem),
            Some(hint),
            on,
            true,
            0.0,
        );
        if error {
            // Draw the hint again in red over the muted one.
            let p = page.ui.p;
            page.ui.fill(
                SkRect::from_xywh(LEFT, y + 34.0, RIGHT - LEFT, page.y - y - 38.0),
                0.0,
                p.bg,
            );
            page.ui
                .wrapped(hint, LEFT, y + 34.0, RIGHT - LEFT, 12.0, p.error);
        }
    }
    if replace_modes {
        // Asking or not only matters while the switch is on, but the row stays
        // put so the page doesn't jump; picking a mode turns the switch on.
        page.choice(
            t(T::ReplaceWhen),
            None,
            &[
                (
                    t(T::ReplaceAsk),
                    Id::ReplaceMode(ReplaceMode::Ask),
                    settings.replace_system == ReplaceMode::Ask,
                ),
                (
                    t(T::ReplaceAuto),
                    Id::ReplaceMode(ReplaceMode::Auto),
                    settings.replace_system == ReplaceMode::Auto,
                ),
            ],
        );
        // Works whatever the switch says, e.g. after an old version left PrtSc off.
        if page.matches(&[t(T::RestoreSystem)]) {
            let y = page.y;
            page.ui.link(
                Id::RestoreSystem,
                t(T::RestoreSystem),
                LEFT,
                y + 10.0,
                13.0,
                false,
            );
            page.y += 40.0;
        }
    }
}

fn about(page: &mut Page, settings: &Settings, update: &UpdateStatus) {
    let p = page.ui.p;
    let version = t(T::Version).replace("{}", env!("CARGO_PKG_VERSION"));
    if !page.searching() {
        let y = page.y + 6.0;
        page.ui.logo(LEFT, y, 52.0);
        page.ui.label("Snapt", LEFT + 68.0, y + 4.0, 20.0, p.text);
        page.ui
            .label(&version, LEFT + 68.0, y + 32.0, 12.0, p.muted);
        page.y = y + 70.0;
        let h = page
            .ui
            .wrapped(t(T::License), LEFT, page.y, RIGHT - LEFT, 12.0, p.muted);
        page.y += h + 10.0;
        let mut x = LEFT;
        for (text, url) in [
            (t(T::Website), env!("CARGO_PKG_HOMEPAGE")),
            ("GitHub", SOURCE),
        ] {
            page.ui.link(Id::Link(url), text, x, page.y, 13.0, false);
            x += font::measure(text, 13.0).0 + 20.0;
        }
        page.y += 26.0;
    }

    page.group(t(T::Updates));
    let (status, color) = match update {
        UpdateStatus::Idle => (version.clone(), p.muted),
        UpdateStatus::Checking => (t(T::Checking).into(), p.muted),
        UpdateStatus::UpToDate => (t(T::UpToDate).into(), p.muted),
        UpdateStatus::Available(v) => (t(T::UpdateAvailable).replace("{}", v), p.text),
        UpdateStatus::Downloading => (t(T::Downloading).into(), p.muted),
        UpdateStatus::Failed => (t(T::UpdateFailed).into(), p.error),
    };
    if page.matches(&[t(T::CheckNow), t(T::InstallUpdate), &status]) {
        let y = page.y;
        let w = match update {
            UpdateStatus::Available(_) => {
                let w = font::measure(t(T::InstallUpdate), 12.0).0 + 24.0;
                page.ui.button(
                    Id::InstallUpdate,
                    t(T::InstallUpdate),
                    RIGHT - w,
                    y + 8.0,
                    true,
                );
                w
            }
            UpdateStatus::Checking | UpdateStatus::Downloading => 0.0,
            _ => {
                let w = font::measure(t(T::CheckNow), 12.0).0 + 20.0;
                page.ui
                    .button(Id::CheckUpdates, t(T::CheckNow), RIGHT - w, y + 8.0, false);
                w
            }
        };
        let status = elide_end(&status, 13.0, RIGHT - LEFT - w - 12.0);
        page.ui.label(&status, LEFT, y + 14.0, 13.0, color);
        page.y += ROW;
    }
    let on = settings.auto_update;
    page.switch(Id::AutoUpdate, t(T::AutoUpdate), None, on, true);

    page.group(t(T::MoreFromAuthor));
    for (name, about, url) in [
        ("Leveler", t(T::LevelerAbout), LEVELER),
        ("Video Glow", t(T::VideoGlowAbout), VIDEO_GLOW),
    ] {
        if !page.matches(&[name, about]) {
            continue;
        }
        let y = page.y + 4.0;
        let hovered = page.ui.hit(Id::Link(url), LEFT, y, RIGHT - LEFT, 54.0);
        let color = if hovered { p.control_hover } else { p.control };
        page.ui
            .fill(SkRect::from_xywh(LEFT, y, RIGHT - LEFT, 54.0), 10.0, color);
        page.ui.label(name, LEFT + 14.0, y + 10.0, 13.0, p.text);
        let about = elide_end(about, 12.0, RIGHT - LEFT - 28.0);
        page.ui.label(&about, LEFT + 14.0, y + 30.0, 12.0, p.muted);
        page.y = y + 62.0;
    }
}

/// Lays rows out one under another, and in a search keeps only those that match.
struct Page<'u, 'a> {
    ui: &'u mut Ui<'a>,
    y: f32,
    /// The search, lowercase; `None` shows everything.
    filter: Option<String>,
    /// The section, while searching; its title goes above its first match.
    section: Option<Section>,
    heading_drawn: bool,
    /// The section's own name matches, so all of it shows.
    whole: bool,
    /// How many rows matched.
    shown: usize,
    /// Something was drawn already.
    started: bool,
}

impl<'u, 'a> Page<'u, 'a> {
    fn new(ui: &'u mut Ui<'a>, y: f32, filter: Option<String>, section: Option<Section>) -> Self {
        let whole = match (&filter, section) {
            (Some(f), Some(s)) => s.title().to_lowercase().contains(f.as_str()),
            _ => false,
        };
        Self {
            ui,
            y,
            filter,
            section,
            heading_drawn: false,
            whole,
            shown: 0,
            started: false,
        }
    }

    fn searching(&self) -> bool {
        self.filter.is_some()
    }

    /// Whether a row with these texts shows, without drawing anything.
    fn matches_quietly(&self, texts: &[&str]) -> bool {
        let Some(filter) = &self.filter else {
            return true;
        };
        self.whole
            || texts
                .iter()
                .any(|t| t.to_lowercase().contains(filter.as_str()))
    }

    /// Whether a row with these texts shows; the first one in a search puts
    /// the section's title above it.
    fn matches(&mut self, texts: &[&str]) -> bool {
        if !self.matches_quietly(texts) {
            return false;
        }
        self.reveal()
    }

    /// Counts a match and draws the heading above the section's first one.
    fn reveal(&mut self) -> bool {
        self.shown += 1;
        if let Some(section) = self.section.filter(|_| !self.heading_drawn) {
            self.heading_drawn = true;
            let p = self.ui.p;
            self.ui
                .label(section.title(), LEFT, self.y + 14.0, 12.0, p.accent_text);
            self.y += 36.0;
        }
        true
    }

    /// A caption over a group of rows; left out of search results.
    fn group(&mut self, title: &str) {
        if self.searching() {
            return;
        }
        let p = self.ui.p;
        // Space above every group but the first.
        if self.started {
            self.y += 14.0;
        }
        self.started = true;
        self.ui
            .label(&title.to_uppercase(), LEFT, self.y + 8.0, 11.0, p.muted);
        self.y += 28.0;
    }

    /// Starts a row: draws its label and hint, moves past it, and returns its top.
    fn row(&mut self, label: &str, hint: Option<&str>) -> f32 {
        self.row_at(label, hint, 0.0, RIGHT - LEFT - 170.0)
    }

    fn row_at(&mut self, label: &str, hint: Option<&str>, indent: f32, hint_w: f32) -> f32 {
        let muted = self.ui.p.muted;
        self.row_tinted(label, hint, indent, hint_w, muted)
    }

    /// A row whose hint is in `tint`, such as the error color.
    fn row_tinted(
        &mut self,
        label: &str,
        hint: Option<&str>,
        indent: f32,
        hint_w: f32,
        tint: u32,
    ) -> f32 {
        self.started = true;
        let p = self.ui.p;
        let y = self.y;
        self.ui.label(label, LEFT + indent, y + 14.0, 13.0, p.text);
        let mut h = ROW;
        if let Some(hint) = hint {
            let lines = self
                .ui
                .wrapped(hint, LEFT + indent, y + 34.0, hint_w - indent, 12.0, tint);
            h = 34.0 + lines + 10.0;
        }
        self.ui.fill(
            SkRect::from_xywh(LEFT, y + h - 0.5, RIGHT - LEFT, 1.0),
            0.0,
            p.line,
        );
        self.y += h;
        y
    }

    /// A row with a switch on the right; the whole row toggles.
    fn switch(&mut self, id: Id, label: &str, hint: Option<&str>, on: bool, enabled: bool) {
        if self.matches(&[label, hint.unwrap_or_default()]) {
            self.switch_row(id, label, hint, on, enabled, 0.0);
        }
    }

    fn switch_row(
        &mut self,
        id: Id,
        label: &str,
        hint: Option<&str>,
        on: bool,
        enabled: bool,
        indent: f32,
    ) {
        let p = self.ui.p;
        let y = self.y;
        // Hints run the full width, under the switch too.
        let hint_w = RIGHT - LEFT - 56.0;
        if enabled {
            self.row_at(label, hint, indent, hint_w);
        } else {
            let label_color = p.muted;
            self.ui
                .label(label, LEFT + indent, y + 14.0, 13.0, label_color);
            let mut h = ROW;
            if let Some(hint) = hint {
                h =
                    34.0 + self.ui.wrapped(
                        hint,
                        LEFT + indent,
                        y + 34.0,
                        hint_w - indent,
                        12.0,
                        p.muted,
                    ) + 10.0;
            }
            self.ui.fill(
                SkRect::from_xywh(LEFT, y + h - 0.5, RIGHT - LEFT, 1.0),
                0.0,
                p.line,
            );
            self.y += h;
        }
        let hovered = enabled
            && self
                .ui
                .hit(id, LEFT, y + 2.0, RIGHT - LEFT, self.y - y - 4.0);
        let x = RIGHT - 36.0;
        let track = match (on, enabled, hovered) {
            (true, true, false) => p.accent,
            (true, true, true) => p.accent_hover,
            (true, false, _) => p.pill,
            (false, _, true) => p.control_hover,
            (false, _, false) => p.track,
        };
        self.ui
            .fill(SkRect::from_xywh(x, y + 12.0, 36.0, 20.0), 10.0, track);
        let knob_x = if on { x + 18.0 } else { x + 2.0 };
        let knob = if enabled { 0xFFFFFF } else { p.control };
        self.ui
            .fill(SkRect::from_xywh(knob_x, y + 14.0, 16.0, 16.0), 8.0, knob);
    }

    /// A row with a segmented choice on the right.
    fn choice(&mut self, label: &str, hint: Option<&str>, items: &[(&str, Id, bool)]) {
        let mut texts: Vec<&str> = vec![label, hint.unwrap_or_default()];
        texts.extend(items.iter().map(|(name, _, _)| *name));
        if !self.matches(&texts) {
            return;
        }
        let seg_w = items
            .iter()
            .map(|(name, _, _)| font::measure(name, 12.0).0 + 24.0)
            .fold(56.0, f32::max);
        let width = seg_w * items.len() as f32;
        let y = self.row_at(label, hint, 0.0, RIGHT - LEFT);
        let mut x = RIGHT - width;
        for (i, (name, id, selected)) in items.iter().enumerate() {
            let last = i == items.len() - 1;
            self.ui
                .segment(*id, name, x, y + 8.0, seg_w, *selected, i == 0, last);
            x += seg_w;
        }
    }
}

/// The small icons drawn in code for the sidebar and fields.
#[derive(Debug, Clone, Copy)]
enum Glyph {
    Search,
    Clear,
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

/// Cuts the end off `text` until it fits in `width`, as `A long sente…`.
fn elide_end(text: &str, size: f32, width: f32) -> String {
    if font::measure(text, size).0 <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for end in (0..chars.len()).rev() {
        let candidate: String = chars[..end].iter().copied().chain(['…']).collect();
        if font::measure(&candidate, size).0 <= width {
            return candidate;
        }
    }
    "…".into()
}

/// Breaks `text` into lines that fit `width`; `\n` always breaks.
fn wrap(text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if font::measure(&candidate, size).0 > width && !line.is_empty() {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// Drawing helpers in logical pixels; remembers clickable areas.
struct Ui<'a> {
    pixmap: &'a mut Pixmap,
    scale: f32,
    hover: Option<Id>,
    hits: Vec<(Id, SkRect)>,
    p: Palette,
    /// Where the language list opens.
    anchor: Option<SkRect>,
}

impl<'a> Ui<'a> {
    fn new(pixmap: &'a mut Pixmap, scale: f32, hover: Option<Id>, dark: bool) -> Self {
        Self {
            pixmap,
            scale,
            hover,
            hits: Vec::new(),
            p: if dark { DARK } else { LIGHT },
            anchor: None,
        }
    }

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

    /// Text broken into lines that fit `width`; returns how tall it is.
    fn wrapped(&mut self, text: &str, x: f32, y: f32, width: f32, size: f32, color: u32) -> f32 {
        let lines = wrap(text, size, width);
        let line_h = font::line_height(size);
        for (i, line) in lines.iter().enumerate() {
            self.label(line, x, y + i as f32 * line_h, size, color);
        }
        lines.len() as f32 * line_h
    }

    fn hit(&mut self, id: Id, x: f32, y: f32, w: f32, h: f32) -> bool {
        if let Some(r) = SkRect::from_xywh(x, y, w, h) {
            self.hits.push((id, r));
        }
        self.hover == Some(id)
    }

    /// A thin line across the page.
    fn divider(&mut self, y: f32) {
        let line = self.p.line;
        self.fill(SkRect::from_xywh(LEFT, y, RIGHT - LEFT, 1.0), 0.0, line);
    }

    /// The Snapt icon, `size` across.
    fn logo(&mut self, x: f32, y: f32, size: f32) {
        let mut data = icon::rgba();
        for px in data.chunks_exact_mut(4) {
            let a = px[3] as u16;
            for c in &mut px[..3] {
                *c = (*c as u16 * a / 255) as u8;
            }
        }
        let Some(source) = tiny_skia::IntSize::from_wh(icon::SIZE, icon::SIZE)
            .and_then(|size| Pixmap::from_vec(data, size))
        else {
            return;
        };
        let k = size * self.scale / icon::SIZE as f32;
        let paint = PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        };
        let transform = Transform::from_row(k, 0.0, 0.0, k, x * self.scale, y * self.scale);
        self.pixmap
            .draw_pixmap(0, 0, source.as_ref(), &paint, transform, None);
    }

    /// A toolbar icon, `size` across.
    fn tool_icon(&mut self, button: Button, x: f32, y: f32, size: f32, ink: u32) {
        let k = size / 16.0 * self.scale;
        let transform = Transform::from_row(k, 0.0, 0.0, k, x * self.scale, y * self.scale);
        draw_icon(self.pixmap, button, ink, ink, false, transform);
    }

    fn stroke(&mut self, path: PathBuilder, ink: u32, transform: Transform, width: f32) {
        let Some(path) = path.finish() else { return };
        let mut paint = Paint::default();
        let [r, g, b] = rgb(ink);
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, transform, None);
    }

    fn glyph(&mut self, glyph: Glyph, x: f32, y: f32, size: f32, ink: u32) {
        let k = size / 16.0 * self.scale;
        let transform = Transform::from_row(k, 0.0, 0.0, k, x * self.scale, y * self.scale);
        let mut p = PathBuilder::new();
        match glyph {
            Glyph::Search => {
                p.push_circle(7.0, 7.0, 4.5);
                p.move_to(10.3, 10.3);
                p.line_to(13.8, 13.8);
            }
            Glyph::Clear => {
                p.move_to(4.0, 4.0);
                p.line_to(12.0, 12.0);
                p.move_to(12.0, 4.0);
                p.line_to(4.0, 12.0);
            }
        }
        self.stroke(p, ink, transform, 1.6);
    }

    /// The icon of a sidebar section, 16 across.
    fn section_icon(&mut self, section: Section, x: f32, y: f32, ink: u32) {
        let s = self.scale;
        let transform = Transform::from_row(s, 0.0, 0.0, s, x * s, y * s);
        let mut p = PathBuilder::new();
        match section {
            Section::General => {
                // Two sliders.
                p.move_to(2.0, 5.0);
                p.line_to(14.0, 5.0);
                p.move_to(2.0, 11.0);
                p.line_to(14.0, 11.0);
                p.push_circle(5.5, 5.0, 2.0);
                p.push_circle(10.5, 11.0, 2.0);
            }
            Section::Capture => {
                // The corners of a selection.
                for (cx, cy, dx, dy) in [
                    (2.0, 2.0, 1.0, 1.0),
                    (14.0, 2.0, -1.0, 1.0),
                    (2.0, 14.0, 1.0, -1.0),
                    (14.0, 14.0, -1.0, -1.0),
                ] {
                    p.move_to(cx, cy + 4.0 * dy);
                    p.line_to(cx, cy);
                    p.line_to(cx + 4.0 * dx, cy);
                }
                p.push_circle(8.0, 8.0, 1.2);
            }
            Section::Saving => return self.tool_icon(Button::Save, x, y, 16.0, ink),
            Section::Shortcuts => {
                // A keyboard.
                p.push_rect(SkRect::from_xywh(1.5, 3.5, 13.0, 9.0).unwrap());
                for kx in [4.5, 7.0, 9.5, 12.0] {
                    p.move_to(kx - 0.3, 6.3);
                    p.line_to(kx + 0.3, 6.3);
                }
                p.move_to(5.0, 9.6);
                p.line_to(11.0, 9.6);
            }
            Section::Features => {
                // A sparkle.
                p.move_to(8.0, 1.5);
                p.cubic_to(8.6, 5.6, 10.4, 7.4, 14.5, 8.0);
                p.cubic_to(10.4, 8.6, 8.6, 10.4, 8.0, 14.5);
                p.cubic_to(7.4, 10.4, 5.6, 8.6, 1.5, 8.0);
                p.cubic_to(5.6, 7.4, 7.4, 5.6, 8.0, 1.5);
                p.close();
            }
            Section::Editor => return self.tool_icon(Button::Tool(Tool::Pen), x, y, 16.0, ink),
            Section::System => {
                // A screen on a stand.
                p.push_rect(SkRect::from_xywh(1.5, 2.5, 13.0, 8.5).unwrap());
                p.move_to(8.0, 11.0);
                p.line_to(8.0, 13.5);
                p.move_to(5.0, 13.5);
                p.line_to(11.0, 13.5);
            }
            Section::About => {
                // An "i" in a circle.
                p.push_circle(8.0, 8.0, 6.3);
                p.move_to(8.0, 7.2);
                p.line_to(8.0, 11.2);
                p.move_to(8.0, 4.8);
                p.line_to(8.0, 4.9);
            }
        }
        self.stroke(p, ink, transform, 1.5 * s);
    }

    /// A blinking-free text cursor.
    fn caret(&mut self, x: f32, y: f32) {
        let color = self.p.accent_text;
        self.fill(SkRect::from_xywh(x, y, 1.5, 16.0), 0.0, color);
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
        let p = self.p;
        let hovered = self.hit(id, x, y, w, H);
        let color = if selected {
            p.accent
        } else if hovered {
            p.control_hover
        } else {
            p.control
        };
        // Square inner edges by overdrawing rounded pieces from the neighbors' side.
        self.fill(SkRect::from_xywh(x, y, w, H), 7.0, color);
        if !first {
            self.fill(SkRect::from_xywh(x, y, 8.0, H), 0.0, color);
        }
        if !last {
            self.fill(SkRect::from_xywh(x + w - 8.0, y, 8.0, H), 0.0, color);
        }
        let tw = font::measure(text, 12.0).0;
        let ink = if selected { 0xFFFFFF } else { p.text };
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 12.0, ink);
    }

    /// A drop-down field showing the current choice.
    fn select(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32, open: bool) {
        let p = self.p;
        let hovered = self.hit(id, x, y, w, H);
        let color = if hovered || open {
            p.control_hover
        } else {
            p.control
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 7.0, color);
        self.label(text, x + 10.0, y + 7.0, 12.0, p.text);
        let (cx, cy) = (x + w - 16.0, y + H / 2.0);
        let mut path = PathBuilder::new();
        path.move_to(cx - 4.0, cy - 2.0);
        path.line_to(cx, cy + 2.0);
        path.line_to(cx + 4.0, cy - 2.0);
        let s = self.scale;
        self.stroke(path, p.muted, Transform::from_scale(s, s), 1.5 * s);
    }

    fn field(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32, active: bool) {
        let p = self.p;
        let hovered = self.hit(id, x, y, w, H);
        if active {
            self.fill(
                SkRect::from_xywh(x - 1.5, y - 1.5, w + 3.0, H + 3.0),
                8.0,
                p.accent,
            );
        }
        let color = if hovered && !active {
            p.control_hover
        } else {
            p.control
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 7.0, color);
        let tw = font::measure(text, 12.0).0;
        let ink = if active { p.muted } else { p.text };
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 12.0, ink);
    }

    /// A clickable field with left-aligned, muted text.
    fn field_left(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32) {
        let p = self.p;
        let hovered = self.hit(id, x, y, w, H);
        let color = if hovered { p.control_hover } else { p.control };
        self.fill(SkRect::from_xywh(x, y, w, H), 7.0, color);
        self.label(text, x + 10.0, y + 7.0, 12.0, p.muted);
    }

    /// A field to type into.
    fn text_field(&mut self, id: Id, text: &str, x: f32, y: f32, w: f32, focused: bool) {
        let p = self.p;
        let hovered = self.hit(id, x, y, w, H);
        if focused {
            self.fill(
                SkRect::from_xywh(x - 1.5, y - 1.5, w + 3.0, H + 3.0),
                8.0,
                p.accent,
            );
        }
        let color = if hovered && !focused {
            p.control_hover
        } else {
            p.control
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 7.0, color);
        let shown = elide_start(text, 12.0, w - 22.0);
        self.label(&shown, x + 10.0, y + 7.0, 12.0, p.text);
        if focused {
            self.caret(x + 11.0 + font::measure(&shown, 12.0).0, y + 6.0);
        }
    }

    fn button(&mut self, id: Id, text: &str, x: f32, y: f32, primary: bool) {
        let p = self.p;
        let tw = font::measure(text, 12.0).0;
        let w = tw + if primary { 24.0 } else { 20.0 };
        let hovered = self.hit(id, x, y, w, H);
        let color = match (primary, hovered) {
            (true, false) => p.accent,
            (true, true) => p.accent_hover,
            (false, false) => p.control,
            (false, true) => p.control_hover,
        };
        self.fill(SkRect::from_xywh(x, y, w, H), 7.0, color);
        let ink = if primary { 0xFFFFFF } else { p.text };
        self.label(text, x + (w - tw) / 2.0, y + 7.0, 12.0, ink);
    }

    fn link(&mut self, id: Id, text: &str, x: f32, y: f32, size: f32, warn: bool) {
        let p = self.p;
        let (w, h) = font::measure(text, size);
        let hovered = self.hit(id, x, y, w, h);
        let color = match (warn, hovered) {
            (true, _) => p.error,
            (false, true) => p.link_hover,
            (false, false) => p.accent_text,
        };
        self.label(text, x, y, size, color);
    }

    /// A drawing tool that turns on and off with a click.
    #[allow(clippy::too_many_arguments)]
    fn tile(
        &mut self,
        id: Id,
        name: &str,
        button: Button,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        on: bool,
    ) {
        let p = self.p;
        let hovered = self.hit(id, x, y, w, h);
        if on {
            self.fill(SkRect::from_xywh(x, y, w, h), 10.0, p.accent);
            self.fill(
                SkRect::from_xywh(x + 1.5, y + 1.5, w - 3.0, h - 3.0),
                9.0,
                p.pill,
            );
        } else {
            let color = if hovered { p.control_hover } else { p.control };
            self.fill(SkRect::from_xywh(x, y, w, h), 10.0, color);
        }
        if on && hovered {
            self.fill(
                SkRect::from_xywh(x + 1.5, y + 1.5, w - 3.0, h - 3.0),
                9.0,
                p.control_hover,
            );
        }
        let ink = if on { p.accent_text } else { p.muted };
        self.tool_icon(button, x + (w - 20.0) / 2.0, y + 14.0, 20.0, ink);
        let name = elide_end(name, 12.0, w - 12.0);
        let tw = font::measure(&name, 12.0).0;
        let color = if on { p.text } else { p.muted };
        self.label(&name, x + (w - tw) / 2.0, y + 44.0, 12.0, color);
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

    fn draw(view: &View, dark: bool) -> (Pixmap, Vec<(Id, SkRect)>, f32) {
        let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
        let mut ui = Ui::new(&mut pixmap, 1.0, None, dark);
        let update = UpdateStatus::Available("9.9.9".into());
        let height = build(&mut ui, &Settings::default(), &update, view);
        let hits = ui.hits;
        (pixmap, hits, height)
    }

    #[test]
    fn every_section_lays_out_in_every_language() {
        for lang in Lang::ALL {
            let _lang = crate::snapshot::language(lang);
            for section in Section::ALL {
                let view = View {
                    section,
                    menu_open: true,
                    ..View::new()
                };
                let (_, hits, _) = draw(&view, true);
                for (id, r) in &hits {
                    let in_sidebar = r.right() <= SIDEBAR;
                    let in_page = r.left() >= LEFT - 0.5 && r.right() <= RIGHT + 0.5;
                    assert!(
                        in_sidebar || in_page,
                        "{id:?} {r:?} in {section:?}, {lang:?}"
                    );
                }
                // Labels must not run under the controls on their right.
                let label = font::measure(section.title(), 13.0).0;
                assert!(50.0 + label <= SIDEBAR - 14.0, "{section:?} in {lang:?}");
                assert!(font::measure(section.title(), 19.0).0 <= RIGHT - LEFT);
            }
        }
    }

    /// The Windows rows for asking or replacing must fit too, though these tests run elsewhere.
    #[test]
    fn the_replace_modes_fit_in_every_language() {
        for lang in Lang::ALL {
            let _lang = crate::snapshot::language(lang);
            let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
            let mut ui = Ui::new(&mut pixmap, 1.0, None, true);
            let mut page = Page::new(&mut ui, HEADER, None, None);
            system(&mut page, &Settings::default(), &View::new(), true);
            let end = page.y;
            assert!(end <= HEIGHT as f32, "ends at {end} in {lang:?}");
            for (id, r) in &ui.hits {
                assert!(
                    r.left() >= LEFT - 0.5 && r.right() <= RIGHT + 0.5,
                    "{id:?} {r:?} in {lang:?}"
                );
            }
        }
    }

    #[test]
    fn row_labels_leave_room_for_their_controls() {
        for lang in Lang::ALL {
            let _lang = crate::snapshot::language(lang);
            // The widest control next to a label is the three-way theme choice.
            let theme = [T::ThemeSystem, T::ThemeLight, T::ThemeDark]
                .map(|k| font::measure(t(k), 12.0).0 + 24.0)
                .into_iter()
                .fold(56.0, f32::max)
                * 3.0;
            assert!(LEFT + font::measure(t(T::Theme), 13.0).0 + 12.0 <= RIGHT - theme);
            for (label, control) in [
                (T::ReplaceWhen, [T::ReplaceAsk, T::ReplaceAuto]),
                (T::CopyAs, [T::CopyImage, T::CopyFile]),
            ] {
                let w = [control[0], control[1]]
                    .map(|k| font::measure(t(k), 12.0).0 + 24.0)
                    .into_iter()
                    .fold(56.0, f32::max)
                    * 2.0;
                assert!(
                    LEFT + font::measure(t(label), 13.0).0 + 12.0 <= RIGHT - w,
                    "{label:?} in {lang:?}"
                );
            }
            let history = 4.0 * font::measure(t(T::Off), 12.0).0.max(32.0).max(56.0 - 24.0) + 96.0;
            assert!(
                LEFT + font::measure(t(T::HistoryLimit), 13.0).0 + 12.0 <= RIGHT - history,
                "{lang:?}"
            );
            for which in Shortcut::ALL {
                let reset = font::measure(t(T::Reset), 12.0).0 + 12.0;
                let w = font::measure(shortcut_label(which), 13.0).0;
                assert!(
                    LEFT + w + 12.0 <= RIGHT - 150.0 - reset,
                    "{which:?} in {lang:?}"
                );
            }
            for tool in TOOLS {
                let tile_w = (RIGHT - LEFT - 30.0) / 4.0;
                assert!(
                    font::measure(tool_info(tool).0, 12.0).0 <= tile_w - 12.0,
                    "{tool:?} in {lang:?}"
                );
            }
        }
    }

    #[test]
    fn search_finds_rows_in_every_section() {
        let _lang = crate::snapshot::language(Lang::En);
        let find = |query: &str| {
            let view = View {
                query: query.into(),
                ..View::new()
            };
            draw(&view, true).1
        };
        let hits = find("jpeg");
        assert!(hits.iter().any(|(id, _)| *id == Id::Format(Format::Jpeg)));
        assert!(!hits.iter().any(|(id, _)| *id == Id::Autostart));
        let hits = find("blur");
        assert!(hits.iter().any(|(id, _)| *id == Id::Feature(Feature::Blur)));
        assert!(!hits.iter().any(|(id, _)| *id == Id::Feature(Feature::Pen)));
        // A section's name shows all of it.
        let hits = find("editor");
        assert!(hits.iter().any(|(id, _)| *id == Id::Feature(Feature::Pen)));
        let hits = find("zzzz");
        assert!(
            hits.iter()
                .all(|(id, _)| matches!(id, Id::Section(_) | Id::Search | Id::ClearSearch))
        );
    }

    #[test]
    fn long_pages_scroll_and_hide_under_the_header() {
        let _lang = crate::snapshot::language(Lang::Ru);
        let view = View {
            section: Section::Shortcuts,
            scroll: 60.0,
            ..View::new()
        };
        let (_, hits, height) = draw(&view, false);
        assert!(
            height > HEIGHT as f32 - HEADER,
            "the shortcuts page is long"
        );
        for (id, r) in &hits {
            assert!(r.right() <= SIDEBAR || r.top() >= HEADER, "{id:?} {r:?}");
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
        // A fixed file name, so the example under it doesn't change with the clock.
        settings.file_name = "screenshot".into();
        settings.disabled = vec![Feature::Print, Feature::Crop, Feature::Marker];
        for (lang, name, section, dark, query) in [
            (Lang::En, "settings-en", Section::General, true, ""),
            (Lang::Ru, "settings-ru", Section::Capture, false, ""),
            (Lang::Ru, "settings-saving-ru", Section::Saving, true, ""),
            (
                Lang::En,
                "settings-shortcuts-en",
                Section::Shortcuts,
                false,
                "",
            ),
            (
                Lang::Ru,
                "settings-features-ru",
                Section::Features,
                true,
                "",
            ),
            (Lang::Uk, "settings-editor-uk", Section::Editor, false, ""),
            (Lang::En, "settings-system-en", Section::System, false, ""),
            (
                Lang::En,
                "settings-search-en",
                Section::General,
                true,
                "save",
            ),
        ] {
            let _lang = crate::snapshot::language(lang);
            let mut pixmap = Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
            let mut ui = Ui::new(&mut pixmap, 1.0, None, dark);
            let view = View {
                section,
                query: query.into(),
                ..View::new()
            };
            // Not `Idle`, which shows the version and would change every release.
            build(&mut ui, &settings, &UpdateStatus::UpToDate, &view);
            let image =
                image::RgbaImage::from_raw(WIDTH as u32, HEIGHT as u32, pixmap.take()).unwrap();
            crate::snapshot::assert_matches(name, &image);
        }
    }
}
