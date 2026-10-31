//! Scrolling screenshots, and the panel that also runs recordings.
//!
//! Snapt scrolls the page itself by turning the mouse wheel over the
//! selected area, and glues the new rows of each frame onto the image until
//! the page stops moving. Moving the mouse hands the scrolling over to the
//! user, as does a system where Snapt can't fake the wheel (Wayland). A
//! small panel next to the area says what is going on and shows the height
//! so far (or, when recording, the time): Enter or a click finishes, Esc
//! cancels.

use std::{
    num::NonZeroU32,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
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
    pointer::Pointer,
};

/// The tallest image a scrolling screenshot grows to.
const MAX_HEIGHT: u32 = 30_000;
/// How often frames are taken while the user scrolls by hand.
const INTERVAL: Duration = Duration::from_millis(150);
/// How often frames are taken while waiting for the page to stop moving.
const SETTLE: Duration = Duration::from_millis(60);
const PANEL: (f64, f64) = (340.0, 56.0);

/// The area to capture: a monitor as xcap reports it, and a rectangle in its image pixels.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub monitor: (i32, i32),
    /// Image pixels per unit of `monitor`: 1, or the display's scale on macOS.
    pub scale: f32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// What the panel runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Scroll,
    Record,
}

/// What a scrolling screenshot is doing, for the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Phase {
    /// Snapt scrolls up to the start of the page.
    Top,
    /// Snapt scrolls down by itself.
    Auto,
    /// The user scrolls.
    Manual,
    /// The last frame doesn't fit the image: the user scrolled too far at once.
    Lost,
    /// Snapt couldn't move the page, so the user scrolls.
    ByHand,
}

impl Phase {
    fn from_u8(v: u8) -> Self {
        [
            Self::Top,
            Self::Auto,
            Self::Manual,
            Self::Lost,
            Self::ByHand,
        ]
        .get(v as usize)
        .copied()
        .unwrap_or(Self::Manual)
    }
}

/// What a finished capture produced.
pub enum Finished {
    Image(RgbaImage),
    Video(crate::record::Video),
}

enum Worker {
    Scroll(JoinHandle<Option<RgbaImage>>),
    Record(JoinHandle<Option<crate::record::Video>>),
}

/// What the user decided in the panel.
pub enum ScrollAction {
    Done,
    Cancel,
}

/// What the worker shares with the panel.
struct Shared {
    stop: AtomicBool,
    /// The height so far, or the seconds recorded.
    height: AtomicU32,
    phase: AtomicU8,
}

impl Shared {
    fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }
}

/// A scrolling capture in progress: the worker thread and its panel.
pub struct Scroll {
    // Declared before the context so the surface drops first.
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    shared: Arc<Shared>,
    mode: Mode,
    worker: Option<Worker>,
}

impl Scroll {
    /// Starts capturing `region`; the panel goes at `panel` (physical pixels),
    /// outside the area so it is never captured.
    pub fn start(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<UserEvent>,
        region: Region,
        panel: PhysicalPosition<i32>,
        mode: Mode,
    ) -> Result<Self> {
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;
        let attributes = Window::default_attributes()
            // Its own title tells the windows apart if one ever shows as a bare title bar.
            .with_title("Snapt panel")
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(panel)
            .with_inner_size(LogicalSize::new(PANEL.0, PANEL.1));
        #[cfg(target_os = "windows")]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            // Without a taskbar button a minimized panel would be a bare title bar.
            attributes
                .with_skip_taskbar(true)
                .with_enabled_buttons(winit::window::WindowButtons::CLOSE)
        };
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the scrolling panel")?,
        );
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("failed to create a drawing surface: {e}"))?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            height: AtomicU32::new(match mode {
                Mode::Scroll => region.h,
                Mode::Record => 0,
            }),
            phase: AtomicU8::new(Phase::Manual as u8),
        });
        let worker = {
            let shared = shared.clone();
            match mode {
                Mode::Scroll => Worker::Scroll(std::thread::spawn(move || {
                    let image = capture_loop(region, &shared, &|| {
                        let _ = proxy.send_event(UserEvent::ScrollGrew);
                    });
                    if image.as_ref().is_some_and(|(_, ended)| *ended) {
                        let _ = proxy.send_event(UserEvent::ScrollEnded);
                    }
                    image.map(|(image, _)| image)
                })),
                Mode::Record => Worker::Record(std::thread::spawn(move || {
                    crate::record::capture_loop(region, &shared.stop, &shared.height, &proxy)
                })),
            }
        };
        let mut scroll = Self {
            surface,
            window,
            _context: context,
            shared,
            mode,
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
            // Without a taskbar button a minimized panel is a bare title bar, so it comes back.
            WindowEvent::Resized(size) if size.width == 0 || size.height == 0 => {
                log::warn!("the recording panel was minimized; restoring it");
                self.window.set_minimized(false);
            }
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

    /// The worker grew the image or changed what it does; shows it.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Stops capturing and returns the glued image or the recording.
    pub fn finish(mut self) -> Option<Finished> {
        self.shared.stop.store(true, Ordering::Relaxed);
        match self.worker.take()? {
            Worker::Scroll(w) => w.join().ok().flatten().map(Finished::Image),
            Worker::Record(w) => w.join().ok().flatten().map(Finished::Video),
        }
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
        let height = self.shared.height.load(Ordering::Relaxed);
        let (hint, status) = match self.mode {
            Mode::Scroll => {
                let phase = Phase::from_u8(self.shared.phase.load(Ordering::Relaxed));
                let (hint, status) = match phase {
                    Phase::Top => (T::ScrollTop, T::ScrollAutoHeight),
                    Phase::Auto => (T::ScrollAuto, T::ScrollAutoHeight),
                    Phase::Manual => (T::ScrollHint, T::ScrollHeight),
                    Phase::Lost => (T::ScrollLost, T::ScrollHeight),
                    Phase::ByHand => (T::ScrollByHand, T::ScrollHeight),
                };
                (hint, t(status).replace("{}", &height.to_string()))
            }
            Mode::Record => (
                T::RecordHint,
                t(T::RecordTime).replace("{}", &format!("{}:{:02}", height / 60, height % 60)),
            ),
        };
        font::draw(
            &mut pixmap,
            t(hint),
            12.0 * s,
            9.0 * s,
            13.0 * s,
            0xF4F4F6,
            1.0,
        );
        font::draw(
            &mut pixmap,
            &status,
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
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// The pointer while Snapt scrolls the page with it.
struct Driver {
    pointer: Pointer,
    /// Where the pointer was before, to put it back.
    home: Option<(f64, f64)>,
    /// Where Snapt put it; once it is elsewhere, the user took over.
    at: (f64, f64),
    /// Wheel notches per step, tuned to scroll about half the area.
    notches: i32,
}

impl Driver {
    /// Puts the pointer over the right part of the area, where it is least
    /// likely to sit on a link or an inner scrolling box.
    fn start(region: &Region) -> Option<Self> {
        let pointer = Pointer::new()?;
        let home = pointer.position();
        let scale = region.scale.max(0.1) as f64;
        let x = region.monitor.0 as f64 + (region.x as f64 + region.w as f64 * 0.85) / scale;
        let y = region.monitor.1 as f64 + (region.y as f64 + region.h as f64 * 0.5) / scale;
        if !pointer.move_to(x, y) {
            return None;
        }
        // Read back, as the system may round it.
        let at = pointer.position().unwrap_or((x, y));
        Some(Self {
            pointer,
            home,
            at,
            notches: 1,
        })
    }

    fn taken_over(&self) -> bool {
        self.pointer
            .position()
            .is_some_and(|(x, y)| (x - self.at.0).abs() > 3.0 || (y - self.at.1).abs() > 3.0)
    }

    /// The page moved `step` rows for the last `notches`; aims for half of `view`.
    fn tune(&mut self, step: isize, view: u32) {
        if step <= 0 {
            return;
        }
        let per_notch = step as f64 / self.notches as f64;
        self.notches = ((view as f64 * 0.5 / per_notch) as i32).clamp(1, 20);
    }

    /// Puts the pointer back where it was, unless the user took it.
    fn finish(self) {
        if !self.taken_over()
            && let Some((x, y)) = self.home
        {
            self.pointer.move_to(x, y);
        }
    }
}

/// Captures `region` until it is stopped or the page ends, gluing each frame
/// onto the result; also says whether the page ended. `changed` runs when
/// there is news for the panel.
fn capture_loop(region: Region, shared: &Shared, changed: &dyn Fn()) -> Option<(RgbaImage, bool)> {
    let monitor = match find_monitor(region.monitor) {
        Ok(monitor) => monitor,
        Err(err) => {
            log::error!("scrolling screenshot failed: {err:#}");
            return None;
        }
    };
    let stopped = || shared.stop.load(Ordering::Relaxed);
    let notify = |phase: Phase| {
        shared.set_phase(phase);
        changed();
    };
    let grab = || match monitor.capture_image() {
        Ok(image) => {
            Some(imageops::crop_imm(&image, region.x, region.y, region.w, region.h).to_image())
        }
        Err(err) => {
            log::warn!("scrolling frame skipped: {err}");
            None
        }
    };
    // A frame once the page stops moving, or the last one when it never does.
    let settle = || {
        let mut last = grab()?;
        for _ in 0..12 {
            std::thread::sleep(SETTLE);
            if stopped() {
                break;
            }
            let Some(next) = grab() else { continue };
            if !differ(&last, &next) {
                return Some(next);
            }
            last = next;
        }
        Some(last)
    };

    // Sleeping first also gives the overlay time to disappear.
    std::thread::sleep(INTERVAL * 2);
    let mut driver = Driver::start(&region);
    if let Some(d) = &driver {
        // A whole page starts at its top.
        notify(Phase::Top);
        let mut last = settle();
        for _ in 0..40 {
            if stopped() || d.taken_over() || !d.pointer.wheel(-10) {
                break;
            }
            std::thread::sleep(SETTLE);
            let next = settle();
            let moved = match (&last, &next) {
                (Some(a), Some(b)) => differ(a, b),
                _ => true,
            };
            last = next;
            if !moved {
                break;
            }
        }
    } else {
        notify(Phase::Manual);
    }

    let mut stitcher = loop {
        if stopped() {
            return None;
        }
        if let Some(first) = settle() {
            break Stitcher::new(first);
        }
    };
    // Steps in a row that didn't move the page, and that didn't fit the image.
    let (mut still, mut lost) = (0, 0);
    let mut by_hand = false;
    let mut ended = false;
    while !stopped() {
        let result = match &mut driver {
            Some(d) if d.taken_over() => {
                log::info!("the user took over the scrolling");
                if let Some(d) = driver.take() {
                    d.finish();
                }
                notify(Phase::Manual);
                continue;
            }
            Some(d) => {
                notify(Phase::Auto);
                if !d.pointer.wheel(d.notches) {
                    driver = None;
                    by_hand = true;
                    notify(Phase::ByHand);
                    continue;
                }
                std::thread::sleep(SETTLE);
                let Some(frame) = settle() else { continue };
                let mut result = stitcher.push(&frame);
                if result == Push::Same {
                    // Slow pages may not have moved yet.
                    std::thread::sleep(INTERVAL * 2);
                    if let Some(frame) = settle() {
                        result = stitcher.push(&frame);
                    }
                }
                match result {
                    Push::Moved(step) => {
                        (still, lost) = (0, 0);
                        d.tune(step, stitcher.view_height());
                    }
                    Push::Same => still += 1,
                    Push::Lost => {
                        // Too far at once: back up and take smaller steps.
                        lost += 1;
                        d.pointer.wheel(-d.notches);
                        d.notches = (d.notches / 2).max(1);
                    }
                }
                if still >= 2 || lost >= 4 {
                    if still >= 2 && stitcher.grew() {
                        // The end of the page.
                        ended = true;
                        break;
                    }
                    // The wheel doesn't move this page, or it can't be glued: the user scrolls.
                    log::info!("can't scroll this page by itself; scrolling by hand");
                    if let Some(d) = driver.take() {
                        d.finish();
                    }
                    by_hand = true;
                    notify(Phase::ByHand);
                    continue;
                }
                result
            }
            None => {
                std::thread::sleep(INTERVAL);
                let Some(frame) = grab() else { continue };
                stitcher.push(&frame)
            }
        };
        let phase = match (&driver, result, by_hand) {
            (Some(_), _, _) => Phase::Auto,
            (None, Push::Lost, _) => Phase::Lost,
            (None, _, true) => Phase::ByHand,
            (None, _, false) => Phase::Manual,
        };
        shared
            .height
            .store(stitcher.height().min(MAX_HEIGHT), Ordering::Relaxed);
        notify(phase);
        if stitcher.height() >= MAX_HEIGHT {
            ended = true;
            break;
        }
    }
    if let Some(d) = driver {
        d.finish();
    }
    Some((stitcher.finish(), ended))
}

pub fn find_monitor((x, y): (i32, i32)) -> Result<xcap::Monitor> {
    xcap::Monitor::all()
        .context("failed to list monitors")?
        .into_iter()
        .find(|m| m.x().ok() == Some(x) && m.y().ok() == Some(y))
        .context("the monitor is gone")
}

/// Luma samples taken across each row to compare frames.
const SAMPLES: usize = 64;
/// The luma range that makes a row worth matching; plainer rows match anything.
const DETAIL: u8 = 12;
/// The mean luma difference of a row that didn't change.
const ROW_SAME: u32 = 2;
/// The mean luma difference of rows that match: text redrawn at a fraction
/// of a pixel stays well below it, a line of text one row off goes above.
const ROW_MATCH: u32 = 16;
/// The share of rows that may not match in a good match: fixed bars,
/// floating buttons, animations.
const GOOD: f32 = 0.25;
/// The most a match that clearly beats every other place may miss.
const FAIR: f32 = 0.45;

/// Whether two frames differ by more than a blinking caret or a hover effect.
fn differ(a: &RgbaImage, b: &RgbaImage) -> bool {
    if a.dimensions() != b.dimensions() {
        return true;
    }
    let xs = sample_columns(a.width());
    let (a, b) = (Sig::new(a, &xs), Sig::new(b, &xs));
    let changed = (0..a.rows())
        .filter(|&y| row_diff(a.row(y), b.row(y)) > ROW_SAME)
        .count();
    changed * 100 > a.rows()
}

/// Where the samples of each row are taken.
fn sample_columns(w: u32) -> Vec<u32> {
    let n = SAMPLES.min(w as usize) as u32;
    (0..n).map(|i| (2 * i + 1) * w / (2 * n)).collect()
}

/// The luma samples of an image's rows.
struct Sig {
    n: usize,
    luma: Vec<u8>,
    /// Whether each row has enough detail to match.
    detail: Vec<bool>,
}

impl Sig {
    fn new(image: &RgbaImage, xs: &[u32]) -> Self {
        let mut sig = Self {
            n: xs.len(),
            luma: Vec::with_capacity(xs.len() * image.height() as usize),
            detail: Vec::with_capacity(image.height() as usize),
        };
        sig.push_rows(image, xs, 0..image.height());
        sig
    }

    fn push_rows(&mut self, image: &RgbaImage, xs: &[u32], rows: std::ops::Range<u32>) {
        for y in rows {
            let (mut lo, mut hi) = (u8::MAX, 0);
            for &x in xs {
                let l = luma(image.get_pixel(x, y));
                (lo, hi) = (lo.min(l), hi.max(l));
                self.luma.push(l);
            }
            self.detail.push(hi.saturating_sub(lo) > DETAIL);
        }
    }

    fn rows(&self) -> usize {
        self.detail.len()
    }

    fn row(&self, y: usize) -> &[u8] {
        &self.luma[y * self.n..(y + 1) * self.n]
    }

    /// Keeps only `rows`.
    fn keep(&mut self, rows: std::ops::Range<usize>) {
        self.luma = self.luma[rows.start * self.n..rows.end * self.n].to_vec();
        self.detail = self.detail[rows].to_vec();
    }
}

fn luma(p: &image::Rgba<u8>) -> u8 {
    let [r, g, b, _] = p.0;
    ((77 * r as u32 + 150 * g as u32 + 29 * b as u32 + 128) >> 8) as u8
}

/// The mean difference of two rows of samples.
fn row_diff(a: &[u8], b: &[u8]) -> u32 {
    let sum: u32 = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b) as u32).sum();
    sum / a.len().max(1) as u32
}

/// What a frame did to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// The page moved this many rows (up the page when negative).
    Moved(isize),
    /// The page didn't move.
    Same,
    /// The frame doesn't fit the image anywhere.
    Lost,
}

/// Glues frames of a scrolling page into one tall image.
///
/// Each frame is matched against the whole image glued so far, not just the
/// last frame, so scrolling back up or a jump the next frame makes up for
/// does no harm. Matching allows small differences, as browsers redraw text
/// at fractions of a pixel, and leaves out the columns of fixed side panels.
pub struct Stitcher {
    xs: Vec<u32>,
    frame_h: u32,
    /// The rows at the top and bottom that stay put while the page moves,
    /// such as a browser's toolbar; known once the page first moves.
    bars: Option<(u32, u32)>,
    header: RgbaImage,
    footer: RgbaImage,
    /// The page between the bars.
    body: RgbaImage,
    sig: Sig,
    /// The sample columns used for matching.
    mask: Vec<bool>,
    /// The body row at the top of the last frame.
    pos: usize,
    /// How far the last frame moved.
    step: isize,
}

impl Stitcher {
    pub fn new(first: RgbaImage) -> Self {
        let xs = sample_columns(first.width());
        let sig = Sig::new(&first, &xs);
        Self {
            mask: vec![true; xs.len()],
            xs,
            frame_h: first.height(),
            bars: None,
            header: RgbaImage::new(first.width(), 0),
            footer: RgbaImage::new(first.width(), 0),
            body: first,
            sig,
            pos: 0,
            step: 0,
        }
    }

    pub fn height(&self) -> u32 {
        self.header.height() + self.body.height() + self.footer.height()
    }

    /// Whether anything was added to the first frame.
    pub fn grew(&self) -> bool {
        self.height() > self.frame_h
    }

    /// The rows of a frame that scroll.
    pub fn view_height(&self) -> u32 {
        let (top, bottom) = self.bars.unwrap_or((0, 0));
        self.frame_h - top - bottom
    }

    /// Adds the rows of `frame` that scrolled into view.
    pub fn push(&mut self, frame: &RgbaImage) -> Push {
        if frame.dimensions() != (self.body.width(), self.frame_h) {
            return Push::Lost;
        }
        let full = Sig::new(frame, &self.xs);
        if self.bars.is_none() {
            // The body is still the first frame.
            let changed = (0..full.rows())
                .filter(|&y| row_diff(full.row(y), self.sig.row(y)) > ROW_SAME)
                .count();
            if changed * 50 < full.rows() {
                return Push::Same;
            }
            self.find_panels(&full, 0..full.rows());
            let Some(shift) = self.locate(&full, 0, full.rows()) else {
                return Push::Lost;
            };
            self.find_bars(&full, shift);
        }
        let (top, bottom) = self.bars.unwrap_or((0, 0));
        let view = (self.frame_h - top - bottom) as usize;
        let Some(p) = self.locate(&full, top as usize, view) else {
            return Push::Lost;
        };
        let body_h = self.sig.rows();
        if p + view > body_h {
            // Rows below the image so far: add them.
            let from = top + (body_h - p) as u32;
            let to = top + view as u32;
            let w = self.body.width();
            let rows = imageops::crop_imm(frame, 0, from, w, to - from).to_image();
            let mut raw = std::mem::take(&mut self.body).into_raw();
            raw.extend_from_slice(rows.as_raw());
            let h = (raw.len() / (w as usize * 4)) as u32;
            self.body = RgbaImage::from_raw(w, h, raw).expect("whole rows");
            for y in from..to {
                self.sig.luma.extend_from_slice(full.row(y as usize));
                self.sig.detail.push(full.detail[y as usize]);
            }
        }
        self.step = p as isize - self.pos as isize;
        self.pos = p;
        match self.step {
            0 => Push::Same,
            step => Push::Moved(step),
        }
    }

    /// Finds the bars and the fixed side panels from the first frame that
    /// moved, which moved `shift` rows.
    fn find_bars(&mut self, full: &Sig, shift: usize) {
        let h = full.rows();
        // A row stays put when it looks more like the same row before than
        // like the row it would have scrolled from. Rows that scrolled in have
        // no such row, so they must look nearly the same.
        let still = |y: usize| {
            let same = row_diff(full.row(y), self.sig.row(y));
            match y + shift < h {
                true => same <= row_diff(full.row(y), self.sig.row(y + shift)),
                false => same <= ROW_SAME,
            }
        };
        // Every pixel counts here: a scroll bar's arrow is narrower than the samples are apart.
        let detail: Vec<bool> = self
            .body
            .rows()
            .map(|row| {
                let (lo, hi) = row.fold((u8::MAX, 0), |(lo, hi), p| {
                    let l = luma(p);
                    (lo.min(l), hi.max(l))
                });
                hi.saturating_sub(lo) > DETAIL
            })
            .collect();
        // Plain rows at the inner end may be page, so the bar ends at its last detailed row.
        let bar = |rows: &mut dyn Iterator<Item = usize>| {
            let mut end = 0;
            for (i, y) in rows.enumerate() {
                if !still(y) {
                    break;
                }
                if detail[y] {
                    end = i + 1;
                }
            }
            // Taller than a third of the area, it is more likely a page that barely moved.
            if end > h / 3 { 0 } else { end }
        };
        let top = bar(&mut (0..h));
        let bottom = bar(&mut (top..h).rev());

        let band = top..h - bottom;
        self.find_panels(full, band.clone());

        let w = self.body.width();
        let (top32, bottom32) = (top as u32, bottom as u32);
        self.header = imageops::crop_imm(&self.body, 0, 0, w, top32).to_image();
        self.footer =
            imageops::crop_imm(&self.body, 0, self.frame_h - bottom32, w, bottom32).to_image();
        self.body =
            imageops::crop_imm(&self.body, 0, top32, w, self.frame_h - top32 - bottom32).to_image();
        self.sig.keep(band);
        self.bars = Some((top32, bottom32));
    }

    /// Leaves fixed side panels out of matching: detailed sample columns that
    /// stay put over `rows` while the page moves.
    fn find_panels(&mut self, full: &Sig, rows: std::ops::Range<usize>) {
        let count = rows.len().max(1);
        for (j, used) in self.mask.iter_mut().enumerate() {
            let (mut kept, mut lo, mut hi) = (0, u8::MAX, 0);
            for y in rows.clone() {
                let (a, b) = (self.sig.row(y)[j], full.row(y)[j]);
                kept += (a.abs_diff(b) <= 8) as usize;
                (lo, hi) = (lo.min(a), hi.max(a));
            }
            *used = !(kept * 100 >= count * 97 && hi.saturating_sub(lo) > DETAIL);
        }
        if self.mask.iter().filter(|&&m| m).count() * 4 < self.mask.len() {
            self.mask.fill(true);
        }
    }

    /// The body row where the view of `full` (from row `top`, `view` rows
    /// tall) fits best, if it fits well enough anywhere.
    ///
    /// A place is scored by the share of detailed rows that don't match
    /// there, so a fixed bar or a floating button only costs its own rows.
    fn locate(&self, full: &Sig, top: usize, view: usize) -> Option<usize> {
        let body_h = self.sig.rows();
        // At least a quarter of the view must overlap the image to match reliably.
        let overlap = (view / 4).max(8);
        if body_h < overlap || view < 8 {
            return None;
        }
        let (lo, hi) = (self.pos.saturating_sub(view), body_h - overlap);
        // Tall views are compared on every few rows.
        let stride = (view / 256).max(1);
        let cols: Vec<usize> = (0..self.mask.len()).filter(|&j| self.mask[j]).collect();
        // The share of rows that miss, and the mean difference of those that match.
        let score = |p: usize| -> Option<(f32, f32)> {
            let (mut used, mut missed, mut matched_sum) = (0u32, 0u32, 0u32);
            for i in (0..view.min(body_h - p)).step_by(stride) {
                let (f, b) = (top + i, p + i);
                if !full.detail[f] && !self.sig.detail[b] {
                    continue;
                }
                let (fr, br) = (full.row(f), self.sig.row(b));
                let sum: u32 = cols.iter().map(|&j| fr[j].abs_diff(br[j]) as u32).sum();
                let mean = sum / cols.len() as u32;
                used += 1;
                if mean > ROW_MATCH {
                    missed += 1;
                } else {
                    matched_sum += mean;
                }
            }
            (used >= 4).then(|| {
                let matched = (used - missed).max(1);
                (
                    missed as f32 / used as f32,
                    matched_sum as f32 / matched as f32,
                )
            })
        };
        let scores: Vec<(usize, f32, f32)> = (lo..=hi)
            .filter_map(|p| score(p).map(|(miss, diff)| (p, miss, diff)))
            .collect();
        let best = scores.iter().map(|s| s.1).fold(f32::INFINITY, f32::min);
        let close = |s: &&(usize, f32, f32)| s.1 <= best + 0.02;
        let finest = scores
            .iter()
            .filter(close)
            .map(|s| s.2)
            .fold(f32::INFINITY, f32::min);
        // Of the places about as good as the best, the one the page was heading for.
        let expected = (self.pos as isize + self.step).max(0) as usize;
        let &(p, miss, _) = scores
            .iter()
            .filter(close)
            .filter(|s| s.2 <= finest + 0.5)
            .min_by_key(|s| s.0.abs_diff(expected))?;
        let runner_up = scores
            .iter()
            .filter(|o| o.0.abs_diff(p) > 2)
            .map(|o| o.1)
            .fold(f32::INFINITY, f32::min);
        let clear = miss <= GOOD || (miss <= FAIR && runner_up >= miss + 0.3);
        clear.then_some(p)
    }

    pub fn finish(self) -> RgbaImage {
        let w = self.body.width();
        let mut image = RgbaImage::new(w, self.height());
        imageops::replace(&mut image, &self.header, 0, 0);
        imageops::replace(&mut image, &self.body, 0, self.header.height() as i64);
        imageops::replace(
            &mut image,
            &self.footer,
            0,
            (self.header.height() + self.body.height()) as i64,
        );
        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A "page" of noise, so any window of it is unique.
    fn page(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let v = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663))
                .wrapping_mul(2_654_435_761);
            Rgba([(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, 255])
        })
    }

    fn view(page: &RgbaImage, top: u32, h: u32) -> RgbaImage {
        imageops::crop_imm(page, 0, top, page.width(), h).to_image()
    }

    fn glue(frames: impl IntoIterator<Item = RgbaImage>) -> RgbaImage {
        let mut frames = frames.into_iter();
        let mut s = Stitcher::new(frames.next().unwrap());
        for frame in frames {
            s.push(&frame);
        }
        s.finish()
    }

    #[test]
    fn scrolled_frames_glue_back_into_the_page() {
        let full = page(20, 400);
        let out = glue([0, 0, 30, 30, 95, 170, 240, 300].map(|top| view(&full, top, 100)));
        assert_eq!(out.height(), 400);
        assert_eq!(out, full);
    }

    #[test]
    fn sticky_headers_and_footers_appear_once() {
        let full = page(20, 400);
        let bar = |v: u8| RgbaImage::from_fn(20, 10, move |x, _| Rgba([v, (x * 12) as u8, 0, 255]));
        let (header, footer) = (bar(250), bar(40));
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
            assert!(
                matches!(s.push(&frame(top)), Push::Moved(_)),
                "frame at {top}"
            );
        }
        let out = s.finish();
        assert_eq!(out.height(), 10 + 230 + 10);
        assert_eq!(view(&out, 0, 10), header);
        assert_eq!(view(&out, 10, 230), view(&full, 0, 230));
        assert_eq!(view(&out, 240, 10), footer);
    }

    #[test]
    fn small_differences_still_glue() {
        // Text redrawn at a fraction of a pixel: every pixel a little off.
        let full = page(40, 400);
        let noisy = |top: u32| {
            let mut f = view(&full, top, 100);
            for (x, y, p) in f.enumerate_pixels_mut() {
                let n = ((x * 31 + y * 17 + top * 7) % 7) as u8;
                p.0[2] = p.0[2].saturating_add(n);
            }
            f
        };
        let out = glue([0, 40, 90, 160, 230, 300].map(noisy));
        assert_eq!(out.height(), 400);
    }

    #[test]
    fn fixed_side_panels_do_not_stop_the_gluing() {
        let full = page(60, 400);
        // A detailed panel on the right that never moves, like a chat button or a sidebar.
        let frame = |top: u32| {
            let mut f = view(&full, top, 100);
            for (x, y, p) in f.enumerate_pixels_mut() {
                if x >= 40 {
                    *p = Rgba([(x * 9) as u8, (y * 3) as u8, 90, 255]);
                }
            }
            f
        };
        let out = glue([0, 35, 80, 150, 220, 280, 300].map(frame));
        assert_eq!(out.height(), 400);
        let left = |image: &RgbaImage| imageops::crop_imm(image, 0, 0, 40, 400).to_image();
        assert_eq!(left(&out), left(&full));
    }

    #[test]
    fn scrolling_back_and_jumping_ahead_recover() {
        let full = page(20, 400);
        let mut s = Stitcher::new(view(&full, 0, 100));
        assert!(matches!(s.push(&view(&full, 60, 100)), Push::Moved(60)));
        // Back up: nothing to add.
        assert!(matches!(s.push(&view(&full, 20, 100)), Push::Moved(-40)));
        assert_eq!(s.height(), 160);
        assert!(matches!(s.push(&view(&full, 90, 100)), Push::Moved(70)));
        assert!(matches!(s.push(&view(&full, 150, 100)), Push::Moved(60)));
        // Too far at once: no overlap with what is glued.
        assert_eq!(s.push(&view(&full, 300, 100)), Push::Lost);
        assert_eq!(s.height(), 250);
        // Scrolling back a little picks it up again.
        assert!(matches!(s.push(&view(&full, 220, 100)), Push::Moved(70)));
        s.push(&view(&full, 280, 100));
        s.push(&view(&full, 300, 100));
        assert_eq!(s.finish(), full);
    }

    #[test]
    fn unrelated_frames_are_skipped() {
        let mut s = Stitcher::new(view(&page(20, 200), 0, 100));
        let other = RgbaImage::from_fn(20, 100, |x, y| Rgba([x as u8, 200, y as u8 ^ 0x55, 255]));
        assert_eq!(s.push(&other), Push::Lost);
        assert_eq!(s.height(), 100);
    }

    #[test]
    fn blank_frames_do_not_fake_a_match() {
        let blank = RgbaImage::from_pixel(20, 100, Rgba([255, 255, 255, 255]));
        let mut s = Stitcher::new(blank.clone());
        assert_eq!(s.push(&blank), Push::Same);
        let gray = RgbaImage::from_pixel(20, 100, Rgba([128, 128, 128, 255]));
        assert_eq!(s.push(&gray), Push::Lost);
        assert!(!s.grew());
    }

    // Needs an X screen showing a long page, e.g. a browser in Xvfb:
    // `DISPLAY=:99 cargo test -- --ignored scrolls_a_real_page`.
    #[test]
    #[ignore]
    fn scrolls_a_real_page() {
        let monitor = xcap::Monitor::all().unwrap().remove(0);
        let (w, h) = (monitor.width().unwrap(), monitor.height().unwrap());
        let region = Region {
            monitor: (monitor.x().unwrap(), monitor.y().unwrap()),
            scale: 1.0,
            x: 0,
            y: 0,
            w,
            h,
        };
        let shared = Shared {
            stop: AtomicBool::new(false),
            height: AtomicU32::new(0),
            phase: AtomicU8::new(0),
        };
        let (image, ended) = capture_loop(region, &shared, &|| {}).unwrap();
        let path = std::env::temp_dir().join("snapt-scrolled.png");
        image.save(&path).unwrap();
        eprintln!(
            "{}x{} saved to {}",
            image.width(),
            image.height(),
            path.display()
        );
        assert!(ended);
        assert!(image.height() > h * 2);
    }

    #[test]
    fn a_hover_effect_is_not_a_scroll() {
        let full = page(20, 200);
        let mut s = Stitcher::new(view(&full, 0, 100));
        let mut hovered = view(&full, 0, 100);
        hovered.put_pixel(5, 50, Rgba([0, 0, 0, 255]));
        assert_eq!(s.push(&hovered), Push::Same);
        assert!(differ(&view(&full, 0, 100), &view(&full, 10, 100)));
        assert!(!differ(&view(&full, 0, 100), &hovered));
    }
}
