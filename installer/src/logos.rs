//! The Snapt, Leveler and Video Glow icons as vector drawings, so they stay sharp at any scale.
//!
//! The shapes follow `assets/logo.svg` and `site/apps/*.svg`.

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder,
    Pixmap, Point, Rect, Shader, SpreadMode, Stroke, Transform,
};

use crate::paint::round_rect;

#[derive(Clone, Copy)]
pub enum Logo {
    Snapt,
    Leveler,
    VideoGlow,
}

/// Draws `logo` into a `size` square at (`x`, `y`), all in device pixels.
pub fn draw(pixmap: &mut Pixmap, logo: Logo, x: f32, y: f32, size: f32) {
    match logo {
        Logo::Snapt => snapt(pixmap, x, y, size),
        Logo::Leveler => leveler(pixmap, x, y, size),
        Logo::VideoGlow => video_glow(pixmap, x, y, size),
    }
}

/// Places a `units`-wide drawing whose top-left is (`ox`, `oy`) into the square.
fn place(x: f32, y: f32, size: f32, ox: f32, oy: f32, units: f32) -> Transform {
    let k = size / units;
    Transform::from_row(k, 0.0, 0.0, k, x - ox * k, y - oy * k)
}

fn color(rgb: u32, opacity: f32) -> Color {
    let [r, g, b] = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    Color::from_rgba8(r, g, b, (opacity * 255.0).round() as u8)
}

fn solid(rgb: u32, opacity: f32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color(rgb, opacity));
    paint.anti_alias = true;
    paint
}

fn gradient(
    from: (f32, f32),
    to: (f32, f32),
    stops: &[(f32, u32)],
    opacity: f32,
) -> Paint<'static> {
    let stops = stops
        .iter()
        .map(|&(at, rgb)| GradientStop::new(at, color(rgb, opacity)))
        .collect();
    let shader = LinearGradient::new(
        Point::from_xy(from.0, from.1),
        Point::from_xy(to.0, to.1),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(color(0x8B5CF6, opacity)));
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn fill(pixmap: &mut Pixmap, path: &Path, paint: &Paint, ts: Transform) {
    pixmap.fill_path(path, paint, FillRule::Winding, ts, None);
}

fn snapt(pixmap: &mut Pixmap, x: f32, y: f32, size: f32) {
    let ts = place(x, y, size, 0.0, 0.0, 1.0);
    if let Some(bg) = Rect::from_xywh(0.0, 0.0, 1.0, 1.0).and_then(|r| round_rect(r, 0.22)) {
        let paint = gradient(
            (0.0, 0.0),
            (0.0, 1.0),
            &[(0.0, 0x8B5CF6), (1.0, 0x5B21B6)],
            1.0,
        );
        fill(pixmap, &bg, &paint, ts);
    }
    // Four viewfinder corners and a dot, as in icon.rs.
    let (edge, arm) = (0.24, 0.16);
    let mut p = PathBuilder::new();
    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        let cx = 0.5 - sx * (0.5 - edge);
        let cy = 0.5 - sy * (0.5 - edge);
        p.move_to(cx + sx * arm, cy);
        p.line_to(cx, cy);
        p.line_to(cx, cy + sy * arm);
    }
    if let Some(path) = p.finish() {
        let mut s = stroke(0.07);
        s.line_join = LineJoin::Miter;
        pixmap.stroke_path(&path, &solid(0xFFFFFF, 1.0), &s, ts, None);
    }
    if let Some(dot) = PathBuilder::from_circle(0.5, 0.5, 0.07) {
        fill(pixmap, &dot, &solid(0xFFFFFF, 1.0), ts);
    }
}

fn leveler(pixmap: &mut Pixmap, x: f32, y: f32, size: f32) {
    let ts = place(x, y, size, 0.0, 0.0, 128.0);
    if let Some(bg) = Rect::from_xywh(0.0, 0.0, 128.0, 128.0).and_then(|r| round_rect(r, 30.0)) {
        fill(pixmap, &bg, &solid(0x0B0D10, 1.0), ts);
    }
    // The SVG's smooth curves with their reflected control points written out.
    let mut p = PathBuilder::new();
    p.move_to(20.0, 64.0);
    p.cubic_to(26.0, 64.0, 27.0, 34.0, 33.0, 34.0);
    p.cubic_to(39.0, 34.0, 40.0, 94.0, 46.0, 94.0);
    p.cubic_to(52.0, 94.0, 52.0, 46.0, 57.0, 46.0);
    p.cubic_to(62.0, 46.0, 62.0, 78.0, 66.0, 78.0);
    p.cubic_to(70.0, 78.0, 70.0, 58.0, 74.0, 58.0);
    p.cubic_to(78.0, 58.0, 78.0, 64.0, 82.0, 64.0);
    p.line_to(108.0, 64.0);
    if let Some(path) = p.finish() {
        let paint = gradient(
            (20.0, 0.0),
            (108.0, 0.0),
            &[(0.0, 0x7CF0C0), (1.0, 0x2FBF86)],
            1.0,
        );
        pixmap.stroke_path(&path, &paint, &stroke(9.0), ts, None);
    }
}

fn video_glow(pixmap: &mut Pixmap, x: f32, y: f32, size: f32) {
    let ts = place(x, y, size, 16.0, 16.0, 96.0);
    if let Some(bg) = Rect::from_xywh(16.0, 16.0, 96.0, 96.0).and_then(|r| round_rect(r, 24.0)) {
        fill(pixmap, &bg, &solid(0x0B0B0D, 1.0), ts);
    }
    let stops = [
        (0.0, 0xFFB36B),
        (0.45, 0xFF7A8A),
        (0.75, 0xA98BFF),
        (1.0, 0x5FB4FF),
    ];
    if let Some(screen) = Rect::from_xywh(32.0, 40.0, 64.0, 48.0).and_then(|r| round_rect(r, 10.0))
    {
        // Wide faint strokes stand in for the SVG's blur.
        for (width, opacity) in [(16.0, 0.18), (11.0, 0.3)] {
            let glow = gradient((32.0, 40.0), (96.0, 88.0), &stops, opacity);
            pixmap.stroke_path(&screen, &glow, &stroke(width), ts, None);
        }
        let paint = gradient((32.0, 40.0), (96.0, 88.0), &stops, 1.0);
        pixmap.stroke_path(&screen, &paint, &stroke(5.0), ts, None);
    }
    let mut p = PathBuilder::new();
    p.move_to(56.875, 54.5);
    p.line_to(73.5, 64.0);
    p.line_to(56.875, 73.5);
    p.close();
    if let Some(play) = p.finish() {
        pixmap.stroke_path(&play, &solid(0xF3F2EF, 0.55), &stroke(5.0), ts, None);
        fill(pixmap, &play, &solid(0xF3F2EF, 0.3), ts);
    }
}

/// The Snapt icon as straight-alpha RGBA, for the window icon.
pub fn snapt_rgba(size: u32) -> Vec<u8> {
    let Some(mut pixmap) = Pixmap::new(size, size) else {
        return Vec::new();
    };
    snapt(&mut pixmap, 0.0, 0.0, size as f32);
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}
