use anyhow::{Context, Result, ensure};
use image::{RgbaImage, imageops};
use xcap::Monitor;

/// A frozen image of one monitor.
pub struct Shot {
    /// Monitor position as reported by the OS capture API.
    pub x: i32,
    pub y: i32,
    /// Image pixels per unit of `x` and `y`: 1 where positions are physical,
    /// the display's scale on macOS where they are points.
    pub scale: f32,
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
    let image = monitor
        .capture_image()
        .context("failed to capture the screen")?;
    let width = monitor.width()?.max(1);
    Ok(Shot {
        x: monitor.x()?,
        y: monitor.y()?,
        scale: image.width() as f32 / width as f32,
        image,
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
        }
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
