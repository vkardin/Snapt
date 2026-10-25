//! Software rendering of the overlay into a `0RGB` pixel buffer.

use image::RgbaImage;

use super::selection::{Handle, Rect};

const ACCENT: u32 = 0x8B5CF6;
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
        let len = (width * height) as usize;
        let mut bright = Vec::with_capacity(len);
        let mut dim = Vec::with_capacity(len);
        let (iw, ih) = image.dimensions();
        for y in 0..height {
            let sy = (y as u64 * ih as u64 / height.max(1) as u64) as u32;
            for x in 0..width {
                let sx = (x as u64 * iw as u64 / width.max(1) as u64) as u32;
                let [r, g, b, _] = image.get_pixel(sx.min(iw - 1), sy.min(ih - 1)).0;
                let (r, g, b) = (r as u32, g as u32, b as u32);
                bright.push(r << 16 | g << 8 | b);
                dim.push((r * DIM / 255) << 16 | (g * DIM / 255) << 8 | (b * DIM / 255));
            }
        }
        Self {
            width,
            height,
            bright,
            dim,
        }
    }
}

/// Draws the overlay. `scale` is the monitor's scale factor, `label` the size text.
pub fn draw(buf: &mut [u32], frame: &Frame, selection: Option<Rect>, label: &str, scale: f64) {
    buf.copy_from_slice(&frame.dim);
    let Some(rect) = selection else { return };
    let w = frame.width as i32;

    for y in rect.y..rect.bottom() {
        let row = (y * w) as usize;
        let range = row + rect.x as usize..row + rect.right() as usize;
        buf[range.clone()].copy_from_slice(&frame.bright[range]);
    }

    let mut canvas = Canvas {
        buf,
        width: frame.width as i32,
        height: frame.height as i32,
    };
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
    let glyph = px(2.0);
    let pad = px(4.0);
    let (text_w, text_h) = font::measure(label, glyph);
    let (box_w, box_h) = (text_w + 2 * pad, text_h + 2 * pad);
    let box_x = rect.x.clamp(0, (canvas.width - box_w).max(0));
    let above = rect.y - t - px(4.0) - box_h;
    let box_y = if above >= 0 { above } else { rect.y + px(4.0) };
    canvas.fill(box_x, box_y, box_w, box_h, ACCENT);
    font::draw(&mut canvas, label, box_x + pad, box_y + pad, glyph, WHITE);
}

/// A clipped view of the pixel buffer.
pub struct Canvas<'a> {
    buf: &'a mut [u32],
    width: i32,
    height: i32,
}

impl Canvas<'_> {
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
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

/// A tiny 5×7 bitmap font: digits, `×` and space are all the label needs.
mod font {
    use super::Canvas;

    const W: i32 = 5;
    const H: i32 = 7;
    const GAP: i32 = 1;

    fn glyph(c: char) -> [u8; 7] {
        match c {
            '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
            '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
            '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
            '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
            '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
            '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
            '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
            '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
            '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
            '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
            '×' => [0x00, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x00],
            _ => [0; 7],
        }
    }

    pub fn measure(text: &str, scale: i32) -> (i32, i32) {
        let n = text.chars().count() as i32;
        ((n * (W + GAP) - GAP).max(0) * scale, H * scale)
    }

    pub fn draw(canvas: &mut Canvas, text: &str, x: i32, y: i32, scale: i32, color: u32) {
        for (i, c) in text.chars().enumerate() {
            let gx = x + i as i32 * (W + GAP) * scale;
            for (row, bits) in glyph(c).iter().enumerate() {
                for col in 0..W {
                    if bits & (0x10 >> col) != 0 {
                        let (px, py) = (gx + col * scale, y + row as i32 * scale);
                        canvas.fill(px, py, scale, scale, color);
                    }
                }
            }
        }
    }
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
        draw(&mut buf, &frame, None, "", 1.0);
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
        draw(&mut buf, &frame, Some(rect), "40×30", 1.0);
        // A pixel well inside the selection, away from the grips.
        assert_eq!(buf[45 * 100 + 30], 0xC86432);
        // The border sits just outside the selection.
        assert_eq!(buf[38 * 100 + 19], ACCENT);
    }

    #[test]
    fn frames_are_stretched_to_the_window() {
        let image = RgbaImage::from_pixel(50, 40, Rgba([1, 2, 3, 255]));
        let frame = Frame::new(&image, 100, 80);
        assert_eq!(frame.bright.len(), 100 * 80);
        assert!(frame.bright.iter().all(|&p| p == 0x010203));
    }
}
