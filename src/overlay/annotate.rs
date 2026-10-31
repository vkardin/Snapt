//! Annotations drawn over the selection: pen, line, arrow, rectangle, ellipse,
//! marker, text, numbered steps, pixelation, blur and spotlight.
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
    Ellipse,
    Marker,
    Text,
    /// Numbered steps: each click places the next number.
    Number,
    /// Hides what is under a rectangle behind big blocks of color.
    Pixelate,
    /// Hides what is under a rectangle behind a blur.
    Blur,
    /// Dims everything but a rectangle, to draw the eye to it.
    Spotlight,
}

#[cfg(test)]
impl Tool {
    pub const ALL: [Tool; 11] = [
        Tool::Pen,
        Tool::Line,
        Tool::Arrow,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Marker,
        Tool::Text,
        Tool::Number,
        Tool::Pixelate,
        Tool::Blur,
        Tool::Spotlight,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Pen(Vec<Point>),
    Line(Point, Point),
    Arrow(Point, Point),
    Rect(Point, Point),
    Ellipse(Point, Point),
    Marker(Vec<Point>),
    Text(Point, String),
    /// A filled circle around the center with a number in it.
    Number(Point, u32),
    Pixelate(Point, Point),
    Blur(Point, Point),
    /// The bright rectangle; everything else around it is dimmed.
    Spotlight(Point, Point),
}

/// The unannotated screenshot under a pixmap: pixmap pixel (`px`, `py`) is
/// image pixel ((`px` + `x`) × `scale`, (`py` + `y`) × `scale`).
pub struct Backdrop<'a> {
    pub image: &'a image::RgbaImage,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
}

/// One annotation. `size` is the stroke width, or the font size for text, in window pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub kind: Kind,
    pub color: u32,
    pub size: f32,
    /// Rectangles and ellipses are filled with `color` instead of outlined.
    pub fill: bool,
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
            Tool::Ellipse => Kind::Ellipse(p, p),
            Tool::Marker => Kind::Marker(vec![p]),
            Tool::Text => Kind::Text(p, String::new()),
            Tool::Number => Kind::Number(p, 1),
            Tool::Pixelate => Kind::Pixelate(p, p),
            Tool::Blur => Kind::Blur(p, p),
            Tool::Spotlight => Kind::Spotlight(p, p),
        };
        Self {
            kind,
            color,
            size,
            fill: false,
        }
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
            Kind::Rect(a, b)
            | Kind::Ellipse(a, b)
            | Kind::Pixelate(a, b)
            | Kind::Blur(a, b)
            | Kind::Spotlight(a, b) => *b = if straight { square(*a, p) } else { p },
            Kind::Text(..) | Kind::Number(..) => {}
        }
    }

    /// Whether the shape would leave a visible mark.
    pub fn is_visible(&self) -> bool {
        match &self.kind {
            Kind::Pen(_) | Kind::Marker(_) => true,
            Kind::Line(a, b)
            | Kind::Arrow(a, b)
            | Kind::Rect(a, b)
            | Kind::Ellipse(a, b)
            | Kind::Pixelate(a, b)
            | Kind::Blur(a, b)
            | Kind::Spotlight(a, b) => a.distance(*b) >= 2.0,
            Kind::Text(_, text) => !text.trim().is_empty(),
            Kind::Number(..) => true,
        }
    }

    /// The stroke width a marker of this size paints with.
    fn stroke_width(&self) -> f32 {
        match self.kind {
            Kind::Marker(_) => self.size * MARKER_WIDTH,
            _ => self.size,
        }
    }

    /// The radius of a numbered step's circle.
    fn number_radius(&self) -> f32 {
        self.size * 2.5 + 9.0
    }

    /// Moves the whole shape by `dx`, `dy` window pixels.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let shift = |p: &mut Point| *p = Point::from_xy(p.x + dx, p.y + dy);
        match &mut self.kind {
            Kind::Pen(points) | Kind::Marker(points) => points.iter_mut().for_each(shift),
            Kind::Line(a, b)
            | Kind::Arrow(a, b)
            | Kind::Rect(a, b)
            | Kind::Ellipse(a, b)
            | Kind::Pixelate(a, b)
            | Kind::Blur(a, b)
            | Kind::Spotlight(a, b) => {
                shift(a);
                shift(b);
            }
            Kind::Text(p, _) | Kind::Number(p, _) => shift(p),
        }
    }

    /// Whether the shape paints outside its bounds too: a spotlight dims all around its rectangle.
    pub fn paints_everywhere(&self) -> bool {
        matches!(self.kind, Kind::Spotlight(..))
    }

    /// A box that holds everything the shape paints, in window pixels; for a
    /// spotlight, its bright rectangle, which is what picks and moves it.
    pub fn bounds(&self) -> Option<SkRect> {
        let points: Vec<Point> = match &self.kind {
            Kind::Pen(p) | Kind::Marker(p) => p.clone(),
            Kind::Line(a, b)
            | Kind::Arrow(a, b)
            | Kind::Rect(a, b)
            | Kind::Ellipse(a, b)
            | Kind::Pixelate(a, b)
            | Kind::Blur(a, b)
            | Kind::Spotlight(a, b) => vec![*a, *b],
            Kind::Number(c, _) => {
                let r = self.number_radius();
                vec![
                    Point::from_xy(c.x - r, c.y - r),
                    Point::from_xy(c.x + r, c.y + r),
                ]
            }
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
            Kind::Text(..)
            | Kind::Number(..)
            | Kind::Pixelate(..)
            | Kind::Blur(..)
            | Kind::Spotlight(..) => 2.0,
            _ => self.stroke_width() / 2.0 + 2.0,
        };
        let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in points {
            (l, t, r, b) = (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y));
        }
        SkRect::from_ltrb(l - margin, t - margin, r + margin, b + margin)
    }

    /// Paints the shape into `pixmap`, mapping window pixels through `transform`
    /// (a scale and a translation). Pixelation needs the `backdrop` it hides;
    /// without one it shows as a gray box, as while it is being dragged.
    pub fn draw(&self, pixmap: &mut Pixmap, transform: Transform, backdrop: Option<&Backdrop>) {
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
                    self.paint_path(pixmap, &path, &paint, &stroke, transform);
                }
            }
            Kind::Ellipse(a, b) => {
                let rect =
                    SkRect::from_ltrb(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
                if let Some(path) = rect.and_then(PathBuilder::from_oval) {
                    self.paint_path(pixmap, &path, &paint, &stroke, transform);
                }
            }
            Kind::Number(c, n) => {
                let radius = self.number_radius();
                if let Some(dot) = PathBuilder::from_circle(c.x, c.y, radius) {
                    pixmap.fill_path(&dot, &paint, FillRule::Winding, transform, None);
                }
                let mut center = *c;
                transform.map_point(&mut center);
                let size = radius * 1.1 * transform.sx;
                let text = n.to_string();
                let (w, h) = font::measure(&text, size);
                // White on light colors would vanish; use black there.
                let light = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 > 170;
                let ink = if light { 0x000000 } else { 0xFFFFFF };
                font::draw(
                    pixmap,
                    &text,
                    center.x - w / 2.0,
                    center.y - h / 2.0,
                    size,
                    ink,
                    1.0,
                );
            }
            Kind::Pixelate(a, b) => {
                let (mut a, mut b) = (*a, *b);
                transform.map_point(&mut a);
                transform.map_point(&mut b);
                let block = ((self.size * 3.0).max(8.0) * transform.sx).round().max(2.0) as i32;
                pixelate(pixmap, a, b, block, backdrop);
            }
            Kind::Blur(a, b) => {
                let (mut a, mut b) = (*a, *b);
                transform.map_point(&mut a);
                transform.map_point(&mut b);
                let radius = ((self.size * 2.0).max(6.0) * transform.sx).round().max(1.0) as i32;
                blur(pixmap, a, b, radius, backdrop);
            }
            Kind::Spotlight(a, b) => {
                let (mut a, mut b) = (*a, *b);
                transform.map_point(&mut a);
                transform.map_point(&mut b);
                dim_around(pixmap, &[(a, b)]);
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

impl Shape {
    /// Fills `path` or strokes its outline, as `fill` says. The stroke still
    /// goes around a filled shape, so it keeps its size.
    fn paint_path(
        &self,
        pixmap: &mut Pixmap,
        path: &tiny_skia::Path,
        paint: &Paint,
        stroke: &Stroke,
        transform: Transform,
    ) {
        if self.fill {
            pixmap.fill_path(path, paint, FillRule::Winding, transform, None);
        }
        pixmap.stroke_path(path, paint, stroke, transform, None);
    }
}

/// Paints `shapes` in order into `pixmap`. Spotlights go first, under the
/// rest, and share one dimmed backdrop, so two of them do not dim each other.
pub fn draw_all(
    shapes: &[Shape],
    pixmap: &mut Pixmap,
    transform: Transform,
    backdrop: Option<&Backdrop>,
) {
    let holes: Vec<(Point, Point)> = shapes
        .iter()
        .filter_map(|s| match s.kind {
            Kind::Spotlight(mut a, mut b) => {
                transform.map_point(&mut a);
                transform.map_point(&mut b);
                Some((a, b))
            }
            _ => None,
        })
        .collect();
    if !holes.is_empty() {
        dim_around(pixmap, &holes);
    }
    for shape in shapes.iter().filter(|s| !s.paints_everywhere()) {
        shape.draw(pixmap, transform, backdrop);
    }
}

/// How dark a spotlight makes the rest of the picture.
const SPOTLIGHT_DIM: f32 = 0.55;

/// Darkens `pixmap` everywhere outside the device-pixel rectangles `holes`.
fn dim_around(pixmap: &mut Pixmap, holes: &[(Point, Point)]) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let holes: Vec<(i32, i32, i32, i32)> = holes
        .iter()
        .map(|(a, b)| {
            (
                (a.x.min(b.x).round() as i32).clamp(0, pw),
                (a.y.min(b.y).round() as i32).clamp(0, ph),
                (a.x.max(b.x).round() as i32).clamp(0, pw),
                (a.y.max(b.y).round() as i32).clamp(0, ph),
            )
        })
        .collect();
    let keep = 1.0 - SPOTLIGHT_DIM;
    let add = (SPOTLIGHT_DIM * 255.0).round() as u32;
    let scale = |v: u8| (v as f32 * keep).round() as u8;
    let data = pixmap.data_mut();
    for y in 0..ph {
        for x in 0..pw {
            let lit = holes
                .iter()
                .any(|&(l, t, r, b)| x >= l && x < r && y >= t && y < b);
            if lit {
                continue;
            }
            // Black at SPOTLIGHT_DIM over what is there, premultiplied.
            let i = ((y * pw + x) * 4) as usize;
            let px = &mut data[i..i + 4];
            let alpha = (px[3] as u32 * (255 - add) / 255 + add).min(255) as u8;
            let dimmed = [scale(px[0]), scale(px[1]), scale(px[2]), alpha];
            px.copy_from_slice(&dimmed);
        }
    }
}

/// Blurs the backdrop under the device-pixel rectangle `a`–`b` into `pixmap`:
/// three box blurs of `radius` in a row, which look close to a Gaussian one.
fn blur(pixmap: &mut Pixmap, a: Point, b: Point, radius: i32, backdrop: Option<&Backdrop>) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let left = (a.x.min(b.x).round() as i32).clamp(0, pw);
    let top = (a.y.min(b.y).round() as i32).clamp(0, ph);
    let right = (a.x.max(b.x).round() as i32).clamp(0, pw);
    let bottom = (a.y.max(b.y).round() as i32).clamp(0, ph);
    let Some(backdrop) = backdrop else {
        let rect = SkRect::from_ltrb(left as f32, top as f32, right as f32, bottom as f32);
        crate::paint::fill_round(pixmap, rect, 0.0, 0x808080, 0.6);
        return;
    };
    let (w, h) = ((right - left) as usize, (bottom - top) as usize);
    if w == 0 || h == 0 {
        return;
    }
    let (iw, ih) = (
        backdrop.image.width() as i32,
        backdrop.image.height() as i32,
    );
    let mut px: Vec<[u32; 3]> = Vec::with_capacity(w * h);
    for y in top..bottom {
        for x in left..right {
            let ix = (((x + backdrop.x) as f32 * backdrop.scale) as i32).clamp(0, iw - 1);
            let iy = (((y + backdrop.y) as f32 * backdrop.scale) as i32).clamp(0, ih - 1);
            let p = backdrop.image.get_pixel(ix as u32, iy as u32).0;
            px.push([p[0] as u32, p[1] as u32, p[2] as u32]);
        }
    }
    let mut tmp = px.clone();
    for _ in 0..3 {
        box_blur(&px, &mut tmp, w, h, radius as usize, 1, w);
        box_blur(&tmp, &mut px, h, w, radius as usize, w, 1);
    }
    let data = pixmap.data_mut();
    for (j, row) in px.chunks_exact(w).enumerate() {
        for (i, c) in row.iter().enumerate() {
            let at = (((top as usize + j) * pw as usize) + left as usize + i) * 4;
            data[at..at + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
}

/// One box-blur pass of `radius` along `len`-long lines, `lines` of them. A
/// line's pixels are `step` apart, and the lines start `stride` apart; the
/// edge pixels repeat past the ends.
fn box_blur(
    src: &[[u32; 3]],
    dst: &mut [[u32; 3]],
    len: usize,
    lines: usize,
    radius: usize,
    step: usize,
    stride: usize,
) {
    let window = (2 * radius + 1) as u32;
    let at = |line: usize, i: isize| line * stride + (i.clamp(0, len as isize - 1) as usize) * step;
    for line in 0..lines {
        let mut sum = [0u32; 3];
        for i in -(radius as isize)..=radius as isize {
            let p = src[at(line, i)];
            for c in 0..3 {
                sum[c] += p[c];
            }
        }
        for i in 0..len as isize {
            dst[at(line, i)] = sum.map(|s| s / window);
            let (add, drop) = (
                src[at(line, i + radius as isize + 1)],
                src[at(line, i - radius as isize)],
            );
            for c in 0..3 {
                sum[c] = sum[c] + add[c] - drop[c];
            }
        }
    }
}

/// Fills the device-pixel rectangle `a`–`b` with `block`-sized squares, each the
/// average color of the backdrop under it.
fn pixelate(pixmap: &mut Pixmap, a: Point, b: Point, block: i32, backdrop: Option<&Backdrop>) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let left = (a.x.min(b.x).round() as i32).clamp(0, pw);
    let top = (a.y.min(b.y).round() as i32).clamp(0, ph);
    let right = (a.x.max(b.x).round() as i32).clamp(0, pw);
    let bottom = (a.y.max(b.y).round() as i32).clamp(0, ph);
    let Some(backdrop) = backdrop else {
        let rect = SkRect::from_ltrb(left as f32, top as f32, right as f32, bottom as f32);
        crate::paint::fill_round(pixmap, rect, 0.0, 0x808080, 0.6);
        return;
    };
    let (iw, ih) = (
        backdrop.image.width() as i32,
        backdrop.image.height() as i32,
    );
    let data = pixmap.data_mut();
    let mut by = top;
    while by < bottom {
        let mut bx = left;
        let bh = block.min(bottom - by);
        while bx < right {
            let bw = block.min(right - bx);
            let (mut sum, mut n) = ([0u32; 3], 0u32);
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let ix = ((x + backdrop.x) as f32 * backdrop.scale) as i32;
                    let iy = ((y + backdrop.y) as f32 * backdrop.scale) as i32;
                    if ix >= 0 && iy >= 0 && ix < iw && iy < ih {
                        let p = backdrop.image.get_pixel(ix as u32, iy as u32).0;
                        for c in 0..3 {
                            sum[c] += p[c] as u32;
                        }
                        n += 1;
                    }
                }
            }
            if let [Some(r), Some(g), Some(b)] = sum.map(|c| c.checked_div(n)) {
                let avg = [r, g, b];
                for y in by..by + bh {
                    for x in bx..bx + bw {
                        let i = ((y * pw + x) * 4) as usize;
                        data[i..i + 4].copy_from_slice(&[
                            avg[0] as u8,
                            avg[1] as u8,
                            avg[2] as u8,
                            255,
                        ]);
                    }
                }
            }
            bx += block;
        }
        by += block;
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
    /// Earlier states of `shapes`, for undo, and the undone ones, for redo.
    undo: Vec<Vec<Shape>>,
    redo: Vec<Vec<Shape>>,
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

    /// Remembers the current state before a change.
    fn checkpoint(&mut self) {
        self.undo.push(self.shapes.clone());
        self.redo.clear();
        self.version += 1;
    }

    pub fn push(&mut self, shape: Shape) {
        if shape.is_visible() {
            self.checkpoint();
            self.shapes.push(shape);
        }
    }

    /// The topmost shape whose bounds hold `p`.
    pub fn hit(&self, p: Point) -> Option<usize> {
        self.shapes.iter().rposition(|s| {
            s.bounds().is_some_and(|b| {
                p.x >= b.left() && p.x <= b.right() && p.y >= b.top() && p.y <= b.bottom()
            })
        })
    }

    /// Starts moving shape `i`, raising it above the rest: one undo step covers
    /// the whole drag. Returns its new index.
    pub fn begin_move(&mut self, i: usize) -> usize {
        self.checkpoint();
        if i < self.shapes.len() {
            let shape = self.shapes.remove(i);
            self.shapes.push(shape);
        }
        self.shapes.len().saturating_sub(1)
    }

    pub fn translate(&mut self, i: usize, dx: f32, dy: f32) {
        if let Some(shape) = self.shapes.get_mut(i) {
            shape.translate(dx, dy);
            self.version += 1;
        }
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.shapes.len() {
            self.checkpoint();
            self.shapes.remove(i);
        }
    }

    /// The number the next numbered step gets: one past the highest on screen.
    pub fn next_number(&self) -> u32 {
        self.shapes
            .iter()
            .filter_map(|s| match s.kind {
                Kind::Number(_, n) => Some(n),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.shapes, previous));
        self.version += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.shapes, next));
        self.version += 1;
        true
    }

    pub fn clear(&mut self) {
        if !self.shapes.is_empty() || !self.undo.is_empty() {
            self.shapes.clear();
            self.undo.clear();
            self.redo.clear();
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
    let original = image.clone();
    let backdrop = Backdrop {
        image: &original,
        x: 0,
        y: 0,
        scale: 1.0,
    };
    draw_all(shapes, &mut layer, transform, Some(&backdrop));
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
    fn moves_and_removals_undo_too() {
        let mut canvas = Canvas::default();
        let mut rect = Shape::start(Tool::Rect, pt(10.0, 10.0), 0xFF0000, 3.0);
        rect.drag_to(pt(30.0, 30.0), false);
        canvas.push(rect.clone());
        let i = canvas.hit(pt(20.0, 11.0)).unwrap();
        let i = canvas.begin_move(i);
        canvas.translate(i, 5.0, 5.0);
        canvas.translate(i, 5.0, 5.0);
        assert_eq!(canvas.hit(pt(10.0, 10.0)), None);
        canvas.remove(i);
        assert!(canvas.is_empty());
        assert!(canvas.undo());
        assert!(canvas.undo());
        assert_eq!(canvas.shapes(), &[rect]);
    }

    #[test]
    fn pixelation_averages_the_backdrop() {
        let mut image = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        for x in 0..20 {
            for y in 0..10 {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let mut shape = Shape::start(Tool::Pixelate, pt(0.0, 0.0), 0, 10.0 / 3.0);
        shape.drag_to(pt(20.0, 20.0), false);
        flatten(&mut image, &[shape], 0.0, 0.0, 1.0);
        // Blocks of 10: the top row stays white, the bottom black, each block flat.
        assert_eq!(image.get_pixel(3, 3).0, [255, 255, 255, 255]);
        assert_eq!(image.get_pixel(15, 15).0, [0, 0, 0, 255]);
    }

    #[test]
    fn blur_softens_an_edge_and_keeps_flat_areas() {
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        for x in 0..40 {
            for y in 0..20 {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let mut shape = Shape::start(Tool::Blur, pt(0.0, 0.0), 0, 1.0);
        shape.drag_to(pt(40.0, 40.0), false);
        flatten(&mut image, &[shape], 0.0, 0.0, 1.0);
        // Far from the edge nothing changes; next to it, gray.
        assert_eq!(image.get_pixel(20, 1).0, [255, 255, 255, 255]);
        assert_eq!(image.get_pixel(20, 38).0, [0, 0, 0, 255]);
        let near = image.get_pixel(20, 19).0[0];
        assert!(near > 60 && near < 250, "{near}");
    }

    #[test]
    fn spotlights_dim_everything_but_their_holes() {
        let mut image = RgbaImage::from_pixel(60, 30, Rgba([200, 200, 200, 255]));
        let spot = |x: f32| {
            let mut s = Shape::start(Tool::Spotlight, pt(x, 5.0), 0, 3.0);
            s.drag_to(pt(x + 10.0, 25.0), false);
            s
        };
        flatten(&mut image, &[spot(5.0), spot(40.0)], 0.0, 0.0, 1.0);
        // Both holes stay as they were, and the two do not darken each other.
        assert_eq!(image.get_pixel(10, 15).0, [200, 200, 200, 255]);
        assert_eq!(image.get_pixel(45, 15).0, [200, 200, 200, 255]);
        let dim = image.get_pixel(30, 15).0;
        assert!(dim[0] > 60 && dim[0] < 120 && dim[3] == 255, "{dim:?}");
        assert_eq!(image.get_pixel(30, 2).0, dim);
    }

    #[test]
    fn filled_rectangles_cover_their_inside() {
        let mut image = RgbaImage::from_pixel(30, 30, Rgba([0, 0, 0, 255]));
        let mut shape = Shape::start(Tool::Rect, pt(5.0, 5.0), 0xFF0000, 2.0);
        shape.drag_to(pt(25.0, 25.0), false);
        shape.fill = true;
        flatten(&mut image, &[shape], 0.0, 0.0, 1.0);
        assert_eq!(image.get_pixel(15, 15).0, [255, 0, 0, 255]);
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
        for tool in Tool::ALL.into_iter().filter(|&t| t != Tool::Spotlight) {
            let size = if tool == Tool::Text { 20.0 } else { 3.0 };
            let mut shape = Shape::start(tool, pt(20.0, 20.0), 0x00FF00, size);
            shape.drag_to(pt(60.0, 50.0), false);
            if let Kind::Text(_, text) = &mut shape.kind {
                text.push_str("Hi");
            }
            let bounds = shape.bounds().unwrap();
            let mut pixmap = Pixmap::new(100, 100).unwrap();
            shape.draw(&mut pixmap, Transform::identity(), None);
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
