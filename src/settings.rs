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

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The capture shortcut, as `global_hotkey` parses it, e.g. `Ctrl+Shift+S`.
    pub hotkey: String,
    pub folder: PathBuf,
    pub format: Format,
    /// Save straight into `folder` instead of asking where.
    pub quick_save: bool,
    pub autostart: bool,
    pub color: u32,
    pub stroke: f32,
    pub text_size: f32,
}

/// The capture shortcuts offered in the tray menu. Others can be set in the file.
#[cfg(target_os = "macos")]
pub const HOTKEYS: &[&str] = &["Cmd+Shift+9", "Cmd+Shift+S", "Cmd+Shift+X", "Ctrl+Shift+S"];
#[cfg(not(target_os = "macos"))]
pub const HOTKEYS: &[&str] = &[
    "PrintScreen",
    "Ctrl+PrintScreen",
    "Shift+PrintScreen",
    "Alt+PrintScreen",
    "Ctrl+Shift+S",
    "Ctrl+Shift+X",
];

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Macs have no PrtSc key; LightShot for Mac uses ⌘⇧9.
            hotkey: HOTKEYS[0].into(),
            folder: default_folder(),
            format: Format::Png,
            quick_save: false,
            autostart: false,
            color: 0xFF3B30,
            stroke: 3.0,
            text_size: 20.0,
        }
    }
}

impl Settings {
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

    fn parse(text: &str) -> Self {
        let mut s = Self::default();
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
                "hotkey" if !value.is_empty() => s.hotkey = value.into(),
                "folder" if !value.is_empty() => s.folder = PathBuf::from(value),
                "format" => {
                    s.format = match value.to_ascii_lowercase().as_str() {
                        "jpg" | "jpeg" => Format::Jpeg,
                        _ => Format::Png,
                    }
                }
                "quick_save" => s.quick_save = value == "true",
                "autostart" => s.autostart = value == "true",
                "color" => {
                    if let Ok(c) = u32::from_str_radix(value.trim_start_matches('#'), 16) {
                        s.color = c & 0xFFFFFF;
                    }
                }
                "stroke" => s.stroke = value.parse().unwrap_or(s.stroke).clamp(1.0, 30.0),
                "text_size" => s.text_size = value.parse().unwrap_or(s.text_size).clamp(10.0, 96.0),
                _ => {}
            }
        }
        s
    }

    fn to_text(&self) -> String {
        let mut t = String::from("# Snapt settings. Edit while Snapt is closed.\n");
        let _ = writeln!(
            t,
            "# hotkey examples: PrintScreen, Ctrl+Shift+S, Cmd+Shift+9"
        );
        let _ = writeln!(t, "hotkey = {}", self.hotkey);
        let _ = writeln!(t, "folder = {}", self.folder.display());
        let _ = writeln!(t, "format = {}", self.format.extension());
        let _ = writeln!(t, "quick_save = {}", self.quick_save);
        let _ = writeln!(t, "autostart = {}", self.autostart);
        let _ = writeln!(t, "color = #{:06X}", self.color);
        let _ = writeln!(t, "stroke = {}", self.stroke);
        let _ = writeln!(t, "text_size = {}", self.text_size);
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
fn launch_path() -> Result<PathBuf> {
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
            hotkey: "Ctrl+Shift+S".into(),
            folder: PathBuf::from("/tmp/shots"),
            format: Format::Jpeg,
            quick_save: true,
            autostart: true,
            color: 0x00FF7F,
            stroke: 5.0,
            text_size: 28.0,
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
    }

    #[test]
    fn bad_lines_fall_back_to_defaults() {
        let s = Settings::parse("nonsense\nstroke = lots\ncolor = blue\nformat = gif\nunknown = 1");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn offered_hotkeys_parse() {
        for hotkey in HOTKEYS {
            assert!(
                hotkey.parse::<global_hotkey::hotkey::HotKey>().is_ok(),
                "{hotkey}"
            );
        }
    }
}
