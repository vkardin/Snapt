use anyhow::{Context, Result, ensure};
use image::RgbaImage;
use xcap::Monitor;

/// A frozen image of one monitor.
pub struct Shot {
    /// Monitor position as reported by the OS capture API.
    pub x: i32,
    pub y: i32,
    pub image: RgbaImage,
}

/// Captures every monitor. Monitors that fail are skipped with a warning.
pub fn all_monitors() -> Result<Vec<Shot>> {
    let monitors = Monitor::all().context("failed to list monitors")?;
    let shots: Vec<Shot> = monitors
        .iter()
        .filter_map(|m| match capture(m) {
            Ok(shot) => Some(shot),
            Err(err) => {
                log::warn!("skipping a monitor: {err:#}");
                None
            }
        })
        .collect();
    ensure!(!shots.is_empty(), "could not capture any monitor");
    Ok(shots)
}

fn capture(monitor: &Monitor) -> Result<Shot> {
    Ok(Shot {
        x: monitor.x()?,
        y: monitor.y()?,
        image: monitor
            .capture_image()
            .context("failed to capture the screen")?,
    })
}
