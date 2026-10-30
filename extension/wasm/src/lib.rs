//! Snapt's annotation code for the browser extension's editor.
//!
//! The shapes, their rendering, the font, undo and redo are the desktop app's
//! own modules, compiled to WebAssembly unchanged. This file is a small C-style
//! API over them: JavaScript passes numbers, and strings and pixels through
//! buffers in the module's memory. All coordinates are image pixels.

#[allow(dead_code)]
#[path = "../../../src/overlay/annotate.rs"]
mod annotate;
#[allow(dead_code)]
#[path = "../../../src/font.rs"]
mod font;
#[allow(dead_code)]
#[path = "../../../src/paint.rs"]
mod paint;

use std::cell::RefCell;

use annotate::{Canvas, Kind, Shape, Tool};
use image::RgbaImage;
use tiny_skia::{Point, Rect as SkRect};

const TOOLS: [Tool; 10] = [
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
];

#[derive(Default)]
struct Editor {
    image: RgbaImage,
    canvas: Canvas,
    /// The shape being drawn or typed, not yet on the canvas.
    live: Option<Shape>,
    /// The last render, which JavaScript reads straight from memory.
    pixels: Vec<u8>,
    /// Four floats for `bounds`.
    rect: [f32; 4],
}

thread_local! {
    static EDITOR: RefCell<Editor> = RefCell::new(Editor::default());
}

fn with<T>(f: impl FnOnce(&mut Editor) -> T) -> T {
    EDITOR.with(|e| f(&mut e.borrow_mut()))
}

/// Reserves `len` bytes for JavaScript to write into.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Frees a buffer from `alloc` that was not handed back to the editor.
///
/// # Safety
/// `ptr` and `len` must come from one `alloc` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Starts over with the `width` × `height` RGBA image in the buffer at `ptr`,
/// which the editor takes over.
///
/// # Safety
/// `ptr` must come from `alloc(width * height * 4)` and be filled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn load(ptr: *mut u8, width: u32, height: u32) -> bool {
    let len = width as usize * height as usize * 4;
    let data = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let Some(image) = RgbaImage::from_raw(width, height, data) else {
        return false;
    };
    with(|e| {
        *e = Editor {
            image,
            ..Editor::default()
        }
    });
    true
}

/// Starts a shape with tool `tool` (an index into the toolbar order) at (`x`, `y`).
#[unsafe(no_mangle)]
pub extern "C" fn begin(tool: u32, x: f32, y: f32, color: u32, size: f32, fill: bool) {
    let Some(&tool) = TOOLS.get(tool as usize) else {
        return;
    };
    with(|e| {
        let mut shape = Shape::start(tool, Point::from_xy(x, y), color, size);
        shape.fill = fill;
        if let Kind::Number(_, n) = &mut shape.kind {
            *n = e.canvas.next_number();
        }
        e.live = Some(shape);
    });
}

/// Extends the live shape to (`x`, `y`); `straight` snaps angles and squares.
#[unsafe(no_mangle)]
pub extern "C" fn drag(x: f32, y: f32, straight: bool) {
    with(|e| {
        if let Some(shape) = &mut e.live {
            shape.drag_to(Point::from_xy(x, y), straight);
        }
    });
}

/// Replaces the text of the live text shape with the UTF-8 in `ptr`, which is freed.
///
/// # Safety
/// `ptr` must come from `alloc(len)` and hold `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn set_text(ptr: *mut u8, len: usize) {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    with(|e| {
        if let Some(Shape {
            kind: Kind::Text(_, t),
            ..
        }) = &mut e.live
        {
            *t = text;
        }
    });
}

/// Puts the live shape on the canvas, or drops it if it would leave no mark.
/// Returns whether it was added.
#[unsafe(no_mangle)]
pub extern "C" fn commit() -> bool {
    with(|e| {
        let Some(shape) = e.live.take() else {
            return false;
        };
        let visible = shape.is_visible();
        e.canvas.push(shape);
        visible
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn cancel() {
    with(|e| e.live = None);
}

/// The topmost shape at (`x`, `y`), or -1.
#[unsafe(no_mangle)]
pub extern "C" fn hit(x: f32, y: f32) -> i32 {
    with(|e| e.canvas.hit(Point::from_xy(x, y)).map_or(-1, |i| i as i32))
}

/// Starts moving shape `i` and returns its new index.
#[unsafe(no_mangle)]
pub extern "C" fn begin_move(i: u32) -> u32 {
    with(|e| e.canvas.begin_move(i as usize) as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn translate(i: u32, dx: f32, dy: f32) {
    with(|e| e.canvas.translate(i as usize, dx, dy));
}

#[unsafe(no_mangle)]
pub extern "C" fn remove(i: u32) {
    with(|e| e.canvas.remove(i as usize));
}

#[unsafe(no_mangle)]
pub extern "C" fn undo() -> bool {
    with(|e| e.canvas.undo())
}

#[unsafe(no_mangle)]
pub extern "C" fn redo() -> bool {
    with(|e| e.canvas.redo())
}

#[unsafe(no_mangle)]
pub extern "C" fn count() -> u32 {
    with(|e| e.canvas.shapes().len() as u32)
}

/// Writes the bounds of shape `i`, or of the live shape when `i` is -1, as
/// left, top, right, bottom to the four floats at `rect_ptr()`. Returns false
/// when there is no such shape.
#[unsafe(no_mangle)]
pub extern "C" fn bounds(i: i32) -> bool {
    with(|e| {
        let shape = if i < 0 {
            e.live.as_ref()
        } else {
            e.canvas.shapes().get(i as usize)
        };
        let Some(b) = shape.and_then(Shape::bounds) else {
            return false;
        };
        e.rect = [b.left(), b.top(), b.right(), b.bottom()];
        true
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn rect_ptr() -> *const f32 {
    with(|e| e.rect.as_ptr())
}

/// Renders the image with every shape, the live one on top, cropped to the
/// `width` × `height` area at (`x`, `y`). Returns a pointer to the RGBA pixels,
/// valid until the next call.
#[unsafe(no_mangle)]
pub extern "C" fn render(x: u32, y: u32, width: u32, height: u32) -> *const u8 {
    with(|e| {
        let (w, h) = e.image.dimensions();
        let x = x.min(w);
        let y = y.min(h);
        let width = width.min(w - x);
        let height = height.min(h - y);
        let mut crop = image::imageops::crop_imm(&e.image, x, y, width, height).to_image();
        let in_view = |s: &&Shape| {
            s.bounds().is_some_and(|b| {
                SkRect::from_xywh(x as f32, y as f32, width as f32, height as f32)
                    .is_some_and(|view| b.intersect(&view).is_some())
            })
        };
        let shapes: Vec<Shape> = e
            .canvas
            .shapes()
            .iter()
            .chain(e.live.iter())
            .filter(in_view)
            .cloned()
            .collect();
        annotate::flatten(&mut crop, &shapes, x as f32, y as f32, 1.0);
        e.pixels = crop.into_raw();
        e.pixels.as_ptr()
    })
}

/// Writes where the caret of the live text shape goes, as x, y and height, to
/// the floats at `rect_ptr()`. Returns false when no text is being typed.
#[unsafe(no_mangle)]
pub extern "C" fn caret() -> bool {
    with(|e| {
        let Some(Shape {
            kind: Kind::Text(at, text),
            size,
            ..
        }) = &e.live
        else {
            return false;
        };
        let line = font::line_height(*size);
        let lines = text.split('\n').count().max(1) as f32;
        e.rect = [
            at.x + font::last_line_width(text, *size),
            at.y + (lines - 1.0) * line,
            line,
            0.0,
        ];
        true
    })
}
