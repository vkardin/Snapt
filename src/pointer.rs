//! Moves the mouse pointer and turns the mouse wheel, so a scrolling
//! screenshot can scroll the page by itself.
//!
//! Positions are in the units the capture API uses for monitors: physical
//! pixels on Windows and X11, points on macOS. Wayland lets no app move the
//! pointer or fake input, so there the user scrolls by hand.

/// The system pointer, while Snapt drives it.
pub struct Pointer {
    #[cfg(target_os = "linux")]
    x11: x11::X11,
}

impl Pointer {
    /// `None` when this system can't fake mouse input.
    pub fn new() -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            if crate::platform::is_wayland() {
                return None;
            }
            match x11::X11::connect() {
                Ok(x11) => Some(Self { x11 }),
                Err(err) => {
                    log::warn!("can't scroll by itself: {err:#}");
                    None
                }
            }
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            Some(Self {})
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            None
        }
    }

    /// Where the pointer is now.
    pub fn position(&self) -> Option<(f64, f64)> {
        #[cfg(target_os = "linux")]
        return self.x11.position().ok();
        #[cfg(target_os = "windows")]
        return windows::position();
        #[cfg(target_os = "macos")]
        return macos::position();
        #[allow(unreachable_code)]
        None
    }

    pub fn move_to(&self, x: f64, y: f64) -> bool {
        #[cfg(target_os = "linux")]
        return self.x11.move_to(x, y).is_ok();
        #[cfg(target_os = "windows")]
        return windows::move_to(x, y);
        #[cfg(target_os = "macos")]
        return macos::move_to(x, y);
        #[allow(unreachable_code)]
        {
            let _ = (x, y);
            false
        }
    }

    /// Turns the wheel under the pointer by `notches`: positive scrolls
    /// towards the end of the page, negative towards its start.
    pub fn wheel(&self, notches: i32) -> bool {
        #[cfg(target_os = "linux")]
        return self.x11.wheel(notches).is_ok();
        #[cfg(target_os = "windows")]
        return windows::wheel(notches);
        #[cfg(target_os = "macos")]
        return macos::wheel(notches);
        #[allow(unreachable_code)]
        {
            let _ = notches;
            false
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use windows::Win32::{
        Foundation::POINT,
        UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
            },
            WindowsAndMessaging::{GetCursorPos, SetCursorPos},
        },
    };

    /// What one notch of a mouse wheel turns.
    const WHEEL_DELTA: i32 = 120;

    pub fn position() -> Option<(f64, f64)> {
        let mut p = POINT::default();
        // SAFETY: `p` is a valid place for the position.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x as f64, p.y as f64))
    }

    pub fn move_to(x: f64, y: f64) -> bool {
        // SAFETY: plain call with numbers.
        unsafe { SetCursorPos(x.round() as i32, y.round() as i32) }.is_ok()
    }

    pub fn wheel(notches: i32) -> bool {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    // Positive turns the wheel away from the user, which scrolls up.
                    mouseData: (-notches * WHEEL_DELTA) as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    ..Default::default()
                },
            },
        };
        // SAFETY: one fully initialized input, with its size.
        unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 1 }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    const LINE_UNITS: u32 = 1;
    const HID_EVENT_TAP: u32 = 0;
    /// Lines per notch, close to what a mouse wheel scrolls.
    const LINES: i32 = 3;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGEventCreateScrollWheelEvent2(
            source: *const c_void,
            units: u32,
            wheel_count: u32,
            wheel1: i32,
            wheel2: i32,
            wheel3: i32,
        ) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub fn position() -> Option<(f64, f64)> {
        // SAFETY: an event made only to read where the pointer is, released after.
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

    pub fn move_to(x: f64, y: f64) -> bool {
        // SAFETY: plain call with numbers.
        unsafe { CGWarpMouseCursorPosition(CGPoint { x, y }) == 0 }
    }

    /// Without the Accessibility permission macOS drops the event silently;
    /// the screenshot then notices the page never moves.
    pub fn wheel(notches: i32) -> bool {
        // SAFETY: the event is posted and released; a null one is not used.
        unsafe {
            // Negative scrolls down.
            let event = CGEventCreateScrollWheelEvent2(
                std::ptr::null(),
                LINE_UNITS,
                1,
                -notches * LINES,
                0,
                0,
            );
            if event.is_null() {
                return false;
            }
            CGEventPost(HID_EVENT_TAP, event);
            CFRelease(event);
        }
        true
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use anyhow::{Context, Result};
    use x11rb::{
        CURRENT_TIME, NONE,
        connection::Connection,
        protocol::{
            xproto::{BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, Window},
            xtest::ConnectionExt as _,
        },
        rust_connection::RustConnection,
    };

    pub struct X11 {
        conn: RustConnection,
        root: Window,
    }

    impl X11 {
        pub fn connect() -> Result<Self> {
            let (conn, screen) = x11rb::connect(None).context("no X server")?;
            let root = conn.setup().roots[screen].root;
            conn.xtest_get_version(2, 2)
                .context("no XTEST extension")?
                .reply()
                .context("no XTEST extension")?;
            Ok(Self { conn, root })
        }

        pub fn position(&self) -> Result<(f64, f64)> {
            let reply = self.conn.query_pointer(self.root)?.reply()?;
            Ok((reply.root_x as f64, reply.root_y as f64))
        }

        pub fn move_to(&self, x: f64, y: f64) -> Result<()> {
            self.conn.warp_pointer(
                NONE,
                self.root,
                0,
                0,
                0,
                0,
                x.round() as i16,
                y.round() as i16,
            )?;
            self.conn.flush()?;
            Ok(())
        }

        pub fn wheel(&self, notches: i32) -> Result<()> {
            // Buttons 4 and 5 are the wheel up and down.
            let button = if notches > 0 { 5 } else { 4 };
            for _ in 0..notches.unsigned_abs() {
                for kind in [BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT] {
                    self.conn
                        .xtest_fake_input(kind, button, CURRENT_TIME, self.root, 0, 0, 0)?;
                }
            }
            self.conn.flush()?;
            Ok(())
        }
    }
}
