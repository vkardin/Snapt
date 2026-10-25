//! The full-screen overlay where the user selects an area and annotates it.
//!
//! Every monitor gets a borderless window showing its frozen, dimmed screenshot.
//! The selected area shows through at full brightness, with the drawing tools
//! and actions in two panels next to it, as in LightShot.

mod annotate;
mod render;
mod selection;
mod toolbar;

use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use image::{RgbaImage, imageops};
use softbuffer::Surface;
use tiny_skia::{Pixmap, Transform};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, OwnedDisplayHandle},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    monitor::MonitorHandle,
    window::{CursorIcon, Window, WindowId, WindowLevel},
};

use crate::capture::{Bounds, Shot};
use crate::i18n::{T, t};
use annotate::{Canvas, Kind, Shape};
use render::Frame;
use selection::{Handle, Hit, Point, Rect, Selection};
use toolbar::{Button, Look, Target, Toolbar};

pub use annotate::Tool;

/// What the user decided to do with the selection.
pub enum Outcome {
    Copy(RgbaImage),
    Save(RgbaImage),
    Print(RgbaImage),
    Search(RgbaImage),
    /// Pin the image on top of all windows, `size` big at `position` (physical pixels).
    Pin {
        image: RgbaImage,
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    },
    Cancel,
}

/// A selected area and the monitor it was on, to select again next time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    monitor: PhysicalPosition<i32>,
    rect: Rect,
}

/// The drawing settings, kept from one screenshot to the next.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub tool: Option<Tool>,
    pub color: u32,
    /// Stroke width in logical pixels.
    pub stroke: f32,
    /// Font size in logical pixels.
    pub text_size: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            tool: None,
            color: toolbar::PALETTE[0],
            stroke: 3.0,
            text_size: 20.0,
        }
    }
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const STROKES: std::ops::RangeInclusive<f32> = 1.0..=30.0;
const TEXT_SIZES: std::ops::RangeInclusive<f32> = 10.0..=96.0;

pub struct Overlay {
    // Declared before the context so surfaces drop first.
    screens: Vec<Screen>,
    _context: softbuffer::Context<OwnedDisplayHandle>,
    modifiers: ModifiersState,
    style: Style,
    palette: bool,
    /// The eyedropper waits for a click on the screen.
    picking: bool,
    /// Text for the clipboard, such as a picked color.
    copied: Option<String>,
    /// A selection waiting for text recognition.
    recognize: Option<RgbaImage>,
    /// A short message over the screen, such as OCR progress.
    status: Option<String>,
}

struct Screen {
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    window: Rc<Window>,
    /// The monitor's top-left corner on the desktop.
    origin: PhysicalPosition<i32>,
    image: RgbaImage,
    frame: Frame,
    selection: Selection,
    cursor: Point,
    last_click: Option<(Instant, Point)>,
    canvas: Canvas,
    /// The shape being drawn, or the text being typed.
    live: Option<Shape>,
    /// Whether the mouse is down drawing `live`.
    drawing: bool,
    /// Finished shapes rendered for the current selection.
    layer: Option<Layer>,
    hover: Option<Target>,
    /// The other apps' windows, front to back, in image pixels.
    windows: Vec<Bounds>,
    /// The window under the cursor, outlined until a selection is made.
    snap: Option<Rect>,
}

struct Layer {
    rect: Rect,
    version: u64,
    pixmap: Pixmap,
}

impl Overlay {
    /// Opens one overlay window per monitor, each showing its shot, with
    /// `area` already selected when it is given and its monitor is still there.
    pub fn open(
        event_loop: &ActiveEventLoop,
        shots: Vec<Shot>,
        style: Style,
        area: Option<Area>,
    ) -> Result<Self> {
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;

        let mut shots: Vec<Option<Shot>> = shots.into_iter().map(Some).collect();
        let mut screens = Vec::new();
        for monitor in event_loop.available_monitors() {
            let Some(shot) = take_matching_shot(&monitor, &mut shots) else {
                log::warn!("no screenshot for monitor {:?}", monitor.name());
                continue;
            };
            match Screen::new(event_loop, &context, &monitor, shot) {
                Ok(screen) => screens.push(screen),
                Err(err) => log::error!("failed to open an overlay: {err:#}"),
            }
        }
        ensure!(!screens.is_empty(), "could not open any overlay window");

        let mut overlay = Self {
            screens,
            _context: context,
            modifiers: ModifiersState::empty(),
            style,
            palette: false,
            picking: false,
            copied: None,
            recognize: None,
            status: None,
        };
        if let Some(area) = area
            && let Some(screen) = overlay
                .screens
                .iter_mut()
                .find(|s| s.origin == area.monitor)
        {
            screen.selection.set(area.rect);
        }
        // Show every window only after its first frame is drawn, so nothing flickers.
        for i in 0..overlay.screens.len() {
            overlay.redraw(i);
            overlay.screens[i].window.set_visible(true);
        }
        // Keyboard focus goes to the first screen; clicking another one moves it there.
        overlay.screens[0].window.focus_window();
        Ok(overlay)
    }

    /// The area selected right now, if any.
    pub fn area(&self) -> Option<Area> {
        self.screens.iter().find_map(|s| {
            Some(Area {
                monitor: s.origin,
                rect: s.selection.rect()?,
            })
        })
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.screens.iter().any(|s| s.window.id() == id)
    }

    pub fn style(&self) -> Style {
        self.style
    }

    /// A selection to recognize text in, once.
    pub fn take_recognize(&mut self) -> Option<RgbaImage> {
        self.recognize.take()
    }

    /// Shows `status` over the screens, or hides it.
    pub fn set_status(&mut self, status: Option<String>) {
        self.status = status;
        self.request_redraw_all();
    }

    fn start_recognize(&mut self, image: RgbaImage) {
        self.recognize = Some(image);
        self.set_status(Some(t(T::Recognizing).into()));
    }

    /// Text the overlay wants on the clipboard, once.
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    /// Handles an event for one of the overlay windows.
    pub fn window_event(&mut self, id: WindowId, event: WindowEvent) -> Option<Outcome> {
        let index = self.screens.iter().position(|s| s.window.id() == id)?;
        match event {
            WindowEvent::CloseRequested => return Some(Outcome::Cancel),
            WindowEvent::RedrawRequested => self.redraw(index),
            WindowEvent::Resized(size) => self.screens[index].resize(size),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => self.cursor_moved(index, position),
            WindowEvent::MouseInput { state, button, .. } => {
                return self.mouse_input(index, state, button);
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(index, delta),
            WindowEvent::KeyboardInput { event, .. } => return self.key(index, event),
            _ => {}
        }
        None
    }

    fn redraw(&mut self, index: usize) {
        let style = self.style;
        let palette = self.palette;
        let picking = self.picking;
        let status = self.status.clone();
        self.screens[index].redraw(&style, palette, picking, status.as_deref());
    }

    fn request_redraw_all(&self) {
        for screen in &self.screens {
            screen.window.request_redraw();
        }
    }

    fn cursor_moved(&mut self, index: usize, position: PhysicalPosition<f64>) {
        let straight = self.modifiers.shift_key();
        let palette = self.palette;
        let style = self.style;
        let screen = &mut self.screens[index];
        screen.cursor = Point::new(position.x as i32, position.y as i32);
        if screen.drawing {
            if let Some(shape) = &mut screen.live {
                shape.drag_to(screen.cursor.into(), straight);
                screen.window.request_redraw();
            }
        } else if screen.selection.motion(screen.cursor) {
            screen.window.request_redraw();
        }
        let hover = screen
            .toolbar(palette)
            .and_then(|bar| bar.hit(screen.cursor));
        if hover != screen.hover {
            screen.hover = hover;
            screen.window.request_redraw();
        }
        let snap = screen.snap_target(self.picking);
        if snap != screen.snap {
            screen.snap = snap;
            screen.window.request_redraw();
        }
        screen.update_cursor_icon(&style);
        if self.picking {
            // The preview next to the cursor follows it.
            screen.window.set_cursor(CursorIcon::Crosshair);
            screen.window.request_redraw();
        }
    }

    fn mouse_input(
        &mut self,
        index: usize,
        state: ElementState,
        button: MouseButton,
    ) -> Option<Outcome> {
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => return self.press(index),
            (MouseButton::Left, ElementState::Released) => {
                let style = self.style;
                let straight = self.modifiers.shift_key();
                let screen = &mut self.screens[index];
                if screen.drawing {
                    screen.drawing = false;
                    if let Some(mut shape) = screen.live.take() {
                        shape.drag_to(screen.cursor.into(), straight);
                        screen.canvas.push(shape);
                    }
                } else {
                    screen.selection.release(screen.cursor);
                    // A click selects the window under it.
                    if screen.selection.rect().is_none()
                        && let Some(rect) = screen.window_at(screen.cursor)
                    {
                        screen.selection.set(rect);
                    }
                    screen.snap = None;
                }
                screen.update_cursor_icon(&style);
                screen.window.request_redraw();
            }
            (MouseButton::Right, ElementState::Pressed) => return Some(Outcome::Cancel),
            _ => {}
        }
        None
    }

    fn press(&mut self, index: usize) -> Option<Outcome> {
        if self.status.take().is_some() {
            self.request_redraw_all();
        }
        if self.picking {
            let screen = &mut self.screens[index];
            let color = screen.pixel_at(screen.cursor);
            self.picking = false;
            self.set_color(color);
            self.copied = Some(toolbar::hex(color));
            self.request_redraw_all();
            return None;
        }
        let target = self.screens[index]
            .toolbar(self.palette)
            .and_then(|bar| bar.hit(self.screens[index].cursor));
        if let Some(target) = target {
            return self.click_toolbar(index, target);
        }
        if self.palette {
            self.palette = false;
            self.request_redraw_all();
        }

        // Only one monitor holds a selection at a time.
        for (i, other) in self.screens.iter_mut().enumerate() {
            if i != index && other.selection.rect().is_some() {
                other.selection.clear();
                other.canvas.clear();
                other.live = None;
                other.window.request_redraw();
            }
        }

        let style = self.style;
        let screen = &mut self.screens[index];
        screen.window.request_redraw();
        let hit = screen.selection.hit(screen.cursor, screen.grip());
        screen.commit_text();
        match (hit, style.tool) {
            (Hit::Handle(_), _) => screen.selection.press(screen.cursor, screen.grip()),
            (Hit::Inside, Some(tool)) => {
                let scale = screen.window.scale_factor() as f32;
                let size = match tool {
                    Tool::Text => style.text_size,
                    _ => style.stroke,
                } * scale;
                let mut at: tiny_skia::Point = screen.cursor.into();
                if tool == Tool::Text {
                    // The click marks where the first line's middle goes.
                    at = tiny_skia::Point::from_xy(
                        at.x,
                        at.y - crate::font::line_height(size) / 2.0,
                    );
                }
                screen.live = Some(Shape::start(tool, at, style.color, size));
                screen.drawing = tool != Tool::Text;
            }
            (Hit::Inside, None) => {
                if screen.is_double_click() {
                    return screen.output().map(Outcome::Copy);
                }
                screen.selection.press(screen.cursor, screen.grip());
            }
            // Drawings are never thrown away by a stray click: Esc or undo starts over.
            (Hit::Outside, _) if !screen.canvas.is_empty() => {}
            (Hit::Outside, _) => {
                screen.last_click = None;
                screen.selection.press(screen.cursor, screen.grip());
            }
        }
        None
    }

    fn click_toolbar(&mut self, index: usize, target: Target) -> Option<Outcome> {
        let screen = &mut self.screens[index];
        match target {
            Target::Panel => return None,
            Target::Swatch(color) => {
                self.palette = false;
                self.set_color(color);
            }
            // The palette stays open to fine-tune the pick.
            Target::Spectrum(color) => self.set_color(color),
            Target::Button(Button::Eyedropper) => {
                self.palette = false;
                self.picking = true;
                screen.window.set_cursor(CursorIcon::Crosshair);
            }
            Target::Button(Button::Tool(tool)) => {
                screen.commit_text();
                self.style.tool = (self.style.tool != Some(tool)).then_some(tool);
                self.palette = false;
            }
            Target::Button(Button::Color) => self.palette = !self.palette,
            Target::Button(Button::Undo) => screen.undo(),
            Target::Button(Button::Close) => return Some(Outcome::Cancel),
            Target::Button(Button::Recognize) => {
                if let Some(image) = screen.output() {
                    self.start_recognize(image);
                }
                return None;
            }
            Target::Button(Button::Search) => return screen.output().map(Outcome::Search),
            Target::Button(Button::Print) => return screen.output().map(Outcome::Print),
            Target::Button(Button::Pin) => return screen.pin(),
            Target::Button(Button::Copy) => return screen.output().map(Outcome::Copy),
            Target::Button(Button::Save) => return screen.output().map(Outcome::Save),
        }
        self.request_redraw_all();
        None
    }

    /// Sets the drawing color, for the text being typed too.
    fn set_color(&mut self, color: u32) {
        self.style.color = color;
        for screen in &mut self.screens {
            if let Some(shape) = screen.live.as_mut() {
                shape.color = color;
            }
        }
    }

    /// The wheel sets the stroke width, or the font size for text.
    fn wheel(&mut self, index: usize, delta: MouseScrollDelta) {
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => y.signum(),
            MouseScrollDelta::PixelDelta(p) => (p.y as f32).signum(),
        };
        if steps == 0.0 {
            return;
        }
        let screen = &mut self.screens[index];
        let scale = screen.window.scale_factor() as f32;
        if self.style.tool == Some(Tool::Text) {
            self.style.text_size =
                (self.style.text_size + 2.0 * steps).clamp(*TEXT_SIZES.start(), *TEXT_SIZES.end());
            if let Some(shape) = screen
                .live
                .as_mut()
                .filter(|s| matches!(s.kind, Kind::Text(..)))
            {
                shape.size = self.style.text_size * scale;
            }
        } else {
            self.style.stroke = (self.style.stroke + steps).clamp(*STROKES.start(), *STROKES.end());
        }
        screen.window.request_redraw();
    }

    fn key(&mut self, index: usize, event: KeyEvent) -> Option<Outcome> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let command = self.modifiers.control_key() || self.modifiers.super_key();
        let style = self.style;
        let screen = &mut self.screens[index];

        if screen.is_typing() && !command {
            screen.type_key(&event);
            screen.window.request_redraw();
            return None;
        }

        // Physical keys, so shortcuts work with any keyboard layout.
        let PhysicalKey::Code(code) = event.physical_key else {
            return None;
        };
        let has_selection = screen.selection.rect().is_some();
        let tool = match code {
            KeyCode::KeyP => Some(Tool::Pen),
            KeyCode::KeyL => Some(Tool::Line),
            KeyCode::KeyA => Some(Tool::Arrow),
            KeyCode::KeyR => Some(Tool::Rect),
            KeyCode::KeyM => Some(Tool::Marker),
            KeyCode::KeyT => Some(Tool::Text),
            _ => None,
        };
        match code {
            KeyCode::Escape if self.picking => {
                self.picking = false;
                screen.update_cursor_icon(&style);
                self.request_redraw_all();
                None
            }
            KeyCode::KeyI if !command && has_selection => {
                self.picking = !self.picking;
                self.palette = false;
                self.request_redraw_all();
                None
            }
            KeyCode::Escape if self.palette => {
                self.palette = false;
                self.request_redraw_all();
                None
            }
            KeyCode::Escape => Some(Outcome::Cancel),
            KeyCode::Enter | KeyCode::NumpadEnter => Some(Outcome::Copy(screen.output_or_all())),
            KeyCode::KeyC if command && self.modifiers.shift_key() => {
                let image = screen.output_or_all();
                self.start_recognize(image);
                None
            }
            KeyCode::KeyC if command => Some(Outcome::Copy(screen.output_or_all())),
            KeyCode::KeyS if command => Some(Outcome::Save(screen.output_or_all())),
            KeyCode::KeyP if command => Some(Outcome::Print(screen.output_or_all())),
            KeyCode::KeyF if command => Some(Outcome::Search(screen.output_or_all())),
            KeyCode::F3 => screen.pin(),
            KeyCode::KeyA if command => {
                screen.commit_text();
                screen.selection.select_all();
                screen.window.request_redraw();
                None
            }
            KeyCode::KeyZ if command && self.modifiers.shift_key() => {
                screen.redo();
                None
            }
            KeyCode::KeyY if command => {
                screen.redo();
                None
            }
            KeyCode::KeyZ if command => {
                screen.undo();
                None
            }
            _ if !command && has_selection && tool.is_some() => {
                self.style.tool = (style.tool != tool).then_some(tool).flatten();
                self.palette = false;
                screen.update_cursor_icon(&self.style);
                self.request_redraw_all();
                None
            }
            _ => None,
        }
    }
}

impl From<Point> for tiny_skia::Point {
    fn from(p: Point) -> Self {
        tiny_skia::Point::from_xy(p.x as f32, p.y as f32)
    }
}

impl Screen {
    fn new(
        event_loop: &ActiveEventLoop,
        context: &softbuffer::Context<OwnedDisplayHandle>,
        monitor: &MonitorHandle,
        shot: Shot,
    ) -> Result<Self> {
        let Shot { image, windows, .. } = shot;
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
            origin: monitor.position(),
            frame: Frame::new(&image, size.width.max(1), size.height.max(1)),
            selection: Selection::new(size.width as i32, size.height as i32),
            window,
            image,
            cursor: Point::new(0, 0),
            last_click: None,
            canvas: Canvas::default(),
            live: None,
            drawing: false,
            layer: None,
            hover: None,
            windows,
            snap: None,
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
            self.canvas.clear();
            self.live = None;
        }
        self.window.request_redraw();
    }

    /// The panels, shown once a selection is made and not being dragged.
    fn toolbar(&self, palette: bool) -> Option<Toolbar> {
        let rect = self.selection.rect()?;
        if self.selection.is_dragging() {
            return None;
        }
        Some(Toolbar::layout(
            rect,
            self.frame.width as i32,
            self.frame.height as i32,
            self.window.scale_factor(),
            palette,
        ))
    }

    fn redraw(&mut self, style: &Style, palette: bool, picking: bool, status: Option<&str>) {
        let rect = self.selection.rect().or(self.snap);
        let label = rect.map(|r| self.label(r, style)).unwrap_or_default();
        let scale = self.window.scale_factor();
        let toolbar = self.toolbar(palette);
        let picked = picking.then(|| self.pixel_at(self.cursor));
        if let Some(rect) = rect {
            self.update_layer(rect);
        }
        let width = self.frame.width as i32;
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
        render::draw(&mut buffer, &self.frame, rect);
        if let Some(rect) = rect {
            if let Some(layer) = &self.layer {
                render::blend_pixmap(&mut buffer, width, &layer.pixmap, rect.x, rect.y);
            }
            if let Some(shape) = &self.live {
                draw_live(&mut buffer, width, rect, shape, !self.drawing, scale);
            }
            render::draw_frame(&mut buffer, width, rect, &label, scale);
        }
        if let Some(bar) = toolbar {
            let look = Look {
                tool: style.tool,
                color: style.color,
                hover: self.hover,
                picking,
            };
            bar.draw(&mut buffer, width, &look);
        }
        if let Some(status) = status {
            draw_status(&mut buffer, width, status, scale);
        }
        if let Some(color) = picked {
            draw_pick_preview(&mut buffer, width, self.cursor, color, scale);
        }
        if let Err(err) = buffer.present() {
            log::error!("failed to present the overlay: {err}");
        }
    }

    /// The window to outline under the cursor: only while nothing is selected or drawn.
    fn snap_target(&self, picking: bool) -> Option<Rect> {
        if picking
            || self.selection.rect().is_some()
            || self.selection.is_dragging()
            || !self.canvas.is_empty()
        {
            return None;
        }
        self.window_at(self.cursor)
    }

    /// The front-most window under `p`, in window pixels.
    fn window_at(&self, p: Point) -> Option<Rect> {
        let (iw, ih) = self.image.dimensions();
        let sx = iw as f64 / self.frame.width as f64;
        let sy = ih as f64 / self.frame.height as f64;
        let (x, y) = ((p.x as f64 * sx) as i32, (p.y as f64 * sy) as i32);
        let b = self
            .windows
            .iter()
            .find(|b| x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h)?;
        let left = (b.x as f64 / sx).round() as i32;
        let top = (b.y as f64 / sy).round() as i32;
        let right = ((b.x + b.w) as f64 / sx).round() as i32;
        let bottom = ((b.y + b.h) as f64 / sy).round() as i32;
        (right > left && bottom > top).then_some(Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        })
    }

    /// The size of the selection in image pixels, plus the stroke width while drawing.
    fn label(&self, rect: Rect, style: &Style) -> String {
        let (_, _, w, h) = self.to_image(rect).unwrap_or_default();
        match style.tool {
            Some(Tool::Text) => format!("{w} × {h}  ·  {} px", style.text_size),
            Some(_) => format!("{w} × {h}  ·  {} px", style.stroke),
            None => format!("{w} × {h}"),
        }
    }

    /// Renders the finished shapes once per change, clipped to the selection.
    fn update_layer(&mut self, rect: Rect) {
        if self.canvas.is_empty() {
            self.layer = None;
            return;
        }
        if self
            .layer
            .as_ref()
            .is_some_and(|l| l.rect == rect && l.version == self.canvas.version)
        {
            return;
        }
        let Some(mut pixmap) = Pixmap::new(rect.w as u32, rect.h as u32) else {
            return;
        };
        let transform = Transform::from_translate(-rect.x as f32, -rect.y as f32);
        for shape in self.canvas.shapes() {
            shape.draw(&mut pixmap, transform);
        }
        self.layer = Some(Layer {
            rect,
            version: self.canvas.version,
            pixmap,
        });
    }

    fn update_cursor_icon(&self, style: &Style) {
        if self.selection.is_dragging() || self.drawing {
            return;
        }
        if self.hover.is_some() {
            self.window.set_cursor(CursorIcon::Pointer);
            return;
        }
        let icon = match (self.selection.hit(self.cursor, self.grip()), style.tool) {
            (Hit::Outside, _) => CursorIcon::Crosshair,
            (Hit::Inside, Some(Tool::Text)) => CursorIcon::Text,
            (Hit::Inside, Some(_)) => CursorIcon::Crosshair,
            (Hit::Inside, None) => CursorIcon::Move,
            (Hit::Handle(Handle::N | Handle::S), _) => CursorIcon::NsResize,
            (Hit::Handle(Handle::E | Handle::W), _) => CursorIcon::EwResize,
            (Hit::Handle(Handle::NW | Handle::SE), _) => CursorIcon::NwseResize,
            (Hit::Handle(Handle::NE | Handle::SW), _) => CursorIcon::NeswResize,
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

    /// Whether keys go into a text annotation.
    fn is_typing(&self) -> bool {
        !self.drawing
            && matches!(
                self.live,
                Some(Shape {
                    kind: Kind::Text(..),
                    ..
                })
            )
    }

    fn type_key(&mut self, event: &KeyEvent) {
        let Some(Shape {
            kind: Kind::Text(_, text),
            ..
        }) = self.live.as_mut()
        else {
            return;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => self.commit_text(),
            Key::Named(NamedKey::Enter) => text.push('\n'),
            Key::Named(NamedKey::Backspace) => {
                text.pop();
            }
            _ => {
                if let Some(typed) = &event.text {
                    text.extend(typed.chars().filter(|c| !c.is_control()));
                }
            }
        }
    }

    /// Finishes the text being typed, if any.
    fn commit_text(&mut self) {
        if self.is_typing()
            && let Some(shape) = self.live.take()
        {
            self.canvas.push(shape);
        }
    }

    fn undo(&mut self) {
        if self.is_typing() {
            self.live = None;
        } else {
            self.canvas.undo();
        }
        self.window.request_redraw();
    }

    fn redo(&mut self) {
        self.commit_text();
        self.canvas.redo();
        self.window.request_redraw();
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

    /// The screenshot's color under a window point, as `0xRRGGBB`.
    fn pixel_at(&self, p: Point) -> u32 {
        let (iw, ih) = self.image.dimensions();
        let x = (p.x.max(0) as u64 * iw as u64 / self.frame.width.max(1) as u64) as u32;
        let y = (p.y.max(0) as u64 * ih as u64 / self.frame.height.max(1) as u64) as u32;
        let [r, g, b, _] = self.image.get_pixel(x.min(iw - 1), y.min(ih - 1)).0;
        (r as u32) << 16 | (g as u32) << 8 | b as u32
    }

    /// The selected area with the drawings painted on.
    fn output(&mut self) -> Option<RgbaImage> {
        self.commit_text();
        let (x, y, w, h) = self.to_image(self.selection.rect()?)?;
        let mut shot = imageops::crop_imm(&self.image, x, y, w, h).to_image();
        let scale = self.image.width() as f32 / self.frame.width as f32;
        annotate::flatten(
            &mut shot,
            self.canvas.shapes(),
            x as f32 / scale,
            y as f32 / scale,
            scale,
        );
        Some(shot)
    }

    /// The selection, or the whole monitor when nothing is selected.
    fn output_or_all(&mut self) -> RgbaImage {
        self.output().unwrap_or_else(|| self.image.clone())
    }

    /// Pins the selection right where it is on screen.
    fn pin(&mut self) -> Option<Outcome> {
        let rect = self.selection.rect()?;
        Some(Outcome::Pin {
            image: self.output()?,
            position: PhysicalPosition::new(self.origin.x + rect.x, self.origin.y + rect.y),
            size: PhysicalSize::new(rect.w as u32, rect.h as u32),
        })
    }
}

/// A message at the top middle of the screen.
fn draw_status(buf: &mut [u32], width: i32, text: &str, scale: f64) {
    let s = scale as f32;
    let size = 14.0 * s;
    let (tw, th) = crate::font::measure(text, size);
    let (pad_x, pad_y) = (14.0 * s, 9.0 * s);
    let w = (tw + 2.0 * pad_x).ceil() as i32;
    let h = (th + 2.0 * pad_y).ceil() as i32;
    let Some(mut pixmap) = Pixmap::new(w as u32, h as u32) else {
        return;
    };
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32);
    crate::paint::fill_round(&mut pixmap, full, 8.0 * s, 0x111113, 0.94);
    crate::font::draw(&mut pixmap, text, pad_x, pad_y, size, 0xFFFFFF, 1.0);
    let x = ((width - w) / 2).max(0);
    render::blend_pixmap(buf, width, &pixmap, x, (24.0 * s) as i32);
}

/// A small label next to the cursor with the color under it.
fn draw_pick_preview(buf: &mut [u32], width: i32, cursor: Point, color: u32, scale: f64) {
    let s = scale as f32;
    let text = toolbar::hex(color);
    let size = 12.5 * s;
    let (tw, th) = crate::font::measure(&text, size);
    let pad = 6.0 * s;
    let dot = 14.0 * s;
    let w = (pad * 3.0 + dot + tw).ceil() as i32;
    let h = (th.max(dot) + 2.0 * pad).ceil() as i32;
    let height = buf.len() as i32 / width.max(1);
    let gap = (18.0 * s) as i32;
    let mut x = cursor.x + gap;
    let mut y = cursor.y + gap;
    if x + w > width {
        x = cursor.x - gap - w;
    }
    if y + h > height {
        y = cursor.y - gap - h;
    }
    let Some(mut pixmap) = Pixmap::new(w as u32, h as u32) else {
        return;
    };
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32);
    crate::paint::fill_round(&mut pixmap, full, 6.0 * s, 0x111113, 0.94);
    let swatch = tiny_skia::Rect::from_xywh(pad, (h as f32 - dot) / 2.0, dot, dot);
    crate::paint::fill_round(&mut pixmap, swatch, 3.0 * s, 0xFFFFFF, 1.0);
    let inner = tiny_skia::Rect::from_xywh(
        pad + s,
        (h as f32 - dot) / 2.0 + s,
        dot - 2.0 * s,
        dot - 2.0 * s,
    );
    crate::paint::fill_round(&mut pixmap, inner, 2.5 * s, color, 1.0);
    crate::font::draw(
        &mut pixmap,
        &text,
        pad * 2.0 + dot,
        (h as f32 - th) / 2.0,
        size,
        0xFFFFFF,
        1.0,
    );
    render::blend_pixmap(buf, width, &pixmap, x, y);
}

/// Draws the shape in progress, clipped to the selection. Text being typed gets a caret.
fn draw_live(buf: &mut [u32], width: i32, rect: Rect, shape: &Shape, typing: bool, scale: f64) {
    let Some(bounds) = shape.bounds() else { return };
    let left = (bounds.left().floor() as i32).max(rect.x);
    let top = (bounds.top().floor() as i32).max(rect.y);
    let right = (bounds.right().ceil() as i32).min(rect.right());
    let bottom = (bounds.bottom().ceil() as i32).min(rect.bottom());
    if left >= right || top >= bottom {
        return;
    }
    let Some(mut pixmap) = Pixmap::new((right - left) as u32, (bottom - top) as u32) else {
        return;
    };
    let transform = Transform::from_translate(-left as f32, -top as f32);
    shape.draw(&mut pixmap, transform);
    if typing && let Kind::Text(at, text) = &shape.kind {
        let line = crate::font::line_height(shape.size);
        let lines = text.split('\n').count() as f32;
        let x = at.x + crate::font::last_line_width(text, shape.size) + 1.0;
        let y = at.y + (lines - 1.0) * line;
        let caret = tiny_skia::Rect::from_xywh(
            x - left as f32,
            y - top as f32,
            (1.5 * scale as f32).max(1.0),
            line,
        );
        crate::paint::fill_round(&mut pixmap, caret, 0.0, shape.color, 1.0);
    }
    render::blend_pixmap(buf, width, &pixmap, left, top);
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
