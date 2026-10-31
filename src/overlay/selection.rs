//! The selection rectangle and the mouse gestures that shape it.
//!
//! Coordinates are physical pixels of the overlay window. This module has no
//! windowing code, so the whole gesture logic is unit-tested.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A non-empty rectangle. `x + w` and `y + h` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// The rectangle spanning two corners, in any order. It is at least 1×1.
    pub fn from_corners(a: Point, b: Point) -> Self {
        let (left, right) = (a.x.min(b.x), a.x.max(b.x));
        let (top, bottom) = (a.y.min(b.y), a.y.max(b.y));
        Self {
            x: left,
            y: top,
            w: (right - left).max(1),
            h: (bottom - top).max(1),
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Clips the rectangle to `0..width` × `0..height`.
    fn clamp_to(self, width: i32, height: i32) -> Self {
        let left = self.x.clamp(0, width - 1);
        let top = self.y.clamp(0, height - 1);
        let right = self.right().clamp(left + 1, width);
        let bottom = self.bottom().clamp(top + 1, height);
        Self {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        }
    }
}

/// One of the eight grips on the selection border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::NW,
        Handle::N,
        Handle::NE,
        Handle::E,
        Handle::SE,
        Handle::S,
        Handle::SW,
        Handle::W,
    ];

    /// Where the grip sits on `rect`.
    pub fn position(self, rect: Rect) -> Point {
        let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        let (l, t, r, b) = (rect.x, rect.y, rect.right() - 1, rect.bottom() - 1);
        match self {
            Handle::N => Point::new(cx, t),
            Handle::S => Point::new(cx, b),
            Handle::E => Point::new(r, cy),
            Handle::W => Point::new(l, cy),
            Handle::NE => Point::new(r, t),
            Handle::NW => Point::new(l, t),
            Handle::SE => Point::new(r, b),
            Handle::SW => Point::new(l, b),
        }
    }

    fn moves_left(self) -> bool {
        matches!(self, Handle::W | Handle::NW | Handle::SW)
    }

    fn moves_right(self) -> bool {
        matches!(self, Handle::E | Handle::NE | Handle::SE)
    }

    fn moves_top(self) -> bool {
        matches!(self, Handle::N | Handle::NE | Handle::NW)
    }

    fn moves_bottom(self) -> bool {
        matches!(self, Handle::S | Handle::SE | Handle::SW)
    }
}

/// What lies under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Outside,
    Inside,
    Handle(Handle),
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    Idle,
    Creating {
        anchor: Point,
    },
    Moving {
        start: Point,
        from: Rect,
    },
    Resizing {
        handle: Handle,
        start: Point,
        from: Rect,
    },
}

/// The selection on one overlay, plus the drag in progress.
#[derive(Debug)]
pub struct Selection {
    rect: Option<Rect>,
    drag: Drag,
    width: i32,
    height: i32,
}

impl Selection {
    /// An empty selection on a `width` × `height` surface.
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            rect: None,
            drag: Drag::Idle,
            width: width.max(1),
            height: height.max(1),
        }
    }

    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    pub fn is_dragging(&self) -> bool {
        !matches!(self.drag, Drag::Idle)
    }

    /// Whether the selection is being drawn or resized, not just moved.
    /// A new area is being dragged out, not an existing one moved or resized.
    pub fn is_creating(&self) -> bool {
        matches!(self.drag, Drag::Creating { .. })
    }

    pub fn is_sizing(&self) -> bool {
        matches!(self.drag, Drag::Creating { .. } | Drag::Resizing { .. })
    }

    pub fn clear(&mut self) {
        self.rect = None;
        self.drag = Drag::Idle;
    }

    /// Selects `rect`, cut to the surface.
    pub fn set(&mut self, rect: Rect) {
        let x = rect.x.clamp(0, self.width - 1);
        let y = rect.y.clamp(0, self.height - 1);
        self.rect = Some(Rect {
            x,
            y,
            w: rect.w.clamp(1, self.width - x),
            h: rect.h.clamp(1, self.height - y),
        });
        self.drag = Drag::Idle;
    }

    pub fn select_all(&mut self) {
        self.rect = Some(Rect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        });
        self.drag = Drag::Idle;
    }

    /// What is under `p`. Grips are hit within `grip` pixels.
    pub fn hit(&self, p: Point, grip: i32) -> Hit {
        let Some(rect) = self.rect else {
            return Hit::Outside;
        };
        for handle in Handle::ALL {
            let h = handle.position(rect);
            if (p.x - h.x).abs() <= grip && (p.y - h.y).abs() <= grip {
                return Hit::Handle(handle);
            }
        }
        if rect.contains(p) {
            Hit::Inside
        } else {
            Hit::Outside
        }
    }

    /// Starts a drag: resize on a grip, move inside, or a new selection elsewhere.
    pub fn press(&mut self, p: Point, grip: i32) {
        let p = self.clamp_point(p);
        self.drag = match (self.hit(p, grip), self.rect) {
            (Hit::Handle(handle), Some(from)) => Drag::Resizing {
                handle,
                start: p,
                from,
            },
            (Hit::Inside, Some(from)) => Drag::Moving { start: p, from },
            _ => {
                self.rect = None;
                Drag::Creating { anchor: p }
            }
        };
    }

    /// Updates the drag. Returns true when the selection changed.
    pub fn motion(&mut self, p: Point) -> bool {
        let p = self.clamp_point(p);
        let next = match self.drag {
            Drag::Idle => return false,
            Drag::Creating { anchor } => Rect::from_corners(anchor, p),
            Drag::Moving { start, from } => {
                let x = (from.x + p.x - start.x).clamp(0, self.width - from.w);
                let y = (from.y + p.y - start.y).clamp(0, self.height - from.h);
                Rect { x, y, ..from }
            }
            Drag::Resizing {
                handle,
                start,
                from,
            } => {
                let (dx, dy) = (p.x - start.x, p.y - start.y);
                let mut a = Point::new(from.x, from.y);
                let mut b = Point::new(from.right(), from.bottom());
                if handle.moves_left() {
                    a.x += dx;
                }
                if handle.moves_right() {
                    b.x += dx;
                }
                if handle.moves_top() {
                    a.y += dy;
                }
                if handle.moves_bottom() {
                    b.y += dy;
                }
                Rect::from_corners(a, b).clamp_to(self.width, self.height)
            }
        };
        let changed = self.rect != Some(next);
        self.rect = Some(next);
        changed
    }

    /// Ends the drag. A click without movement leaves no selection.
    pub fn release(&mut self, p: Point) {
        self.motion(p);
        if let Drag::Creating { anchor } = self.drag
            && anchor == self.clamp_point(p)
        {
            self.rect = None;
        }
        self.drag = Drag::Idle;
    }

    fn clamp_point(&self, p: Point) -> Point {
        Point::new(p.x.clamp(0, self.width), p.y.clamp(0, self.height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRIP: i32 = 6;

    fn drag(sel: &mut Selection, from: (i32, i32), to: (i32, i32)) {
        sel.press(Point::new(from.0, from.1), GRIP);
        sel.motion(Point::new(to.0, to.1));
        sel.release(Point::new(to.0, to.1));
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Option<Rect> {
        Some(Rect { x, y, w, h })
    }

    #[test]
    fn dragging_creates_a_selection_in_any_direction() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (300, 200), (100, 50));
        assert_eq!(sel.rect(), rect(100, 50, 200, 150));
    }

    #[test]
    fn a_click_without_movement_selects_nothing() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (100, 100), (100, 100));
        assert_eq!(sel.rect(), None);
    }

    #[test]
    fn creating_is_clamped_to_the_screen() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (700, 500), (2000, -50));
        assert_eq!(sel.rect(), rect(700, 0, 100, 500));
    }

    #[test]
    fn dragging_inside_moves_and_stays_on_screen() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (100, 100), (300, 200));
        drag(&mut sel, (150, 150), (200, 170));
        assert_eq!(sel.rect(), rect(150, 120, 200, 100));
        drag(&mut sel, (200, 150), (5000, 5000));
        assert_eq!(sel.rect(), rect(600, 500, 200, 100));
    }

    #[test]
    fn dragging_a_corner_resizes() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (100, 100), (300, 200));
        // The bottom-right grip sits on the last pixel inside the selection.
        drag(&mut sel, (299, 199), (399, 249));
        assert_eq!(sel.rect(), rect(100, 100, 300, 150));
    }

    #[test]
    fn dragging_an_edge_resizes_one_side_only() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (100, 100), (300, 200));
        drag(&mut sel, (200, 100), (250, 40));
        assert_eq!(sel.rect(), rect(100, 40, 200, 160));
    }

    #[test]
    fn resizing_past_the_opposite_edge_flips() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (100, 100), (300, 200));
        drag(&mut sel, (100, 150), (400, 150));
        assert_eq!(sel.rect(), rect(300, 100, 100, 100));
    }

    #[test]
    fn pressing_outside_starts_over() {
        let mut sel = Selection::new(800, 600);
        drag(&mut sel, (100, 100), (300, 200));
        drag(&mut sel, (500, 400), (600, 450));
        assert_eq!(sel.rect(), rect(500, 400, 100, 50));
    }

    #[test]
    fn hit_testing_finds_grips_inside_and_outside() {
        let mut sel = Selection::new(800, 600);
        assert_eq!(sel.hit(Point::new(10, 10), GRIP), Hit::Outside);
        drag(&mut sel, (100, 100), (300, 200));
        assert_eq!(sel.hit(Point::new(98, 102), GRIP), Hit::Handle(Handle::NW));
        assert_eq!(sel.hit(Point::new(200, 199), GRIP), Hit::Handle(Handle::S));
        assert_eq!(sel.hit(Point::new(150, 150), GRIP), Hit::Inside);
        assert_eq!(sel.hit(Point::new(50, 50), GRIP), Hit::Outside);
    }

    #[test]
    fn select_all_covers_the_screen() {
        let mut sel = Selection::new(800, 600);
        sel.select_all();
        assert_eq!(sel.rect(), rect(0, 0, 800, 600));
    }
}
