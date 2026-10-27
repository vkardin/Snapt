//! Snapt's own installer for Windows: one small window that installs Snapt
//! per user or for everyone, adds shortcuts and an Apps entry, and removes it
//! all again when run as `uninstall.exe --uninstall`.
//!
//! Switches: `/S` (or `--silent`) installs or uninstalls without a window,
//! `--update` installs right away and restarts Snapt (the updater uses it),
//! `--all-users` and `--no-desktop` preset the two choices.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[allow(dead_code)]
#[path = "../../src/font.rs"]
mod font;
mod logos;
#[allow(dead_code)]
#[path = "../../src/paint.rs"]
mod paint;
mod system;
mod text;
mod ui;

use std::{
    num::NonZeroU32,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use softbuffer::{Context, Surface};
use tiny_skia::{Pixmap, Rect};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle},
    keyboard::{Key, NamedKey},
    window::{CursorIcon, Theme, Window, WindowButtons, WindowId},
};

use system::{Existing, Plan, Step};
use ui::{Id, Page};

#[derive(Debug, Default)]
struct Args {
    uninstall: bool,
    silent: bool,
    update: bool,
    all_users: bool,
    auto: bool,
    desktop: Option<bool>,
    dir: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.to_lowercase().as_str() {
            "/s" | "/silent" | "/verysilent" | "--silent" => args.silent = true,
            "--uninstall" => args.uninstall = true,
            "--update" => args.update = true,
            "--all-users" => args.all_users = true,
            "--auto" => args.auto = true,
            "--desktop" => args.desktop = Some(true),
            "--no-desktop" => args.desktop = Some(false),
            "--dir" => args.dir = it.next().map(PathBuf::from),
            _ => {}
        }
    }
    // Apps & Features runs uninstall.exe; take its name as a hint too.
    let me = std::env::current_exe().ok();
    if me
        .as_ref()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("uninstall"))
    {
        args.uninstall = true;
    }
    args
}

fn main() {
    let args = parse_args();
    let code = if args.uninstall {
        uninstall_main(args)
    } else {
        install_main(args)
    };
    std::process::exit(code);
}

fn install_main(args: Args) -> i32 {
    let existing = system::existing();
    let all_users = args.all_users || existing.as_ref().is_some_and(|e| e.all_users);
    let desktop = args
        .desktop
        .or(existing.as_ref().map(|e| e.desktop))
        .unwrap_or(true);
    let dir = dir_for(existing.as_ref(), all_users);
    if args.silent {
        let plan = Plan {
            dir: dir.clone(),
            all_users,
            desktop,
        };
        return match system::install(&plan, &|_, _| {}) {
            Ok(was_running) => {
                if was_running || args.update {
                    system::launch(&dir);
                }
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    let start = args.auto || args.update;
    run_window(Setup {
        uninstall: false,
        update: args.update,
        existing,
        all_users,
        desktop,
        dir,
        page: Page::Welcome,
        start,
    })
}

fn uninstall_main(args: Args) -> i32 {
    let Some(dir) = args
        .dir
        .clone()
        .or_else(|| std::env::current_exe().ok()?.parent().map(PathBuf::from))
    else {
        return 1;
    };
    if args.dir.is_none() {
        // A program can't delete its own folder, so run a copy from the temp folder.
        let copy = system::uninstall_copy();
        let me = match std::env::current_exe() {
            Ok(me) => me,
            Err(_) => return 1,
        };
        if std::fs::copy(&me, &copy).is_err() {
            return 1;
        }
        let mut pass = vec![
            "--uninstall".to_string(),
            "--dir".into(),
            dir.display().to_string(),
        ];
        if args.silent {
            pass.push("--silent".into());
        }
        let elevate = system::needs_admin(&dir) && !system::is_admin();
        return match system::run(&copy, &pass, elevate) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    let code = if args.silent {
        match system::uninstall(&dir, &|_, _| {}) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    } else {
        run_window(Setup {
            uninstall: true,
            update: false,
            existing: None,
            all_users: system::needs_admin(&dir),
            desktop: false,
            dir,
            page: Page::Confirm,
            start: false,
        })
    };
    system::delete_self_later();
    code
}

/// The folder to install to: the existing install's, when its kind matches.
fn dir_for(existing: Option<&Existing>, all_users: bool) -> PathBuf {
    match existing {
        Some(e) if e.all_users == all_users => e.dir.clone(),
        _ => system::install_dir(all_users),
    }
}

/// The window's starting point.
struct Setup {
    uninstall: bool,
    update: bool,
    existing: Option<Existing>,
    all_users: bool,
    desktop: bool,
    dir: PathBuf,
    page: Page,
    /// Starts the work as soon as the window is up.
    start: bool,
}

enum Event {
    Progress(f32, Step),
    Finished(Result<bool, String>),
}

fn run_window(setup: Setup) -> i32 {
    let event_loop = match EventLoop::<Event>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("failed to open a window: {e}");
            return 1;
        }
    };
    let mut app = App {
        proxy: event_loop.create_proxy(),
        text: text::pick(),
        setup,
        window: None,
        busy: false,
        target: 0.0,
        shown: 0.0,
        step: None,
        finished: None,
        hover: None,
        hits: Vec::new(),
        code: 0,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("{e}");
        return 1;
    }
    app.code
}

struct Shown {
    window: Rc<Window>,
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    _context: Context<OwnedDisplayHandle>,
}

struct App {
    proxy: EventLoopProxy<Event>,
    text: &'static text::Text,
    setup: Setup,
    window: Option<Shown>,
    busy: bool,
    /// The progress reported by the work, and the one drawn, which eases towards it.
    target: f32,
    shown: f32,
    step: Option<Step>,
    finished: Option<bool>,
    hover: Option<Id>,
    hits: Vec<(Id, Rect)>,
    code: i32,
}

impl App {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let size = LogicalSize::new(ui::WIDTH as f64, ui::height(self.setup.uninstall) as f64);
        let icon = winit::window::Icon::from_rgba(logos::snapt_rgba(64), 64, 64).ok();
        let mut attributes = Window::default_attributes()
            .with_title(self.text.window)
            .with_inner_size(size)
            .with_resizable(false)
            .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
            .with_theme(Some(Theme::Dark))
            .with_window_icon(icon)
            .with_visible(false);
        if let Some(monitor) = event_loop.primary_monitor() {
            let scale = monitor.scale_factor();
            let (m, at) = (monitor.size(), monitor.position());
            let w = (size.width * scale) as i32;
            let h = (size.height * scale) as i32;
            attributes = attributes.with_position(PhysicalPosition::new(
                at.x + (m.width as i32 - w) / 2,
                at.y + (m.height as i32 - h) / 2,
            ));
        }
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        let context = Context::new(event_loop.owned_display_handle()).map_err(|e| e.to_string())?;
        let surface = Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
        window.set_visible(true);
        window.focus_window();
        self.window = Some(Shown {
            window,
            surface,
            _context: context,
        });
        Ok(())
    }

    fn view_page(&self) -> &Page {
        &self.setup.page
    }

    fn redraw(&mut self) {
        let Some(shown) = &mut self.window else {
            return;
        };
        let size = shown.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if shown.surface.resize(w, h).is_err() {
            return;
        }
        let Some(mut pixmap) = Pixmap::new(size.width, size.height) else {
            return;
        };
        let scale = shown.window.scale_factor() as f32;
        let t = self.text;
        let action = match &self.setup.existing {
            Some(e) if e.version == system::VERSION => t.reinstall,
            Some(_) => t.update,
            None => t.install,
        };
        let dir = self.setup.dir.display().to_string();
        let view = ui::View {
            text: t,
            page: &self.setup.page,
            desktop: self.setup.desktop,
            all_users: self.setup.all_users,
            dir: &dir,
            action,
            version: system::VERSION,
            progress: self.shown,
            step: self.step,
            hover: self.hover,
        };
        self.hits = ui::draw(&mut pixmap, scale, &view, self.setup.uninstall);
        let Some(shown) = &mut self.window else {
            return;
        };
        let Ok(mut buffer) = shown.surface.buffer_mut() else {
            return;
        };
        for (dst, src) in buffer.iter_mut().zip(pixmap.data().chunks_exact(4)) {
            *dst = (src[0] as u32) << 16 | (src[1] as u32) << 8 | src[2] as u32;
        }
        let _ = buffer.present();
    }

    fn request_redraw(&self) {
        if let Some(shown) = &self.window {
            shown.window.request_redraw();
        }
    }

    /// Starts installing or uninstalling on a worker thread.
    fn start(&mut self, event_loop: &ActiveEventLoop) {
        let s = &mut self.setup;
        if !s.uninstall && s.all_users && !system::is_admin() {
            // Ask for administrator rights by starting again elevated, with the same choices.
            let mut args = vec!["--all-users".to_string(), "--auto".into()];
            args.push(
                if s.desktop {
                    "--desktop"
                } else {
                    "--no-desktop"
                }
                .into(),
            );
            if s.update {
                args.push("--update".into());
            }
            if let Ok(me) = std::env::current_exe()
                && system::run(&me, &args, true).is_ok()
            {
                event_loop.exit();
            }
            // Declined: stay on the first page.
            return;
        }
        self.busy = true;
        self.target = 0.0;
        self.shown = if s.uninstall { 0.001 } else { 0.0 };
        self.step = None;
        self.finished = None;
        if !s.uninstall {
            s.page = Page::Working;
        } else {
            s.page = Page::Confirm;
        }
        let proxy = self.proxy.clone();
        let uninstall = s.uninstall;
        let plan = Plan {
            dir: s.dir.clone(),
            all_users: s.all_users,
            desktop: s.desktop,
        };
        std::thread::spawn(move || {
            let progress = |v, step| {
                let _ = proxy.send_event(Event::Progress(v, step));
            };
            let result = if uninstall {
                system::uninstall(&plan.dir, &progress).map(|()| false)
            } else {
                system::install(&plan, &progress)
            };
            let _ = proxy.send_event(Event::Finished(result));
        });
        self.request_redraw();
    }

    /// The work is done and the bar is full.
    fn finish(&mut self, event_loop: &ActiveEventLoop, was_running: bool) {
        self.busy = false;
        if self.setup.uninstall {
            self.setup.page = Page::Removed;
        } else if self.setup.update || was_running {
            system::launch(&self.setup.dir);
            if self.setup.update {
                event_loop.exit();
                return;
            }
            self.setup.page = Page::Done;
        } else {
            self.setup.page = Page::Done;
        }
        self.request_redraw();
    }

    fn click(&mut self, event_loop: &ActiveEventLoop, id: Id) {
        let s = &mut self.setup;
        match id {
            Id::Desktop if s.page == Page::Welcome => s.desktop = !s.desktop,
            Id::AllUsers if s.page == Page::Welcome => {
                s.all_users = !s.all_users;
                s.dir = dir_for(s.existing.as_ref(), s.all_users);
            }
            Id::Install | Id::Retry | Id::Uninstall if !self.busy => self.start(event_loop),
            Id::Launch => {
                system::launch(&s.dir);
                event_loop.exit();
            }
            Id::Close if !self.busy => {
                if matches!(s.page, Page::Failed(_)) {
                    self.code = 1;
                }
                event_loop.exit();
            }
            Id::Leveler => system::open_url(ui::LEVELER_URL),
            Id::VideoGlow => system::open_url(ui::VIDEO_GLOW_URL),
            _ => {}
        }
        self.request_redraw();
    }

    fn cursor_moved(&mut self, x: f32, y: f32) {
        let hover = self
            .hits
            .iter()
            .find(|(_, r)| x >= r.left() && x < r.right() && y >= r.top() && y < r.bottom())
            .map(|(id, _)| *id);
        if hover != self.hover {
            self.hover = hover;
            if let Some(shown) = &self.window {
                shown.window.set_cursor(if hover.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                });
            }
            self.request_redraw();
        }
    }
}

impl ApplicationHandler<Event> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(e) = self.open(event_loop) {
            eprintln!("failed to open the window: {e}");
            self.code = 1;
            event_loop.exit();
            return;
        }
        if std::mem::take(&mut self.setup.start) {
            self.start(event_loop);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Progress(v, step) => {
                self.target = self.target.max(v);
                self.step = Some(step);
            }
            Event::Finished(Ok(was_running)) => {
                self.target = 1.0;
                self.finished = Some(was_running);
            }
            Event::Finished(Err(e)) => {
                self.busy = false;
                self.shown = 0.0;
                self.setup.page = Page::Failed(e);
            }
        }
        self.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested if !self.busy => {
                if matches!(self.view_page(), Page::Failed(_)) {
                    self.code = 1;
                }
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw()
            }
            WindowEvent::CursorMoved { position, .. } => {
                let s = self
                    .window
                    .as_ref()
                    .map_or(1.0, |w| w.window.scale_factor());
                self.cursor_moved((position.x / s) as f32, (position.y / s) as f32);
            }
            WindowEvent::CursorLeft { .. } => self.cursor_moved(-1.0, -1.0),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(id) = self.hover {
                    self.click(event_loop, id);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Enter) => {
                    if let Some(id) = ui::default_button(self.view_page()) {
                        self.click(event_loop, id);
                    }
                }
                Key::Named(NamedKey::Escape) => self.click(event_loop, Id::Close),
                _ => {}
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.busy {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        // Ease the bar towards the reported progress, so even a fast install reads as one motion.
        let gap = self.target - self.shown;
        if gap > 0.0 {
            self.shown = (self.shown + gap * 0.12 + 0.004).min(self.target);
            self.request_redraw();
        }
        if self.shown >= 1.0
            && let Some(was_running) = self.finished
        {
            self.finish(event_loop, was_running);
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}
