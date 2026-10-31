//! Platform quirks that the cross-platform crates leave to the app.

/// Whether Snapt may capture the screen. On macOS this needs the Screen
/// Recording permission: without it captures show only the wallpaper, so the
/// first attempt asks for it instead.
#[cfg(target_os = "macos")]
pub fn screen_access() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    // SAFETY: both functions take no arguments and only query or prompt for the permission.
    unsafe {
        if CGPreflightScreenCaptureAccess() {
            return true;
        }
        log::warn!("Screen Recording permission missing; asking for it");
        CGRequestScreenCaptureAccess()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn screen_access() -> bool {
    true
}

/// Whether this is a Wayland session, where shortcuts and screenshots go through the portal.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn is_wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Hands the screenshot key over to Snapt, or back to the system tool.
///
/// Windows and GNOME can be switched over directly. macOS only lets the user
/// change its screenshot shortcuts by hand, so its keyboard settings are opened.
pub fn replace_system_screenshot(on: bool, hotkey: &str) -> anyhow::Result<()> {
    system_screenshot::apply(on, hotkey)
}

/// Whether the system's screenshot tool still has some of its keys or links,
/// so Snapt has something to ask about. Only Windows can tell.
pub fn system_tool_active() -> bool {
    #[cfg(target_os = "windows")]
    return !crate::windows_shot::in_place();
    #[cfg(not(target_os = "windows"))]
    false
}

/// Whether the system lets the user pick between asking and replacing by itself.
pub const REPLACE_MODES: bool = cfg!(target_os = "windows");

/// Opens the page where the user makes Snapt the default for the screenshot
/// links, unless it already is. Windows only lets the user make that choice.
pub fn confirm_links() {
    #[cfg(target_os = "windows")]
    if !crate::windows_shot::links_are_snapt() {
        crate::windows_shot::open_link_choice();
    }
}

/// When Snapt is the default for the screenshot links, opens the page where
/// the user gives them back to the system tool.
pub fn release_links() {
    #[cfg(target_os = "windows")]
    if crate::windows_shot::links_are_snapt() {
        crate::windows_shot::open_snipping_tool_choice();
    }
}

/// Starts (`Some`) or stops catching the system's screenshot keys while Snapt
/// runs. Only Windows needs it: its Win+Shift+S can't be reassigned.
pub fn catch_system_keys(proxy: Option<winit::event_loop::EventLoopProxy<crate::app::UserEvent>>) {
    #[cfg(target_os = "windows")]
    match proxy {
        Some(proxy) => crate::windows_shot::start_hook(proxy),
        None => crate::windows_shot::stop_hook(),
    }
    #[cfg(not(target_os = "windows"))]
    let _ = proxy;
}

/// Lets the caught keys through while a shortcut field records keys.
pub fn pause_system_keys(paused: bool) {
    #[cfg(target_os = "windows")]
    crate::windows_shot::pause_hook(paused);
    #[cfg(not(target_os = "windows"))]
    let _ = paused;
}

/// What the "replace the system screenshot tool" option does on this system.
pub fn replace_system_hint() -> crate::i18n::T {
    use crate::i18n::T;
    if cfg!(target_os = "windows") {
        T::ReplaceHintWindows
    } else if cfg!(target_os = "macos") {
        T::ReplaceHintMac
    } else if system_screenshot::is_gnome() {
        T::ReplaceHintGnome
    } else {
        T::ReplaceHintLinux
    }
}

/// What to install when the system has no text recognizer.
pub fn ocr_missing_hint() -> crate::i18n::T {
    if cfg!(target_os = "windows") {
        crate::i18n::T::OcrMissingWindows
    } else {
        crate::i18n::T::OcrMissingLinux
    }
}

#[cfg(target_os = "windows")]
mod system_screenshot {
    pub fn is_gnome() -> bool {
        false
    }

    pub fn apply(on: bool, _hotkey: &str) -> anyhow::Result<()> {
        crate::windows_shot::apply(on)
    }
}

#[cfg(target_os = "macos")]
mod system_screenshot {
    pub fn is_gnome() -> bool {
        false
    }

    pub fn apply(on: bool, _hotkey: &str) -> anyhow::Result<()> {
        if on {
            open::that_detached("x-apple.systempreferences:com.apple.Keyboard-Settings.extension")?;
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod system_screenshot {
    use std::process::Command;

    use anyhow::{Context, bail};

    const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const SNAPT_BINDING: &str =
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/snapt/";

    pub fn is_gnome() -> bool {
        std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_ascii_uppercase().contains("GNOME"))
    }

    /// On GNOME, frees PrtSc from the built-in screenshot UI and binds the Snapt
    /// shortcut to `snapt --capture`, which also works on Wayland.
    pub fn apply(on: bool, hotkey: &str) -> anyhow::Result<()> {
        if !is_gnome() {
            // Other desktops are switched by hand, as the settings hint explains.
            return Ok(());
        }
        let list = gsettings(&["get", MEDIA_KEYS, "custom-keybindings"]).unwrap_or_default();
        let mut paths: Vec<String> = list
            .split('\'')
            .filter(|p| p.starts_with('/'))
            .map(str::to_string)
            .collect();
        paths.retain(|p| p != SNAPT_BINDING);
        if on {
            // Older GNOME has the media-keys screenshot keys, newer the screenshot UI.
            let _ = gsettings(&[
                "set",
                "org.gnome.shell.keybindings",
                "show-screenshot-ui",
                "[]",
            ]);
            let _ = gsettings(&["set", MEDIA_KEYS, "screenshot", "[]"]);
            paths.push(SNAPT_BINDING.into());
            let schema = format!("{MEDIA_KEYS}.custom-keybinding:{SNAPT_BINDING}");
            let exe = crate::settings::launch_path()?;
            gsettings(&["set", &schema, "name", "Snapt"])?;
            gsettings(&[
                "set",
                &schema,
                "command",
                &format!("\"{}\" --capture", exe.display()),
            ])?;
            gsettings(&["set", &schema, "binding", &gnome_binding(hotkey)])?;
        } else {
            let _ = gsettings(&["reset", "org.gnome.shell.keybindings", "show-screenshot-ui"]);
            let _ = gsettings(&["reset", MEDIA_KEYS, "screenshot"]);
        }
        let list = format!(
            "[{}]",
            paths
                .iter()
                .map(|p| format!("'{p}'"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        gsettings(&["set", MEDIA_KEYS, "custom-keybindings", &list])?;
        Ok(())
    }

    fn gsettings(args: &[&str]) -> anyhow::Result<String> {
        let out = Command::new("gsettings")
            .args(args)
            .output()
            .context("failed to run gsettings")?;
        if !out.status.success() {
            bail!(
                "gsettings {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// `Ctrl+Shift+S` in GNOME's accelerator syntax: `<Control><Shift>s`.
    pub fn gnome_binding(hotkey: &str) -> String {
        let mut out = String::new();
        for part in hotkey.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => out.push_str("<Control>"),
                "alt" | "option" => out.push_str("<Alt>"),
                "shift" => out.push_str("<Shift>"),
                "super" | "cmd" | "command" | "meta" => out.push_str("<Super>"),
                "printscreen" | "prtsc" => out.push_str("Print"),
                key => {
                    let key = key
                        .strip_prefix("key")
                        .or_else(|| key.strip_prefix("digit"))
                        .unwrap_or(key);
                    if key.len() > 1 && key.starts_with('f') {
                        out.push_str(&key.to_ascii_uppercase());
                    } else {
                        out.push_str(key);
                    }
                }
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn hotkeys_become_gnome_accelerators() {
            assert_eq!(super::gnome_binding("PrintScreen"), "Print");
            assert_eq!(super::gnome_binding("Ctrl+Shift+S"), "<Control><Shift>s");
            assert_eq!(super::gnome_binding("Alt+F9"), "<Alt>F9");
        }
    }
}
