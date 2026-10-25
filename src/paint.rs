//! Small drawing helpers shared by the overlay and the settings window.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect as SkRect, Transform};

/// Splits `0xRRGGBB` into bytes.
pub fn rgb(color: u32) -> [u8; 3] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}

/// Fills a rounded rectangle in `color` at `opacity`.
pub fn fill_round(
    pixmap: &mut Pixmap,
    rect: Option<SkRect>,
    radius: f32,
    color: u32,
    opacity: f32,
) {
    let Some(rect) = rect else { return };
    let Some(path) = round_rect(rect, radius) else {
        return;
    };
    let mut paint = Paint::default();
    let [r, g, b] = rgb(color);
    paint.set_color(Color::from_rgba8(r, g, b, (opacity * 255.0).round() as u8));
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

pub fn round_rect(r: SkRect, radius: f32) -> Option<tiny_skia::Path> {
    let k = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    // Cubic approximation of a quarter circle.
    let c = k * 0.4477;
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    let mut p = PathBuilder::new();
    p.move_to(l + k, t);
    p.line_to(rt - k, t);
    p.cubic_to(rt - c, t, rt, t + c, rt, t + k);
    p.line_to(rt, b - k);
    p.cubic_to(rt, b - c, rt - c, b, rt - k, b);
    p.line_to(l + k, b);
    p.cubic_to(l + c, b, l, b - c, l, b - k);
    p.line_to(l, t + k);
    p.cubic_to(l, t + c, l + c, t, l + k, t);
    p.close();
    p.finish()
}
