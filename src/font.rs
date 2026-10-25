//! The one font Snapt draws with: a Latin and Cyrillic subset of Inter SemiBold.
//!
//! Text is laid out line by line and rasterized straight into a `tiny_skia`
//! pixmap, which keeps text rendering free of any system font dependency.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use tiny_skia::Pixmap;

static DATA: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.otf");

fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(DATA).expect("the bundled font is valid"))
}

/// Whether the bundled font can draw `c`.
#[cfg(test)]
pub fn has_glyph(c: char) -> bool {
    c.is_whitespace() || font().glyph_id(c).0 != 0
}

/// The height of one line of text at `size` pixels.
pub fn line_height(size: f32) -> f32 {
    let f = font().as_scaled(PxScale::from(size));
    f.height() + f.line_gap()
}

/// The width and height of `text`, which may span several lines.
pub fn measure(text: &str, size: f32) -> (f32, f32) {
    let f = font().as_scaled(PxScale::from(size));
    let width = text
        .split('\n')
        .map(|line| line_width(&f, line))
        .fold(0.0, f32::max);
    let lines = text.split('\n').count() as f32;
    (width, lines * line_height(size))
}

/// The x offset of the end of `text`'s last line, for placing a caret.
pub fn last_line_width(text: &str, size: f32) -> f32 {
    let f = font().as_scaled(PxScale::from(size));
    line_width(&f, text.rsplit('\n').next().unwrap_or(""))
}

fn line_width(f: &impl ScaleFont<&'static FontRef<'static>>, line: &str) -> f32 {
    let mut width = 0.0;
    let mut previous = None;
    for c in line.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = previous {
            width += f.kern(p, id);
        }
        width += f.h_advance(id);
        previous = Some(id);
    }
    width
}

/// Draws `text` with its top-left corner at (`x`, `y`) in straight RGB `color` at `opacity`.
pub fn draw(pixmap: &mut Pixmap, text: &str, x: f32, y: f32, size: f32, color: u32, opacity: f32) {
    let f = font().as_scaled(PxScale::from(size));
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let rgb = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
    let data = pixmap.data_mut();
    for (n, line) in text.split('\n').enumerate() {
        let baseline = y + f.ascent() + n as f32 * line_height(size);
        let mut cx = x;
        let mut previous = None;
        for c in line.chars() {
            let id = f.glyph_id(c);
            if let Some(p) = previous {
                cx += f.kern(p, id);
            }
            previous = Some(id);
            let glyph = id.with_scale_and_position(size, point(cx, baseline));
            cx += f.h_advance(id);
            let Some(outlined) = font().outline_glyph(glyph) else {
                continue;
            };
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let alpha = (coverage.clamp(0.0, 1.0) * opacity * 255.0).round() as u32;
                let i = ((py * w + px) * 4) as usize;
                blend(&mut data[i..i + 4], rgb, alpha);
            });
        }
    }
}

/// Source-over blending of a straight color into a premultiplied RGBA pixel.
fn blend(dst: &mut [u8], rgb: [u8; 3], alpha: u32) {
    let keep = 255 - alpha;
    for c in 0..3 {
        dst[c] = ((rgb[c] as u32 * alpha + dst[c] as u32 * keep + 127) / 255) as u8;
    }
    dst[3] = ((alpha * 255 + dst[3] as u32 * keep + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_lines_and_draws_ink() {
        let (w1, h1) = measure("Snapt", 20.0);
        let (w2, h2) = measure("Snapt\nПривет, мир", 20.0);
        assert!(w1 > 30.0 && w2 > w1);
        assert!((h2 - 2.0 * h1).abs() < 0.01);

        let mut pixmap = Pixmap::new(120, 40).unwrap();
        draw(&mut pixmap, "Ж1", 2.0, 2.0, 24.0, 0xFF0000, 1.0);
        let ink = pixmap.data().chunks(4).filter(|p| p[3] > 128).count();
        assert!(ink > 50, "only {ink} pixels drawn");
    }
}
