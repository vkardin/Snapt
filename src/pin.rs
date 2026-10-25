//! Screenshots pinned on top of every window, as in Snipaste.
//!
//! A pin is a borderless, always-on-top window showing one image. Drag it
//! anywhere, zoom it with the wheel, close it with Esc or a double click.
//! Ctrl+C copies the image again and Ctrl+S saves it.

use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use image::{RgbaImage, imageops};
use softbuffer::Surface;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{CursorIcon, Window, WindowId, WindowLevel},
};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const ZOOMS: std::ops::RangeInclusive<f64> = 0.1..=5.0;
/// The thin frame that tells a pin apart from what is really on screen.
const BORDER: u32 = 0x8B5CF6;

/// What the user asked a pin for.
pub enum PinAction {
    Close,
    Copy(RgbaImage),
    Save(RgbaImage),
}

pub struct Pin {
    // Declared before the context so the surface drops first.
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    image: RgbaImage,
    /// The window size at 100 %.
    base: PhysicalSize<u32>,
    zoom: f64,
    /// The image resized to the window, redone only when the size changes.
    scaled: Option<RgbaImage>,
    modifiers: ModifiersState,
    last_click: Option<Instant>,
}

impl Pin {
    /// Pins `image` at `position`, `size` big on screen (both in physical
    /// pixels), so it covers exactly the area it was taken from.
    pub fn open(
        event_loop: &ActiveEventLoop,
        image: RgbaImage,
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    ) -> Result<Self> {
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;
        let base = PhysicalSize::new(size.width.max(1), size.height.max(1));
        let attributes = Window::default_attributes()
            .with_title("Snapt")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(position)
            .with_inner_size(base);
        #[cfg(target_os = "windows")]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes.with_skip_taskbar(true)
        };
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the pin window")?,
        );
        window.set_cursor(CursorIcon::Move);
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("failed to create a drawing surface: {e}"))?;
        let mut pin = Self {
            surface,
            window,
            _context: context,
            image,
            base,
            zoom: 1.0,
            scaled: None,
            modifiers: ModifiersState::empty(),
            last_click: None,
        };
        pin.redraw();
        pin.window.set_visible(true);
        pin.window.focus_window();
        Ok(pin)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Handles an event; returns what the user asked for, if anything.
    pub fn window_event(&mut self, event: WindowEvent) -> Option<PinAction> {
        match event {
            WindowEvent::CloseRequested => return Some(PinAction::Close),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) => self.window.request_redraw(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => return self.press(button),
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let command = self.modifiers.control_key() || self.modifiers.super_key();
                return match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => Some(PinAction::Close),
                    PhysicalKey::Code(KeyCode::KeyW) if command => Some(PinAction::Close),
                    PhysicalKey::Code(KeyCode::KeyC) if command => {
                        Some(PinAction::Copy(self.image.clone()))
                    }
                    PhysicalKey::Code(KeyCode::KeyS) if command => {
                        Some(PinAction::Save(self.image.clone()))
                    }
                    PhysicalKey::Code(KeyCode::Equal | KeyCode::NumpadAdd) => {
                        self.zoom_by(1.0);
                        None
                    }
                    PhysicalKey::Code(KeyCode::Minus | KeyCode::NumpadSubtract) => {
                        self.zoom_by(-1.0);
                        None
                    }
                    PhysicalKey::Code(KeyCode::Digit0 | KeyCode::Numpad0) => {
                        self.set_zoom(1.0);
                        None
                    }
                    _ => None,
                };
            }
            _ => {}
        }
        None
    }

    fn press(&mut self, button: MouseButton) -> Option<PinAction> {
        match button {
            MouseButton::Left => {
                let now = Instant::now();
                if self
                    .last_click
                    .replace(now)
                    .is_some_and(|at| now - at < DOUBLE_CLICK)
                {
                    return Some(PinAction::Close);
                }
                // The system moves the window, which works the same everywhere, Wayland included.
                if let Err(err) = self.window.drag_window() {
                    log::warn!("failed to move the pin: {err}");
                }
            }
            MouseButton::Middle => return Some(PinAction::Close),
            _ => {}
        }
        None
    }

    fn wheel(&mut self, delta: MouseScrollDelta) {
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => y.signum() as f64,
            MouseScrollDelta::PixelDelta(p) => p.y.signum(),
        };
        if steps != 0.0 {
            self.zoom_by(steps);
        }
    }

    /// Zooms in 10 % steps, or 5 % below half size where 10 % is a big jump.
    fn zoom_by(&mut self, steps: f64) {
        let step = if self.zoom < 0.5 { 0.05 } else { 0.1 };
        let zoom = ((self.zoom + steps * step) / step).round() * step;
        self.set_zoom(zoom);
    }

    fn set_zoom(&mut self, zoom: f64) {
        let zoom = zoom.clamp(*ZOOMS.start(), *ZOOMS.end());
        if (zoom - self.zoom).abs() < f64::EPSILON {
            return;
        }
        self.zoom = zoom;
        let size = zoomed(self.base, zoom);
        // When the system resizes right away, the Resized event redraws.
        if let Some(size) = self.window.request_inner_size(size)
            && size == self.window.inner_size()
        {
            self.window.request_redraw();
        }
    }

    fn redraw(&mut self) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if let Err(err) = self.surface.resize(w, h) {
            log::error!("failed to resize the pin: {err}");
            return;
        }
        if self
            .scaled
            .as_ref()
            .is_none_or(|s| s.dimensions() != (size.width, size.height))
        {
            self.scaled = Some(if self.image.dimensions() == (size.width, size.height) {
                self.image.clone()
            } else {
                imageops::resize(
                    &self.image,
                    size.width,
                    size.height,
                    imageops::FilterType::Triangle,
                )
            });
        }
        let Some(scaled) = &self.scaled else { return };
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(err) => {
                log::error!("failed to draw the pin: {err}");
                return;
            }
        };
        if buffer.len() != (size.width * size.height) as usize {
            return;
        }
        for (dst, src) in buffer.iter_mut().zip(scaled.pixels()) {
            let [r, g, b, _] = src.0;
            *dst = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
        draw_border(&mut buffer, size.width as usize, size.height as usize);
        if let Err(err) = buffer.present() {
            log::error!("failed to present the pin: {err}");
        }
    }
}

fn zoomed(base: PhysicalSize<u32>, zoom: f64) -> PhysicalSize<u32> {
    PhysicalSize::new(
        ((base.width as f64 * zoom).round() as u32).max(1),
        ((base.height as f64 * zoom).round() as u32).max(1),
    )
}

/// A one-pixel frame around the edge of a `w` × `h` buffer.
fn draw_border(buf: &mut [u32], w: usize, h: usize) {
    if w == 0 || h == 0 {
        return;
    }
    buf[..w].fill(BORDER);
    buf[(h - 1) * w..].fill(BORDER);
    for row in buf.chunks_exact_mut(w) {
        row[0] = BORDER;
        row[w - 1] = BORDER;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_aspect_ratio_and_never_reaches_zero() {
        let base = PhysicalSize::new(400, 300);
        assert_eq!(zoomed(base, 1.5), PhysicalSize::new(600, 450));
        assert_eq!(
            zoomed(PhysicalSize::new(3, 1), 0.1),
            PhysicalSize::new(1, 1)
        );
    }

    #[test]
    fn the_border_frames_the_buffer_only() {
        let (w, h) = (5, 4);
        let mut buf = vec![0u32; w * h];
        draw_border(&mut buf, w, h);
        for y in 0..h {
            for x in 0..w {
                let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                assert_eq!(buf[y * w + x] == BORDER, edge, "({x}, {y})");
            }
        }
    }
}
