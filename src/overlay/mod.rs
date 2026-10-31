//! The full-screen overlay where the user selects an area and annotates it.
//!
//! Every monitor gets a borderless window showing its frozen, dimmed screenshot.
//! The selected area shows through at full brightness, with the drawing tools
//! and actions in two panels next to it, as in LightShot.

mod annotate;
mod render;
mod selection;
#[cfg(test)]
mod snapshots;
mod toolbar;

use std::{
    cell::Cell,
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
use crate::settings::{self, ColorFormat, Feature, Settings, Shortcut, enabled as on};
use annotate::{Backdrop, Canvas, Kind, Shape};
use render::Frame;
use selection::{Handle, Hit, Point, Rect, Selection};
use toolbar::{Button, Look, Target, Toolbar};

pub use annotate::Tool;

/// What the overlay is opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A screenshot, edited before it goes out.
    Shot,
    /// Only to pick a color: no selection, the click copies and closes.
    Pick,
    /// A scrolling screenshot of a page: selecting the page starts it.
    ScrollPage,
}

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
    /// Capture the area while the user scrolls it, or record it.
    Scroll {
        region: crate::scroll::Region,
        /// Where the scrolling panel goes, outside the area (physical pixels).
        panel: PhysicalPosition<i32>,
        mode: crate::scroll::Mode,
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
    /// Rectangles and ellipses come out filled.
    pub fill: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            tool: None,
            color: toolbar::PALETTE[0],
            stroke: 3.0,
            text_size: 20.0,
            fill: false,
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
    /// Opened only to pick a color: no selection, the click copies and closes.
    pick_only: bool,
    /// Opened for a scrolling screenshot: selecting an area or a window starts it.
    scroll_page: bool,
    format: ColorFormat,
    /// A newly selected area is copied right away, without the editor.
    copy_area: bool,
    /// So is a window picked with a click.
    copy_window: bool,
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
    /// The same corner as the capture API reports it.
    shot_origin: (i32, i32),
    /// Image pixels per unit of `shot_origin`.
    shot_scale: f32,
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
    /// The shape picked with no tool active, to move or delete.
    selected: Option<usize>,
    /// Where the mouse last was while dragging `selected`, and whether it has moved yet.
    moving: Option<(Point, bool)>,
    hover: Option<Target>,
    /// The other apps' windows, front to back, in image pixels.
    windows: Vec<Bounds>,
    /// The window under the cursor, outlined until a selection is made.
    snap: Option<Rect>,
    /// The crop tool is on: a rectangle drawn inside the selection becomes the selection.
    cropping: bool,
    /// Where the mouse went down drawing the crop rectangle.
    crop_from: Option<Point>,
    /// The size being typed for the selection, in image pixels, such as `800x6`.
    size_entry: Option<String>,
    /// What the last frame drew over the dimmed screen, bright part included,
    /// or `None` when the next frame has to be drawn whole.
    drawn: Option<Rect>,
    /// A redraw was asked for by Snapt, not by the system repainting the window,
    /// so only what changed needs drawing.
    asked: Cell<bool>,
}

struct Layer {
    rect: Rect,
    version: u64,
    pixmap: Pixmap,
}

impl Overlay {
    /// Opens one overlay window per monitor, each showing its shot, with
    /// `area` already selected when it is given and its monitor is still there.
    /// As the color picker it copies colors in the format `settings` ask for.
    pub fn open(
        event_loop: &ActiveEventLoop,
        shots: Vec<Shot>,
        style: Style,
        area: Option<Area>,
        purpose: Purpose,
        settings: &Settings,
    ) -> Result<Self> {
        let pick_only = purpose == Purpose::Pick;
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
            picking: pick_only,
            pick_only,
            scroll_page: purpose == Purpose::ScrollPage,
            format: settings.color_format,
            copy_area: settings.copy_on_select,
            copy_window: settings.copy_on_select && !settings.window_opens_editor,
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
        let picker = self.picking.then_some(self.format);
        let status = self.status.clone().or_else(|| {
            // Says what to do until something is selected.
            (self.scroll_page && self.area().is_none()).then(|| t(T::ScrollPageHint).to_string())
        });
        let pick_only = self.pick_only;
        self.screens[index].redraw(&style, palette, picker, pick_only, status.as_deref());
    }

    fn request_redraw_all(&self) {
        for screen in &self.screens {
            screen.request_redraw();
        }
    }

    fn cursor_moved(&mut self, index: usize, position: PhysicalPosition<f64>) {
        let straight = self.modifiers.shift_key();
        let palette = self.palette;
        let style = self.style;
        let screen = &mut self.screens[index];
        screen.cursor = Point::new(position.x as i32, position.y as i32);
        if let (Some((last, moved)), Some(mut i)) = (screen.moving, screen.selected) {
            if !moved {
                // One undo step for the whole drag, taken only if it really moves.
                i = screen.canvas.begin_move(i);
                screen.selected = Some(i);
            }
            let (dx, dy) = (screen.cursor.x - last.x, screen.cursor.y - last.y);
            screen.canvas.translate(i, dx as f32, dy as f32);
            screen.moving = Some((screen.cursor, true));
            screen.request_redraw();
        } else if screen.crop_from.is_some() {
            screen.request_redraw();
        } else if screen.drawing {
            if let Some(shape) = &mut screen.live {
                shape.drag_to(screen.cursor.into(), straight);
                screen.request_redraw();
            }
        } else if screen.selection.motion(screen.cursor) {
            screen.request_redraw();
        }
        let hover = screen
            .toolbar(palette)
            .and_then(|bar| bar.hit(screen.cursor));
        if hover != screen.hover {
            screen.hover = hover;
            screen.request_redraw();
        }
        let snap = screen.snap_target(self.picking);
        if snap != screen.snap {
            screen.snap = snap;
            screen.request_redraw();
        }
        screen.update_cursor_icon(&style);
        if self.picking {
            // The preview next to the cursor follows it.
            screen.window.set_cursor(CursorIcon::Crosshair);
            screen.request_redraw();
        }
        if screen.selection.rect().is_none() {
            // So does the magnifier before a selection.
            screen.request_redraw();
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
                if let Some(from) = screen.crop_from.take() {
                    if let Some(rect) = screen.crop_rect(from)
                        && rect.w >= 4
                        && rect.h >= 4
                    {
                        screen.selection.set(rect);
                        screen.cropping = false;
                    }
                } else if screen.moving.take().is_some() {
                } else if screen.drawing {
                    screen.drawing = false;
                    if let Some(mut shape) = screen.live.take() {
                        shape.drag_to(screen.cursor.into(), straight);
                        screen.canvas.push(shape);
                    }
                } else {
                    let creating = screen.selection.is_creating();
                    screen.selection.release(screen.cursor);
                    let mut copy = self.copy_area;
                    // A click selects the window under it.
                    if screen.selection.rect().is_none()
                        && let Some(rect) = screen.window_at(screen.cursor)
                    {
                        screen.selection.set(rect);
                        copy = self.copy_window;
                    }
                    screen.snap = None;
                    if self.scroll_page && screen.selection.rect().is_some() {
                        return self.start_scroll(index, crate::scroll::Mode::Scroll);
                    }
                    // Copying right away skips the editor, so only a fresh
                    // selection with nothing drawn yet goes straight out.
                    if copy && creating && screen.canvas.is_empty() {
                        if let Some(image) = screen.output() {
                            return Some(Outcome::Copy(image));
                        }
                    }
                }
                screen.update_cursor_icon(&style);
                screen.request_redraw();
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
            self.copied = Some(self.format.format(color));
            if self.pick_only {
                return Some(Outcome::Cancel);
            }
            self.picking = false;
            self.set_color(color);
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
                other.selected = None;
                other.live = None;
                other.request_redraw();
            }
        }

        let style = self.style;
        let screen = &mut self.screens[index];
        screen.request_redraw();
        let hit = screen.selection.hit(screen.cursor, screen.grip());
        screen.commit_text();
        screen.selected = None;
        screen.size_entry = None;
        if screen.cropping && hit != Hit::Outside {
            screen.crop_from = Some(screen.cursor);
            return None;
        }
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
                let mut shape = Shape::start(tool, at, style.color, size);
                shape.fill = style.fill;
                if let Kind::Number(_, n) = &mut shape.kind {
                    *n = screen.canvas.next_number();
                }
                screen.live = Some(shape);
                screen.drawing = tool != Tool::Text;
            }
            (Hit::Inside, None) if screen.canvas.hit(screen.cursor.into()).is_some() => {
                screen.selected = screen.canvas.hit(screen.cursor.into());
                screen.moving = Some((screen.cursor, false));
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
                screen.cropping = false;
                self.style.tool = (self.style.tool != Some(tool)).then_some(tool);
                self.palette = false;
            }
            Target::Button(Button::Crop) => {
                screen.commit_text();
                screen.cropping = !screen.cropping;
                self.style.tool = None;
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
            Target::Button(Button::Scroll) => {
                return self.start_scroll(index, crate::scroll::Mode::Scroll);
            }
            Target::Button(Button::Record) => {
                return self.start_scroll(index, crate::scroll::Mode::Record);
            }
            Target::Button(Button::Copy) => return screen.output().map(Outcome::Copy),
            Target::Button(Button::Save) => return screen.output().map(Outcome::Save),
        }
        self.request_redraw_all();
        None
    }

    /// Starts a scrolling screenshot or a recording of the selection, or of
    /// the window under the mouse; with neither, says to select one.
    fn start_scroll(&mut self, index: usize, mode: crate::scroll::Mode) -> Option<Outcome> {
        let screen = &mut self.screens[index];
        screen.commit_text();
        let outcome = screen.scroll(mode);
        if outcome.is_none() {
            self.set_status(Some(t(T::ScrollSelectFirst).into()));
        }
        outcome
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
        screen.request_redraw();
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
            screen.request_redraw();
            return None;
        }

        // Physical keys, so shortcuts work with any keyboard layout.
        let PhysicalKey::Code(code) = event.physical_key else {
            return None;
        };
        if screen.size_entry.is_some() && !command {
            screen.type_size(code);
            screen.request_redraw();
            return None;
        }
        let has_selection = screen.selection.rect().is_some();
        let pressed = hotkey_code(code)
            .and_then(|code| settings::overlay_shortcut(hotkey_modifiers(self.modifiers), code));
        let tool = match code {
            KeyCode::KeyP => Some(Tool::Pen),
            KeyCode::KeyL => Some(Tool::Line),
            KeyCode::KeyA => Some(Tool::Arrow),
            KeyCode::KeyR => Some(Tool::Rect),
            KeyCode::KeyE => Some(Tool::Ellipse),
            KeyCode::KeyN => Some(Tool::Number),
            KeyCode::KeyB => Some(Tool::Pixelate),
            KeyCode::KeyU => Some(Tool::Blur),
            KeyCode::KeyM => Some(Tool::Marker),
            KeyCode::KeyT => Some(Tool::Text),
            KeyCode::KeyH => Some(Tool::Spotlight),
            _ => None,
        }
        .filter(|tool| on(toolbar::tool_feature(*tool)));
        match code {
            KeyCode::Escape if self.pick_only => Some(Outcome::Cancel),
            KeyCode::Escape if self.picking => {
                self.picking = false;
                screen.update_cursor_icon(&style);
                self.request_redraw_all();
                None
            }
            KeyCode::KeyF if !command && has_selection => {
                self.style.fill = !self.style.fill;
                self.request_redraw_all();
                None
            }
            KeyCode::KeyI if !command && has_selection => {
                self.picking = !self.picking;
                self.palette = false;
                self.request_redraw_all();
                None
            }
            KeyCode::Escape if screen.cropping => {
                screen.cropping = false;
                screen.crop_from = None;
                screen.update_cursor_icon(&style);
                screen.request_redraw();
                None
            }
            KeyCode::Escape if self.palette => {
                self.palette = false;
                self.request_redraw_all();
                None
            }
            KeyCode::Escape => Some(Outcome::Cancel),
            KeyCode::Enter | KeyCode::NumpadEnter => Some(Outcome::Copy(screen.output_or_all())),
            _ if pressed.is_some() => match pressed? {
                Shortcut::Recognize if on(Feature::Recognize) => {
                    let image = screen.output_or_all();
                    self.start_recognize(image);
                    None
                }
                Shortcut::Scroll if on(Feature::Scroll) => {
                    self.start_scroll(index, crate::scroll::Mode::Scroll)
                }
                Shortcut::Record if on(Feature::Record) => {
                    self.start_scroll(index, crate::scroll::Mode::Record)
                }
                Shortcut::Pin if on(Feature::Pin) => screen.pin(),
                Shortcut::Copy => Some(Outcome::Copy(screen.output_or_all())),
                Shortcut::Save => Some(Outcome::Save(screen.output_or_all())),
                _ => None,
            },
            KeyCode::KeyP if command && on(Feature::Print) => {
                Some(Outcome::Print(screen.output_or_all()))
            }
            KeyCode::KeyF if command && on(Feature::Search) => {
                Some(Outcome::Search(screen.output_or_all()))
            }
            KeyCode::KeyA if command => {
                screen.commit_text();
                screen.selection.select_all();
                screen.request_redraw();
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
            KeyCode::Delete | KeyCode::Backspace if screen.selected.is_some() => {
                if let Some(i) = screen.selected.take() {
                    screen.canvas.remove(i);
                }
                screen.request_redraw();
                None
            }
            KeyCode::KeyC if !command && has_selection && on(Feature::Crop) => {
                screen.commit_text();
                screen.cropping = !screen.cropping;
                self.style.tool = None;
                self.palette = false;
                screen.update_cursor_icon(&self.style);
                self.request_redraw_all();
                None
            }
            _ if !command && has_selection && digit(code).is_some() => {
                screen.size_entry = digit(code).map(String::from);
                screen.request_redraw();
                None
            }
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown
                if !command && has_selection =>
            {
                let step = if self.modifiers.shift_key() { 10 } else { 1 };
                let (dx, dy) = match code {
                    KeyCode::ArrowLeft => (-step, 0),
                    KeyCode::ArrowRight => (step, 0),
                    KeyCode::ArrowUp => (0, -step),
                    _ => (0, step),
                };
                screen.nudge(dx, dy, self.modifiers.alt_key());
                None
            }
            _ if !command && has_selection && tool.is_some() => {
                screen.cropping = false;
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
        let Shot {
            image,
            windows,
            x,
            y,
            scale,
        } = shot;
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
            shot_origin: (x, y),
            shot_scale: scale,
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
            selected: None,
            moving: None,
            hover: None,
            windows,
            snap: None,
            cropping: false,
            crop_from: None,
            size_entry: None,
            drawn: None,
            asked: Cell::new(false),
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
        self.drawn = None;
        self.request_redraw();
    }

    fn request_redraw(&self) {
        self.asked.set(true);
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

    fn redraw(
        &mut self,
        style: &Style,
        palette: bool,
        picker: Option<ColorFormat>,
        pick_only: bool,
        status: Option<&str>,
    ) {
        let picking = picker.is_some();
        let rect = self.selection.rect().or(self.snap);
        let label = rect.map(|r| self.label(r, style)).unwrap_or_default();
        let scale = self.window.scale_factor();
        let toolbar = self.toolbar(palette);
        let magnifier = picker
            .map(|format| format.format(self.pixel_at(self.cursor)))
            .or_else(|| {
                // Pixel-exact edges: before and while sizing the selection, show its corner.
                let aiming = self.selection.rect().is_none() || self.selection.is_sizing();
                (aiming && !pick_only).then(|| {
                    let (iw, ih) = self.image.dimensions();
                    let x = self.cursor.x as i64 * iw as i64 / self.frame.width.max(1) as i64;
                    let y = self.cursor.y as i64 * ih as i64 / self.frame.height.max(1) as i64;
                    format!("{x}, {y}")
                })
            })
            .map(|text| (self.pixels_around(self.cursor, 11), text));
        if let Some(rect) = rect {
            self.update_layer(rect);
        }
        let crop = self.crop_from.and_then(|from| self.crop_rect(from));
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
        // The picker alone shows the screen as it is, not dimmed.
        let full = Rect {
            x: 0,
            y: 0,
            w: self.frame.width as i32,
            h: self.frame.height as i32,
        };
        let bright = if pick_only { Some(full) } else { rect };
        // When the buffer still holds the last frame, only the parts drawn then
        // or now change; on a 4K screen that saves copying and sending ~33 MB a frame.
        let asked = self.asked.replace(false);
        let last = self.drawn.filter(|_| asked && buffer.age() == 1);
        let restored = last.map(|last| {
            let area = render::union(Some(last), bright).unwrap_or(last);
            render::restore(&mut buffer, &self.frame, bright, area);
            area
        });
        if restored.is_none() {
            render::draw(&mut buffer, &self.frame, bright);
        }
        let mut drawn = bright;
        if let Some(rect) = rect {
            if let Some(layer) = &self.layer {
                render::blend_pixmap(&mut buffer, width, &layer.pixmap, rect.x, rect.y);
            }
            if let Some(bounds) = self
                .selected
                .and_then(|i| self.canvas.shapes().get(i)?.bounds())
            {
                draw_dashed(&mut buffer, width, bounds, rect, scale);
            }
            if let Some(shape) = &self.live {
                draw_live(&mut buffer, width, rect, shape, !self.drawing, scale);
            }
            if let Some(crop) = crop {
                // draw_dashed pads what it outlines; take that back off.
                let pad = (4.0 * scale).round() as f32;
                let outline = tiny_skia::Rect::from_ltrb(
                    crop.x as f32 + pad,
                    crop.y as f32 + pad,
                    (crop.right() - 1) as f32 - pad,
                    (crop.bottom() - 1) as f32 - pad,
                )
                .or_else(|| tiny_skia::Rect::from_xywh(crop.x as f32, crop.y as f32, 1.0, 1.0));
                if let Some(outline) = outline {
                    draw_dashed(&mut buffer, width, outline, rect, scale);
                }
            }
            let frame = render::draw_frame(&mut buffer, width, rect, &label, scale);
            drawn = render::union(drawn, Some(frame));
        }
        if let Some(bar) = toolbar {
            let look = Look {
                tool: style.tool,
                color: style.color,
                fill: style.fill,
                hover: self.hover,
                picking,
                cropping: self.cropping,
            };
            drawn = render::union(drawn, bar.draw(&mut buffer, width, &look));
        }
        if let Some(status) = status {
            drawn = render::union(drawn, draw_status(&mut buffer, width, status, scale));
        }
        if let Some((grid, text)) = &magnifier {
            let at = draw_magnifier(&mut buffer, width, self.cursor, grid, text, scale);
            drawn = render::union(drawn, at);
        }
        let damage = restored
            .and_then(|area| render::union(Some(area), drawn))
            .and_then(|area| render::clip(area, width, self.frame.height as i32));
        // A 1×1 stand-in when nothing at all is drawn, so the next frame can be partial too.
        self.drawn = Some(drawn.unwrap_or(Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        }));
        let presented = match (restored, damage) {
            (Some(_), Some(d)) => buffer.present_with_damage(&[softbuffer::Rect {
                x: d.x as u32,
                y: d.y as u32,
                width: NonZeroU32::new(d.w as u32).unwrap_or(NonZeroU32::MIN),
                height: NonZeroU32::new(d.h as u32).unwrap_or(NonZeroU32::MIN),
            }]),
            _ => buffer.present(),
        };
        if let Err(err) = presented {
            log::error!("failed to present the overlay: {err}");
            self.drawn = None;
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
        if let Some(entry) = &self.size_entry {
            return format!("{}_", entry.replace('x', " × "));
        }
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
        let backdrop = Backdrop {
            image: &self.image,
            x: rect.x,
            y: rect.y,
            scale: self.image.width() as f32 / self.frame.width.max(1) as f32,
        };
        annotate::draw_all(
            self.canvas.shapes(),
            &mut pixmap,
            transform,
            Some(&backdrop),
        );
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
            _ if self.cropping => CursorIcon::Crosshair,
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
        self.selected = None;
        if self.is_typing() {
            self.live = None;
        } else {
            self.canvas.undo();
        }
        self.request_redraw();
    }

    fn redo(&mut self) {
        self.selected = None;
        self.commit_text();
        self.canvas.redo();
        self.request_redraw();
    }

    /// The crop rectangle from `from` to the cursor, cut to the selection.
    fn crop_rect(&self, from: Point) -> Option<Rect> {
        let sel = self.selection.rect()?;
        let (to, from) = (self.cursor, from);
        let left = from.x.min(to.x).max(sel.x);
        let top = from.y.min(to.y).max(sel.y);
        let right = from.x.max(to.x).min(sel.right());
        let bottom = from.y.max(to.y).min(sel.bottom());
        (right > left && bottom > top).then_some(Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        })
    }

    /// Moves the selection by `dx`, `dy` window pixels, or with `resize`
    /// moves its right and bottom edges instead.
    fn nudge(&mut self, dx: i32, dy: i32, resize: bool) {
        let Some(rect) = self.selection.rect() else {
            return;
        };
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let next = if resize {
            Rect {
                w: (rect.w + dx).max(1),
                h: (rect.h + dy).max(1),
                ..rect
            }
        } else {
            Rect {
                x: (rect.x + dx).clamp(0, (w - rect.w).max(0)),
                y: (rect.y + dy).clamp(0, (h - rect.h).max(0)),
                ..rect
            }
        };
        self.selection.set(next);
        self.request_redraw();
    }

    /// Takes a key while the selection's size is typed: digits, `x` between
    /// width and height, Backspace, Enter to apply and Esc to give up.
    fn type_size(&mut self, code: KeyCode) {
        let Some(entry) = self.size_entry.as_mut() else {
            return;
        };
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let entry = self.size_entry.take().unwrap_or_default();
                self.apply_size(&entry);
            }
            KeyCode::Escape => self.size_entry = None,
            KeyCode::Backspace => {
                entry.pop();
                if entry.is_empty() {
                    self.size_entry = None;
                }
            }
            KeyCode::KeyX | KeyCode::NumpadMultiply | KeyCode::Space | KeyCode::Tab => {
                if !entry.contains('x') {
                    entry.push('x');
                }
            }
            _ => {
                if let Some(d) = digit(code)
                    && entry.len() < 11
                {
                    entry.push(d);
                }
            }
        }
    }

    /// Resizes the selection to a typed `width x height` in image pixels,
    /// keeping its top-left corner where it fits. A missing side stays as it is.
    fn apply_size(&mut self, entry: &str) {
        let Some(rect) = self.selection.rect() else {
            return;
        };
        let Some((_, _, iw, ih)) = self.to_image(rect) else {
            return;
        };
        let (w, h) = parse_size(entry, (iw, ih));
        let (img_w, img_h) = self.image.dimensions();
        let (fw, fh) = (self.frame.width as i32, self.frame.height as i32);
        let to_window = |v: u32, image: u32, frame: i32| {
            ((v as f64 * frame as f64 / image.max(1) as f64).round() as i32).clamp(1, frame)
        };
        let (w, h) = (to_window(w, img_w, fw), to_window(h, img_h, fh));
        self.selection.set(Rect {
            x: rect.x.min(fw - w).max(0),
            y: rect.y.min(fh - h).max(0),
            w,
            h,
        });
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

    /// The screenshot's pixels in an `n` × `n` square centered under `p`, row by row.
    fn pixels_around(&self, p: Point, n: i32) -> Vec<u32> {
        let (iw, ih) = self.image.dimensions();
        let cx = (p.x.max(0) as i64 * iw as i64 / self.frame.width.max(1) as i64) as i32;
        let cy = (p.y.max(0) as i64 * ih as i64 / self.frame.height.max(1) as i64) as i32;
        let mut out = Vec::with_capacity((n * n) as usize);
        for dy in -(n / 2)..=(n / 2) {
            for dx in -(n / 2)..=(n / 2) {
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= iw as i32 || y >= ih as i32 {
                    out.push(0x000000);
                    continue;
                }
                let [r, g, b, _] = self.image.get_pixel(x as u32, y as u32).0;
                out.push((r as u32) << 16 | (g as u32) << 8 | b as u32);
            }
        }
        out
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

    /// Starts a scrolling screenshot or a recording of the selection, with
    /// the panel below it, or above when there is no room below.
    fn scroll(&mut self, mode: crate::scroll::Mode) -> Option<Outcome> {
        let rect = self.selection.rect().or(self.snap)?;
        let (x, y, w, h) = self.to_image(rect)?;
        let gap = (8.0 * self.window.scale_factor()) as i32;
        let panel_h = (60.0 * self.window.scale_factor()) as i32;
        let below = rect.bottom() + gap;
        let top = if below + panel_h <= self.frame.height as i32 {
            below
        } else {
            (rect.y - gap - panel_h).max(0)
        };
        Some(Outcome::Scroll {
            region: crate::scroll::Region {
                monitor: self.shot_origin,
                scale: self.shot_scale,
                x,
                y,
                w,
                h,
            },
            panel: PhysicalPosition::new(self.origin.x + rect.x, self.origin.y + top),
            mode,
        })
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
fn draw_status(buf: &mut [u32], width: i32, text: &str, scale: f64) -> Option<Rect> {
    let s = scale as f32;
    let size = 14.0 * s;
    let (tw, th) = crate::font::measure(text, size);
    let (pad_x, pad_y) = (14.0 * s, 9.0 * s);
    let w = (tw + 2.0 * pad_x).ceil() as i32;
    let h = (th + 2.0 * pad_y).ceil() as i32;
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32);
    crate::paint::fill_round(&mut pixmap, full, 8.0 * s, 0x111113, 0.94);
    crate::font::draw(&mut pixmap, text, pad_x, pad_y, size, 0xFFFFFF, 1.0);
    let x = ((width - w) / 2).max(0);
    render::blend_pixmap(buf, width, &pixmap, x, (24.0 * s) as i32)
}

/// Pixels around the cursor, zoomed, with the color under it as text.
fn draw_magnifier(
    buf: &mut [u32],
    width: i32,
    cursor: Point,
    grid: &[u32],
    text: &str,
    scale: f64,
) -> Option<Rect> {
    let s = scale as f32;
    let n = (grid.len() as f64).sqrt() as i32;
    let cell = (9.0 * s).round().max(1.0) as i32;
    let pad = (4.0 * s).round() as i32;
    let size = 12.5 * s;
    let (tw, th) = crate::font::measure(text, size);
    let zoom = n * cell;
    let w = (zoom + 2 * pad).max((tw + 2.0 * pad as f32).ceil() as i32);
    let h = zoom + 2 * pad + (th + 2.0 * pad as f32).ceil() as i32;
    let height = buf.len() as i32 / width.max(1);
    let gap = (20.0 * s) as i32;
    let mut x = cursor.x + gap;
    let mut y = cursor.y + gap;
    if x + w > width {
        x = cursor.x - gap - w;
    }
    if y + h > height {
        y = cursor.y - gap - h;
    }
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32);
    crate::paint::fill_round(&mut pixmap, full, 8.0 * s, 0x111113, 0.96);
    let left = (w - zoom) / 2;
    for (i, &c) in grid.iter().enumerate() {
        let (col, row) = (i as i32 % n, i as i32 / n);
        let r = tiny_skia::Rect::from_xywh(
            (left + col * cell) as f32,
            (pad + row * cell) as f32,
            cell as f32,
            cell as f32,
        );
        crate::paint::fill_round(&mut pixmap, r, 0.0, c, 1.0);
    }
    // Ring the pixel that a click takes, in a color that stands out on it.
    let center = grid[grid.len() / 2];
    let [r, g, b] = crate::paint::rgb(center);
    let ring = if (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 > 128 {
        0x000000
    } else {
        0xFFFFFF
    };
    let (cx, cy) = (left + (n / 2) * cell, pad + (n / 2) * cell);
    let t = s.max(1.0);
    for (rx, ry, rw, rh) in [
        (cx as f32 - t, cy as f32 - t, cell as f32 + 2.0 * t, t),
        (cx as f32 - t, (cy + cell) as f32, cell as f32 + 2.0 * t, t),
        (cx as f32 - t, cy as f32, t, cell as f32),
        ((cx + cell) as f32, cy as f32, t, cell as f32),
    ] {
        crate::paint::fill_round(
            &mut pixmap,
            tiny_skia::Rect::from_xywh(rx, ry, rw, rh),
            0.0,
            ring,
            1.0,
        );
    }
    let ty = (zoom + 2 * pad) as f32 + ((h - zoom - 2 * pad) as f32 - th) / 2.0 - pad as f32 / 2.0;
    crate::font::draw(
        &mut pixmap,
        text,
        (w as f32 - tw) / 2.0,
        ty,
        size,
        0xFFFFFF,
        1.0,
    );
    render::blend_pixmap(buf, width, &pixmap, x, y)
}

/// The digit a key types, on the main row or the number pad.
fn digit(code: KeyCode) -> Option<char> {
    let name = format!("{code:?}");
    let d = name
        .strip_prefix("Digit")
        .or_else(|| name.strip_prefix("Numpad"))?;
    let mut chars = d.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_digit() => Some(c),
        _ => None,
    }
}

/// A typed `width x height`; a side left out or zero keeps its `current` value.
fn parse_size(entry: &str, current: (u32, u32)) -> (u32, u32) {
    let (w, h) = entry.split_once('x').unwrap_or((entry, ""));
    let side = |s: &str, keep: u32| s.parse().ok().filter(|&v| v > 0).unwrap_or(keep);
    (side(w, current.0), side(h, current.1))
}

/// The `global_hotkey` key for a winit key; both use the W3C key names.
fn hotkey_code(code: KeyCode) -> Option<global_hotkey::hotkey::Code> {
    format!("{code:?}").parse().ok()
}

fn hotkey_modifiers(m: ModifiersState) -> global_hotkey::hotkey::Modifiers {
    use global_hotkey::hotkey::Modifiers;
    let mut out = Modifiers::empty();
    out.set(Modifiers::SHIFT, m.shift_key());
    out.set(Modifiers::CONTROL, m.control_key());
    out.set(Modifiers::ALT, m.alt_key());
    out.set(Modifiers::META, m.super_key());
    out
}

/// Outlines the picked shape's `bounds` with a dashed line, clipped to the selection.
fn draw_dashed(buf: &mut [u32], width: i32, bounds: tiny_skia::Rect, clip: Rect, scale: f64) {
    let pad = (4.0 * scale).round() as i32;
    let dash = (4.0 * scale).round().max(2.0) as i32;
    let left = bounds.left().floor() as i32 - pad;
    let top = bounds.top().floor() as i32 - pad;
    let right = bounds.right().ceil() as i32 + pad;
    let bottom = bounds.bottom().ceil() as i32 + pad;
    let mut put = |x: i32, y: i32, i: i32| {
        if x >= clip.x && x < clip.right() && y >= clip.y && y < clip.bottom() {
            let on = (i / dash) % 2 == 0;
            buf[(y * width + x) as usize] = if on { 0xFFFFFF } else { 0x000000 };
        }
    };
    for x in left..=right {
        put(x, top, x - left);
        put(x, bottom, x - left);
    }
    for y in top..=bottom {
        put(left, y, y - top);
        put(right, y, y - top);
    }
}

/// Draws the shape in progress, clipped to the selection. Text being typed gets a caret.
fn draw_live(buf: &mut [u32], width: i32, rect: Rect, shape: &Shape, typing: bool, scale: f64) {
    let Some(bounds) = shape.bounds() else { return };
    // A spotlight dims the whole selection around its rectangle.
    let bounds = if shape.paints_everywhere() {
        tiny_skia::Rect::from_xywh(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32)
            .unwrap_or(bounds)
    } else {
        bounds
    };
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
    shape.draw(&mut pixmap, transform, None);
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
    // No minimize button: Win+M or Win+D would otherwise shrink the overlay to a
    // bare "Snapt" title bar, since it has no taskbar button to come back from.
    attributes
        .with_skip_taskbar(true)
        .with_enabled_buttons(winit::window::WindowButtons::CLOSE)
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

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn typed_sizes_parse_and_keep_what_is_left_out() {
        assert_eq!(parse_size("800x600", (10, 20)), (800, 600));
        assert_eq!(parse_size("800", (10, 20)), (800, 20));
        assert_eq!(parse_size("800x", (10, 20)), (800, 20));
        assert_eq!(parse_size("x0", (10, 20)), (10, 20));
        assert_eq!(digit(KeyCode::Digit7), Some('7'));
        assert_eq!(digit(KeyCode::Numpad3), Some('3'));
        assert_eq!(digit(KeyCode::NumpadEnter), None);
    }
}
