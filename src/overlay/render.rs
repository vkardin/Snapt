//! Software rendering of the overlay into a `0RGB` pixel buffer.

use image::RgbaImage;
use tiny_skia::{Pixmap, Rect as SkRect};

use super::selection::{Handle, Rect};
use crate::font;

pub const ACCENT: u32 = 0x8B5CF6;
const WHITE: u32 = 0xFFFFFF;
/// How much of the original brightness is kept outside the selection.
const DIM: u32 = 115; // out of 255

/// The frozen screenshot, prepared once in both bright and dimmed form.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    bright: Vec<u32>,
    dim: Vec<u32>,
}

impl Frame {
    /// Prepares `image` for a `width` × `height` window. The image is
    /// stretched only when its size differs from the window's.
    pub fn new(image: &RgbaImage, width: u32, height: u32) -> Self {
        let (iw, ih) = image.dimensions();
        let bright: Vec<u32> = if (iw, ih) == (width, height) {
            // The usual case: one image pixel per window pixel.
            image
                .as_raw()
                .chunks_exact(4)
                .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
                .collect()
        } else {
            let mut bright = Vec::with_capacity((width * height) as usize);
            for y in 0..height {
                let sy = (y as u64 * ih as u64 / height.max(1) as u64) as u32;
                for x in 0..width {
                    let sx = (x as u64 * iw as u64 / width.max(1) as u64) as u32;
                    let [r, g, b, _] = image.get_pixel(sx.min(iw - 1), sy.min(ih - 1)).0;
                    bright.push((r as u32) << 16 | (g as u32) << 8 | b as u32);
                }
            }
            bright
        };
        let dim = bright
            .iter()
            .map(|&p| {
                let c = |shift: u32| ((p >> shift & 0xFF) * DIM / 255) << shift;
                c(16) | c(8) | c(0)
            })
            .collect();
        Self {
            width,
            height,
            bright,
            dim,
        }
    }
}

/// Draws the dimmed screen with the selection at full brightness.
pub fn draw(buf: &mut [u32], frame: &Frame, selection: Option<Rect>) {
    buf.copy_from_slice(&frame.dim);
    let Some(rect) = selection else { return };
    let w = frame.width as i32;
    for y in rect.y..rect.bottom() {
        let row = (y * w) as usize;
        let range = row + rect.x as usize..row + rect.right() as usize;
        buf[range.clone()].copy_from_slice(&frame.bright[range]);
    }
}

/// Puts the dimmed screen and the bright `selection` back, only inside `area`.
/// A redraw that changed just part of the screen starts with this instead of `draw`.
pub fn restore(buf: &mut [u32], frame: &Frame, selection: Option<Rect>, area: Rect) {
    let w = frame.width as i32;
    let Some(area) = clip(area, w, frame.height as i32) else {
        return;
    };
    let bright = selection.and_then(|s| intersect(s, area));
    for y in area.y..area.bottom() {
        let row = (y * w) as usize;
        let range = row + area.x as usize..row + area.right() as usize;
        buf[range.clone()].copy_from_slice(&frame.dim[range]);
        if let Some(b) = bright.filter(|b| y >= b.y && y < b.bottom()) {
            let range = row + b.x as usize..row + b.right() as usize;
            buf[range.clone()].copy_from_slice(&frame.bright[range]);
        }
    }
}

/// The smallest rectangle around both; `None` stands for nothing.
pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let (x, y) = (a.x.min(b.x), a.y.min(b.y));
            Some(Rect {
                x,
                y,
                w: a.right().max(b.right()) - x,
                h: a.bottom().max(b.bottom()) - y,
            })
        }
        (a, b) => a.or(b),
    }
}

pub fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    let (r, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    (x < r && y < bottom).then_some(Rect {
        x,
        y,
        w: r - x,
        h: bottom - y,
    })
}

/// The part of `r` on a `width` × `height` screen.
pub fn clip(r: Rect, width: i32, height: i32) -> Option<Rect> {
    intersect(
        r,
        Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        },
    )
}

/// Draws the selection border, its grips and the size `label`, and returns
/// where it drew. `scale` is the monitor's scale factor.
pub fn draw_frame(buf: &mut [u32], width: i32, rect: Rect, label: &str, scale: f64) -> Rect {
    let height = buf.len() as i32 / width.max(1);
    let mut canvas = Canvas { buf, width, height };
    let px = |v: f64| ((v * scale).round() as i32).max(1);

    // Border, drawn just outside the selection so it never covers selected pixels.
    let t = px(1.0);
    canvas.fill(rect.x - t, rect.y - t, rect.w + 2 * t, t, ACCENT);
    canvas.fill(rect.x - t, rect.bottom(), rect.w + 2 * t, t, ACCENT);
    canvas.fill(rect.x - t, rect.y, t, rect.h, ACCENT);
    canvas.fill(rect.right(), rect.y, t, rect.h, ACCENT);

    // Grips: white squares with an accent outline.
    let half = px(3.5);
    for handle in Handle::ALL {
        let p = handle.position(rect);
        let size = 2 * half + 1;
        canvas.fill(
            p.x - half - t,
            p.y - half - t,
            size + 2 * t,
            size + 2 * t,
            ACCENT,
        );
        canvas.fill(p.x - half, p.y - half, size, size, WHITE);
    }

    // Size label above the selection, or inside it when there is no room.
    let label = draw_label(buf, width, rect, label, scale);
    let out = half + t + 1;
    let around = Rect {
        x: rect.x - out,
        y: rect.y - out,
        w: rect.w + 2 * out,
        h: rect.h + 2 * out,
    };
    union(Some(around), label).unwrap_or(around)
}

/// A clipped view of the pixel buffer.
struct Canvas<'a> {
    buf: &'a mut [u32],
    width: i32,
    height: i32,
}

impl Canvas<'_> {
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(self.width), (y + h).min(self.height));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for row in y0..y1 {
            let start = (row * self.width) as usize;
            self.buf[start + x0 as usize..start + x1 as usize].fill(color);
        }
    }
}

fn draw_label(buf: &mut [u32], width: i32, rect: Rect, label: &str, scale: f64) -> Option<Rect> {
    if label.is_empty() {
        return None;
    }
    let s = scale as f32;
    let size = 12.5 * s;
    let (tw, th) = font::measure(label, size);
    let (pad_x, pad_y) = (7.0 * s, 3.5 * s);
    let (w, h) = (
        (tw + 2.0 * pad_x).ceil() as i32,
        (th + 2.0 * pad_y).ceil() as i32,
    );
    let gap = (6.0 * s).round() as i32;
    let x = rect.x.clamp(0, (width - w).max(0));
    let above = rect.y - gap - h;
    let y = if above >= 0 { above } else { rect.y + gap };
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let bg = SkRect::from_xywh(0.0, 0.0, w as f32, h as f32);
    crate::paint::fill_round(&mut pixmap, bg, 5.0 * s, 0x111113, 0.85);
    font::draw(&mut pixmap, label, pad_x, pad_y, size, WHITE, 1.0);
    blend_pixmap(buf, width, &pixmap, x, y)
}

/// Composites a premultiplied RGBA pixmap over a `0RGB` buffer `width` pixels wide,
/// with the pixmap's top-left corner at (`x`, `y`). Parts off the buffer are clipped.
/// Returns the part of the buffer it covered.
pub fn blend_pixmap(buf: &mut [u32], width: i32, pixmap: &Pixmap, x: i32, y: i32) -> Option<Rect> {
    let height = buf.len() as i32 / width.max(1);
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + pw).min(width), (y + ph).min(height));
    let data = pixmap.data();
    for row in y0..y1 {
        let src_row = ((row - y) * pw) as usize;
        let dst_row = (row * width) as usize;
        for col in x0..x1 {
            let i = (src_row + (col - x) as usize) * 4;
            let a = data[i + 3] as u32;
            if a == 0 {
                continue;
            }
            let dst = &mut buf[dst_row + col as usize];
            if a == 255 {
                *dst = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32;
                continue;
            }
            let keep = 255 - a;
            let mix = |src: u8, shift: u32| {
                let d = (*dst >> shift) & 0xFF;
                ((src as u32 + (d * keep + 127) / 255).min(255)) << shift
            };
            *dst = mix(data[i], 16) | mix(data[i + 1], 8) | mix(data[i + 2], 0);
        }
    }
    (x0 < x1 && y0 < y1).then_some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn frame() -> Frame {
        let image = RgbaImage::from_pixel(100, 80, Rgba([200, 100, 50, 255]));
        Frame::new(&image, 100, 80)
    }

    #[test]
    fn without_selection_everything_is_dimmed() {
        let frame = frame();
        let mut buf = vec![0; 100 * 80];
        draw(&mut buf, &frame, None);
        let dimmed = (200 * DIM / 255) << 16 | (100 * DIM / 255) << 8 | (50 * DIM / 255);
        assert!(buf.iter().all(|&p| p == dimmed));
    }

    #[test]
    fn the_selection_shows_original_pixels() {
        let frame = frame();
        let mut buf = vec![0; 100 * 80];
        let rect = Rect {
            x: 20,
            y: 30,
            w: 40,
            h: 30,
        };
        draw(&mut buf, &frame, Some(rect));
        draw_frame(&mut buf, 100, rect, "40 × 30", 1.0);
        // A pixel well inside the selection, away from the grips.
        assert_eq!(buf[45 * 100 + 30], 0xC86432);
        // The border sits just outside the selection.
        assert_eq!(buf[38 * 100 + 19], ACCENT);
    }

    #[test]
    fn a_partial_redraw_matches_a_full_one() {
        let image = RgbaImage::from_fn(100, 80, |x, y| Rgba([x as u8 * 2, y as u8 * 3, 90, 255]));
        let frame = Frame::new(&image, 100, 80);
        let first = Rect {
            x: 10,
            y: 30,
            w: 30,
            h: 20,
        };
        let second = Rect {
            x: 50,
            y: 40,
            w: 35,
            h: 25,
        };
        let mut partial = vec![0; 100 * 80];
        draw(&mut partial, &frame, Some(first));
        let last = draw_frame(&mut partial, 100, first, "30 × 20", 1.0);
        let area = union(Some(last), Some(second)).unwrap();
        restore(&mut partial, &frame, Some(second), area);
        draw_frame(&mut partial, 100, second, "35 × 25", 1.0);
        let mut full = vec![0; 100 * 80];
        draw(&mut full, &frame, Some(second));
        draw_frame(&mut full, 100, second, "35 × 25", 1.0);
        assert!(partial == full);
    }

    #[test]
    fn frames_are_stretched_to_the_window() {
        let image = RgbaImage::from_pixel(50, 40, Rgba([1, 2, 3, 255]));
        let frame = Frame::new(&image, 100, 80);
        assert_eq!(frame.bright.len(), 100 * 80);
        assert!(frame.bright.iter().all(|&p| p == 0x010203));
    }
}
