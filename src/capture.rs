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
    #[cfg(target_os = "linux")]
    if crate::platform::is_wayland() {
        // One portal screenshot of the whole desktop, so the user is asked for
        // permission once and not per monitor.
        match crate::portal::screenshot() {
            Ok(desktop) => return Ok(split(desktop, &layout(&monitors))),
            Err(err) => log::warn!("portal screenshot failed, trying other ways: {err:#}"),
        }
    }
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

/// Where each monitor is on the desktop, in the units `Monitor` reports.
#[cfg(target_os = "linux")]
fn layout(monitors: &[Monitor]) -> Vec<(i32, i32, u32, u32)> {
    monitors
        .iter()
        .filter_map(|m| Some((m.x().ok()?, m.y().ok()?, m.width().ok()?, m.height().ok()?)))
        .collect()
}

/// Cuts a screenshot of the whole desktop into one shot per monitor. The
/// image may be in physical pixels while the layout is in logical ones, so
/// it is scaled to fit the layout's bounding box.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn split(desktop: RgbaImage, layout: &[(i32, i32, u32, u32)]) -> Vec<Shot> {
    let left = layout.iter().map(|m| m.0).min().unwrap_or(0);
    let top = layout.iter().map(|m| m.1).min().unwrap_or(0);
    let right = layout.iter().map(|m| m.0 + m.2 as i32).max().unwrap_or(0);
    let bottom = layout.iter().map(|m| m.1 + m.3 as i32).max().unwrap_or(0);
    if layout.len() < 2 || right <= left || bottom <= top {
        return vec![Shot {
            x: left,
            y: top,
            image: desktop,
        }];
    }
    let sx = desktop.width() as f64 / (right - left) as f64;
    let sy = desktop.height() as f64 / (bottom - top) as f64;
    layout
        .iter()
        .filter_map(|&(x, y, w, h)| {
            let x0 = (((x - left) as f64 * sx).round() as u32).min(desktop.width());
            let y0 = (((y - top) as f64 * sy).round() as u32).min(desktop.height());
            let x1 = ((((x - left) as f64 + w as f64) * sx).round() as u32).min(desktop.width());
            let y1 = ((((y - top) as f64 + h as f64) * sy).round() as u32).min(desktop.height());
            (x1 > x0 && y1 > y0).then(|| Shot {
                x,
                y,
                image: image::imageops::crop_imm(&desktop, x0, y0, x1 - x0, y1 - y0).to_image(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_screenshot_is_cut_per_monitor() {
        // Two 100×50 monitors side by side, captured at 2×.
        let desktop =
            RgbaImage::from_fn(400, 100, |x, _| image::Rgba([(x / 200) as u8, 0, 0, 255]));
        let shots = split(desktop, &[(0, 0, 100, 50), (100, 0, 100, 50)]);
        assert_eq!(shots.len(), 2);
        assert_eq!((shots[1].x, shots[1].y), (100, 0));
        assert_eq!(shots[1].image.dimensions(), (200, 100));
        assert_eq!(shots[0].image.get_pixel(199, 0)[0], 0);
        assert_eq!(shots[1].image.get_pixel(0, 0)[0], 1);
    }

    #[test]
    fn one_monitor_gets_the_whole_image() {
        let shots = split(RgbaImage::new(30, 20), &[(0, 0, 15, 10)]);
        assert_eq!(shots[0].image.dimensions(), (30, 20));
    }
}
