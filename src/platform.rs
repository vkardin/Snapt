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
