//! The mouse pointer on screenshots.
//!
//! Screen capture APIs leave the pointer out, so when the user wants it on
//! the shot, Snapt asks the system where it is and draws a standard arrow
//! there itself.

use image::RgbaImage;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, PixmapMut, Stroke, Transform};

/// Where the pointer is, in the units of `capture::Shot::x`: physical pixels
/// on Windows and X11, points on macOS. `None` where the system won't say,
/// as on Wayland.
pub fn position() -> Option<(f64, f64)> {
    platform::position()
}

/// Draws the arrow with its tip at (`x`, `y`) in image pixels, `scale` times
/// its 1× size of about 12 × 19 pixels.
pub fn draw(image: &mut RgbaImage, x: f32, y: f32, scale: f32) {
    let (w, h) = image.dimensions();
    // A screenshot is opaque, so its straight RGBA is already premultiplied.
    let Some(mut pixmap) = PixmapMut::from_bytes(image.as_mut(), w, h) else {
        return;
    };
    let mut p = PathBuilder::new();
    p.move_to(0.0, 0.0);
    p.line_to(0.0, 16.5);
    p.line_to(4.0, 12.8);
    p.line_to(6.9, 19.2);
    p.line_to(9.6, 18.0);
    p.line_to(6.8, 11.8);
    p.line_to(12.0, 11.8);
    p.close();
    let Some(path) = p.finish() else { return };
    let transform = Transform::from_scale(scale, scale).post_translate(x, y);
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(255, 255, 255, 255);
    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
    paint.set_color_rgba8(0, 0, 0, 255);
    let stroke = Stroke {
        width: 1.1,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

    pub fn position() -> Option<(f64, f64)> {
        let mut p = POINT::default();
        // SAFETY: writes into the point it is given.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x as f64, p.y as f64))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(object: *const c_void);
    }

    pub fn position() -> Option<(f64, f64)> {
        // SAFETY: an empty event carries the pointer position in global
        // top-left points; it is released right after.
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let p = CGEventGetLocation(event);
            CFRelease(event);
            Some((p.x, p.y))
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn position() -> Option<(f64, f64)> {
        // XWayland only knows the pointer while it is over X windows.
        if crate::platform::is_wayland() {
            return None;
        }
        let (conn, screen) = xcb::Connection::connect(None).ok()?;
        let root = conn.get_setup().roots().nth(screen as usize)?.root();
        let cookie = conn.send_request(&xcb::x::QueryPointer { window: root });
        let reply = conn.wait_for_reply(cookie).ok()?;
        Some((reply.root_x() as f64, reply.root_y() as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_is_white_with_a_dark_edge() {
        let mut image = RgbaImage::from_pixel(80, 80, image::Rgba([0, 128, 255, 255]));
        draw(&mut image, 10.0, 10.0, 3.0);
        // Inside the arrow.
        assert_eq!(image.get_pixel(19, 34).0, [255, 255, 255, 255]);
        // On its left edge.
        assert_eq!(image.get_pixel(10, 25).0, [0, 0, 0, 255]);
        // Away from it nothing changes.
        assert_eq!(image.get_pixel(70, 12).0, [0, 128, 255, 255]);
    }
}
