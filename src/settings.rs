//! User settings, stored as `key = value` lines in the config folder.
//!
//! The format is kept trivial on purpose: it is easy to edit by hand and
//! needs no parser dependency. Unknown keys and bad values fall back to defaults.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::{ProjectDirs, UserDirs};

use crate::i18n::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
        }
    }
}

/// How a picked color goes to the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    /// `#1E90FF`
    Hex,
    /// `rgb(30, 144, 255)`
    Rgb,
}

impl ColorFormat {
    pub fn format(self, color: u32) -> String {
        let [r, g, b] = crate::paint::rgb(color);
        match self {
            ColorFormat::Hex => format!("#{:06X}", color & 0xFFFFFF),
            ColorFormat::Rgb => format!("rgb({r}, {g}, {b})"),
        }
    }
}

/// What copying a shot puts on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyAs {
    /// The picture itself, for pasting into chats and editors.
    Image,
    /// An image file, for pasting into a folder in the file manager.
    File,
}

/// Whether Snapt takes over from the system's own screenshot tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceMode {
    /// The system tool keeps its keys.
    Off,
    /// Snapt asks before taking over, and asks again when the system takes them back.
    Ask,
    /// Snapt takes over by itself.
    Auto,
}

impl ReplaceMode {
    pub fn is_on(self) -> bool {
        self != ReplaceMode::Off
    }

    fn code(self) -> &'static str {
        match self {
            ReplaceMode::Off => "off",
            ReplaceMode::Ask => "ask",
            ReplaceMode::Auto => "auto",
        }
    }
}

/// Parts of Snapt that can be turned off in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Recognize,
    Search,
    Print,
    Pin,
    Scroll,
    Record,
    Picker,
    // The editor's tools; they leave the toolbar and their keys stop working.
    Pen,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Marker,
    Text,
    Number,
    Pixelate,
    Blur,
    Spotlight,
    Crop,
    /// Reads QR codes in the selection.
    Qr,
    /// Blurs e-mails, phones and card numbers found in the selection.
    Redact,
}

impl Feature {
    pub const ALL: [Feature; 21] = [
        Feature::Recognize,
        Feature::Search,
        Feature::Print,
        Feature::Pin,
        Feature::Scroll,
        Feature::Record,
        Feature::Picker,
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
        Feature::Qr,
        Feature::Redact,
    ];

    fn code(self) -> &'static str {
        match self {
            Feature::Recognize => "ocr",
            Feature::Search => "search",
            Feature::Print => "print",
            Feature::Pin => "pin",
            Feature::Scroll => "scroll",
            Feature::Record => "record",
            Feature::Picker => "picker",
            Feature::Pen => "pen",
            Feature::Line => "line",
            Feature::Arrow => "arrow",
            Feature::Rect => "rect",
            Feature::Ellipse => "ellipse",
            Feature::Marker => "marker",
            Feature::Text => "text",
            Feature::Number => "number",
            Feature::Pixelate => "pixelate",
            Feature::Blur => "blur",
            Feature::Spotlight => "spotlight",
            Feature::Crop => "crop",
            Feature::Qr => "qr",
            Feature::Redact => "redact",
        }
    }

    /// Tools most people don't need start turned off; the settings file
    /// lists them under `enabled` once turned on.
    fn off_by_default(self) -> bool {
        matches!(self, Feature::Crop)
    }

    fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// The features turned off, for code that has no `Settings` at hand.
static DISABLED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Whether `feature` is on, as the settings last applied say.
pub fn enabled(feature: Feature) -> bool {
    DISABLED.load(std::sync::atomic::Ordering::Relaxed) & feature.bit() == 0
}

/// The keyboard shortcuts: global ones that work anywhere, then ones for the
/// open screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Capture,
    /// The color picker.
    Pick,
    FullScreen,
    RepeatArea,
    /// Pins the newest screenshot from the history.
    PinLast,
    /// Opens the screenshot for a scrolling screenshot of a page.
    ScrollPage,
    Recognize,
    Scroll,
    Record,
    Pin,
    Copy,
    Save,
}

#[cfg(target_os = "macos")]
macro_rules! cmd {
    ($keys:literal) => {
        concat!("Cmd+", $keys)
    };
}
#[cfg(not(target_os = "macos"))]
macro_rules! cmd {
    ($keys:literal) => {
        concat!("Ctrl+", $keys)
    };
}

impl Shortcut {
    pub const ALL: [Shortcut; 12] = [
        Shortcut::Capture,
        Shortcut::Pick,
        Shortcut::FullScreen,
        Shortcut::RepeatArea,
        Shortcut::PinLast,
        Shortcut::ScrollPage,
        Shortcut::Recognize,
        Shortcut::Scroll,
        Shortcut::Record,
        Shortcut::Pin,
        Shortcut::Copy,
        Shortcut::Save,
    ];

    /// Whether it works anywhere, not just on the open screenshot.
    pub fn is_global(self) -> bool {
        matches!(
            self,
            Shortcut::Capture
                | Shortcut::Pick
                | Shortcut::FullScreen
                | Shortcut::RepeatArea
                | Shortcut::PinLast
                | Shortcut::ScrollPage
        )
    }

    /// The keys out of the box; empty for none.
    pub fn default_keys(self) -> &'static str {
        match self {
            Shortcut::Capture => DEFAULT_HOTKEY,
            Shortcut::Pick => DEFAULT_PICK_HOTKEY,
            Shortcut::FullScreen | Shortcut::RepeatArea | Shortcut::PinLast => "",
            Shortcut::ScrollPage => DEFAULT_SCROLL_PAGE_HOTKEY,
            Shortcut::Recognize => cmd!("Shift+C"),
            Shortcut::Scroll => cmd!("Shift+S"),
            Shortcut::Record => cmd!("Shift+R"),
            Shortcut::Pin => "F3",
            Shortcut::Copy => cmd!("C"),
            Shortcut::Save => cmd!("S"),
        }
    }

    /// Its name in the settings file.
    fn code(self) -> &'static str {
        match self {
            Shortcut::Capture => "hotkey",
            Shortcut::Pick => "pick_hotkey",
            Shortcut::FullScreen => "full_screen_hotkey",
            Shortcut::RepeatArea => "repeat_area_hotkey",
            Shortcut::PinLast => "pin_last_hotkey",
            Shortcut::ScrollPage => "scroll_page_hotkey",
            Shortcut::Recognize => "copy_text_key",
            Shortcut::Scroll => "scroll_key",
            Shortcut::Record => "record_key",
            Shortcut::Pin => "pin_key",
            Shortcut::Copy => "copy_key",
            Shortcut::Save => "save_key",
        }
    }
}

/// The screenshot's own shortcuts, parsed, for the overlay.
static OVERLAY_KEYS: std::sync::Mutex<Vec<(Shortcut, global_hotkey::hotkey::HotKey)>> =
    std::sync::Mutex::new(Vec::new());

/// The screenshot shortcut that `code` with `modifiers` held presses, if any.
pub fn overlay_shortcut(
    modifiers: global_hotkey::hotkey::Modifiers,
    code: global_hotkey::hotkey::Code,
) -> Option<Shortcut> {
    let keys = OVERLAY_KEYS.lock().ok()?;
    keys.iter()
        .find(|(_, hotkey)| hotkey.matches(modifiers, code))
        .map(|(which, _)| *which)
}

/// The look of Snapt's windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The keys of each `Shortcut`, as `global_hotkey` parses them, e.g.
    /// `Ctrl+Shift+S`; empty for none.
    pub shortcuts: [String; 12],
    pub color_format: ColorFormat,
    pub folder: PathBuf,
    pub format: Format,
    pub copy_as: CopyAs,
    /// Copy a selected area as soon as the mouse is let go, without the editor.
    pub copy_on_select: bool,
    /// With `copy_on_select`, clicking a window still opens the editor
    /// instead of copying the window right away.
    pub window_opens_editor: bool,
    /// Save straight into `folder` instead of asking where.
    pub quick_save: bool,
    /// Copying a shot also saves it into `folder`.
    pub save_on_copy: bool,
    /// Open a saved shot in the system's image viewer or editor.
    pub open_after_save: bool,
    /// Draw the mouse pointer onto screenshots.
    pub capture_cursor: bool,
    pub autostart: bool,
    /// Take over PrtSc (and on Windows Win+Shift+S) from the system's own screenshot tool.
    pub replace_system: ReplaceMode,
    /// Look for a new release once a day.
    pub auto_update: bool,
    /// The interface language; `None` follows the system.
    pub language: Option<Lang>,
    /// Light or dark windows; `System` follows the system.
    pub theme: Theme,
    pub color: u32,
    pub stroke: f32,
    pub text_size: f32,
    /// How many recent screenshots the tray menu keeps; 0 turns the history off.
    pub history: usize,
    /// The name of saved files, in `strftime` syntax, without the extension.
    pub file_name: String,
    /// Features turned off.
    pub disabled: Vec<Feature>,
}

/// The file name template used when none is set: `Snapt_2026-03-07_09-05-02`.
pub const DEFAULT_FILE_NAME: &str = "Snapt_%Y-%m-%d_%H-%M-%S";

/// Macs have no PrtSc key; LightShot for Mac uses Cmd+Shift+9.
#[cfg(target_os = "macos")]
pub const DEFAULT_HOTKEY: &str = "Cmd+Shift+9";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_HOTKEY: &str = "PrintScreen";
pub const DEFAULT_PICK_HOTKEY: &str = "Ctrl+Alt+C";
/// A scrolling screenshot of a page: next to the screenshot key.
#[cfg(target_os = "macos")]
pub const DEFAULT_SCROLL_PAGE_HOTKEY: &str = "Cmd+Shift+0";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_SCROLL_PAGE_HOTKEY: &str = "Ctrl+Shift+PrintScreen";

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcuts: Shortcut::ALL.map(|s| s.default_keys().to_string()),
            color_format: ColorFormat::Hex,
            folder: default_folder(),
            format: Format::Png,
            copy_as: CopyAs::Image,
            copy_on_select: false,
            window_opens_editor: true,
            quick_save: false,
            save_on_copy: false,
            open_after_save: false,
            capture_cursor: false,
            autostart: false,
            // New Windows users are asked once whether Snapt should take over.
            replace_system: if cfg!(target_os = "windows") {
                ReplaceMode::Ask
            } else {
                ReplaceMode::Off
            },
            auto_update: true,
            language: None,
            theme: Theme::System,
            color: 0xFF3B30,
            stroke: 3.0,
            text_size: 20.0,
            history: crate::history::DEFAULT_LIMIT,
            disabled: Feature::ALL
                .into_iter()
                .filter(|f| f.off_by_default())
                .collect(),
            file_name: DEFAULT_FILE_NAME.into(),
        }
    }
}

impl Settings {
    /// The language the interface shows.
    pub fn lang(&self) -> Lang {
        self.language.unwrap_or_else(Lang::system)
    }

    pub fn shortcut(&self, which: Shortcut) -> &str {
        &self.shortcuts[which as usize]
    }

    pub fn shortcut_mut(&mut self, which: Shortcut) -> &mut String {
        &mut self.shortcuts[which as usize]
    }

    pub fn path() -> Option<PathBuf> {
        ProjectDirs::from("io.github", "vkardin", "Snapt")
            .map(|dirs| dirs.config_dir().join("settings.ini"))
    }

    /// Loads the settings, or the defaults when there is no file yet.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().context("could not find the config folder")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).context("failed to create the config folder")?;
        }
        fs::write(&path, self.to_text())
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn is_enabled(&self, feature: Feature) -> bool {
        !self.disabled.contains(&feature)
    }

    /// Turns `feature` on or off.
    pub fn toggle(&mut self, feature: Feature) {
        if let Some(i) = self.disabled.iter().position(|f| *f == feature) {
            self.disabled.remove(i);
        } else {
            self.disabled.push(feature);
        }
        self.apply_features();
    }

    /// Makes `enabled` and `overlay_shortcut` follow these settings.
    pub fn apply_features(&self) {
        let bits = self.disabled.iter().fold(0, |b, f| b | f.bit());
        DISABLED.store(bits, std::sync::atomic::Ordering::Relaxed);
        let keys = Shortcut::ALL
            .into_iter()
            .filter(|w| !w.is_global())
            .filter_map(|w| Some((w, self.shortcut(w).parse().ok()?)))
            .collect();
        if let Ok(mut overlay) = OVERLAY_KEYS.lock() {
            *overlay = keys;
        }
    }

    fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let (mut off, mut on) = (Vec::new(), Vec::new());
        let codes = |value: &str| -> Vec<Feature> {
            Feature::ALL
                .into_iter()
                .filter(|f| value.split(',').any(|v| v.trim() == f.code()))
                .collect()
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                key if let Some(which) = Shortcut::ALL.into_iter().find(|w| w.code() == key) => {
                    // The capture shortcut can't be empty: nothing else starts a capture.
                    if !value.is_empty() || which != Shortcut::Capture {
                        s.shortcuts[which as usize] = value.into();
                    }
                }
                "color_format" => {
                    s.color_format = match value.to_ascii_lowercase().as_str() {
                        "rgb" => ColorFormat::Rgb,
                        _ => ColorFormat::Hex,
                    }
                }
                "folder" if !value.is_empty() => s.folder = PathBuf::from(value),
                "format" => {
                    s.format = match value.to_ascii_lowercase().as_str() {
                        "jpg" | "jpeg" => Format::Jpeg,
                        _ => Format::Png,
                    }
                }
                "copy_as" => {
                    s.copy_as = match value.to_ascii_lowercase().as_str() {
                        "file" => CopyAs::File,
                        _ => CopyAs::Image,
                    }
                }
                "copy_on_select" => s.copy_on_select = value == "true",
                "window_opens_editor" => s.window_opens_editor = value != "false",
                "quick_save" => s.quick_save = value == "true",
                "save_on_copy" => s.save_on_copy = value == "true",
                "open_after_save" => s.open_after_save = value == "true",
                "capture_cursor" => s.capture_cursor = value == "true",
                "autostart" => s.autostart = value == "true",
                "replace_system" => {
                    // Older versions wrote true or false.
                    s.replace_system = match value {
                        "ask" => ReplaceMode::Ask,
                        "auto" | "true" => ReplaceMode::Auto,
                        _ => ReplaceMode::Off,
                    }
                }
                "auto_update" => s.auto_update = value != "false",
                "language" => s.language = Lang::from_code(value),
                "theme" => {
                    s.theme = match value.to_ascii_lowercase().as_str() {
                        "light" => Theme::Light,
                        "dark" => Theme::Dark,
                        _ => Theme::System,
                    }
                }
                "color" => {
                    if let Ok(c) = u32::from_str_radix(value.trim_start_matches('#'), 16) {
                        s.color = c & 0xFFFFFF;
                    }
                }
                "stroke" => s.stroke = value.parse().unwrap_or(s.stroke).clamp(1.0, 30.0),
                "text_size" => s.text_size = value.parse().unwrap_or(s.text_size).clamp(10.0, 96.0),
                "history" => s.history = value.parse().unwrap_or(s.history).min(50),
                "file_name" if !value.is_empty() => s.file_name = value.into(),
                "disabled" => off = codes(value),
                "enabled" => on = codes(value),
                _ => {}
            }
        }
        s.disabled = Feature::ALL
            .into_iter()
            .filter(|f| {
                if f.off_by_default() {
                    !on.contains(f)
                } else {
                    off.contains(f)
                }
            })
            .collect();
        s
    }

    fn to_text(&self) -> String {
        let mut t = String::from("# Snapt settings. Edit while Snapt is closed.\n");
        let _ = writeln!(
            t,
            "# hotkey examples: PrintScreen, Ctrl+Shift+S, Cmd+Shift+9"
        );
        for which in Shortcut::ALL {
            let _ = writeln!(t, "{} = {}", which.code(), self.shortcut(which));
        }
        let format = match self.color_format {
            ColorFormat::Hex => "hex",
            ColorFormat::Rgb => "rgb",
        };
        let _ = writeln!(t, "color_format = {format}");
        let _ = writeln!(t, "folder = {}", self.folder.display());
        let _ = writeln!(t, "format = {}", self.format.extension());
        let copy_as = match self.copy_as {
            CopyAs::Image => "image",
            CopyAs::File => "file",
        };
        let _ = writeln!(t, "copy_as = {copy_as}");
        let _ = writeln!(t, "copy_on_select = {}", self.copy_on_select);
        let _ = writeln!(t, "window_opens_editor = {}", self.window_opens_editor);
        let _ = writeln!(t, "quick_save = {}", self.quick_save);
        let _ = writeln!(t, "save_on_copy = {}", self.save_on_copy);
        let _ = writeln!(t, "open_after_save = {}", self.open_after_save);
        let _ = writeln!(t, "capture_cursor = {}", self.capture_cursor);
        let _ = writeln!(t, "autostart = {}", self.autostart);
        let _ = writeln!(t, "replace_system = {}", self.replace_system.code());
        let _ = writeln!(t, "auto_update = {}", self.auto_update);
        let language = self.language.map_or("auto", Lang::code);
        let _ = writeln!(t, "language = {language}");
        let theme = match self.theme {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        };
        let _ = writeln!(t, "theme = {theme}");
        let _ = writeln!(t, "color = #{:06X}", self.color);
        let _ = writeln!(t, "stroke = {}", self.stroke);
        let _ = writeln!(t, "text_size = {}", self.text_size);
        let _ = writeln!(t, "# recent screenshots in the tray menu, 0 to keep none");
        let _ = writeln!(t, "history = {}", self.history);
        let _ = writeln!(
            t,
            "# saved file names: %Y year, %m month, %d day, %H hour, %M minute, %S second"
        );
        let _ = writeln!(t, "file_name = {}", self.file_name);
        let disabled: Vec<&str> = (self.disabled.iter())
            .filter(|f| !f.off_by_default())
            .map(|f| f.code())
            .collect();
        let enabled: Vec<&str> = (Feature::ALL.into_iter())
            .filter(|f| f.off_by_default() && self.is_enabled(*f))
            .map(Feature::code)
            .collect();
        let _ = writeln!(
            t,
            "# turned off: ocr, search, print, pin, scroll, record, picker, pen, line, arrow,"
        );
        let _ = writeln!(
            t,
            "# rect, ellipse, marker, text, number, pixelate, blur, spotlight, qr, redact"
        );
        let _ = writeln!(t, "disabled = {}", disabled.join(", "));
        let _ = writeln!(t, "# turned on, though off at first: crop");
        let _ = writeln!(t, "enabled = {}", enabled.join(", "));
        t
    }
}

/// `~/Pictures/Snapt`, falling back to the home folder when there is no Pictures folder.
fn default_folder() -> PathBuf {
    match UserDirs::new() {
        Some(dirs) => dirs.picture_dir().unwrap_or(dirs.home_dir()).join("Snapt"),
        None => PathBuf::from("Snapt"),
    }
}

/// Turns starting Snapt at login on or off.
pub fn set_autostart(on: bool) -> Result<()> {
    let exe = launch_path()?;
    autostart::apply(on, &exe)
}

/// The path that relaunches this app. An AppImage runs from a temporary mount,
/// so its own file is used instead.
pub fn launch_path() -> Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe().context("could not find the Snapt executable")
}

#[cfg(target_os = "windows")]
mod autostart {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn apply(on: bool, exe: &Path) -> Result<()> {
        let mut cmd = Command::new("reg");
        if on {
            let value = format!("\"{}\"", exe.display());
            cmd.args([
                "add", RUN_KEY, "/v", "Snapt", "/t", "REG_SZ", "/d", &value, "/f",
            ]);
        } else {
            cmd.args(["delete", RUN_KEY, "/v", "Snapt", "/f"]);
        }
        let status = cmd
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .context("failed to run reg.exe")?;
        // Deleting a value that is not there fails, which is fine.
        anyhow::ensure!(status.success() || !on, "reg.exe failed: {status}");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod autostart {
    use super::*;

    pub fn apply(on: bool, exe: &Path) -> Result<()> {
        let home = UserDirs::new().context("could not find the home folder")?;
        let path = home
            .home_dir()
            .join("Library/LaunchAgents/io.github.vkardin.snapt.plist");
        if !on {
            let _ = fs::remove_file(&path);
            return Ok(());
        }
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>io.github.vkardin.snapt</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
            xml_escape(&exe.display().to_string())
        );
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, plist).with_context(|| format!("failed to write {}", path.display()))
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod autostart {
    use super::*;

    pub fn apply(on: bool, exe: &Path) -> Result<()> {
        // A Flatpak can't write the host's autostart folder; the portal does it.
        #[cfg(target_os = "linux")]
        if std::env::var_os("FLATPAK_ID").is_some() {
            return crate::portal::autostart(on);
        }
        let dirs = directories::BaseDirs::new().context("could not find the config folder")?;
        let path = dirs.config_dir().join("autostart/snapt.desktop");
        if !on {
            let _ = fs::remove_file(&path);
            return Ok(());
        }
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Snapt\nComment=Screenshot tool\nExec=\"{}\"\nIcon=snapt\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, entry).with_context(|| format!("failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip() {
        let s = Settings {
            shortcuts: Shortcut::ALL.map(|w| match w {
                Shortcut::Capture => "Ctrl+Shift+S".into(),
                Shortcut::Pin => String::new(),
                _ => format!("Alt+F{}", w as usize + 1),
            }),
            color_format: ColorFormat::Rgb,
            folder: PathBuf::from("/tmp/shots"),
            format: Format::Jpeg,
            copy_as: CopyAs::File,
            copy_on_select: true,
            window_opens_editor: false,
            quick_save: true,
            save_on_copy: true,
            open_after_save: true,
            capture_cursor: true,
            autostart: true,
            replace_system: ReplaceMode::Ask,
            auto_update: false,
            language: Some(Lang::Uk),
            theme: Theme::Light,
            color: 0x00FF7F,
            stroke: 5.0,
            text_size: 28.0,
            history: 0,
            file_name: "shot %d.%m.%Y".into(),
            disabled: vec![Feature::Pin, Feature::Picker, Feature::Blur, Feature::Crop],
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
    }

    #[test]
    fn crop_starts_off_and_can_be_turned_on() {
        assert!(!Settings::default().is_enabled(Feature::Crop));
        // Files from before crop had a switch keep it off too.
        let old = Settings::parse("disabled = pin");
        assert!(!old.is_enabled(Feature::Crop) && !old.is_enabled(Feature::Pin));
        let on = Settings::parse("enabled = crop\ndisabled = ");
        assert!(on.is_enabled(Feature::Crop));
        assert_eq!(Settings::parse(&on.to_text()), on);
    }

    #[test]
    fn bad_lines_fall_back_to_defaults() {
        let s = Settings::parse(
            "nonsense\nstroke = lots\ncolor = blue\nformat = gif\ncopy_as = folder\nunknown = 1",
        );
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn old_replace_switches_still_read() {
        let mode = |text: &str| Settings::parse(text).replace_system;
        assert_eq!(mode("replace_system = true"), ReplaceMode::Auto);
        assert_eq!(mode("replace_system = false"), ReplaceMode::Off);
        assert_eq!(mode("replace_system = ask"), ReplaceMode::Ask);
        assert_eq!(mode("replace_system = auto"), ReplaceMode::Auto);
    }

    #[test]
    fn the_default_hotkeys_parse() {
        for which in Shortcut::ALL {
            let keys = which.default_keys();
            assert!(keys.is_empty() || keys.parse::<global_hotkey::hotkey::HotKey>().is_ok());
        }
    }

    #[test]
    fn colors_format_as_hex_or_rgb() {
        assert_eq!(ColorFormat::Hex.format(0x1E90FF), "#1E90FF");
        assert_eq!(ColorFormat::Rgb.format(0x1E90FF), "rgb(30, 144, 255)");
    }
}
