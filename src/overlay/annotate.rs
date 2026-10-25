//! Annotations drawn over the selection: pen, line, arrow, rectangle, marker and text.
//!
//! Shapes are kept as vectors in window pixels and rasterized with `tiny_skia`,
//! so the same shapes render on screen and, scaled, onto the saved image.

use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Point, Rect as SkRect, Stroke,
    Transform,
};

use crate::font;

/// The drawing tools, in toolbar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Line,
    Arrow,
    Rect,
    Marker,
    Text,
}

#[cfg(test)]
impl Tool {
    pub const ALL: [Tool; 6] = [
        Tool::Pen,
        Tool::Line,
        Tool::Arrow,
        Tool::Rect,
        Tool::Marker,
        Tool::Text,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Pen(Vec<Point>),
    Line(Point, Point),
    Arrow(Point, Point),
    Rect(Point, Point),
    Marker(Vec<Point>),
    Text(Point, String),
}

/// One annotation. `size` is the stroke width, or the font size for text, in window pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub kind: Kind,
    pub color: u32,
    pub size: f32,
}

/// The marker is a wide, see-through stroke.
const MARKER_OPACITY: f32 = 0.4;
const MARKER_WIDTH: f32 = 4.0;

impl Shape {
    /// Starts a shape of `tool` at `p`.
    pub fn start(tool: Tool, p: Point, color: u32, size: f32) -> Self {
        let kind = match tool {
            Tool::Pen => Kind::Pen(vec![p]),
            Tool::Line => Kind::Line(p, p),
            Tool::Arrow => Kind::Arrow(p, p),
            Tool::Rect => Kind::Rect(p, p),
            Tool::Marker => Kind::Marker(vec![p]),
            Tool::Text => Kind::Text(p, String::new()),
        };
        Self { kind, color, size }
    }

    /// Extends the shape to `p` while the mouse is dragged.
    /// `straight` snaps lines to 45° steps and rectangles to squares.
    pub fn drag_to(&mut self, p: Point, straight: bool) {
        match &mut self.kind {
            Kind::Pen(points) | Kind::Marker(points) => {
                // Skip sub-pixel moves: they add points without changing the stroke.
                if points.last().is_none_or(|last| last.distance(p) >= 1.0) {
                    points.push(p);
                }
            }
            Kind::Line(a, b) | Kind::Arrow(a, b) => {
                *b = if straight { snap_angle(*a, p) } else { p }
            }
            Kind::Rect(a, b) => *b = if straight { square(*a, p) } else { p },
            Kind::Text(..) => {}
        }
    }

    /// Whether the shape would leave a visible mark.
    pub fn is_visible(&self) -> bool {
        match &self.kind {
            Kind::Pen(_) | Kind::Marker(_) => true,
            Kind::Line(a, b) | Kind::Arrow(a, b) | Kind::Rect(a, b) => a.distance(*b) >= 2.0,
            Kind::Text(_, text) => !text.trim().is_empty(),
        }
    }

    /// The stroke width a marker of this size paints with.
    fn stroke_width(&self) -> f32 {
        match self.kind {
            Kind::Marker(_) => self.size * MARKER_WIDTH,
            _ => self.size,
        }
    }

    /// A box that holds everything the shape paints, in window pixels.
    pub fn bounds(&self) -> Option<SkRect> {
        let points: Vec<Point> = match &self.kind {
            Kind::Pen(p) | Kind::Marker(p) => p.clone(),
            Kind::Line(a, b) | Kind::Arrow(a, b) | Kind::Rect(a, b) => vec![*a, *b],
            Kind::Text(at, text) => {
                let (w, h) = font::measure(text, self.size);
                // Room for the caret while the text is edited.
                let w = w + self.size;
                vec![
                    *at,
                    Point::from_xy(at.x + w, at.y + h.max(font::line_height(self.size))),
                ]
            }
        };
        let margin = match self.kind {
            Kind::Arrow(..) => self.size * 4.0 + 4.0,
            Kind::Text(..) => 2.0,
            _ => self.stroke_width() / 2.0 + 2.0,
        };
        let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in points {
            (l, t, r, b) = (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y));
        }
        SkRect::from_ltrb(l - margin, t - margin, r + margin, b + margin)
    }

    /// Paints the shape into `pixmap`, mapping window pixels through `transform`
    /// (a scale and a translation).
    pub fn draw(&self, pixmap: &mut Pixmap, transform: Transform) {
        let mut paint = Paint::default();
        let [r, g, b] = rgb(self.color);
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;
        let mut stroke = Stroke {
            width: self.stroke_width(),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };

        match &self.kind {
            Kind::Pen(points) | Kind::Marker(points) => {
                if matches!(self.kind, Kind::Marker(_)) {
                    paint.set_color_rgba8(r, g, b, (MARKER_OPACITY * 255.0) as u8);
                }
                let mut path = PathBuilder::new();
                path.move_to(points[0].x, points[0].y);
                if points.len() == 1 {
                    // A click paints a dot.
                    path.line_to(points[0].x + 0.01, points[0].y);
                }
                for p in &points[1..] {
                    path.line_to(p.x, p.y);
                }
                if let Some(path) = path.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
            Kind::Line(a, b) => {
                let mut path = PathBuilder::new();
                path.move_to(a.x, a.y);
                path.line_to(b.x, b.y);
                if let Some(path) = path.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
            Kind::Arrow(a, b) => draw_arrow(pixmap, *a, *b, self.size, &paint, transform),
            Kind::Rect(a, b) => {
                stroke.line_join = LineJoin::Miter;
                let rect =
                    SkRect::from_ltrb(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
                if let Some(rect) = rect {
                    let path = PathBuilder::from_rect(rect);
                    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                }
            }
            Kind::Text(at, text) => {
                let mut at = *at;
                transform.map_point(&mut at);
                font::draw(
                    pixmap,
                    text,
                    at.x,
                    at.y,
                    self.size * transform.sx,
                    self.color,
                    1.0,
                );
            }
        }
    }
}

fn draw_arrow(
    pixmap: &mut Pixmap,
    a: Point,
    b: Point,
    width: f32,
    paint: &Paint,
    transform: Transform,
) {
    let length = a.distance(b);
    if length < 0.5 {
        return;
    }
    let (dx, dy) = ((b.x - a.x) / length, (b.y - a.y) / length);
    // The head grows with the stroke but never takes more than half of a short arrow.
    let head = (width * 3.5 + 8.0).min(length * 0.5);
    let half = head * 0.55;
    let base = Point::from_xy(b.x - dx * head, b.y - dy * head);

    let mut shaft = PathBuilder::new();
    shaft.move_to(a.x, a.y);
    // Stop inside the head so the round cap does not poke out of its tip.
    shaft.line_to(base.x + dx * head * 0.3, base.y + dy * head * 0.3);
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    if let Some(path) = shaft.finish() {
        pixmap.stroke_path(&path, paint, &stroke, transform, None);
    }

    let mut tip = PathBuilder::new();
    tip.move_to(b.x, b.y);
    tip.line_to(base.x - dy * half, base.y + dx * half);
    tip.line_to(base.x + dy * half, base.y - dx * half);
    tip.close();
    if let Some(path) = tip.finish() {
        pixmap.fill_path(&path, paint, FillRule::Winding, transform, None);
    }
}

/// `p` moved onto the nearest 45° ray from `a`.
fn snap_angle(a: Point, p: Point) -> Point {
    let (dx, dy) = (p.x - a.x, p.y - a.y);
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    let length = (dx * dx + dy * dy).sqrt();
    Point::from_xy(a.x + length * angle.cos(), a.y + length * angle.sin())
}

/// The corner of the largest square from `a` toward `p`.
fn square(a: Point, p: Point) -> Point {
    let side = (p.x - a.x).abs().max((p.y - a.y).abs());
    Point::from_xy(
        a.x + side.copysign(p.x - a.x),
        a.y + side.copysign(p.y - a.y),
    )
}

pub fn rgb(color: u32) -> [u8; 3] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}

/// The shapes on one screen, with undo and redo.
#[derive(Default)]
pub struct Canvas {
    shapes: Vec<Shape>,
    undone: Vec<Shape>,
    /// Bumped on every change, so cached renders know when they are stale.
    pub version: u64,
}

impl Canvas {
    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub fn push(&mut self, shape: Shape) {
        if shape.is_visible() {
            self.shapes.push(shape);
            self.undone.clear();
            self.version += 1;
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(shape) = self.shapes.pop() else {
            return false;
        };
        self.undone.push(shape);
        self.version += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(shape) = self.undone.pop() else {
            return false;
        };
        self.shapes.push(shape);
        self.version += 1;
        true
    }

    pub fn clear(&mut self) {
        if !self.shapes.is_empty() || !self.undone.is_empty() {
            self.shapes.clear();
            self.undone.clear();
            self.version += 1;
        }
    }
}

/// Paints `shapes` onto `image`, a crop of the screen at (`x`, `y`) window pixels,
/// where one window pixel is `scale` image pixels.
pub fn flatten(image: &mut image::RgbaImage, shapes: &[Shape], x: f32, y: f32, scale: f32) {
    if shapes.is_empty() {
        return;
    }
    let Some(mut layer) = Pixmap::new(image.width(), image.height()) else {
        return;
    };
    let transform = Transform::from_translate(-x, -y).post_scale(scale, scale);
    for shape in shapes {
        shape.draw(&mut layer, transform);
    }
    for (dst, src) in image.pixels_mut().zip(layer.data().chunks_exact(4)) {
        let a = src[3] as u32;
        if a == 0 {
            continue;
        }
        let keep = 255 - a;
        for (d, &s) in dst.0.iter_mut().zip(src).take(3) {
            *d = (s as u32 + (*d as u32 * keep + 127) / 255).min(255) as u8;
        }
        dst.0[3] = (a + (dst.0[3] as u32 * keep + 127) / 255).min(255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn pt(x: f32, y: f32) -> Point {
        Point::from_xy(x, y)
    }

    #[test]
    fn shift_snaps_lines_and_squares() {
        let p = snap_angle(pt(0.0, 0.0), pt(100.0, 8.0));
        assert!((p.y).abs() < 0.01 && p.x > 99.0);
        let p = snap_angle(pt(0.0, 0.0), pt(50.0, 47.0));
        assert!((p.x - p.y).abs() < 0.01);
        assert_eq!(square(pt(10.0, 10.0), pt(40.0, -5.0)), pt(40.0, -20.0));
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut canvas = Canvas::default();
        let mut line = Shape::start(Tool::Line, pt(0.0, 0.0), 0xFF0000, 3.0);
        line.drag_to(pt(50.0, 50.0), false);
        canvas.push(line.clone());
        // A click without a drag leaves nothing to undo.
        canvas.push(Shape::start(Tool::Rect, pt(5.0, 5.0), 0xFF0000, 3.0));
        assert_eq!(canvas.shapes().len(), 1);
        assert!(canvas.undo());
        assert!(canvas.is_empty());
        assert!(!canvas.undo());
        assert!(canvas.redo());
        assert_eq!(canvas.shapes(), &[line]);
    }

    #[test]
    fn flatten_paints_shapes_at_image_scale() {
        // A 2× screen: a 10 px wide window area is 20 image pixels.
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
        let mut rect = Shape::start(Tool::Rect, pt(12.0, 12.0), 0x0000FF, 2.0);
        rect.drag_to(pt(22.0, 22.0), false);
        flatten(&mut image, &[rect], 10.0, 10.0, 2.0);
        // The left edge sits at (12 - 10) × 2 = 4 image pixels.
        assert_eq!(image.get_pixel(4, 20).0, [0, 0, 255, 255]);
        assert_eq!(image.get_pixel(20, 20).0, [255, 255, 255, 255]);
    }

    #[test]
    fn every_tool_paints_inside_its_bounds() {
        for tool in Tool::ALL {
            let size = if tool == Tool::Text { 20.0 } else { 3.0 };
            let mut shape = Shape::start(tool, pt(20.0, 20.0), 0x00FF00, size);
            shape.drag_to(pt(60.0, 50.0), false);
            if let Kind::Text(_, text) = &mut shape.kind {
                text.push_str("Hi");
            }
            let bounds = shape.bounds().unwrap();
            let mut pixmap = Pixmap::new(100, 100).unwrap();
            shape.draw(&mut pixmap, Transform::identity());
            let mut ink = 0;
            for (i, p) in pixmap.data().chunks(4).enumerate() {
                if p[3] > 0 {
                    ink += 1;
                    let (x, y) = ((i % 100) as f32, (i / 100) as f32);
                    assert!(
                        x >= bounds.left() - 1.0
                            && x <= bounds.right()
                            && y >= bounds.top() - 1.0
                            && y <= bounds.bottom(),
                        "{tool:?} painted ({x}, {y}) outside {bounds:?}"
                    );
                }
            }
            assert!(ink > 10, "{tool:?} painted nothing");
        }
    }
}
