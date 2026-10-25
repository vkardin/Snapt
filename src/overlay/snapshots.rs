//! Snapshot tests of what the overlay draws and what it saves.
//! See `crate::snapshot` for how to update the references.

use image::{Rgba, RgbaImage};
use tiny_skia::{Pixmap, Point as SkPoint, Transform};

use super::{
    annotate::{self, Kind, Shape, Tool},
    render::{self, Frame},
    selection::Rect,
    toolbar::{Button, Look, PALETTE, Target, Toolbar},
};
use crate::snapshot::{assert_matches, from_0rgb};

const W: u32 = 480;
const H: u32 = 320;

/// A stand-in screenshot: a gradient with a grid, so dimming and stretching show.
fn screen() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        if x % 40 == 0 || y % 40 == 0 {
            Rgba([40, 40, 40, 255])
        } else {
            Rgba([(x * 255 / W) as u8, (y * 255 / H) as u8, 200, 255])
        }
    })
}

fn shapes() -> Vec<Shape> {
    let p = SkPoint::from_xy;
    let mut out = Vec::new();
    let mut add = |tool, from, to, color, size| {
        let mut shape = Shape::start(tool, from, color, size);
        shape.drag_to(to, false);
        out.push(shape);
    };
    add(Tool::Rect, p(90.0, 70.0), p(200.0, 140.0), PALETTE[0], 3.0);
    add(
        Tool::Arrow,
        p(110.0, 210.0),
        p(230.0, 150.0),
        PALETTE[1],
        4.0,
    );
    add(Tool::Line, p(250.0, 80.0), p(330.0, 120.0), PALETTE[2], 2.0);
    add(
        Tool::Marker,
        p(240.0, 200.0),
        p(340.0, 200.0),
        PALETTE[3],
        14.0,
    );
    add(Tool::Pen, p(260.0, 150.0), p(320.0, 175.0), PALETTE[4], 3.0);
    add(Tool::Text, p(100.0, 160.0), p(100.0, 160.0), 0xFFFFFF, 18.0);
    if let Kind::Text(_, text) = &mut out.last_mut().unwrap().kind {
        text.push_str("Snapt 1×2");
    }
    out
}

/// The overlay with a selection, shapes and the toolbar, with or without the
/// color palette open.
fn overlay(palette: bool) -> RgbaImage {
    let sel = Rect {
        x: 80,
        y: 60,
        w: 280,
        h: 170,
    };
    let frame = Frame::new(&screen(), W, H);
    let mut buf = vec![0u32; (W * H) as usize];
    render::draw(&mut buf, &frame, Some(sel));
    let mut layer = Pixmap::new(sel.w as u32, sel.h as u32).unwrap();
    let transform = Transform::from_translate(-sel.x as f32, -sel.y as f32);
    for shape in shapes() {
        shape.draw(&mut layer, transform, None);
    }
    render::blend_pixmap(&mut buf, W as i32, &layer, sel.x, sel.y);
    render::draw_frame(&mut buf, W as i32, sel, "280 × 170", 1.0);
    let look = Look {
        tool: Some(Tool::Arrow),
        color: PALETTE[1],
        hover: Some(Target::Button(Button::Copy)),
        picking: false,
    };
    Toolbar::layout(sel, W as i32, H as i32, 1.0, palette).draw(&mut buf, W as i32, &look);
    from_0rgb(&buf, W, H)
}

#[test]
fn overlay_with_selection_shapes_and_toolbar() {
    // The hovered button shows its tooltip.
    let _lang = crate::snapshot::language(crate::i18n::Lang::En);
    assert_matches("overlay", &overlay(false));
    assert_matches("overlay-palette", &overlay(true));
}

#[test]
fn nothing_selected_dims_the_whole_screen() {
    // Half the size, as on a 2× monitor that reports its logical size.
    let frame = Frame::new(&screen(), W / 2, H / 2);
    let mut buf = vec![0u32; (W * H / 4) as usize];
    render::draw(&mut buf, &frame, None);
    assert_matches("overlay-idle", &from_0rgb(&buf, W / 2, H / 2));
}

#[test]
fn saved_image_has_the_shapes_at_image_scale() {
    // What a 2× screen saves for a selection at (80, 60) in window pixels.
    let screen = screen();
    let mut image = RgbaImage::from_fn(560, 340, |x, y| *screen.get_pixel(40 + x / 2, 30 + y / 2));
    annotate::flatten(&mut image, &shapes(), 80.0, 60.0, 2.0);
    assert_matches("saved-2x", &image);
}
