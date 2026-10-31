//! The tray menu Snapt draws itself on Windows, and the icons of every tray menu.
//!
//! Windows' own tray menus are plain grey lists, so there a click on the tray
//! icon opens this borderless window instead: icons, the shortcuts, the delays
//! as buttons and the update right in the menu, in the system's light or dark
//! theme. macOS and Linux keep their system menus, with the same icons: a Mac
//! menu bar item is expected to open a real system menu, and on Linux the
//! desktop shell draws tray menus itself (on Wayland an app can't even place
//! a window next to the tray).

// Elsewhere only the icons are used; the tests draw the whole menu everywhere.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint,
    Rect as SkRect, Stroke, Transform,
};

use crate::{
    font,
    i18n::{T, t},
    icon,
    paint::fill_round,
    settings_window::UpdateStatus,
    tray::{Command, DELAYS},
};

/// The pictures next to the menu items, drawn on a 16 by 16 grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Capture,
    Picker,
    ScrollPage,
    FullScreen,
    Repeat,
    Timer,
    History,
    Folder,
    Settings,
    Update,
    Download,
    Check,
    About,
    Quit,
    Back,
    Image,
    Clear,
}

/// Renders `glyph` as straight RGBA pixels, `size` by `size`, for the system menus.
pub fn glyph_rgba(glyph: Glyph, size: u32, color: u32) -> Vec<u8> {
    let Some(mut pixmap) = Pixmap::new(size, size) else {
        return Vec::new();
    };
    let scale = size as f32 / 16.0;
    draw_glyph(
        &mut pixmap,
        glyph,
        color,
        Transform::from_scale(scale, scale),
    );
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

/// The violet the system menus draw their icons in; it reads on light and dark menus.
pub const NATIVE_ICON: u32 = 0x8B5CF6;

/// The update item: what it says and does, and whether it goes at the top.
pub struct UpdateRow {
    pub glyph: Glyph,
    pub text: String,
    /// `None` while there is nothing to do but wait.
    pub command: Option<Command>,
    /// A new version is out: the item leads the menu.
    pub top: bool,
}

pub fn update_row(status: &UpdateStatus) -> UpdateRow {
    let row = |glyph, text: &str, command, top| UpdateRow {
        glyph,
        text: text.into(),
        command,
        top,
    };
    match status {
        UpdateStatus::Idle => row(
            Glyph::Update,
            t(T::TrayCheckUpdates),
            Some(Command::CheckUpdates),
            false,
        ),
        UpdateStatus::Checking => row(Glyph::Update, t(T::Checking), None, false),
        UpdateStatus::UpToDate => row(
            Glyph::Check,
            t(T::TrayLatest),
            Some(Command::CheckUpdates),
            false,
        ),
        UpdateStatus::Failed => row(
            Glyph::Update,
            t(T::TrayCheckFailed),
            Some(Command::CheckUpdates),
            false,
        ),
        UpdateStatus::Available(version) => UpdateRow {
            glyph: Glyph::Download,
            text: t(T::TrayUpdate).replace("{}", version),
            command: Some(Command::Update),
            top: true,
        },
        UpdateStatus::Downloading => row(Glyph::Download, t(T::Downloading), None, true),
    }
}

/// The light or dark look of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

/// The colors of the site, https://vkardin.github.io/snapt/.
struct Palette {
    bg: u32,
    hover: u32,
    text: u32,
    muted: u32,
    line: u32,
    icon: u32,
    chip: u32,
    accent: u32,
    accent_hover: u32,
}

impl Theme {
    fn palette(self) -> Palette {
        match self {
            Theme::Dark => Palette {
                bg: 0x17132B,
                hover: 0x2D2650,
                text: 0xECE9F7,
                muted: 0xA49DBD,
                line: 0x2A2445,
                icon: 0xA78BFA,
                chip: 0x231D3D,
                accent: 0x7C3AED,
                accent_hover: 0x8B5CF6,
            },
            Theme::Light => Palette {
                bg: 0xFFFFFF,
                hover: 0xF1ECFF,
                text: 0x1C1530,
                muted: 0x5D5675,
                line: 0xECE8F7,
                icon: 0x7C3AED,
                chip: 0xF6F4FE,
                accent: 0x7C3AED,
                accent_hover: 0x6D28D9,
            },
        }
    }
}

/// What the menu shows. The app builds it whenever the menu opens or changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The running version, shown at the top.
    pub version: &'static str,
    pub update: UpdateStatus,
    /// The shortcuts of taking a screenshot, the picker, the full screen,
    /// the last area and a scrolling screenshot of a page, as shown; empty when not set.
    pub keys: [String; 5],
    pub picker: bool,
    /// Scrolling screenshots are on.
    pub scroll_page: bool,
    pub can_repeat: bool,
    /// The recent screenshots, newest first.
    pub history: Vec<String>,
}

/// What a spot in the menu does when clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Run(Command),
    OpenHistory,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    History,
}

enum Hint {
    None,
    Keys(String),
    Chevron,
}

enum Row {
    Header,
    Item {
        glyph: Glyph,
        text: String,
        hint: Hint,
        /// `None` shows the item greyed out.
        target: Option<Target>,
        accent: bool,
    },
    Delay,
    Separator,
    Note(String),
}

impl Row {
    fn height(&self) -> f32 {
        match self {
            Row::Header => 40.0,
            Row::Separator => 9.0,
            Row::Item { .. } | Row::Delay | Row::Note(_) => ROW,
        }
    }
}

const PAD: f32 = 6.0;
const ROW: f32 = 32.0;
const TEXT_X: f32 = PAD + 38.0;
const SIZE: f32 = 13.0;
const HINT_SIZE: f32 = 12.0;
const MIN_WIDTH: f32 = 260.0;
const CHIP_H: f32 = 22.0;
const CHIP_GAP: f32 = 4.0;
/// How many recent screenshots show at once; the wheel scrolls through the rest.
pub const HISTORY_ROWS: usize = 10;

fn item(glyph: Glyph, text: &str, hint: Hint, target: Option<Target>) -> Row {
    Row::Item {
        glyph,
        text: text.into(),
        hint,
        target,
        accent: false,
    }
}

fn keys(keys: &str) -> Hint {
    if keys.is_empty() {
        Hint::None
    } else {
        Hint::Keys(keys.into())
    }
}

fn rows(model: &Model, page: Page, scroll: usize) -> Vec<Row> {
    let run = |c| Some(Target::Run(c));
    let update = update_row(&model.update);
    let update_item = |accent| Row::Item {
        glyph: update.glyph,
        text: update.text.clone(),
        hint: Hint::None,
        target: update.command.map(Target::Run),
        accent,
    };
    match page {
        Page::Main => {
            let mut rows = vec![Row::Header, Row::Separator];
            if update.top {
                rows.push(update_item(true));
            }
            rows.extend([
                item(
                    Glyph::Capture,
                    t(T::TakeScreenshot),
                    keys(&model.keys[0]),
                    run(Command::Capture),
                ),
                item(
                    Glyph::Picker,
                    t(T::PickColor),
                    keys(&model.keys[1]),
                    model.picker.then_some(Target::Run(Command::PickColor)),
                ),
                item(
                    Glyph::ScrollPage,
                    t(T::ScrollPage),
                    keys(&model.keys[4]),
                    model
                        .scroll_page
                        .then_some(Target::Run(Command::ScrollPage)),
                ),
                item(
                    Glyph::FullScreen,
                    t(T::FullScreen),
                    keys(&model.keys[2]),
                    run(Command::FullScreen),
                ),
                item(
                    Glyph::Repeat,
                    t(T::RepeatArea),
                    keys(&model.keys[3]),
                    model.can_repeat.then_some(Target::Run(Command::RepeatArea)),
                ),
                Row::Delay,
                item(
                    Glyph::History,
                    t(T::History),
                    Hint::Chevron,
                    Some(Target::OpenHistory),
                ),
                Row::Separator,
                item(
                    Glyph::Folder,
                    t(T::OpenFolder),
                    Hint::None,
                    run(Command::OpenFolder),
                ),
                item(
                    Glyph::Settings,
                    t(T::SettingsMenu),
                    Hint::None,
                    run(Command::Settings),
                ),
            ]);
            if !update.top {
                rows.push(update_item(false));
            }
            rows.extend([
                Row::Separator,
                item(Glyph::About, t(T::About), Hint::None, run(Command::About)),
                item(Glyph::Quit, t(T::Quit), Hint::None, run(Command::Quit)),
            ]);
            rows
        }
        Page::History => {
            let mut rows = vec![
                item(Glyph::Back, t(T::History), Hint::None, Some(Target::Back)),
                Row::Separator,
            ];
            if model.history.is_empty() {
                rows.push(Row::Note(t(T::HistoryEmpty).into()));
            }
            let first = scroll.min(model.history.len().saturating_sub(HISTORY_ROWS));
            rows.extend(
                model
                    .history
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take(HISTORY_ROWS)
                    .map(|(i, label)| {
                        item(Glyph::Image, label, Hint::None, run(Command::History(i)))
                    }),
            );
            rows.extend([
                Row::Separator,
                item(
                    Glyph::Clear,
                    t(T::ClearHistory),
                    Hint::None,
                    (!model.history.is_empty()).then_some(Target::Run(Command::ClearHistory)),
                ),
            ]);
            rows
        }
    }
}

fn chip_label(seconds: u64) -> String {
    t(T::SecondsShort).replace("{}", &seconds.to_string())
}

fn chip_width(label: &str) -> f32 {
    (font::measure(label, HINT_SIZE).0 + 16.0).max(34.0)
}

fn chips_width() -> f32 {
    DELAYS
        .iter()
        .map(|s| chip_width(&chip_label(*s)) + CHIP_GAP)
        .sum::<f32>()
        - CHIP_GAP
}

fn row_width(row: &Row) -> f32 {
    let text = |s: &str| font::measure(s, SIZE).0;
    match row {
        Row::Header | Row::Separator => 0.0,
        Row::Note(s) => TEXT_X + text(s) + 16.0,
        Row::Delay => TEXT_X + text(t(T::Delayed)) + 24.0 + chips_width() + 6.0,
        Row::Item { text: s, hint, .. } => {
            let hint = match hint {
                Hint::None => 0.0,
                Hint::Keys(k) => 28.0 + font::measure(k, HINT_SIZE).0,
                Hint::Chevron => 28.0,
            };
            TEXT_X + text(s) + hint + 16.0
        }
    }
}

/// The size of the menu in logical pixels.
pub fn size(model: &Model, page: Page, scroll: usize) -> (f32, f32) {
    let shown = rows(model, page, scroll);
    // The page with the history is as wide as the main one, so the menu keeps its place.
    let width = rows(model, Page::Main, 0)
        .iter()
        .chain(shown.iter())
        .map(row_width)
        .fold(MIN_WIDTH - 2.0 * PAD, f32::max)
        + 2.0 * PAD;
    let height = shown.iter().map(Row::height).sum::<f32>() + 2.0 * PAD;
    (width.ceil(), height.ceil())
}

/// Draws the menu onto `pixmap` at `scale` and returns where its targets are, in logical pixels.
pub fn draw(
    pixmap: &mut Pixmap,
    scale: f32,
    theme: Theme,
    model: &Model,
    page: Page,
    scroll: usize,
    hover: Option<Target>,
) -> Vec<(Target, SkRect)> {
    let (width, _) = size(model, page, scroll);
    let mut p = Painter {
        pixmap,
        s: scale,
        pal: theme.palette(),
    };
    pixmap_fill(&mut p);
    let mut hits = Vec::new();
    let mut y = PAD;
    for row in rows(model, page, scroll) {
        let h = row.height();
        let right = width - PAD;
        match &row {
            Row::Header => {
                p.logo(PAD + 9.0, y + 10.0, 20.0);
                p.text("Snapt", PAD + 37.0, y + h / 2.0, 14.0, p.pal.text, 1.0);
                let version = model.version;
                let w = font::measure(version, HINT_SIZE).0;
                p.text(
                    version,
                    right - 10.0 - w,
                    y + h / 2.0,
                    HINT_SIZE,
                    p.pal.muted,
                    1.0,
                );
            }
            Row::Separator => {
                p.fill(
                    SkRect::from_xywh(PAD + 8.0, y + 4.0, width - 2.0 * PAD - 16.0, 1.0),
                    0.0,
                    p.pal.line,
                );
            }
            Row::Note(text) => p.text(text, TEXT_X, y + h / 2.0, SIZE, p.pal.muted, 1.0),
            Row::Delay => {
                p.glyph(Glyph::Timer, PAD + 11.0, y + 8.0, p.pal.icon);
                p.text(t(T::Delayed), TEXT_X, y + h / 2.0, SIZE, p.pal.text, 1.0);
                let mut x = right - 5.0 - chips_width();
                for seconds in DELAYS {
                    let label = chip_label(seconds);
                    let w = chip_width(&label);
                    let target = Target::Run(Command::Delay(seconds));
                    let rect = SkRect::from_xywh(x, y + (h - CHIP_H) / 2.0, w, CHIP_H);
                    let on = hover == Some(target);
                    p.fill(
                        rect,
                        CHIP_H / 2.0,
                        if on { p.pal.accent } else { p.pal.chip },
                    );
                    let tw = font::measure(&label, HINT_SIZE).0;
                    let color = if on { 0xFFFFFF } else { p.pal.text };
                    p.text(
                        &label,
                        x + (w - tw) / 2.0,
                        y + h / 2.0,
                        HINT_SIZE,
                        color,
                        1.0,
                    );
                    if let Some(rect) = rect {
                        hits.push((target, rect));
                    }
                    x += w + CHIP_GAP;
                }
            }
            Row::Item {
                glyph,
                text,
                hint,
                target,
                accent,
            } => {
                let rect = SkRect::from_xywh(PAD, y, width - 2.0 * PAD, h);
                let on = target.is_some() && hover == *target;
                let enabled = target.is_some();
                if *accent {
                    let bg = if on { p.pal.accent_hover } else { p.pal.accent };
                    p.fill(rect, 6.0, bg);
                } else if on {
                    p.fill(rect, 6.0, p.pal.hover);
                }
                let (icon, ink, opacity) = match (*accent, enabled) {
                    (true, _) => (0xFFFFFF, 0xFFFFFF, 1.0),
                    (false, true) => (p.pal.icon, p.pal.text, 1.0),
                    (false, false) => (p.pal.muted, p.pal.muted, 0.6),
                };
                p.glyph_faded(*glyph, PAD + 11.0, y + 8.0, icon, opacity);
                p.text(text, TEXT_X, y + h / 2.0, SIZE, ink, opacity);
                match hint {
                    Hint::None => {}
                    Hint::Keys(keys) => {
                        let w = font::measure(keys, HINT_SIZE).0;
                        let color = if *accent { 0xFFFFFF } else { p.pal.muted };
                        p.text(
                            keys,
                            right - 10.0 - w,
                            y + h / 2.0,
                            HINT_SIZE,
                            color,
                            opacity,
                        );
                    }
                    Hint::Chevron => p.chevron(right - 16.0, y + h / 2.0, p.pal.muted),
                }
                if let (Some(target), Some(rect)) = (target, rect) {
                    hits.push((*target, rect));
                }
            }
        }
        y += h;
    }
    hits
}

fn pixmap_fill(p: &mut Painter) {
    let [r, g, b] = crate::paint::rgb(p.pal.bg);
    p.pixmap.fill(Color::from_rgba8(r, g, b, 255));
}

struct Painter<'a> {
    pixmap: &'a mut Pixmap,
    s: f32,
    pal: Palette,
}

impl Painter<'_> {
    fn scaled(&self, r: Option<SkRect>) -> Option<SkRect> {
        let r = r?;
        SkRect::from_xywh(
            r.x() * self.s,
            r.y() * self.s,
            r.width() * self.s,
            r.height() * self.s,
        )
    }

    fn fill(&mut self, r: Option<SkRect>, radius: f32, color: u32) {
        let r = self.scaled(r);
        fill_round(self.pixmap, r, radius * self.s, color, 1.0);
    }

    /// Draws `text` with its line centred on `cy`.
    fn text(&mut self, text: &str, x: f32, cy: f32, size: f32, color: u32, opacity: f32) {
        let top = cy - font::line_height(size) / 2.0;
        font::draw(
            self.pixmap,
            text,
            (x * self.s).round(),
            (top * self.s).round(),
            size * self.s,
            color,
            opacity,
        );
    }

    fn glyph(&mut self, glyph: Glyph, x: f32, y: f32, color: u32) {
        self.glyph_faded(glyph, x, y, color, 1.0);
    }

    fn glyph_faded(&mut self, glyph: Glyph, x: f32, y: f32, color: u32, opacity: f32) {
        let s = self.s;
        let transform = Transform::from_scale(s, s).pre_translate(x, y);
        if opacity >= 1.0 {
            draw_glyph(self.pixmap, glyph, color, transform);
            return;
        }
        // Fade the whole glyph at once, so overlapping strokes don't add up.
        let size = (16.0 * s).ceil() as u32 + 2;
        let Some(mut layer) = Pixmap::new(size, size) else {
            return;
        };
        draw_glyph(&mut layer, glyph, color, Transform::from_scale(s, s));
        let paint = PixmapPaint {
            opacity,
            ..PixmapPaint::default()
        };
        self.pixmap.draw_pixmap(
            (x * s).round() as i32,
            (y * s).round() as i32,
            layer.as_ref(),
            &paint,
            Transform::identity(),
            None,
        );
    }

    fn chevron(&mut self, x: f32, cy: f32, color: u32) {
        let mut p = PathBuilder::new();
        p.move_to(x + 2.0, cy - 4.0);
        p.line_to(x + 6.0, cy);
        p.line_to(x + 2.0, cy + 4.0);
        if let Some(path) = p.finish() {
            let s = self.s;
            stroke(self.pixmap, &path, color, 1.5, Transform::from_scale(s, s));
        }
    }

    /// The Snapt icon, `size` logical pixels across.
    fn logo(&mut self, x: f32, y: f32, size: f32) {
        let Some(logo) = logo() else { return };
        let k = size * self.s / icon::SIZE as f32;
        let paint = PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        };
        self.pixmap.draw_pixmap(
            0,
            0,
            logo.as_ref(),
            &paint,
            Transform::from_scale(k, k).post_translate((x * self.s).round(), (y * self.s).round()),
            None,
        );
    }
}

/// The app icon as a premultiplied pixmap.
fn logo() -> Option<Pixmap> {
    let mut data = icon::rgba();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(icon::SIZE, icon::SIZE)?)
}

fn stroke(pixmap: &mut Pixmap, path: &tiny_skia::Path, color: u32, width: f32, t: Transform) {
    let mut paint = Paint::default();
    let [r, g, b] = crate::paint::rgb(color);
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, t, None);
}

/// Adds a polyline along a circle around (`cx`, `cy`), from `from` to `to` degrees,
/// clockwise from three o'clock.
fn arc(p: &mut PathBuilder, cx: f32, cy: f32, r: f32, from: f32, to: f32) {
    let steps = 24;
    for i in 0..=steps {
        let a = (from + (to - from) * i as f32 / steps as f32).to_radians();
        let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
        if i == 0 {
            p.move_to(x, y);
        } else {
            p.line_to(x, y);
        }
    }
}

fn draw_glyph(pixmap: &mut Pixmap, glyph: Glyph, color: u32, t: Transform) {
    let mut p = PathBuilder::new();
    let mut dots: Vec<(f32, f32, f32)> = Vec::new();
    match glyph {
        Glyph::Capture => {
            // The viewfinder of the logo.
            for (x, y, dx, dy) in [
                (2.0, 2.0, 1.0, 1.0),
                (14.0, 2.0, -1.0, 1.0),
                (2.0, 14.0, 1.0, -1.0),
                (14.0, 14.0, -1.0, -1.0),
            ] {
                p.move_to(x, y + 3.5 * dy);
                p.line_to(x, y);
                p.line_to(x + 3.5 * dx, y);
            }
            dots.push((8.0, 8.0, 1.8));
        }
        Glyph::Picker => {
            p.move_to(3.0, 13.0);
            p.line_to(9.0, 7.0);
            p.move_to(7.0, 5.0);
            p.line_to(11.0, 9.0);
            p.push_circle(11.6, 4.4, 2.1);
            p.move_to(3.0, 13.0);
            p.line_to(2.5, 13.5);
        }
        Glyph::FullScreen => {
            p.move_to(2.5, 2.5);
            p.line_to(13.5, 2.5);
            p.line_to(13.5, 10.5);
            p.line_to(2.5, 10.5);
            p.close();
            p.move_to(8.0, 10.5);
            p.line_to(8.0, 13.5);
            p.move_to(5.0, 13.5);
            p.line_to(11.0, 13.5);
        }
        Glyph::ScrollPage => {
            // A page with an arrow down it.
            p.move_to(3.5, 2.5);
            p.line_to(12.5, 2.5);
            p.line_to(12.5, 13.5);
            p.line_to(3.5, 13.5);
            p.close();
            p.move_to(8.0, 5.0);
            p.line_to(8.0, 11.0);
            p.move_to(5.5, 8.5);
            p.line_to(8.0, 11.0);
            p.line_to(10.5, 8.5);
        }
        Glyph::Repeat => {
            p.move_to(2.5, 8.0);
            p.line_to(2.5, 6.5);
            p.quad_to(2.5, 4.0, 5.0, 4.0);
            p.line_to(13.0, 4.0);
            p.move_to(11.0, 2.0);
            p.line_to(13.0, 4.0);
            p.line_to(11.0, 6.0);
            p.move_to(13.5, 8.0);
            p.line_to(13.5, 9.5);
            p.quad_to(13.5, 12.0, 11.0, 12.0);
            p.line_to(3.0, 12.0);
            p.move_to(5.0, 10.0);
            p.line_to(3.0, 12.0);
            p.line_to(5.0, 14.0);
        }
        Glyph::Timer => {
            p.push_circle(8.0, 9.0, 5.5);
            p.move_to(8.0, 9.0);
            p.line_to(8.0, 6.0);
            p.move_to(6.5, 1.5);
            p.line_to(9.5, 1.5);
            p.move_to(12.2, 4.3);
            p.line_to(13.2, 3.3);
        }
        Glyph::History => {
            arc(&mut p, 8.0, 8.0, 5.8, 200.0, 520.0);
            p.move_to(1.6, 4.4);
            p.line_to(2.6, 6.0);
            p.line_to(4.4, 5.2);
            p.move_to(8.0, 5.0);
            p.line_to(8.0, 8.0);
            p.line_to(10.3, 9.5);
        }
        Glyph::Folder => {
            p.move_to(1.5, 12.5);
            p.line_to(1.5, 3.5);
            p.line_to(6.0, 3.5);
            p.line_to(7.5, 5.0);
            p.line_to(14.5, 5.0);
            p.line_to(14.5, 12.5);
            p.close();
        }
        Glyph::Settings => {
            p.push_circle(8.0, 8.0, 2.2);
            p.push_circle(8.0, 8.0, 4.8);
            for i in 0..8 {
                let a = (i as f32 * 45.0 + 22.5).to_radians();
                p.move_to(8.0 + 4.8 * a.cos(), 8.0 + 4.8 * a.sin());
                p.line_to(8.0 + 6.6 * a.cos(), 8.0 + 6.6 * a.sin());
            }
        }
        Glyph::Update => {
            // A clockwise arrow round the circle, its head at the top right.
            arc(&mut p, 8.0, 8.0, 6.0, 0.0, 315.0);
            p.line_to(14.0, 5.4);
            p.move_to(14.0, 2.0);
            p.line_to(14.0, 5.4);
            p.line_to(10.6, 5.4);
        }
        Glyph::Download => {
            p.move_to(8.0, 2.0);
            p.line_to(8.0, 10.5);
            p.move_to(4.5, 7.0);
            p.line_to(8.0, 10.5);
            p.line_to(11.5, 7.0);
            p.move_to(2.5, 13.5);
            p.line_to(13.5, 13.5);
        }
        Glyph::Check => {
            p.move_to(3.0, 8.5);
            p.line_to(6.5, 12.0);
            p.line_to(13.0, 4.5);
        }
        Glyph::About => {
            p.push_circle(8.0, 8.0, 6.3);
            p.move_to(8.0, 7.5);
            p.line_to(8.0, 11.5);
            dots.push((8.0, 4.9, 1.0));
        }
        Glyph::Quit => {
            arc(&mut p, 8.0, 8.8, 5.6, -55.0, 235.0);
            p.move_to(8.0, 1.8);
            p.line_to(8.0, 7.8);
        }
        Glyph::Back => {
            p.move_to(10.0, 3.0);
            p.line_to(5.0, 8.0);
            p.line_to(10.0, 13.0);
        }
        Glyph::Image => {
            p.move_to(2.0, 3.0);
            p.line_to(14.0, 3.0);
            p.line_to(14.0, 13.0);
            p.line_to(2.0, 13.0);
            p.close();
            p.move_to(2.5, 12.0);
            p.line_to(6.5, 8.0);
            p.line_to(9.0, 10.5);
            p.line_to(11.0, 8.5);
            p.line_to(13.5, 11.0);
            dots.push((10.8, 6.0, 1.2));
        }
        Glyph::Clear => {
            p.move_to(2.5, 4.0);
            p.line_to(13.5, 4.0);
            p.move_to(6.0, 4.0);
            p.line_to(6.0, 2.5);
            p.line_to(10.0, 2.5);
            p.line_to(10.0, 4.0);
            p.move_to(3.8, 4.0);
            p.line_to(4.8, 13.5);
            p.line_to(11.2, 13.5);
            p.line_to(12.2, 4.0);
        }
    }
    if let Some(path) = p.finish() {
        stroke(pixmap, &path, color, 1.5, t);
    }
    let mut paint = Paint::default();
    let [r, g, b] = crate::paint::rgb(color);
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    for (x, y, r) in dots {
        if let Some(dot) = PathBuilder::from_circle(x, y, r) {
            pixmap.fill_path(&dot, &paint, FillRule::Winding, t, None);
        }
    }
}

#[cfg(target_os = "windows")]
pub use window::MenuWindow;

#[cfg(target_os = "windows")]
mod window {
    use std::{
        num::NonZeroU32,
        rc::Rc,
        time::{Duration, Instant},
    };

    use anyhow::{Context, Result};
    use softbuffer::Surface;
    use tiny_skia::{Pixmap, Rect as SkRect};
    use winit::{
        dpi::{PhysicalPosition, PhysicalSize},
        event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
        event_loop::{ActiveEventLoop, OwnedDisplayHandle},
        keyboard::{KeyCode, PhysicalKey},
        platform::windows::{
            Color, CornerPreference, WindowAttributesExtWindows, WindowExtWindows,
        },
        window::{Window, WindowId, WindowLevel},
    };

    use super::{HISTORY_ROWS, Model, Page, Target, Theme, draw, size};
    use crate::tray::Command;

    /// A click on the tray icon this soon after the menu closed only closed it.
    const REOPEN_DELAY: Duration = Duration::from_millis(400);

    /// The monitor the menu opened on.
    #[derive(Clone, Copy)]
    struct Screen {
        origin: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
        scale: f64,
    }

    pub struct MenuWindow {
        // Declared before the context so the surface drops first.
        surface: Surface<OwnedDisplayHandle, Rc<Window>>,
        window: Rc<Window>,
        _context: softbuffer::Context<OwnedDisplayHandle>,
        model: Option<Model>,
        page: Page,
        scroll: usize,
        hover: Option<Target>,
        hits: Vec<(Target, SkRect)>,
        /// The point the menu opens from: where the tray icon was clicked.
        anchor: PhysicalPosition<f64>,
        screen: Screen,
        visible: bool,
        hidden_at: Option<Instant>,
    }

    impl MenuWindow {
        pub fn open(event_loop: &ActiveEventLoop) -> Result<Self> {
            let context = softbuffer::Context::new(event_loop.owned_display_handle())
                .map_err(|e| anyhow::anyhow!("failed to start the renderer: {e}"))?;
            let attributes = Window::default_attributes()
                .with_title("Snapt")
                .with_decorations(false)
                .with_resizable(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_inner_size(PhysicalSize::new(1, 1))
                .with_skip_taskbar(true)
                .with_drag_and_drop(false)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round);
            let window = Rc::new(
                event_loop
                    .create_window(attributes)
                    .context("failed to create the tray menu")?,
            );
            let surface = Surface::new(&context, window.clone())
                .map_err(|e| anyhow::anyhow!("failed to create a drawing surface: {e}"))?;
            Ok(Self {
                surface,
                window,
                _context: context,
                model: None,
                page: Page::Main,
                scroll: 0,
                hover: None,
                hits: Vec::new(),
                anchor: PhysicalPosition::new(0.0, 0.0),
                screen: Screen {
                    origin: PhysicalPosition::new(0, 0),
                    size: PhysicalSize::new(0, 0),
                    scale: 1.0,
                },
                visible: false,
                hidden_at: None,
            })
        }

        pub fn id(&self) -> WindowId {
            self.window.id()
        }

        pub fn is_visible(&self) -> bool {
            self.visible
        }

        /// Opens the menu at `at`, the clicked point of the tray icon, or
        /// closes it when it is open: a second click on the icon toggles it.
        pub fn toggle(
            &mut self,
            event_loop: &ActiveEventLoop,
            at: PhysicalPosition<f64>,
            model: Model,
        ) {
            if self.visible {
                self.hide();
                return;
            }
            // The click itself took the focus away and closed the menu a moment ago.
            if self.hidden_at.is_some_and(|t| t.elapsed() < REOPEN_DELAY) {
                self.hidden_at = None;
                return;
            }
            let monitor = event_loop
                .available_monitors()
                .find(|m| {
                    let (o, s) = (m.position(), m.size());
                    at.x >= o.x as f64
                        && at.y >= o.y as f64
                        && at.x < o.x as f64 + s.width as f64
                        && at.y < o.y as f64 + s.height as f64
                })
                .or_else(|| event_loop.primary_monitor());
            if let Some(m) = monitor {
                self.screen = Screen {
                    origin: m.position(),
                    size: m.size(),
                    scale: m.scale_factor(),
                };
            }
            self.anchor = at;
            self.model = Some(model);
            self.page = Page::Main;
            self.scroll = 0;
            self.hover = None;
            self.place();
            self.redraw();
            self.window.set_visible(true);
            self.window.focus_window();
            self.visible = true;
        }

        /// Shows what changed while the menu is open, such as the update check.
        pub fn update(&mut self, model: Model) {
            if self.model.as_ref() == Some(&model) {
                return;
            }
            self.model = Some(model);
            if self.visible {
                self.place();
                self.window.request_redraw();
            }
        }

        pub fn hide(&mut self) {
            if self.visible {
                self.visible = false;
                self.hidden_at = Some(Instant::now());
                self.window.set_visible(false);
            }
            self.hover = None;
        }

        /// The menu's size in physical pixels on its screen.
        fn physical_size(&self) -> PhysicalSize<u32> {
            let Some(model) = &self.model else {
                return PhysicalSize::new(1, 1);
            };
            let (w, h) = size(model, self.page, self.scroll);
            let s = self.screen.scale;
            PhysicalSize::new((w as f64 * s).ceil() as u32, (h as f64 * s).ceil() as u32)
        }

        /// Puts the menu next to the anchor, inside the screen: above the
        /// point when the taskbar is at the bottom, below it when it is at the top.
        fn place(&mut self) {
            let size = self.physical_size();
            let (w, h) = (size.width as f64, size.height as f64);
            let (ox, oy) = (self.screen.origin.x as f64, self.screen.origin.y as f64);
            let (sw, sh) = (
                self.screen.size.width as f64,
                self.screen.size.height as f64,
            );
            let at = self.anchor;
            let mut x = if at.x + w > ox + sw { at.x - w } else { at.x };
            let mut y = if at.y - h >= oy { at.y - h } else { at.y };
            x = x.clamp(ox, (ox + sw - w).max(ox));
            y = y.clamp(oy, (oy + sh - h).max(oy));
            self.window
                .set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
            let _ = self.window.request_inner_size(size);
        }

        fn theme(&self) -> Theme {
            match self.window.theme() {
                Some(winit::window::Theme::Light) => Theme::Light,
                _ => Theme::Dark,
            }
        }

        /// Handles an event; returns the command the user picked, if any.
        pub fn window_event(&mut self, event: WindowEvent) -> Option<Command> {
            match event {
                WindowEvent::Focused(false) | WindowEvent::CloseRequested => self.hide(),
                WindowEvent::RedrawRequested => self.redraw(),
                WindowEvent::ThemeChanged(_) | WindowEvent::Resized(_) => {
                    self.window.request_redraw();
                }
                WindowEvent::ScaleFactorChanged {
                    mut inner_size_writer,
                    ..
                } => {
                    let _ = inner_size_writer.request_inner_size(self.physical_size());
                    self.window.request_redraw();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let s = self.screen.scale;
                    let (x, y) = ((position.x / s) as f32, (position.y / s) as f32);
                    let hover = self
                        .hits
                        .iter()
                        .find(|(_, r)| {
                            x >= r.left() && x < r.right() && y >= r.top() && y < r.bottom()
                        })
                        .map(|(target, _)| *target);
                    self.set_hover(hover);
                }
                WindowEvent::CursorLeft { .. } => self.set_hover(None),
                WindowEvent::MouseWheel { delta, .. } if self.page == Page::History => {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => (p.y / 32.0) as f32,
                    };
                    let len = self.model.as_ref().map_or(0, |m| m.history.len());
                    let max = len.saturating_sub(HISTORY_ROWS) as i64;
                    let scroll = (self.scroll as i64 - lines.round() as i64).clamp(0, max) as usize;
                    if scroll != self.scroll {
                        self.scroll = scroll;
                        self.window.request_redraw();
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left | MouseButton::Right,
                    ..
                } => return self.hover.and_then(|t| self.activate(t)),
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    let PhysicalKey::Code(code) = event.physical_key else {
                        return None;
                    };
                    match code {
                        KeyCode::Escape | KeyCode::ArrowLeft | KeyCode::Backspace
                            if self.page == Page::History =>
                        {
                            return self.activate(Target::Back);
                        }
                        KeyCode::Escape => self.hide(),
                        KeyCode::ArrowDown => self.step(1),
                        KeyCode::ArrowUp => self.step(-1),
                        KeyCode::ArrowRight if self.hover == Some(Target::OpenHistory) => {
                            return self.activate(Target::OpenHistory);
                        }
                        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                            return self.hover.and_then(|t| self.activate(t));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            None
        }

        fn set_hover(&mut self, hover: Option<Target>) {
            if hover != self.hover {
                self.hover = hover;
                self.window.request_redraw();
            }
        }

        /// Moves the highlight with the arrow keys.
        fn step(&mut self, by: isize) {
            let n = self.hits.len() as isize;
            if n == 0 {
                return;
            }
            let at = self
                .hover
                .and_then(|h| self.hits.iter().position(|(t, _)| *t == h))
                .map_or(if by > 0 { -1 } else { 0 }, |i| i as isize);
            let next = (at + by).rem_euclid(n) as usize;
            self.set_hover(Some(self.hits[next].0));
        }

        fn activate(&mut self, target: Target) -> Option<Command> {
            match target {
                Target::OpenHistory | Target::Back => {
                    self.page = if target == Target::Back {
                        Page::Main
                    } else {
                        Page::History
                    };
                    self.scroll = 0;
                    self.hover = None;
                    self.place();
                    self.window.request_redraw();
                    None
                }
                // The menu stays open to show how the check went.
                Target::Run(Command::CheckUpdates) => Some(Command::CheckUpdates),
                Target::Run(command) => {
                    self.hide();
                    Some(command)
                }
            }
        }

        fn redraw(&mut self) {
            let Some(model) = &self.model else { return };
            let size = self.window.inner_size();
            let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
            else {
                return;
            };
            if let Err(err) = self.surface.resize(w, h) {
                log::error!("failed to resize the tray menu: {err}");
                return;
            }
            let Some(mut pixmap) = Pixmap::new(size.width, size.height) else {
                return;
            };
            let theme = self.theme();
            let line = match theme {
                Theme::Dark => Color::from_rgb(0x2A, 0x24, 0x45),
                Theme::Light => Color::from_rgb(0xDD, 0xD6, 0xF3),
            };
            self.window.set_border_color(Some(line));
            self.hits = draw(
                &mut pixmap,
                self.screen.scale as f32,
                theme,
                model,
                self.page,
                self.scroll,
                self.hover,
            );
            let mut buffer = match self.surface.buffer_mut() {
                Ok(buffer) => buffer,
                Err(err) => {
                    log::error!("failed to draw the tray menu: {err}");
                    return;
                }
            };
            for (dst, src) in buffer.iter_mut().zip(pixmap.data().chunks_exact(4)) {
                *dst = (src[0] as u32) << 16 | (src[1] as u32) << 8 | src[2] as u32;
            }
            if let Err(err) = buffer.present() {
                log::error!("failed to present the tray menu: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        Model {
            version: "1.0.0",
            update: UpdateStatus::UpToDate,
            keys: [
                "Alt+Shift+S".into(),
                "Ctrl+Alt+C".into(),
                String::new(),
                String::new(),
                "Ctrl+Shift+PrtSc".into(),
            ],
            picker: true,
            scroll_page: true,
            can_repeat: false,
            history: vec![
                "09:05:02  ·  800 × 600".into(),
                "24.10 18:40  ·  1920 × 1080".into(),
            ],
        }
    }

    fn render(model: &Model, page: Page, theme: Theme, hover: Option<Target>) -> image::RgbaImage {
        let (w, h) = size(model, page, 0);
        let mut pixmap = Pixmap::new(w as u32, h as u32).unwrap();
        draw(&mut pixmap, 1.0, theme, model, page, 0, hover);
        image::RgbaImage::from_raw(w as u32, h as u32, pixmap.take()).unwrap()
    }

    #[test]
    fn every_item_can_be_clicked_and_greyed_items_cant() {
        let _lang = crate::snapshot::language(crate::i18n::Lang::En);
        let model = model();
        let (w, h) = size(&model, Page::Main, 0);
        let mut pixmap = Pixmap::new(w as u32, h as u32).unwrap();
        let hits = draw(&mut pixmap, 1.0, Theme::Dark, &model, Page::Main, 0, None);
        let targets: Vec<Target> = hits.iter().map(|(t, _)| *t).collect();
        for command in [
            Command::Capture,
            Command::PickColor,
            Command::ScrollPage,
            Command::FullScreen,
            Command::Delay(3),
            Command::Delay(10),
            Command::OpenFolder,
            Command::Settings,
            Command::CheckUpdates,
            Command::About,
            Command::Quit,
        ] {
            assert!(targets.contains(&Target::Run(command)), "{command:?}");
        }
        assert!(targets.contains(&Target::OpenHistory));
        // Nothing to repeat yet.
        assert!(!targets.contains(&Target::Run(Command::RepeatArea)));
        for (_, r) in &hits {
            assert!(r.left() >= 0.0 && r.right() <= w && r.bottom() <= h);
        }
    }

    #[test]
    fn a_new_version_leads_the_menu() {
        let _lang = crate::snapshot::language(crate::i18n::Lang::Ru);
        let mut model = model();
        model.update = UpdateStatus::Available("2.6.0".into());
        let rows = rows(&model, Page::Main, 0);
        let first = rows.iter().find_map(|r| match r {
            Row::Item { text, accent, .. } => Some((text.clone(), *accent)),
            _ => None,
        });
        assert_eq!(first, Some(("Обновить до Snapt 2.6.0".into(), true)));
        let updates = rows
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    Row::Item {
                        glyph: Glyph::Update | Glyph::Check | Glyph::Download,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(updates, 1);
    }

    #[test]
    fn long_histories_scroll_and_the_menu_keeps_its_width() {
        // Widths depend on the language, which other tests switch meanwhile.
        let _lang = crate::snapshot::language(crate::i18n::Lang::En);
        let mut model = model();
        model.history = (0..30).map(|i| format!("shot {i}")).collect();
        let (main_w, _) = size(&model, Page::Main, 0);
        let (w, h) = size(&model, Page::History, 0);
        assert_eq!(w, main_w);
        assert_eq!(size(&model, Page::History, 25), (w, h));
        let shown = |scroll| {
            rows(&model, Page::History, scroll)
                .iter()
                .filter_map(|r| match r {
                    Row::Item {
                        target: Some(Target::Run(Command::History(i))),
                        ..
                    } => Some(*i),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(0), (0..10).collect::<Vec<_>>());
        assert_eq!(shown(99), (20..30).collect::<Vec<_>>());
    }

    #[test]
    fn every_language_fits() {
        for lang in crate::i18n::Lang::ALL {
            let _lang = crate::snapshot::language(lang);
            let mut model = model();
            for status in [
                UpdateStatus::Idle,
                UpdateStatus::Failed,
                UpdateStatus::Available("12.34.56".into()),
                UpdateStatus::Downloading,
            ] {
                model.update = status;
                let (w, _) = size(&model, Page::Main, 0);
                assert!(w <= 420.0, "{lang:?}: {w}");
                for row in rows(&model, Page::Main, 0) {
                    assert!(row_width(&row) <= w, "{lang:?}");
                }
            }
        }
    }

    #[test]
    fn system_menu_icons_have_ink() {
        let rgba = glyph_rgba(Glyph::Settings, 32, NATIVE_ICON);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.chunks(4).filter(|p| p[3] > 128).count() > 40);
    }

    #[test]
    fn tray_menu_snapshot() {
        let mut model = model();
        let cases = [
            (
                "tray-menu-dark-ru",
                crate::i18n::Lang::Ru,
                Theme::Dark,
                Page::Main,
                Some(Target::Run(Command::Capture)),
            ),
            (
                "tray-menu-light-en",
                crate::i18n::Lang::En,
                Theme::Light,
                Page::Main,
                Some(Target::Run(Command::Delay(5))),
            ),
            (
                "tray-menu-history-uk",
                crate::i18n::Lang::Uk,
                Theme::Dark,
                Page::History,
                Some(Target::Back),
            ),
        ];
        for (name, lang, theme, page, hover) in cases {
            let _lang = crate::snapshot::language(lang);
            let image = render(&model, page, theme, hover);
            crate::snapshot::assert_matches(name, &image);
        }
        let _lang = crate::snapshot::language(crate::i18n::Lang::Ru);
        model.update = UpdateStatus::Available("2.6.0".into());
        model.can_repeat = true;
        let image = render(&model, Page::Main, Theme::Light, None);
        crate::snapshot::assert_matches("tray-menu-update-ru", &image);
    }
}
