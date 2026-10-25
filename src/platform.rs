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

/// Whether this is a Wayland session, where apps can't register global shortcuts.
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
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    use anyhow::{Context, ensure};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn is_gnome() -> bool {
        false
    }

    /// Windows 11 opens the Snipping Tool on PrtSc unless this per-user switch is off.
    pub fn apply(on: bool, _hotkey: &str) -> anyhow::Result<()> {
        let value = if on { "0" } else { "1" };
        let status = Command::new("reg")
            .args([
                "add",
                r"HKCU\Control Panel\Keyboard",
                "/v",
                "PrintScreenKeyForSnippingEnabled",
                "/t",
                "REG_DWORD",
                "/d",
                value,
                "/f",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .context("failed to run reg.exe")?;
        ensure!(status.success(), "reg.exe failed: {status}");
        Ok(())
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
