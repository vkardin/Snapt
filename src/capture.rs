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

/// Captures every monitor, with the mouse pointer drawn in when `cursor` is set.
/// Monitors that fail are skipped with a warning.
pub fn all_monitors(cursor: bool) -> Result<Vec<Shot>> {
    let monitors = Monitor::all().context("failed to list monitors")?;
    let mut shots = Vec::new();
    #[cfg(target_os = "linux")]
    if crate::platform::is_wayland() {
        // One portal screenshot of the whole desktop, so the user is asked for
        // permission once and not per monitor.
        match crate::portal::screenshot() {
            Ok(desktop) => shots = split(desktop, &layout(&monitors)),
            Err(err) => log::warn!("portal screenshot failed, trying other ways: {err:#}"),
        }
    }
    if shots.is_empty() {
        shots = capture_each(&monitors);
    }
    ensure!(!shots.is_empty(), "could not capture any monitor");
    let windows = windows();
    for shot in &mut shots {
        shot.windows = on_shot(&windows, shot);
    }
    if cursor && let Some(pointer) = crate::cursor::position() {
        draw_pointer(&mut shots, &monitors, pointer);
    }
    Ok(shots)
}

/// Draws the pointer onto the shot it is over, sized for that monitor.
fn draw_pointer(shots: &mut [Shot], monitors: &[Monitor], (x, y): (f64, f64)) {
    for shot in shots {
        let px = ((x - shot.x as f64) * shot.scale as f64) as f32;
        let py = ((y - shot.y as f64) * shot.scale as f64) as f32;
        let (w, h) = shot.image.dimensions();
        if px < 0.0 || py < 0.0 || px >= w as f32 || py >= h as f32 {
            continue;
        }
        let scale = monitors
            .iter()
            .find(|m| m.x().ok() == Some(shot.x) && m.y().ok() == Some(shot.y))
            .and_then(|m| m.scale_factor().ok())
            .unwrap_or(shot.scale);
        crate::cursor::draw(&mut shot.image, px, py, scale.max(1.0));
        return;
    }
}

fn capture_each(monitors: &[Monitor]) -> Vec<Shot> {
    monitors
        .iter()
        .filter_map(|m| match capture(m) {
            Ok(shot) => Some(shot),
            Err(err) => {
                log::warn!("skipping a monitor: {err:#}");
                None
            }
        })
        .collect()
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
        let scale = match layout.first() {
            Some(&(_, _, w, _)) if w > 0 => desktop.width() as f32 / w as f32,
            _ => 1.0,
        };
        return vec![Shot {
            x: left,
            y: top,
            scale,
            image: desktop,
            windows: Vec::new(),
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
                scale: sx as f32,
                windows: Vec::new(),
                image: image::imageops::crop_imm(&desktop, x0, y0, x1 - x0, y1 - y0).to_image(),
            })
        })
        .collect()
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
