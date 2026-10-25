use anyhow::{Context, Result, ensure};
use image::{RgbaImage, imageops};
use xcap::{Monitor, Window};

/// A frozen image of one monitor.
pub struct Shot {
    /// Monitor position as reported by the OS capture API.
    pub x: i32,
    pub y: i32,
    /// Image pixels per unit of `x` and `y`: 1 where positions are physical,
    /// the display's scale on macOS where they are points.
    pub scale: f32,
    pub image: RgbaImage,
    /// The windows on this monitor, front to back, in image pixels.
    pub windows: Vec<Bounds>,
}

/// A rectangle: position and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
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
    let windows = windows();
    let mut shots = shots;
    for shot in &mut shots {
        shot.windows = on_shot(&windows, shot);
    }
    Ok(shots)
}

/// The visible windows of other apps, front to back, in the units of `Shot::x`.
/// Snapping to windows is a nicety, so an error just leaves the list empty.
fn windows() -> Vec<Bounds> {
    let own = std::process::id();
    let windows = match Window::all() {
        Ok(windows) => windows,
        Err(err) => {
            log::warn!("failed to list windows: {err}");
            return Vec::new();
        }
    };
    let mut found: Vec<(i32, Bounds)> = windows
        .iter()
        .filter_map(|w| {
            if w.is_minimized().unwrap_or(true) || w.pid().ok()? == own {
                return None;
            }
            let bounds = Bounds {
                x: w.x().ok()?,
                y: w.y().ok()?,
                w: w.width().ok()? as i32,
                h: w.height().ok()? as i32,
            };
            (bounds.w > 1 && bounds.h > 1).then_some((w.z().unwrap_or(0), bounds))
        })
        .collect();
    // A higher z is closer to the front.
    found.sort_by_key(|&(z, _)| std::cmp::Reverse(z));
    found.into_iter().map(|(_, b)| b).collect()
}

/// `windows` moved into the image of `shot` and cut to it; those off it are dropped.
fn on_shot(windows: &[Bounds], shot: &Shot) -> Vec<Bounds> {
    let (iw, ih) = (shot.image.width() as i32, shot.image.height() as i32);
    windows
        .iter()
        .filter_map(|w| {
            let px = |v: i32| (v as f32 * shot.scale).round() as i32;
            let left = px(w.x - shot.x).max(0);
            let top = px(w.y - shot.y).max(0);
            let right = px(w.x + w.w - shot.x).min(iw);
            let bottom = px(w.y + w.h - shot.y).min(ih);
            (right > left && bottom > top).then_some(Bounds {
                x: left,
                y: top,
                w: right - left,
                h: bottom - top,
            })
        })
        .collect()
}

fn capture(monitor: &Monitor) -> Result<Shot> {
    let image = monitor
        .capture_image()
        .context("failed to capture the screen")?;
    let width = monitor.width()?.max(1);
    Ok(Shot {
        x: monitor.x()?,
        y: monitor.y()?,
        scale: image.width() as f32 / width as f32,
        image,
        windows: Vec::new(),
    })
}

/// Joins the shots of all monitors into one image, laid out as the monitors are.
pub fn stitch(shots: &[Shot]) -> RgbaImage {
    let place = |s: &Shot| {
        (
            (s.x as f32 * s.scale).round() as i64,
            (s.y as f32 * s.scale).round() as i64,
        )
    };
    let (mut left, mut top, mut right, mut bottom) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for shot in shots {
        let (x, y) = place(shot);
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + shot.image.width() as i64);
        bottom = bottom.max(y + shot.image.height() as i64);
    }
    if shots.is_empty() {
        return RgbaImage::new(1, 1);
    }
    let mut out = RgbaImage::new((right - left) as u32, (bottom - top) as u32);
    for shot in shots {
        let (x, y) = place(shot);
        imageops::replace(&mut out, &shot.image, x - left, y - top);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn shot(x: i32, y: i32, scale: f32, w: u32, h: u32, v: u8) -> Shot {
        Shot {
            x,
            y,
            scale,
            image: RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255])),
            windows: Vec::new(),
        }
    }

    #[test]
    fn windows_are_moved_onto_each_monitor_and_cut_to_it() {
        let windows = [
            Bounds {
                x: 950,
                y: 10,
                w: 100,
                h: 50,
            },
            Bounds {
                x: 100,
                y: 100,
                w: 20,
                h: 20,
            },
        ];
        // A 2× monitor whose position is in points, as on macOS.
        let right = shot(960, 0, 2.0, 1000, 800, 0);
        assert_eq!(
            on_shot(&windows, &right),
            vec![Bounds {
                x: 0,
                y: 20,
                w: 180,
                h: 100
            }]
        );
        let left = shot(0, 0, 1.0, 960, 1080, 0);
        assert_eq!(
            on_shot(&windows, &left),
            vec![
                Bounds {
                    x: 950,
                    y: 10,
                    w: 10,
                    h: 50
                },
                Bounds {
                    x: 100,
                    y: 100,
                    w: 20,
                    h: 20
                },
            ]
        );
    }

    #[test]
    fn monitors_are_stitched_side_by_side() {
        // A second monitor left of the primary one, lower down.
        let out = stitch(&[shot(0, 0, 1.0, 4, 3, 10), shot(-2, 1, 1.0, 2, 2, 20)]);
        assert_eq!(out.dimensions(), (6, 3));
        assert_eq!(out.get_pixel(2, 0).0[0], 10);
        assert_eq!(out.get_pixel(0, 1).0[0], 20);
        // The corner no monitor covers stays transparent.
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn point_positions_are_scaled_to_pixels() {
        let out = stitch(&[shot(0, 0, 2.0, 4, 2, 10), shot(2, 0, 2.0, 4, 2, 20)]);
        assert_eq!(out.dimensions(), (8, 2));
        assert_eq!(out.get_pixel(4, 0).0[0], 20);
    }
}
