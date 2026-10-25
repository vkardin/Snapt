//! The full-screen overlay where the user selects an area.
//!
//! Every monitor gets a borderless window showing its frozen, dimmed screenshot.
//! The selected area shows through at full brightness.

mod render;
mod selection;

use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use image::{RgbaImage, imageops};
use softbuffer::Surface;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    monitor::MonitorHandle,
    window::{CursorIcon, Window, WindowId, WindowLevel},
};

use crate::capture::Shot;
use render::Frame;
use selection::{Handle, Hit, Point, Rect, Selection};

/// What the user decided to do with the selection.
pub enum Outcome {
    Copy(RgbaImage),
    Save(RgbaImage),
    Cancel,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub struct Overlay {
    // Declared before the context so surfaces drop first.
    screens: Vec<Screen>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    modifiers: ModifiersState,
}

struct Screen {
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    image: RgbaImage,
    frame: Frame,
    selection: Selection,
    cursor: Point,
    last_click: Option<(Instant, Point)>,
}

impl Overlay {
    /// Opens one overlay window per monitor, each showing its shot.
    pub fn open(event_loop: &ActiveEventLoop, shots: Vec<Shot>) -> Result<Self> {
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;

        let mut shots: Vec<Option<Shot>> = shots.into_iter().map(Some).collect();
        let mut screens = Vec::new();
        for monitor in event_loop.available_monitors() {
            let Some(shot) = take_matching_shot(&monitor, &mut shots) else {
                log::warn!("no screenshot for monitor {:?}", monitor.name());
                continue;
            };
            match Screen::new(event_loop, &context, &monitor, shot.image) {
                Ok(screen) => screens.push(screen),
                Err(err) => log::error!("failed to open an overlay: {err:#}"),
            }
        }
        ensure!(!screens.is_empty(), "could not open any overlay window");

        // Show every window only after its first frame is drawn, so nothing flickers.
        for screen in &mut screens {
            screen.redraw();
            screen.window.set_visible(true);
        }
        // Keyboard focus goes to the first screen; clicking another one moves it there.
        screens[0].window.focus_window();

        Ok(Self {
            screens,
            _context: context,
            modifiers: ModifiersState::empty(),
        })
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.screens.iter().any(|s| s.window.id() == id)
    }

    /// Handles an event for one of the overlay windows.
    pub fn window_event(&mut self, id: WindowId, event: WindowEvent) -> Option<Outcome> {
        let index = self.screens.iter().position(|s| s.window.id() == id)?;
        match event {
            WindowEvent::CloseRequested => return Some(Outcome::Cancel),
            WindowEvent::RedrawRequested => self.screens[index].redraw(),
            WindowEvent::Resized(size) => self.screens[index].resize(size),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.screens[index].cursor_moved(position);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                return self.mouse_input(index, state, button);
            }
            WindowEvent::KeyboardInput { event, .. } => return self.key(index, event),
            _ => {}
        }
        None
    }

    fn mouse_input(
        &mut self,
        index: usize,
        state: ElementState,
        button: MouseButton,
    ) -> Option<Outcome> {
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                // Only one monitor holds a selection at a time.
                for (i, other) in self.screens.iter_mut().enumerate() {
                    if i != index && other.selection.rect().is_some() {
                        other.selection.clear();
                        other.window.request_redraw();
                    }
                }
                let screen = &mut self.screens[index];
                if screen.is_double_click() {
                    return screen.crop().map(Outcome::Copy);
                }
                screen.selection.press(screen.cursor, screen.grip());
                screen.window.request_redraw();
            }
            (MouseButton::Left, ElementState::Released) => {
                let screen = &mut self.screens[index];
                screen.selection.release(screen.cursor);
                screen.update_cursor_icon();
                screen.window.request_redraw();
            }
            (MouseButton::Right, ElementState::Pressed) => return Some(Outcome::Cancel),
            _ => {}
        }
        None
    }

    fn key(&mut self, index: usize, event: KeyEvent) -> Option<Outcome> {
        if event.state != ElementState::Pressed {
            return None;
        }
        // Physical keys, so shortcuts work with any keyboard layout.
        let PhysicalKey::Code(code) = event.physical_key else {
            return None;
        };
        let command = self.modifiers.control_key() || self.modifiers.super_key();
        let screen = &mut self.screens[index];
        match code {
            KeyCode::Escape => Some(Outcome::Cancel),
            KeyCode::Enter | KeyCode::NumpadEnter => Some(Outcome::Copy(screen.crop_or_all())),
            KeyCode::KeyC if command => Some(Outcome::Copy(screen.crop_or_all())),
            KeyCode::KeyS if command => Some(Outcome::Save(screen.crop_or_all())),
            KeyCode::KeyA if command => {
                screen.selection.select_all();
                screen.window.request_redraw();
                None
            }
            _ => None,
        }
    }
}

impl Screen {
    fn new(
        event_loop: &ActiveEventLoop,
        context: &softbuffer::Context<OwnedDisplayHandle>,
        monitor: &MonitorHandle,
        image: RgbaImage,
    ) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title("Snapt")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(monitor.position())
            .with_inner_size(monitor.size());
        let attributes = platform_attributes(attributes, monitor);
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create a window")?,
        );
        #[cfg(target_os = "macos")]
        {
            // Covers the menu bar without the animated switch to a new Space.
            use winit::platform::macos::WindowExtMacOS;
            window.set_simple_fullscreen(true);
        }
        window.set_cursor(CursorIcon::Crosshair);

        let surface = Surface::new(context, window.clone())
            .map_err(|e| anyhow::anyhow!("failed to create a drawing surface: {e}"))?;
        let size = window.inner_size();
        let mut screen = Self {
            surface,
            frame: Frame::new(&image, size.width.max(1), size.height.max(1)),
            selection: Selection::new(size.width as i32, size.height as i32),
            window,
            image,
            cursor: Point::new(0, 0),
            last_click: None,
        };
        screen.resize(size);
        Ok(screen)
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if let Err(err) = self.surface.resize(w, h) {
            log::error!("failed to resize the overlay: {err}");
            return;
        }
        if (self.frame.width, self.frame.height) != (size.width, size.height) {
            self.frame = Frame::new(&self.image, size.width, size.height);
            self.selection = Selection::new(size.width as i32, size.height as i32);
        }
        self.window.request_redraw();
    }

    fn redraw(&mut self) {
        let rect = self.selection.rect();
        let label = rect
            .map(|r| {
                let (w, h) = self.to_image(r).map_or((0, 0), |(_, _, w, h)| (w, h));
                format!("{w}×{h}")
            })
            .unwrap_or_default();
        let scale = self.window.scale_factor();
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(err) => {
                log::error!("failed to draw the overlay: {err}");
                return;
            }
        };
        if buffer.len() != (self.frame.width * self.frame.height) as usize {
            // The surface is mid-resize; the next Resized event redraws.
            return;
        }
        render::draw(&mut buffer, &self.frame, rect, &label, scale);
        if let Err(err) = buffer.present() {
            log::error!("failed to present the overlay: {err}");
        }
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor = Point::new(position.x as i32, position.y as i32);
        if self.selection.motion(self.cursor) {
            self.window.request_redraw();
        }
        self.update_cursor_icon();
    }

    fn update_cursor_icon(&self) {
        if self.selection.is_dragging() {
            return;
        }
        let icon = match self.selection.hit(self.cursor, self.grip()) {
            Hit::Outside => CursorIcon::Crosshair,
            Hit::Inside => CursorIcon::Move,
            Hit::Handle(Handle::N | Handle::S) => CursorIcon::NsResize,
            Hit::Handle(Handle::E | Handle::W) => CursorIcon::EwResize,
            Hit::Handle(Handle::NW | Handle::SE) => CursorIcon::NwseResize,
            Hit::Handle(Handle::NE | Handle::SW) => CursorIcon::NeswResize,
        };
        self.window.set_cursor(icon);
    }

    /// How close to a grip, in physical pixels, still counts as grabbing it.
    fn grip(&self) -> i32 {
        (6.0 * self.window.scale_factor()).round() as i32
    }

    /// Records a click and reports whether it completes a double click inside the selection.
    fn is_double_click(&mut self) -> bool {
        let now = Instant::now();
        let previous = self.last_click.replace((now, self.cursor));
        let inside = self.selection.hit(self.cursor, 0) == Hit::Inside;
        let double = previous.is_some_and(|(at, p)| {
            now - at < DOUBLE_CLICK
                && (p.x - self.cursor.x).abs() < 4
                && (p.y - self.cursor.y).abs() < 4
        });
        if double {
            self.last_click = None;
        }
        double && inside
    }

    /// Converts a window rectangle into image pixels: (x, y, width, height).
    fn to_image(&self, rect: Rect) -> Option<(u32, u32, u32, u32)> {
        let (iw, ih) = self.image.dimensions();
        let sx = iw as f64 / self.frame.width as f64;
        let sy = ih as f64 / self.frame.height as f64;
        let x = ((rect.x as f64 * sx).round() as u32).min(iw.saturating_sub(1));
        let y = ((rect.y as f64 * sy).round() as u32).min(ih.saturating_sub(1));
        let right = ((rect.right() as f64 * sx).round() as u32).clamp(x + 1, iw);
        let bottom = ((rect.bottom() as f64 * sy).round() as u32).clamp(y + 1, ih);
        (iw > 0 && ih > 0).then_some((x, y, right - x, bottom - y))
    }

    fn crop(&self) -> Option<RgbaImage> {
        let (x, y, w, h) = self.to_image(self.selection.rect()?)?;
        Some(imageops::crop_imm(&self.image, x, y, w, h).to_image())
    }

    /// The selection, or the whole monitor when nothing is selected.
    fn crop_or_all(&self) -> RgbaImage {
        self.crop().unwrap_or_else(|| self.image.clone())
    }
}

#[cfg(target_os = "windows")]
fn platform_attributes(
    attributes: winit::window::WindowAttributes,
    monitor: &MonitorHandle,
) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attributes
        .with_skip_taskbar(true)
        .with_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(
            monitor.clone(),
        ))))
}

#[cfg(target_os = "macos")]
fn platform_attributes(
    attributes: winit::window::WindowAttributes,
    _monitor: &MonitorHandle,
) -> winit::window::WindowAttributes {
    // Native fullscreen would slide into a new Space; simple fullscreen is set after creation.
    attributes
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_attributes(
    attributes: winit::window::WindowAttributes,
    monitor: &MonitorHandle,
) -> winit::window::WindowAttributes {
    attributes.with_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(
        monitor.clone(),
    ))))
}

/// Picks the shot taken of `monitor`, removing it from `shots`.
///
/// Capture APIs report positions in physical pixels on Windows and Linux and
/// in points on macOS, so both readings of the monitor position are tried.
fn take_matching_shot(monitor: &MonitorHandle, shots: &mut [Option<Shot>]) -> Option<Shot> {
    let physical = monitor.position();
    let logical = physical.to_logical::<i32>(monitor.scale_factor());
    let distance = |shot: &Shot| {
        let d = |x: i32, y: i32| (shot.x - x).unsigned_abs() + (shot.y - y).unsigned_abs();
        d(physical.x, physical.y).min(d(logical.x, logical.y))
    };
    let best = shots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (i, distance(s))))
        .min_by_key(|&(_, d)| d)?
        .0;
    shots[best].take()
}
