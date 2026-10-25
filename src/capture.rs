use anyhow::{Context, Result, anyhow};
use image::RgbaImage;
use xcap::Monitor;

/// Captures the whole primary monitor.
///
/// This is the simplest useful capture. Region selection on every monitor
/// replaces it in the next milestone.
pub fn primary_monitor() -> Result<RgbaImage> {
    let monitors = Monitor::all().context("failed to list monitors")?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| anyhow!("no monitors found"))?;
    monitor
        .capture_image()
        .context("failed to capture the screen")
}
