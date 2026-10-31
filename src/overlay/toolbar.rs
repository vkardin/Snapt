//! The two LightShot-style panels around the selection: drawing tools on the
//! right, actions below, plus the color palette and button tooltips.

use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect as SkRect, Stroke, Transform,
};

use super::annotate::{Tool, rgb};
use super::render::{self, ACCENT};
use super::selection::{Point, Rect};
use crate::font;
use crate::i18n::{T, t};
use crate::paint::fill_round;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Tool(Tool),
    /// Cuts the selection down to a rectangle drawn inside it.
    Crop,
    /// Blurs the e-mails, phones and card numbers found in the selection.
    Redact,
    Color,
    Undo,
    Close,
    Recognize,
    /// Copies what the QR codes in the selection say.
    Qr,
    Search,
    Print,
    Pin,
    Scroll,
    Record,
    Copy,
    Save,
    /// Picks the drawing color from the screen; sits in the palette.
    Eyedropper,
}

const TOOLS: [Button; 16] = [
    Button::Tool(Tool::Pen),
    Button::Tool(Tool::Line),
    Button::Tool(Tool::Arrow),
    Button::Tool(Tool::Rect),
    Button::Tool(Tool::Ellipse),
    Button::Tool(Tool::Marker),
    Button::Tool(Tool::Text),
    Button::Tool(Tool::Number),
    Button::Tool(Tool::Pixelate),
    Button::Tool(Tool::Blur),
    Button::Tool(Tool::Spotlight),
    Button::Redact,
    Button::Crop,
    Button::Color,
    Button::Undo,
    Button::Close,
];
/// The setting that turns `tool` on and off.
pub fn tool_feature(tool: Tool) -> crate::settings::Feature {
    use crate::settings::Feature;
    match tool {
        Tool::Pen => Feature::Pen,
        Tool::Line => Feature::Line,
        Tool::Arrow => Feature::Arrow,
        Tool::Rect => Feature::Rect,
        Tool::Ellipse => Feature::Ellipse,
        Tool::Marker => Feature::Marker,
        Tool::Text => Feature::Text,
        Tool::Number => Feature::Number,
        Tool::Pixelate => Feature::Pixelate,
        Tool::Blur => Feature::Blur,
        Tool::Spotlight => Feature::Spotlight,
    }
}

/// The tool column, without the tools turned off in the settings.
fn tools() -> Vec<Button> {
    use crate::settings::{Feature, enabled};
    TOOLS
        .into_iter()
        .filter(|b| match b {
            Button::Tool(tool) => enabled(tool_feature(*tool)),
            Button::Crop => enabled(Feature::Crop),
            Button::Redact => enabled(Feature::Redact),
            _ => true,
        })
        .collect()
}

/// The action buttons, without the features turned off in the settings.
fn actions() -> Vec<Button> {
    use crate::settings::{Feature, enabled};
    ACTIONS
        .into_iter()
        .filter(|b| match b {
            Button::Recognize => enabled(Feature::Recognize),
            Button::Qr => enabled(Feature::Qr),
            Button::Search => enabled(Feature::Search),
            Button::Print => enabled(Feature::Print),
            Button::Pin => enabled(Feature::Pin),
            Button::Scroll => enabled(Feature::Scroll),
            Button::Record => enabled(Feature::Record),
            _ => true,
        })
        .collect()
}

const ACTIONS: [Button; 9] = [
    Button::Recognize,
    Button::Qr,
    Button::Search,
    Button::Print,
    Button::Pin,
    Button::Scroll,
    Button::Record,
    Button::Copy,
    Button::Save,
];

pub const PALETTE: [u32; 10] = [
    0xFF3B30, 0xFF9500, 0xFFCC00, 0x34C759, 0x32ADE6, 0x007AFF, 0xAF52DE, 0xFF2D78, 0x000000,
    0xFFFFFF,
];

#[cfg(target_os = "macos")]
const MOD: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

impl Button {
    pub fn tooltip(self) -> String {
        let (text, key) = match self {
            Button::Tool(Tool::Pen) => (T::Pen, "P".to_string()),
            Button::Tool(Tool::Line) => (T::Line, "L".into()),
            Button::Tool(Tool::Arrow) => (T::Arrow, "A".into()),
            Button::Tool(Tool::Rect) => (T::Rectangle, "R".into()),
            Button::Tool(Tool::Ellipse) => (T::Ellipse, "E".into()),
            Button::Tool(Tool::Blur) => (T::Blur, "U".into()),
            Button::Tool(Tool::Number) => (T::Number, "N".into()),
            Button::Tool(Tool::Pixelate) => (T::Pixelate, "B".into()),
            Button::Tool(Tool::Marker) => (T::Marker, "M".into()),
            Button::Tool(Tool::Text) => (T::Text, "T".into()),
            Button::Tool(Tool::Spotlight) => (T::Spotlight, "H".into()),
            Button::Crop => (T::Crop, "C".into()),
            Button::Redact => (T::HidePersonal, "D".into()),
            Button::Qr => (T::ReadQr, "Q".into()),
            Button::Color => return t(T::Color).into(),
            Button::Undo => (T::Undo, format!("{MOD}+Z")),
            Button::Close => (T::Close, "Esc".into()),
            Button::Recognize => (T::Recognize, format!("{MOD}+Shift+C")),
            Button::Search => (T::Search, format!("{MOD}+F")),
            Button::Print => (T::Print, format!("{MOD}+P")),
            Button::Pin => (T::Pin, "F3".into()),
            Button::Scroll => (T::Scroll, format!("{MOD}+Shift+S")),
            Button::Record => (T::Record, format!("{MOD}+Shift+R")),
            Button::Copy => (T::Copy, format!("{MOD}+C")),
            Button::Save => (T::Save, format!("{MOD}+S")),
            Button::Eyedropper => (T::Eyedropper, "I".into()),
        };
        format!("{} ({key})", t(text))
    }
}

/// What a point on the toolbars hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Button(Button),
    Swatch(u32),
    /// A point on the color spectrum, with the color under it.
    Spectrum(u32),
    /// Panel background between buttons.
    Panel,
}

/// The placed panels for one selection.
pub struct Toolbar {
    scale: f64,
    panels: Vec<Rect>,
    buttons: Vec<(Button, Rect)>,
    swatches: Vec<(u32, Rect)>,
    spectrum: Option<Rect>,
}

/// What the toolbar shows besides its layout.
pub struct Look {
    pub tool: Option<Tool>,
    pub color: u32,
    /// Rectangles and ellipses are drawn filled.
    pub fill: bool,
    pub hover: Option<Target>,
    /// The eyedropper is waiting for a click.
    pub picking: bool,
    /// The crop tool is on.
    pub cropping: bool,
}

impl Toolbar {
    /// Places the panels around `sel` on a `width` × `height` screen.
    pub fn layout(sel: Rect, width: i32, height: i32, scale: f64, palette: bool) -> Self {
        let px = |v: f64| (v * scale).round() as i32;
        let (button, pad, gap) = (px(30.0), px(3.0), px(6.0));

        // Tools: a column right of the selection, bottom-aligned with it.
        let tools_w = button + 2 * pad;
        let tool_buttons = tools();
        let tools_h = tool_buttons.len() as i32 * button + 2 * pad;
        let tools_x = if sel.right() + gap + tools_w <= width {
            sel.right() + gap
        } else if sel.x - gap - tools_w >= 0 {
            sel.x - gap - tools_w
        } else {
            sel.right() - gap - tools_w
        };
        let tools_y = (sel.bottom() - tools_h).clamp(0, (height - tools_h).max(0));
        let mut tools = Rect {
            x: tools_x.max(0),
            y: tools_y,
            w: tools_w,
            h: tools_h,
        };

        // Actions: a row below the selection, right-aligned with it.
        let action_buttons = actions();
        let actions_w = action_buttons.len() as i32 * button + 2 * pad;
        let actions_h = button + 2 * pad;
        let actions_y = if sel.bottom() + gap + actions_h <= height {
            sel.bottom() + gap
        } else if sel.y - gap - actions_h >= 0 {
            sel.y - gap - actions_h
        } else {
            sel.bottom() - gap - actions_h
        };
        let actions_x = (sel.right() - actions_w).clamp(0, (width - actions_w).max(0));
        let actions = Rect {
            x: actions_x,
            y: actions_y,
            w: actions_w,
            h: actions_h,
        };

        // When both panels end up inside the selection, lift the tools above the actions.
        if overlaps(tools, actions) {
            tools.y = (actions.y - gap - tools_h).max(0);
        }

        let mut buttons = Vec::new();
        for (i, &b) in tool_buttons.iter().enumerate() {
            let r = Rect {
                x: tools.x + pad,
                y: tools.y + pad + i as i32 * button,
                w: button,
                h: button,
            };
            buttons.push((b, r));
        }
        for (i, &b) in action_buttons.iter().enumerate() {
            let r = Rect {
                x: actions.x + pad + i as i32 * button,
                y: actions.y + pad,
                w: button,
                h: button,
            };
            buttons.push((b, r));
        }

        let mut panels = vec![tools, actions];
        let mut swatches = Vec::new();
        let mut spectrum = None;
        if palette {
            // Two rows of swatches next to the color button, on the side with room,
            // then a spectrum for any other color and a row with the eyedropper.
            let swatch = px(24.0);
            let cols = PALETTE.len() as i32 / 2;
            let spectrum_h = px(72.0);
            let w = cols * swatch + 2 * pad;
            let h = 3 * swatch + spectrum_h + 2 * gap / 2 + 2 * pad;
            let color_button = buttons.iter().find(|(b, _)| *b == Button::Color).unwrap().1;
            let x = if tools.x - gap - w >= 0 {
                tools.x - gap - w
            } else {
                tools.right() + gap
            };
            let y = (color_button.y + color_button.h / 2 - h / 2).clamp(0, (height - h).max(0));
            let panel = Rect { x, y, w, h };
            for (i, &c) in PALETTE.iter().enumerate() {
                let (col, row) = (i as i32 % cols, i as i32 / cols);
                let r = Rect {
                    x: x + pad + col * swatch,
                    y: y + pad + row * swatch,
                    w: swatch,
                    h: swatch,
                };
                swatches.push((c, r));
            }
            let spectrum_rect = Rect {
                x: x + pad + px(3.0),
                y: y + pad + 2 * swatch + gap / 2,
                w: cols * swatch - px(6.0),
                h: spectrum_h,
            };
            buttons.push((
                Button::Eyedropper,
                Rect {
                    x: x + pad,
                    y: spectrum_rect.bottom() + gap / 2,
                    w: swatch,
                    h: swatch,
                },
            ));
            spectrum = Some(spectrum_rect);
            panels.push(panel);
        }

        Self {
            scale,
            panels,
            buttons,
            swatches,
            spectrum,
        }
    }

    pub fn hit(&self, p: Point) -> Option<Target> {
        if let Some((c, _)) = self.swatches.iter().find(|(_, r)| r.contains(p)) {
            return Some(Target::Swatch(*c));
        }
        if let Some(r) = self.spectrum.filter(|r| r.contains(p)) {
            let fx = (p.x - r.x) as f32 / (r.w - 1).max(1) as f32;
            let fy = (p.y - r.y) as f32 / (r.h - 1).max(1) as f32;
            return Some(Target::Spectrum(spectrum_color(fx, fy)));
        }
        if let Some((b, _)) = self.buttons.iter().find(|(_, r)| r.contains(p)) {
            return Some(Target::Button(*b));
        }
        self.panels
            .iter()
            .any(|r| r.contains(p))
            .then_some(Target::Panel)
    }

    /// Draws the panels into a `0RGB` buffer `width` pixels wide.
    /// Draws the panels and returns where.
    pub fn draw(&self, buf: &mut [u32], width: i32, look: &Look) -> Option<Rect> {
        let s = self.scale as f32;
        let mut drawn = None;
        for (n, panel) in self.panels.iter().enumerate() {
            let Some(mut pixmap) = Pixmap::new(panel.w as u32, panel.h as u32) else {
                continue;
            };
            let local = |r: Rect| {
                SkRect::from_xywh(
                    (r.x - panel.x) as f32,
                    (r.y - panel.y) as f32,
                    r.w as f32,
                    r.h as f32,
                )
            };
            fill_round(
                &mut pixmap,
                local(Rect {
                    x: panel.x,
                    y: panel.y,
                    ..*panel
                }),
                7.0 * s,
                0x202024,
                0.96,
            );

            if n == 2 {
                for &(c, r) in &self.swatches {
                    let inner = inset(local(r), 3.0 * s);
                    let selected = c == look.color;
                    let hovered = look.hover == Some(Target::Swatch(c));
                    if selected || hovered {
                        let ring = if selected { 0xFFFFFF } else { 0x70707A };
                        fill_round(&mut pixmap, inset(local(r), 1.0 * s), 6.0 * s, ring, 1.0);
                    }
                    fill_round(&mut pixmap, inset(inner, 0.5 * s), 4.0 * s, c, 1.0);
                }
                if let Some(r) = self.spectrum {
                    draw_spectrum(&mut pixmap, r.x - panel.x, r.y - panel.y, r.w, r.h);
                }
                if let Some(&(b, r)) = self.buttons.iter().find(|(b, _)| *b == Button::Eyedropper) {
                    let hovered = look.hover == Some(Target::Button(b));
                    if look.picking || hovered {
                        let bg = if look.picking { ACCENT } else { 0x3A3A42 };
                        fill_round(&mut pixmap, inset(local(r), 1.5 * s), 5.0 * s, bg, 1.0);
                    }
                    if let Some(at) = local(r) {
                        let size = 16.0 * s;
                        let transform = Transform::from_scale(s, s).post_translate(
                            at.x() + (at.width() - size) / 2.0,
                            at.y() + (at.height() - size) / 2.0,
                        );
                        icon(&mut pixmap, b, look, transform);
                        // The current color as text, to read or retype elsewhere.
                        let hex = hex(look.color);
                        let size = 12.0 * s;
                        let th = font::line_height(size);
                        let tx = at.right() + 6.0 * s;
                        let ty = at.y() + (at.height() - th) / 2.0;
                        let dot = SkRect::from_xywh(
                            tx,
                            at.y() + at.height() / 2.0 - 5.0 * s,
                            10.0 * s,
                            10.0 * s,
                        );
                        fill_round(&mut pixmap, dot, 3.0 * s, look.color, 1.0);
                        font::draw(&mut pixmap, &hex, tx + 16.0 * s, ty, size, 0xD4D4D8, 1.0);
                    }
                }
            } else {
                for &(b, r) in self.buttons.iter().filter(|(_, r)| overlaps(*r, *panel)) {
                    let active = matches!(b, Button::Tool(t) if Some(t) == look.tool)
                        || (b == Button::Crop && look.cropping);
                    let hovered = look.hover == Some(Target::Button(b));
                    if active {
                        fill_round(&mut pixmap, inset(local(r), 1.5 * s), 5.0 * s, ACCENT, 1.0);
                    } else if hovered {
                        fill_round(
                            &mut pixmap,
                            inset(local(r), 1.5 * s),
                            5.0 * s,
                            0x3A3A42,
                            1.0,
                        );
                    }
                    // Icons are drawn on a 16 × 16 grid centered in the button.
                    let size = 16.0 * s;
                    let Some(at) = local(r) else { continue };
                    let transform = Transform::from_scale(s, s).post_translate(
                        at.x() + (at.width() - size) / 2.0,
                        at.y() + (at.height() - size) / 2.0,
                    );
                    icon(&mut pixmap, b, look, transform);
                }
            }
            let at = render::blend_pixmap(buf, width, &pixmap, panel.x, panel.y);
            drawn = render::union(drawn, at);
        }

        if let Some(Target::Button(b)) = look.hover {
            drawn = render::union(drawn, self.draw_tooltip(buf, width, b));
        }
        drawn
    }

    fn draw_tooltip(&self, buf: &mut [u32], width: i32, button: Button) -> Option<Rect> {
        let &(_, r) = self.buttons.iter().find(|(b, _)| *b == button)?;
        let height = buf.len() as i32 / width.max(1);
        let s = self.scale as f32;
        let text = button.tooltip();
        let size = 12.5 * s;
        let (tw, th) = font::measure(&text, size);
        let (pad_x, pad_y) = (8.0 * s, 4.0 * s);
        let (w, h) = (
            (tw + 2.0 * pad_x).ceil() as i32,
            (th + 2.0 * pad_y).ceil() as i32,
        );
        let gap = (6.0 * s) as i32;
        let in_column = TOOLS.contains(&button);
        let (x, y) = if in_column {
            let left = r.x - (3.0 * s) as i32 - gap - w;
            let x = if left >= 0 {
                left
            } else {
                r.right() + (3.0 * s) as i32 + gap
            };
            (x, r.y + (r.h - h) / 2)
        } else {
            let below = r.bottom() + (3.0 * s) as i32 + gap;
            let y = if below + h <= height {
                below
            } else {
                r.y - (3.0 * s) as i32 - gap - h
            };
            (r.x + (r.w - w) / 2, y)
        };
        let x = x.clamp(0, (width - w).max(0));
        let y = y.clamp(0, (height - h).max(0));
        let mut pixmap = Pixmap::new(w as u32, h as u32)?;
        fill_round(
            &mut pixmap,
            SkRect::from_xywh(0.0, 0.0, w as f32, h as f32),
            5.0 * s,
            0x111113,
            0.94,
        );
        font::draw(&mut pixmap, &text, pad_x, pad_y, size, 0xFFFFFF, 1.0);
        render::blend_pixmap(buf, width, &pixmap, x, y)
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

fn inset(r: Option<SkRect>, by: f32) -> Option<SkRect> {
    let r = r?;
    SkRect::from_ltrb(r.left() + by, r.top() + by, r.right() - by, r.bottom() - by)
}

/// Draws a button's icon on a 16 × 16 grid mapped through `transform`.
fn icon(pixmap: &mut Pixmap, button: Button, look: &Look, transform: Transform) {
    let color = look.color;
    let mut paint = Paint::default();
    paint.set_color_rgba8(0xF4, 0xF4, 0xF6, 255);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: 1.6,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let mut p = PathBuilder::new();
    match button {
        Button::Tool(Tool::Pen) => {
            p.move_to(2.5, 13.5);
            p.line_to(3.3, 10.2);
            p.line_to(11.0, 2.5);
            p.line_to(13.5, 5.0);
            p.line_to(5.8, 12.7);
            p.close();
            p.move_to(9.3, 4.2);
            p.line_to(11.8, 6.7);
        }
        Button::Tool(Tool::Line) => {
            p.move_to(3.0, 13.0);
            p.line_to(13.0, 3.0);
        }
        Button::Tool(Tool::Arrow) => {
            p.move_to(3.0, 13.0);
            p.line_to(13.0, 3.0);
            p.move_to(6.5, 3.0);
            p.line_to(13.0, 3.0);
            p.line_to(13.0, 9.5);
        }
        Button::Tool(Tool::Rect) => p.push_rect(SkRect::from_xywh(2.5, 3.5, 11.0, 9.0).unwrap()),
        Button::Tool(Tool::Ellipse) => p.push_oval(SkRect::from_xywh(2.0, 3.5, 12.0, 9.0).unwrap()),
        Button::Tool(Tool::Blur) => {
            // A drop with soft rings around it.
            p.move_to(8.0, 2.5);
            p.cubic_to(8.0, 2.5, 4.0, 7.0, 4.0, 9.5);
            p.cubic_to(4.0, 11.7, 5.8, 13.5, 8.0, 13.5);
            p.cubic_to(10.2, 13.5, 12.0, 11.7, 12.0, 9.5);
            p.cubic_to(12.0, 7.0, 8.0, 2.5, 8.0, 2.5);
            p.close();
            let mut faint = paint.clone();
            faint.set_color_rgba8(0xF4, 0xF4, 0xF6, 90);
            if let Some(dots) = PathBuilder::from_circle(8.0, 9.5, 1.6) {
                pixmap.fill_path(&dots, &faint, FillRule::Winding, transform, None);
            }
        }
        Button::Tool(Tool::Number) => {
            // A "1" in a circle.
            p.push_circle(8.0, 8.0, 6.0);
            p.move_to(6.6, 6.2);
            p.line_to(8.4, 4.9);
            p.line_to(8.4, 11.2);
        }
        Button::Tool(Tool::Pixelate) => {
            // A checkerboard of blocks.
            p.push_rect(SkRect::from_xywh(2.5, 2.5, 11.0, 11.0).unwrap());
            for (x, y) in [(2.5, 2.5), (8.0, 8.0)] {
                if let Some(block) = SkRect::from_xywh(x, y, 5.5, 5.5) {
                    let path = PathBuilder::from_rect(block);
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                }
            }
        }
        Button::Tool(Tool::Marker) => {
            let mut wide = PathBuilder::new();
            wide.move_to(4.0, 11.0);
            wide.line_to(12.0, 3.0);
            let mut faint = paint.clone();
            faint.set_color_rgba8(0xF4, 0xF4, 0xF6, 110);
            let thick = Stroke {
                width: 5.0,
                line_cap: LineCap::Square,
                ..stroke.clone()
            };
            if let Some(path) = wide.finish() {
                pixmap.stroke_path(&path, &faint, &thick, transform, None);
            }
            p.move_to(2.0, 14.0);
            p.line_to(14.0, 14.0);
        }
        Button::Tool(Tool::Text) => {
            p.move_to(3.0, 3.5);
            p.line_to(13.0, 3.5);
            p.move_to(8.0, 3.5);
            p.line_to(8.0, 13.5);
            p.move_to(6.0, 13.5);
            p.line_to(10.0, 13.5);
        }
        Button::Tool(Tool::Spotlight) => {
            // A bright square in a dimmed frame.
            let mut faint = paint.clone();
            faint.set_color_rgba8(0xF4, 0xF4, 0xF6, 80);
            let mut frame = PathBuilder::new();
            frame.push_rect(SkRect::from_xywh(1.5, 1.5, 13.0, 13.0).unwrap());
            frame.push_rect(SkRect::from_xywh(5.0, 5.0, 6.0, 6.0).unwrap());
            if let Some(path) = frame.finish() {
                pixmap.fill_path(&path, &faint, FillRule::EvenOdd, transform, None);
            }
            p.push_rect(SkRect::from_xywh(5.0, 5.0, 6.0, 6.0).unwrap());
        }
        Button::Redact => {
            // An eye, struck through.
            p.move_to(1.5, 8.0);
            p.cubic_to(3.5, 4.5, 5.6, 3.5, 8.0, 3.5);
            p.cubic_to(10.4, 3.5, 12.5, 4.5, 14.5, 8.0);
            p.cubic_to(12.5, 11.5, 10.4, 12.5, 8.0, 12.5);
            p.cubic_to(5.6, 12.5, 3.5, 11.5, 1.5, 8.0);
            p.close();
            p.push_circle(8.0, 8.0, 2.0);
            p.move_to(2.5, 14.0);
            p.line_to(13.5, 2.0);
        }
        Button::Qr => {
            // Three finder squares and a few modules.
            for (x, y) in [(2.0, 2.0), (10.0, 2.0), (2.0, 10.0)] {
                p.push_rect(SkRect::from_xywh(x, y, 4.0, 4.0).unwrap());
            }
            for (x, y) in [(9.6, 9.6), (12.0, 12.0)] {
                if let Some(module) = SkRect::from_xywh(x, y, 2.6, 2.6) {
                    let path = PathBuilder::from_rect(module);
                    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
                }
            }
        }
        Button::Crop => {
            // Two corner brackets that cross.
            p.move_to(4.5, 1.5);
            p.line_to(4.5, 11.5);
            p.line_to(14.5, 11.5);
            p.move_to(1.5, 4.5);
            p.line_to(11.5, 4.5);
            p.line_to(11.5, 14.5);
        }
        Button::Color => {
            let [r, g, b] = rgb(color);
            let mut fill = paint.clone();
            fill.set_color_rgba8(r, g, b, 255);
            if let Some(dot) = PathBuilder::from_circle(8.0, 8.0, 5.5) {
                pixmap.fill_path(&dot, &fill, FillRule::Winding, transform, None);
            }
            p.push_circle(8.0, 8.0, 6.3);
        }
        Button::Undo => {
            p.move_to(5.5, 3.5);
            p.line_to(2.5, 6.5);
            p.line_to(5.5, 9.5);
            p.move_to(2.5, 6.5);
            p.line_to(10.0, 6.5);
            p.cubic_to(12.5, 6.5, 13.8, 8.2, 13.8, 10.1);
            p.cubic_to(13.8, 12.0, 12.5, 13.5, 10.0, 13.5);
            p.line_to(7.0, 13.5);
        }
        Button::Close => {
            p.move_to(4.0, 4.0);
            p.line_to(12.0, 12.0);
            p.move_to(12.0, 4.0);
            p.line_to(4.0, 12.0);
        }
        Button::Recognize => {
            // A "T" in the corners of a frame.
            for (x, y, dx, dy) in [
                (2.5, 2.5, 1.0, 1.0),
                (13.5, 2.5, -1.0, 1.0),
                (2.5, 13.5, 1.0, -1.0),
                (13.5, 13.5, -1.0, -1.0),
            ] {
                p.move_to(x, y + 3.0 * dy);
                p.line_to(x, y);
                p.line_to(x + 3.0 * dx, y);
            }
            p.move_to(5.5, 5.5);
            p.line_to(10.5, 5.5);
            p.move_to(8.0, 5.5);
            p.line_to(8.0, 10.8);
        }
        Button::Search => {
            p.push_circle(7.0, 7.0, 4.5);
            p.move_to(10.3, 10.3);
            p.line_to(13.8, 13.8);
        }
        Button::Print => {
            p.move_to(4.5, 6.0);
            p.line_to(4.5, 2.5);
            p.line_to(11.5, 2.5);
            p.line_to(11.5, 6.0);
            p.move_to(4.5, 11.5);
            p.line_to(2.5, 11.5);
            p.line_to(2.5, 6.0);
            p.line_to(13.5, 6.0);
            p.line_to(13.5, 11.5);
            p.line_to(11.5, 11.5);
            p.push_rect(SkRect::from_xywh(4.5, 9.0, 7.0, 5.0).unwrap());
        }
        Button::Pin => {
            // A push pin.
            p.move_to(5.5, 2.5);
            p.line_to(10.5, 2.5);
            p.move_to(6.5, 2.5);
            p.line_to(6.5, 6.5);
            p.line_to(4.0, 9.5);
            p.line_to(12.0, 9.5);
            p.line_to(9.5, 6.5);
            p.line_to(9.5, 2.5);
            p.move_to(8.0, 9.5);
            p.line_to(8.0, 14.0);
        }
        Button::Scroll => {
            // A page with arrows up and down.
            p.push_rect(SkRect::from_xywh(4.5, 2.5, 7.0, 11.0).unwrap());
            p.move_to(8.0, 5.0);
            p.line_to(8.0, 11.0);
            p.move_to(6.5, 6.5);
            p.line_to(8.0, 5.0);
            p.line_to(9.5, 6.5);
            p.move_to(6.5, 9.5);
            p.line_to(8.0, 11.0);
            p.line_to(9.5, 9.5);
        }
        Button::Record => {
            // A camera: body and lens.
            p.push_rect(SkRect::from_xywh(2.0, 4.5, 8.5, 7.0).unwrap());
            p.move_to(10.5, 7.0);
            p.line_to(14.0, 5.0);
            p.line_to(14.0, 11.0);
            p.line_to(10.5, 9.0);
        }
        Button::Copy => {
            p.push_rect(SkRect::from_xywh(5.5, 5.5, 8.0, 8.5).unwrap());
            p.move_to(10.5, 2.5);
            p.line_to(2.5, 2.5);
            p.line_to(2.5, 11.0);
        }
        Button::Eyedropper => {
            p.move_to(3.0, 13.0);
            p.line_to(9.0, 7.0);
            p.move_to(7.0, 5.0);
            p.line_to(11.0, 9.0);
            p.push_circle(11.6, 4.4, 2.1);
            p.move_to(3.0, 13.0);
            p.line_to(2.5, 13.5);
        }
        Button::Save => {
            p.move_to(8.0, 2.0);
            p.line_to(8.0, 10.5);
            p.move_to(4.5, 7.0);
            p.line_to(8.0, 10.5);
            p.line_to(11.5, 7.0);
            p.move_to(2.5, 13.5);
            p.line_to(13.5, 13.5);
        }
    }
    if let Some(path) = p.finish() {
        let filled = matches!(button, Button::Tool(Tool::Rect | Tool::Ellipse)) && look.fill;
        if filled {
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

/// `#RRGGBB` for `0xRRGGBB`.
pub fn hex(color: u32) -> String {
    format!("#{:06X}", color & 0xFFFFFF)
}

/// The spectrum's color at `fx`, `fy` in 0..=1: hue across, from white at the
/// top through the full hue in the middle to black at the bottom.
pub fn spectrum_color(fx: f32, fy: f32) -> u32 {
    let h = fx.clamp(0.0, 1.0) * 6.0;
    let x = 1.0 - ((h % 2.0) - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let fy = fy.clamp(0.0, 1.0);
    let mix = |c: f32| {
        let v = if fy < 0.5 {
            let t = fy * 2.0;
            1.0 + (c - 1.0) * t
        } else {
            c * (1.0 - (fy - 0.5) * 2.0)
        };
        (v * 255.0).round() as u32
    };
    mix(r) << 16 | mix(g) << 8 | mix(b)
}

/// Paints the spectrum pixel by pixel into an opaque area of `pixmap`.
fn draw_spectrum(pixmap: &mut Pixmap, x: i32, y: i32, w: i32, h: i32) {
    let pw = pixmap.width() as i32;
    let ph = pixmap.height() as i32;
    let data = pixmap.data_mut();
    for row in 0..h {
        for col in 0..w {
            let (px, py) = (x + col, y + row);
            if px < 0 || py < 0 || px >= pw || py >= ph {
                continue;
            }
            let c = spectrum_color(
                col as f32 / (w - 1).max(1) as f32,
                row as f32 / (h - 1).max(1) as f32,
            );
            let i = ((py * pw + px) * 4) as usize;
            let [r, g, b] = rgb(c);
            data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn panels_sit_outside_the_selection_when_there_is_room() {
        let bar = Toolbar::layout(sel(100, 100, 400, 400), 1920, 1080, 1.0, false);
        let (tools, actions) = (bar.panels[0], bar.panels[1]);
        assert!(tools.x >= 500 && tools.bottom() == 500);
        assert!(actions.y >= 500 && actions.right() == 500);
        assert_eq!(
            bar.hit(Point::new(tools.x + 5, tools.y + 5)),
            Some(Target::Button(Button::Tool(Tool::Pen)))
        );
    }

    #[test]
    fn panels_move_inside_a_full_screen_selection_without_overlapping() {
        let bar = Toolbar::layout(sel(0, 0, 1920, 1080), 1920, 1080, 2.0, true);
        let screen = sel(0, 0, 1920, 1080);
        for panel in &bar.panels {
            assert!(
                panel.x >= 0 && panel.y >= 0 && panel.right() <= 1920 && panel.bottom() <= 1080
            );
            assert!(overlaps(*panel, screen));
        }
        assert!(!overlaps(bar.panels[0], bar.panels[1]));
        assert_eq!(bar.swatches.len(), PALETTE.len());
        let spectrum = bar.spectrum.unwrap();
        assert!(spectrum.bottom() <= 1080 && spectrum.right() <= 1920);
    }

    #[test]
    fn the_spectrum_runs_from_white_through_hues_to_black() {
        assert_eq!(spectrum_color(0.0, 0.0), 0xFFFFFF);
        assert_eq!(spectrum_color(0.0, 0.5), 0xFF0000);
        assert_eq!(spectrum_color(1.0 / 3.0, 0.5), 0x00FF00);
        assert_eq!(spectrum_color(2.0 / 3.0, 0.5), 0x0000FF);
        assert_eq!(spectrum_color(0.5, 1.0), 0x000000);
        assert_eq!(hex(0x00FF7F), "#00FF7F");
    }

    #[test]
    fn toolbars_draw_into_the_buffer() {
        let bar = Toolbar::layout(sel(50, 50, 200, 200), 600, 500, 1.0, true);
        let mut buf = vec![0xFFFFFFu32; 600 * 500];
        let look = Look {
            tool: Some(Tool::Arrow),
            color: PALETTE[0],
            fill: false,
            hover: Some(Target::Button(Button::Save)),
            picking: false,
            cropping: false,
        };
        bar.draw(&mut buf, 600, &look);
        let tools = bar.panels[0];
        let middle = buf[((tools.y + tools.h / 2) * 600 + tools.x + 2) as usize];
        assert!(
            middle & 0xFF < 0x40,
            "the panel background is dark, got {middle:06X}"
        );
    }
}
