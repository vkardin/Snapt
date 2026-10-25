//! Scrolling screenshots (experimental).
//!
//! The user scrolls the page by hand while Snapt captures the selected area
//! a few times a second and glues the new rows of each frame onto the image.
//! A small panel next to the area shows the height so far: Enter or a click
//! finishes, Esc cancels.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    num::NonZeroU32,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context, Result};
use image::{RgbaImage, imageops};
use softbuffer::Surface;
use tiny_skia::{Pixmap, Rect as SkRect};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy, OwnedDisplayHandle},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId, WindowLevel},
};

use crate::{
    app::UserEvent,
    font,
    i18n::{T, t},
    paint::fill_round,
};

/// The tallest image a scrolling screenshot grows to.
const MAX_HEIGHT: u32 = 30_000;
const INTERVAL: Duration = Duration::from_millis(150);
const PANEL: (f64, f64) = (300.0, 56.0);

/// The area to capture: a monitor as xcap reports it, and a rectangle in its image pixels.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub monitor: (i32, i32),
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// What the user decided in the panel.
pub enum ScrollAction {
    Done,
    Cancel,
}

/// A scrolling capture in progress: the worker thread and its panel.
pub struct Scroll {
    // Declared before the context so the surface drops first.
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    stop: Arc<AtomicBool>,
    height: Arc<AtomicU32>,
    worker: Option<JoinHandle<Option<RgbaImage>>>,
}

impl Scroll {
    /// Starts capturing `region`; the panel goes at `panel` (physical pixels),
    /// outside the area so it is never captured.
    pub fn start(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<UserEvent>,
        region: Region,
        panel: PhysicalPosition<i32>,
    ) -> Result<Self> {
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;
        let attributes = Window::default_attributes()
            .with_title("Snapt")
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(panel)
            .with_inner_size(LogicalSize::new(PANEL.0, PANEL.1));
        #[cfg(target_os = "windows")]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes.with_skip_taskbar(true)
        };
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the scrolling panel")?,
        );
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("failed to create a drawing surface: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let height = Arc::new(AtomicU32::new(region.h));
        let worker = {
            let (stop, height) = (stop.clone(), height.clone());
            std::thread::spawn(move || capture_loop(region, &stop, &height, &proxy))
        };
        let mut scroll = Self {
            surface,
            window,
            _context: context,
            stop,
            height,
            worker: Some(worker),
        };
        scroll.redraw();
        Ok(scroll)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn window_event(&mut self, event: WindowEvent) -> Option<ScrollAction> {
        match event {
            WindowEvent::CloseRequested => return Some(ScrollAction::Cancel),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => return Some(ScrollAction::Done),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                return match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => Some(ScrollAction::Cancel),
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                        Some(ScrollAction::Done)
                    }
                    _ => None,
                };
            }
            _ => {}
        }
        None
    }

    /// The worker grew the image; shows the new height.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Stops capturing and returns the glued image.
    pub fn finish(mut self) -> Option<RgbaImage> {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take()?.join().ok().flatten()
    }

    fn redraw(&mut self) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if self.surface.resize(w, h).is_err() {
            return;
        }
        let Some(mut pixmap) = Pixmap::new(size.width, size.height) else {
            return;
        };
        let s = self.window.scale_factor() as f32;
        fill_round(
            &mut pixmap,
            SkRect::from_xywh(0.0, 0.0, size.width as f32, size.height as f32),
            0.0,
            0x1C1C21,
            1.0,
        );
        let height = self.height.load(Ordering::Relaxed);
        font::draw(
            &mut pixmap,
            t(T::ScrollHint),
            12.0 * s,
            9.0 * s,
            13.0 * s,
            0xF4F4F6,
            1.0,
        );
        font::draw(
            &mut pixmap,
            &t(T::ScrollHeight).replace("{}", &height.to_string()),
            12.0 * s,
            30.0 * s,
            12.0 * s,
            0x9A9AA3,
            1.0,
        );
        let Ok(mut buffer) = self.surface.buffer_mut() else {
            return;
        };
        for (dst, src) in buffer.iter_mut().zip(pixmap.data().chunks_exact(4)) {
            *dst = (src[0] as u32) << 16 | (src[1] as u32) << 8 | src[2] as u32;
        }
        let _ = buffer.present();
    }
}

impl Drop for Scroll {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Captures `region` until `stop` is set, gluing each frame onto the result.
fn capture_loop(
    region: Region,
    stop: &AtomicBool,
    height: &AtomicU32,
    proxy: &EventLoopProxy<UserEvent>,
) -> Option<RgbaImage> {
    let monitor = match find_monitor(region.monitor) {
        Ok(monitor) => monitor,
        Err(err) => {
            log::error!("scrolling screenshot failed: {err:#}");
            return None;
        }
    };
    let mut stitcher: Option<Stitcher> = None;
    // Sleeping first also gives the overlay time to disappear.
    loop {
        std::thread::sleep(INTERVAL);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match monitor.capture_image() {
            Ok(image) => {
                let frame =
                    imageops::crop_imm(&image, region.x, region.y, region.w, region.h).to_image();
                let grown = match &mut stitcher {
                    Some(s) => s.push(&frame),
                    None => {
                        stitcher = Some(Stitcher::new(frame));
                        true
                    }
                };
                if let Some(s) = &stitcher
                    && grown
                {
                    height.store(s.height(), Ordering::Relaxed);
                    let _ = proxy.send_event(UserEvent::ScrollGrew);
                    if s.height() >= MAX_HEIGHT {
                        break;
                    }
                }
            }
            Err(err) => log::warn!("scrolling frame skipped: {err}"),
        }
    }
    stitcher.map(Stitcher::finish)
}

fn find_monitor((x, y): (i32, i32)) -> Result<xcap::Monitor> {
    xcap::Monitor::all()
        .context("failed to list monitors")?
        .into_iter()
        .find(|m| m.x().ok() == Some(x) && m.y().ok() == Some(y))
        .context("the monitor is gone")
}

/// Glues frames of a page scrolled downwards into one tall image.
pub struct Stitcher {
    image: RgbaImage,
    last: Vec<u64>,
}

impl Stitcher {
    pub fn new(first: RgbaImage) -> Self {
        Self {
            last: row_hashes(&first),
            image: first,
        }
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    /// Adds the rows of `frame` that scrolled into view. Returns whether the
    /// image grew; a frame that can't be matched to the last one is skipped.
    pub fn push(&mut self, frame: &RgbaImage) -> bool {
        if frame.width() != self.image.width() {
            return false;
        }
        let rows = row_hashes(frame);
        if rows == self.last {
            return false;
        }
        // Sticky headers and footers stay put while the page moves under them.
        let h = rows.len().min(self.last.len());
        let top = static_rows(self.last[..h].iter().zip(&rows[..h]));
        let bottom = static_rows(
            self.last[top..h]
                .iter()
                .rev()
                .zip(rows[top..h].iter().rev()),
        );
        let (last, next) = (&self.last[top..h - bottom], &rows[top..h - bottom]);
        let Some(shift) = find_shift(last, next) else {
            return false;
        };
        // The new rows go above the footer, which stays at the bottom of the image.
        let (h, bottom) = (h as u32, bottom as u32);
        let new = imageops::crop_imm(frame, 0, h - bottom - shift, frame.width(), shift).to_image();
        let footer_at = self.image.height() - bottom;
        let footer =
            imageops::crop_imm(&self.image, 0, footer_at, frame.width(), bottom).to_image();
        let mut image = RgbaImage::new(self.image.width(), self.image.height() + shift);
        imageops::replace(&mut image, &self.image, 0, 0);
        imageops::replace(&mut image, &new, 0, footer_at as i64);
        imageops::replace(&mut image, &footer, 0, (footer_at + shift) as i64);
        self.image = image;
        self.last = rows;
        true
    }

    pub fn finish(self) -> RgbaImage {
        self.image
    }
}

/// How many rows in a row, from the start of `pairs`, are the same in both
/// frames. Blank rows at the inner end don't count, as they may be page.
fn static_rows<'a>(pairs: impl Iterator<Item = (&'a u64, &'a u64)>) -> usize {
    let same: Vec<u64> = pairs.take_while(|(a, b)| a == b).map(|(a, _)| *a).collect();
    same.iter().rposition(|&r| r != 0).map_or(0, |i| i + 1)
}

/// One hash per row; rows of a single color hash to 0 so they never anchor a match.
fn row_hashes(image: &RgbaImage) -> Vec<u64> {
    image
        .rows()
        .map(|row| {
            let pixels: Vec<[u8; 4]> = row.map(|p| p.0).collect();
            if pixels.windows(2).all(|w| w[0] == w[1]) {
                return 0;
            }
            let mut hasher = DefaultHasher::new();
            pixels.hash(&mut hasher);
            hasher.finish()
        })
        .collect()
}

/// How many rows the page moved up between `last` and `next`: the smallest
/// shift where nearly all detailed rows line up. `None` when nothing fits.
fn find_shift(last: &[u64], next: &[u64]) -> Option<u32> {
    let h = last.len().min(next.len());
    // At least a quarter of the frame must stay in view to match reliably.
    for shift in 1..=h * 3 / 4 {
        let (mut detailed, mut matched) = (0, 0);
        for (a, b) in last[shift..h].iter().zip(&next[..h - shift]) {
            if *a != 0 || *b != 0 {
                detailed += 1;
                matched += (a == b) as usize;
            }
        }
        if detailed >= 8 && matched * 100 >= detailed * 95 {
            return Some(shift as u32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A "page" whose every row differs, so any window of it is unique.
    fn page(h: u32) -> RgbaImage {
        RgbaImage::from_fn(20, h, |x, y| {
            Rgba([
                (y * 7 % 251) as u8,
                (y / 251) as u8,
                (x * 13 + y) as u8,
                255,
            ])
        })
    }

    fn view(page: &RgbaImage, top: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(page, 0, top, page.width(), h).to_image()
    }

    #[test]
    fn scrolled_frames_glue_back_into_the_page() {
        let full = page(400);
        let mut s = Stitcher::new(view(&full, 0, 100));
        for top in [0, 30, 30, 95, 170, 240, 300] {
            s.push(&view(&full, top, 100));
        }
        let out = s.finish();
        assert_eq!(out.height(), 400);
        assert_eq!(out, full);
    }

    #[test]
    fn sticky_headers_and_footers_appear_once() {
        let full = page(400);
        let bar = |v: u8| RgbaImage::from_fn(20, 10, move |x, _| Rgba([v, x as u8, 0, 255]));
        let (header, footer) = (bar(250), bar(240));
        // A 100 px view: header, 80 px of page, footer.
        let frame = |top: u32| {
            let mut f = RgbaImage::new(20, 100);
            imageops::replace(&mut f, &header, 0, 0);
            imageops::replace(&mut f, &view(&full, top, 80), 0, 10);
            imageops::replace(&mut f, &footer, 0, 90);
            f
        };
        let mut s = Stitcher::new(frame(0));
        for top in [25, 50, 100, 150] {
            assert!(s.push(&frame(top)), "frame at {top}");
        }
        let out = s.finish();
        assert_eq!(out.height(), 10 + 230 + 10);
        assert_eq!(view(&out, 10, 230), view(&full, 0, 230));
        assert_eq!(view(&out, 240, 10), footer);
    }

    #[test]
    fn unrelated_frames_are_skipped() {
        let mut s = Stitcher::new(view(&page(200), 0, 100));
        let other = RgbaImage::from_fn(20, 100, |x, y| Rgba([x as u8, 200, y as u8 ^ 0x55, 255]));
        assert!(!s.push(&other));
        assert_eq!(s.height(), 100);
    }

    #[test]
    fn blank_rows_do_not_fake_a_match() {
        let blank = vec![0u64; 100];
        assert_eq!(find_shift(&blank, &blank), None);
    }
}
