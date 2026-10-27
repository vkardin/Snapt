//! The installer's one window, drawn with the same software renderer and
//! colours as Snapt's settings window.

use tiny_skia::{Pixmap, Rect};

use crate::{
    font,
    logos::{self, Logo},
    paint::fill_round,
    system::Step,
    text::Text,
};

pub const WIDTH: f32 = 460.0;
const INSTALL_HEIGHT: f32 = 352.0;
const UNINSTALL_HEIGHT: f32 = 236.0;
const MARGIN: f32 = 28.0;
const BUTTON_H: f32 = 36.0;

const BG: u32 = 0x1C1C1F;
const CARD: u32 = 0x232328;
const CARD_HOVER: u32 = 0x2C2C33;
const CONTROL: u32 = 0x2E2E35;
const CONTROL_HOVER: u32 = 0x3A3A43;
const TEXT: u32 = 0xF4F4F6;
const MUTED: u32 = 0x9A9AA3;
const ACCENT: u32 = 0x8B5CF6;
const ACCENT_HOVER: u32 = 0x9D74F8;
const DANGER: u32 = 0xDC2626;
const DANGER_HOVER: u32 = 0xEF4444;
const ERROR: u32 = 0xF87171;

pub const LEVELER_URL: &str = "https://vkardin.github.io/leveler/";
pub const VIDEO_GLOW_URL: &str = "https://vkardin.github.io/video-glow/";

#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Welcome,
    Working,
    Done,
    Failed(String),
    Confirm,
    Removed,
}

/// Everything that can be clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id {
    Desktop,
    AllUsers,
    Install,
    Launch,
    Close,
    Retry,
    Leveler,
    VideoGlow,
    Uninstall,
}

/// What the window shows right now.
pub struct View<'a> {
    pub text: &'static Text,
    pub page: &'a Page,
    pub desktop: bool,
    pub all_users: bool,
    pub dir: &'a str,
    /// The label of the main button: Install, Update or Reinstall.
    pub action: &'static str,
    pub version: &'a str,
    pub progress: f32,
    pub step: Option<Step>,
    pub hover: Option<Id>,
}

pub fn height(uninstall: bool) -> f32 {
    if uninstall {
        UNINSTALL_HEIGHT
    } else {
        INSTALL_HEIGHT
    }
}

/// The button pressed by Enter on this page.
pub fn default_button(page: &Page) -> Option<Id> {
    match page {
        Page::Welcome => Some(Id::Install),
        Page::Done => Some(Id::Launch),
        Page::Failed(_) => Some(Id::Retry),
        Page::Confirm => Some(Id::Uninstall),
        Page::Removed => Some(Id::Close),
        Page::Working => None,
    }
}

/// Draws `view` and returns the clickable areas, in logical pixels.
pub fn draw(pixmap: &mut Pixmap, scale: f32, view: &View, uninstall: bool) -> Vec<(Id, Rect)> {
    let mut ui = Ui {
        pixmap,
        scale,
        hover: view.hover,
        hits: Vec::new(),
    };
    let h = height(uninstall);
    ui.fill(0.0, 0.0, WIDTH, h, 0.0, BG);
    let t = view.text;
    let bottom = h - MARGIN - BUTTON_H;
    let right = WIDTH - MARGIN;
    match view.page {
        Page::Welcome => {
            ui.logo(Logo::Snapt, MARGIN, 30.0, 56.0);
            ui.label("Snapt", 98.0, 32.0, 26.0, TEXT);
            ui.label(
                &format!("{} {}", t.version, view.version),
                99.0,
                68.0,
                12.0,
                MUTED,
            );
            ui.wrapped(t.tagline, MARGIN, 110.0, WIDTH - 2.0 * MARGIN, 13.0, MUTED);
            ui.switch(Id::Desktop, t.desktop, None, 160.0, view.desktop);
            ui.switch(
                Id::AllUsers,
                t.all_users,
                Some(t.all_users_hint),
                204.0,
                view.all_users,
            );
            let folder = format!("{}: {}", t.folder, view.dir);
            ui.label(
                &ellipsize(&folder, WIDTH - 2.0 * MARGIN, 11.0),
                MARGIN,
                262.0,
                11.0,
                MUTED,
            );
            ui.button(Id::Install, view.action, right, bottom, ACCENT);
        }
        Page::Working | Page::Done => {
            let (title, hint) = if *view.page == Page::Done {
                (t.installed, t.installed_hint)
            } else {
                (t.installing, step_text(t, view.step))
            };
            ui.header(title, hint, MUTED);
            ui.progress(if *view.page == Page::Done {
                1.0
            } else {
                view.progress
            });
            ui.label(t.more, MARGIN, 120.0, 12.0, MUTED);
            let w = (WIDTH - 2.0 * MARGIN - 12.0) / 2.0;
            ui.card(
                Id::Leveler,
                Logo::Leveler,
                "Leveler",
                t.extension,
                t.leveler,
                MARGIN,
                w,
            );
            ui.card(
                Id::VideoGlow,
                Logo::VideoGlow,
                "Video Glow",
                t.extension,
                t.video_glow,
                MARGIN + w + 12.0,
                w,
            );
            if *view.page == Page::Done {
                let x = ui.button(Id::Launch, t.launch, right, bottom, ACCENT);
                ui.button(Id::Close, t.close, x - 10.0, bottom, CONTROL);
            }
        }
        Page::Failed(error) => {
            let title = if uninstall {
                t.uninstall_failed
            } else {
                t.failed
            };
            ui.header(title, "", MUTED);
            ui.wrapped(error, MARGIN, 92.0, WIDTH - 2.0 * MARGIN, 12.0, ERROR);
            let x = ui.button(Id::Retry, t.retry, right, bottom, ACCENT);
            ui.button(Id::Close, t.close, x - 10.0, bottom, CONTROL);
        }
        Page::Confirm if view.progress > 0.0 => {
            ui.header(t.uninstalling, step_text(t, view.step), MUTED);
            ui.progress(view.progress);
        }
        Page::Confirm => {
            ui.header(t.uninstall_title, "", MUTED);
            ui.wrapped(
                t.uninstall_text,
                MARGIN,
                92.0,
                WIDTH - 2.0 * MARGIN,
                13.0,
                MUTED,
            );
            let x = ui.button(Id::Uninstall, t.uninstall, right, bottom, DANGER);
            ui.button(Id::Close, t.cancel, x - 10.0, bottom, CONTROL);
        }
        Page::Removed => {
            ui.header(t.uninstalled, t.uninstalled_hint, MUTED);
            ui.button(Id::Close, t.close, right, bottom, ACCENT);
        }
    }
    ui.hits
}

fn step_text(t: &Text, step: Option<Step>) -> &'static str {
    match step {
        Some(Step::Closing) => t.closing,
        Some(Step::Copying) | None => t.copying,
        Some(Step::Shortcuts) => t.shortcuts,
        Some(Step::Registering) => t.registering,
        Some(Step::Removing) => t.removing,
    }
}

/// Shortens `text` in the middle until it fits `width`, keeping the end of a path visible.
fn ellipsize(text: &str, width: f32, size: f32) -> String {
    if font::measure(text, size).0 <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mid = chars.len() * 2 / 5;
    let (mut a, mut b) = (mid, mid);
    loop {
        if a > 0 && (b - a) % 2 == 0 {
            a -= 1;
        } else if b < chars.len() {
            b += 1;
        } else if a > 0 {
            a -= 1;
        } else {
            break;
        }
        let candidate: String = chars[..a]
            .iter()
            .chain(['…'].iter())
            .chain(&chars[b..])
            .collect();
        if font::measure(&candidate, size).0 <= width {
            return candidate;
        }
    }
    text.to_string()
}

struct Ui<'a> {
    pixmap: &'a mut Pixmap,
    scale: f32,
    hover: Option<Id>,
    hits: Vec<(Id, Rect)>,
}

impl Ui<'_> {
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: u32) {
        let s = self.scale;
        let r = Rect::from_xywh(x * s, y * s, w * s, h * s);
        fill_round(self.pixmap, r, radius * s, color, 1.0);
    }

    fn label(&mut self, text: &str, x: f32, y: f32, size: f32, color: u32) {
        let s = self.scale;
        font::draw(self.pixmap, text, x * s, y * s, size * s, color, 1.0);
    }

    /// Text broken into lines that fit `width`; returns the y below it.
    fn wrapped(&mut self, text: &str, x: f32, y: f32, width: f32, size: f32, color: u32) -> f32 {
        let mut line = String::new();
        let mut y = y;
        let step = font::line_height(size) * 1.1;
        for word in text.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if font::measure(&candidate, size).0 > width && !line.is_empty() {
                self.label(&line, x, y, size, color);
                y += step;
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        self.label(&line, x, y, size, color);
        y + step
    }

    fn logo(&mut self, logo: Logo, x: f32, y: f32, size: f32) {
        let s = self.scale;
        logos::draw(self.pixmap, logo, x * s, y * s, size * s);
    }

    fn hit(&mut self, id: Id, x: f32, y: f32, w: f32, h: f32) -> bool {
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            self.hits.push((id, r));
        }
        self.hover == Some(id)
    }

    /// The small logo with a title and a line under it.
    fn header(&mut self, title: &str, hint: &str, hint_color: u32) {
        self.logo(Logo::Snapt, MARGIN, 26.0, 40.0);
        if hint.is_empty() {
            self.label(title, 80.0, 35.0, 17.0, TEXT);
        } else {
            self.label(title, 80.0, 25.0, 17.0, TEXT);
            self.label(hint, 80.0, 50.0, 12.0, hint_color);
        }
    }

    fn progress(&mut self, value: f32) {
        let w = WIDTH - 2.0 * MARGIN;
        self.fill(MARGIN, 88.0, w, 6.0, 3.0, CONTROL);
        if value > 0.0 {
            self.fill(
                MARGIN,
                88.0,
                (w * value.min(1.0)).max(6.0),
                6.0,
                3.0,
                ACCENT,
            );
        }
    }

    /// A row with `text` on the left and a switch on the right; the whole row toggles.
    fn switch(&mut self, id: Id, text: &str, hint: Option<&str>, y: f32, on: bool) {
        let row = if hint.is_some() { 44.0 } else { 36.0 };
        let hovered = self.hit(id, MARGIN, y, WIDTH - 2.0 * MARGIN, row);
        match hint {
            Some(hint) => {
                self.label(text, MARGIN, y + 4.0, 13.0, TEXT);
                self.label(hint, MARGIN, y + 23.0, 11.0, MUTED);
            }
            None => self.label(text, MARGIN, y + 10.0, 13.0, TEXT),
        }
        let x = WIDTH - MARGIN - 34.0;
        let ty = y + (row - 20.0) / 2.0;
        let track = match (on, hovered) {
            (true, _) => ACCENT,
            (false, true) => CONTROL_HOVER,
            (false, false) => CONTROL,
        };
        self.fill(x, ty, 34.0, 20.0, 10.0, track);
        let knob = if on { x + 16.0 } else { x + 2.0 };
        self.fill(knob, ty + 2.0, 16.0, 16.0, 8.0, TEXT);
    }

    /// A button whose right edge is at `right`; returns its left edge.
    fn button(&mut self, id: Id, text: &str, right: f32, y: f32, color: u32) -> f32 {
        let tw = font::measure(text, 13.0).0;
        let w = (tw + 36.0).max(104.0);
        let x = right - w;
        let hovered = self.hit(id, x, y, w, BUTTON_H);
        let color = match (color, hovered) {
            (ACCENT, true) => ACCENT_HOVER,
            (DANGER, true) => DANGER_HOVER,
            (CONTROL, true) => CONTROL_HOVER,
            (c, _) => c,
        };
        self.fill(x, y, w, BUTTON_H, 8.0, color);
        self.label(text, x + (w - tw) / 2.0, y + 10.0, 13.0, TEXT);
        x
    }

    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, id: Id, logo: Logo, name: &str, kind: &str, about: &str, x: f32, w: f32) {
        let (y, h) = (142.0, 120.0);
        let hovered = self.hit(id, x, y, w, h);
        self.fill(x, y, w, h, 10.0, if hovered { CARD_HOVER } else { CARD });
        self.logo(logo, x + 14.0, y + 14.0, 34.0);
        self.label(name, x + 58.0, y + 14.0, 14.0, TEXT);
        self.label(kind, x + 58.0, y + 33.0, 10.5, MUTED);
        self.wrapped(about, x + 14.0, y + 60.0, w - 28.0, 11.0, MUTED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text;

    fn view<'a>(page: &'a Page, t: &'static Text) -> View<'a> {
        View {
            text: t,
            page,
            desktop: true,
            all_users: false,
            dir: r"C:\Users\Patrik\AppData\Local\Snapt",
            action: t.install,
            version: "2.1.0",
            progress: if *page == Page::Confirm { 0.0 } else { 0.6 },
            step: Some(Step::Copying),
            hover: None,
        }
    }

    /// Draws every page in every language. With SNAPT_SETUP_PREVIEW=<dir> it also saves them as PNG.
    #[test]
    fn draws_every_page() {
        let pages = [
            (Page::Welcome, false),
            (Page::Working, false),
            (Page::Done, false),
            (
                Page::Failed(
                    "C:\\Users\\Patrik\\AppData\\Local\\Snapt\\snapt.exe is in use.".into(),
                ),
                false,
            ),
            (Page::Confirm, true),
            (Page::Removed, true),
        ];
        let out = std::env::var_os("SNAPT_SETUP_PREVIEW").map(std::path::PathBuf::from);
        for (lang, t) in text::ALL.into_iter().enumerate() {
            for (n, (page, uninstall)) in pages.iter().enumerate() {
                let scale = 2.0;
                let h = height(*uninstall);
                let mut pixmap = Pixmap::new((WIDTH * scale) as u32, (h * scale) as u32).unwrap();
                let hits = draw(&mut pixmap, scale, &view(page, t), *uninstall);
                if let Some(id) = default_button(page) {
                    assert!(hits.iter().any(|(i, _)| *i == id), "{page:?} has no {id:?}");
                }
                for (_, r) in &hits {
                    assert!(r.right() <= WIDTH && r.bottom() <= h, "{page:?} overflows");
                }
                if let Some(dir) = &out {
                    pixmap
                        .save_png(dir.join(format!("{lang}-{n}.png")))
                        .unwrap();
                }
            }
        }
    }

    #[test]
    fn long_paths_are_shortened_in_the_middle() {
        let long = format!(
            "Folder: C:\\{}\\Snapt",
            "very long folder name\\".repeat(10)
        );
        let short = ellipsize(&long, 300.0, 11.0);
        assert!(font::measure(&short, 11.0).0 <= 300.0);
        assert!(short.starts_with("Folder") && short.ends_with("Snapt") && short.contains('…'));
    }
}
