use anyhow::{Context, Result};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use tray_icon::menu::MenuEvent;
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
    window::WindowId,
};

use crate::{
    capture,
    history::History,
    i18n::{self, T, t},
    instance::Request,
    ocr, output,
    overlay::{Area, Outcome, Overlay, Style},
    pin::{Pin, PinAction},
    platform,
    scroll::{Finished, Scroll, ScrollAction},
    settings::{self, CopyAs, Feature, ReplaceMode, Settings, Shortcut},
    settings_window::{Action, FEATURES, Notice, Section, SettingsWindow, TOOLS, UpdateStatus},
    tray::{Command, Tray},
    updater::{self, Update},
};

/// Events sent into the winit loop from tray, hotkey and other-instance callbacks.
#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuEvent),
    Hotkey(GlobalHotKeyEvent),
    /// Another copy of Snapt was started and asks for something.
    Request(Request),
    /// An update check finished; the flag tells whether the user asked for it.
    UpdateChecked(Result<Option<Update>, String>, bool),
    /// The installer of an update was downloaded.
    UpdateDownloaded(Result<std::path::PathBuf, String>),
    /// Text recognition on a selection finished.
    Recognized(Result<String, ocr::Error>),
    /// The scrolling screenshot grew.
    ScrollGrew,
    /// A system screenshot key was caught while Snapt replaces the system tool.
    #[cfg(target_os = "windows")]
    SystemKey(crate::windows_shot::SystemKey),
    /// The user answered whether Snapt should replace the system screenshot tool.
    ReplaceAnswer(bool),
    /// The desktop portal answered a request for the capture shortcut.
    #[cfg(target_os = "linux")]
    PortalShortcut(String, Result<crate::portal::Shortcut, String>),
}

/// The state of the portal shortcut.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct Portal {
    shortcut: Option<crate::portal::Shortcut>,
    /// The hotkey of a bind request that has not been answered yet.
    pending: Option<String>,
    /// The desktop has no portal for shortcuts, so Snapt doesn't ask again.
    failed: bool,
}

pub struct App {
    // The manager unregisters its hotkeys when dropped, so it must live as long as the app.
    hotkeys: GlobalHotKeyManager,
    /// The registered shortcuts, by [`Shortcut`].
    registered: [Option<HotKey>; Shortcut::ALL.len()],
    tray: Option<Tray>,
    overlay: Option<Overlay>,
    pins: Vec<Pin>,
    scroll: Option<Scroll>,
    history: History,
    /// The area selected last time, for "Repeat last area".
    last_area: Option<Area>,
    settings_window: Option<SettingsWindow>,
    settings: Settings,
    style: Style,
    /// What to open as soon as the app starts.
    start: Option<Request>,
    output: output::Output,
    proxy: EventLoopProxy<UserEvent>,
    update: Option<Update>,
    update_status: UpdateStatus,
    watching_updates: bool,
    /// The capture shortcut on Wayland, bound through the desktop portal.
    #[cfg(target_os = "linux")]
    portal: Portal,
}

impl App {
    pub fn new(start: Option<Request>, proxy: EventLoopProxy<UserEvent>) -> Result<Self> {
        let hotkeys = GlobalHotKeyManager::new().context("failed to start the hotkey manager")?;
        let settings = Settings::load();
        settings.apply_features();
        i18n::set(settings.lang());
        let style = Style {
            tool: None,
            color: settings.color,
            stroke: settings.stroke,
            text_size: settings.text_size,
            fill: false,
        };
        let mut app = Self {
            hotkeys,
            registered: [None; Shortcut::ALL.len()],
            tray: None,
            overlay: None,
            pins: Vec::new(),
            scroll: None,
            history: History::load(settings.history),
            last_area: None,
            settings_window: None,
            settings,
            style,
            start,
            output: output::Output::new()?,
            proxy,
            update: None,
            update_status: UpdateStatus::Idle,
            watching_updates: false,
            #[cfg(target_os = "linux")]
            portal: Portal::default(),
        };
        app.register_hotkeys();
        app.start_replacing();
        if app.settings.auto_update {
            app.watch_updates();
        }
        Ok(app)
    }

    /// Takes over from the system screenshot tool at start, as the settings say.
    /// Only Windows needs this on every start: elsewhere the change stays made.
    fn start_replacing(&mut self) {
        if !platform::REPLACE_MODES {
            return;
        }
        match self.settings.replace_system {
            ReplaceMode::Off => {}
            ReplaceMode::Ask if platform::system_tool_active() => self.ask_to_replace(),
            ReplaceMode::Ask | ReplaceMode::Auto => {
                self.take_over(false);
            }
        }
    }

    /// Asks in a dialog whether Snapt may replace the system screenshot tool;
    /// the answer comes back as [`UserEvent::ReplaceAnswer`].
    fn ask_to_replace(&self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let answer = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Info)
                .set_title("Snapt")
                .set_description(t(T::ReplaceQuestion))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            let yes = answer == rfd::MessageDialogResult::Yes;
            let _ = proxy.send_event(UserEvent::ReplaceAnswer(yes));
        });
    }

    /// Hands the screenshot keys to Snapt. With `confirm`, also opens the page
    /// where the user makes Snapt the default for the screenshot links, which
    /// only the user can do.
    fn take_over(&mut self, confirm: bool) -> bool {
        if let Err(err) =
            platform::replace_system_screenshot(true, self.settings.shortcut(Shortcut::Capture))
        {
            log::error!("failed to replace the system screenshot tool: {err:#}");
            return false;
        }
        platform::catch_system_keys(Some(self.proxy.clone()));
        if confirm {
            platform::confirm_links();
        }
        // The system tool may have held the key until now.
        self.register_hotkey(Shortcut::Capture);
        true
    }

    /// Gives the keys and links back to the system screenshot tool.
    fn give_back(&mut self) -> bool {
        platform::catch_system_keys(None);
        platform::release_links();
        match platform::replace_system_screenshot(false, self.settings.shortcut(Shortcut::Capture))
        {
            Ok(()) => true,
            Err(err) => {
                log::error!("failed to give the keys back to the system tool: {err:#}");
                false
            }
        }
    }

    /// Switches the replacement to `mode`, taking over or giving back as needed.
    fn set_replace_mode(&mut self, mode: ReplaceMode) -> Option<Notice> {
        let was_on = self.settings.replace_system.is_on();
        let done = match (was_on, mode.is_on()) {
            (false, true) => self.take_over(true),
            (true, false) => self.give_back(),
            _ => true,
        };
        if !done {
            return Some(Notice::ReplaceFailed);
        }
        self.settings.replace_system = mode;
        None
    }

    /// Runs a global shortcut.
    fn run_shortcut(&mut self, event_loop: &ActiveEventLoop, which: Shortcut) {
        match which {
            Shortcut::Capture => self.start_capture(event_loop, None, false),
            Shortcut::Pick => self.start_capture(event_loop, None, true),
            Shortcut::FullScreen => self.copy_full_screen(),
            Shortcut::RepeatArea => self.start_capture(event_loop, self.last_area, false),
            Shortcut::PinLast if !self.history.entries().is_empty() => {
                self.pin_from_history(event_loop, 0)
            }
            _ => {}
        }
    }

    /// (Re)registers one shortcut from the settings. Returns false when the
    /// shortcut is invalid or another app owns it; Snapt then still works from
    /// the tray menu.
    fn register_hotkey(&mut self, which: Shortcut) -> bool {
        if !which.is_global() {
            return true;
        }
        #[cfg(target_os = "linux")]
        if which == Shortcut::Capture && platform::is_wayland() && !self.portal.failed {
            self.bind_portal_shortcut();
            return true;
        }
        self.unregister_hotkey(which);
        let keys = self.settings.shortcut(which).to_string();
        let off = which == Shortcut::Pick && !self.settings.is_enabled(Feature::Picker);
        if keys.is_empty() || off {
            return true;
        }
        let hotkey: HotKey = match keys.parse() {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("bad shortcut {keys:?}: {err}");
                return false;
            }
        };
        match self.hotkeys.register(hotkey) {
            Ok(()) => {
                self.registered[which as usize] = Some(hotkey);
                true
            }
            Err(err) => {
                log::warn!("could not register {keys}: {err}");
                false
            }
        }
    }

    /// Asks the desktop portal for the capture shortcut in the background, as
    /// the desktop may first ask the user. The answer comes back as
    /// [`UserEvent::PortalShortcut`].
    #[cfg(target_os = "linux")]
    fn bind_portal_shortcut(&mut self) {
        let hotkey = self.settings.shortcut(Shortcut::Capture).to_string();
        let current = self.portal.shortcut.as_ref().map(|s| &s.trigger);
        if current == Some(&hotkey) || self.portal.pending.as_ref() == Some(&hotkey) {
            return;
        }
        // Closing the old session unbinds the old key.
        self.portal.shortcut = None;
        self.portal.pending = Some(hotkey.clone());
        let proxy = self.proxy.clone();
        let description = i18n::t(i18n::T::TakeScreenshot).to_string();
        std::thread::spawn(move || {
            let events = proxy.clone();
            let pressed = move || {
                events
                    .send_event(UserEvent::Request(Request::Capture))
                    .is_ok()
            };
            let result = crate::portal::Shortcut::bind(&hotkey, &description, pressed)
                .map_err(|e| format!("{e:#}"));
            let _ = proxy.send_event(UserEvent::PortalShortcut(hotkey, result));
        });
    }

    #[cfg(target_os = "linux")]
    fn portal_answered(&mut self, hotkey: String, result: Result<crate::portal::Shortcut, String>) {
        if self.portal.pending.as_ref() != Some(&hotkey) {
            // A newer request is on its way; this shortcut drops and unbinds.
            return;
        }
        self.portal.pending = None;
        match result {
            Ok(shortcut) => self.portal.shortcut = Some(shortcut),
            Err(err) => {
                log::warn!(
                    "no global shortcut from the desktop portal ({err}); \
                     bind `snapt --capture` to a key in the system settings"
                );
                self.portal.failed = true;
                self.register_hotkey(Shortcut::Capture);
            }
        }
    }

    fn register_hotkeys(&mut self) {
        for which in Shortcut::ALL {
            self.register_hotkey(which);
        }
    }

    fn unregister_hotkey(&mut self, which: Shortcut) {
        if let Some(old) = self.registered[which as usize].take() {
            let _ = self.hotkeys.unregister(old);
        }
    }

    fn save_settings(&self) {
        if let Err(err) = self.settings.save() {
            log::error!("{err:#}");
        }
        if let Some(tray) = &self.tray {
            tray.sync(&self.settings);
        }
    }

    /// Freezes the screens and opens the selection overlay, with `area`
    /// already selected when given, or with `pick` only the color picker.
    fn start_capture(&mut self, event_loop: &ActiveEventLoop, area: Option<Area>, pick: bool) {
        if self.overlay.is_some() {
            return;
        }
        if !platform::screen_access() {
            return;
        }
        let settings = &self.settings;
        // The picker reads the pixels under the pointer, so it never gets a drawn one.
        let result = capture::all_monitors(settings.capture_cursor && !pick)
            .and_then(|shots| Overlay::open(event_loop, shots, self.style, area, pick, settings));
        match result {
            Ok(overlay) => self.overlay = Some(overlay),
            Err(err) => log::error!("screenshot failed: {err:#}"),
        }
    }

    fn finish_capture(&mut self, event_loop: &ActiveEventLoop, outcome: Outcome) {
        // Close the overlay first so it is not in the way of the save dialog.
        if let Some(overlay) = self.overlay.take() {
            self.style = overlay.style();
            if let Some(area) = overlay.area() {
                self.last_area = Some(area);
                if let Some(tray) = &self.tray {
                    tray.set_can_repeat(true);
                }
            }
        }
        let (color, stroke, text_size) =
            (self.style.color, self.style.stroke, self.style.text_size);
        let mut changed = (
            self.settings.color,
            self.settings.stroke,
            self.settings.text_size,
        ) != (color, stroke, text_size);
        self.settings.color = color;
        self.settings.stroke = stroke;
        self.settings.text_size = text_size;

        let result = match &outcome {
            Outcome::Cancel => Ok(()),
            Outcome::Copy(shot) => self.copy(shot),
            Outcome::Save(shot) => {
                changed |= self.save(shot);
                Ok(())
            }
            Outcome::Print(shot) => self.output.print(shot),
            Outcome::Search(shot) => self.output.search(shot),
            Outcome::Pin { .. } | Outcome::Scroll { .. } => Ok(()),
        };
        if let Err(err) = result {
            log::error!("{err:#}");
        }
        match outcome {
            Outcome::Cancel => {}
            Outcome::Copy(shot)
            | Outcome::Save(shot)
            | Outcome::Print(shot)
            | Outcome::Search(shot) => self.remember(&shot),
            Outcome::Pin {
                image,
                position,
                size,
            } => {
                self.remember(&image);
                self.pin(event_loop, image, position, size);
            }
            Outcome::Scroll {
                region,
                panel,
                mode,
            } => match Scroll::start(event_loop, self.proxy.clone(), region, panel, mode) {
                Ok(scroll) => self.scroll = Some(scroll),
                Err(err) => log::error!("{err:#}"),
            },
        }
        if changed {
            self.save_settings();
        }
    }

    /// Copies recognized text and closes the overlay, or says in it why there is none.
    fn recognized(&mut self, event_loop: &ActiveEventLoop, result: Result<String, ocr::Error>) {
        let status = match result {
            Ok(text) if !text.is_empty() => {
                match self.output.copy_text(&text) {
                    Ok(()) => log::info!("copied {} recognized characters", text.chars().count()),
                    Err(err) => log::error!("{err:#}"),
                }
                if self.overlay.is_some() {
                    self.finish_capture(event_loop, Outcome::Cancel);
                }
                return;
            }
            Ok(_) => t(T::NoText),
            Err(ocr::Error::Missing) => t(platform::ocr_missing_hint()),
            Err(ocr::Error::Failed(err)) => {
                log::error!("text recognition failed: {err}");
                t(T::RecognizeFailed)
            }
        };
        if let Some(overlay) = &mut self.overlay {
            overlay.set_status(Some(status.to_string()));
        }
    }

    fn copy(&mut self, shot: &image::RgbaImage) -> Result<()> {
        match self.settings.copy_as {
            CopyAs::Image => {
                self.output.copy(shot)?;
                log::info!("copied {}×{} to the clipboard", shot.width(), shot.height());
                if self.settings.save_on_copy {
                    // Straight into the folder: a dialog on every copy would defeat the point.
                    let path = self.output.save_to_folder(shot, &self.settings)?;
                    log::info!("saved {}", path.display());
                }
            }
            CopyAs::File => {
                // The file pasted is the saved one when there is one, else a temporary copy.
                let path = if self.settings.save_on_copy {
                    let path = self.output.save_to_folder(shot, &self.settings)?;
                    log::info!("saved {}", path.display());
                    path
                } else {
                    self.output.save_for_copy(shot, &self.settings)?
                };
                self.output.copy_file(&path)?;
                log::info!("copied {} to the clipboard", path.display());
            }
        }
        Ok(())
    }

    /// Saves `shot`; returns whether the settings changed (the folder picked becomes the default).
    fn save(&mut self, shot: &image::RgbaImage) -> bool {
        let folder = self.settings.folder.clone();
        match self.output.save(shot, &mut self.settings) {
            Ok(Some(path)) => {
                log::info!("saved {}", path.display());
                if self.settings.open_after_save
                    && let Err(err) = open::that_detached(&path)
                {
                    log::error!("failed to open {}: {err}", path.display());
                }
            }
            Ok(None) => {}
            Err(err) => log::error!("{err:#}"),
        }
        folder != self.settings.folder
    }

    /// Adds `shot` to the history and the tray menu.
    fn remember(&mut self, shot: &image::RgbaImage) {
        self.history.add(shot);
        if let Some(tray) = &mut self.tray {
            tray.set_history(self.history.entries());
        }
    }

    /// Copies all screens at once, without the overlay.
    fn copy_full_screen(&mut self) {
        if !platform::screen_access() {
            return;
        }
        match capture::all_monitors(self.settings.capture_cursor) {
            Ok(shots) => {
                let shot = capture::stitch(&shots);
                if let Err(err) = self.copy(&shot) {
                    log::error!("{err:#}");
                }
                self.remember(&shot);
            }
            Err(err) => log::error!("screenshot failed: {err:#}"),
        }
    }

    /// Opens the overlay after `seconds`, so menus and tooltips can be opened meanwhile.
    fn capture_later(&self, seconds: u64) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            let _ = proxy.send_event(UserEvent::Request(Request::Capture));
        });
    }

    /// Pins a screenshot from the history in the middle of the main screen.
    fn pin_from_history(&mut self, event_loop: &ActiveEventLoop, index: usize) {
        let image = match self.history.open(index) {
            Ok(image) => image,
            Err(err) => {
                log::error!("{err:#}");
                return;
            }
        };
        let (position, size) = centered(event_loop, image.dimensions());
        self.pin(event_loop, image, position, size);
    }

    fn pin(
        &mut self,
        event_loop: &ActiveEventLoop,
        image: image::RgbaImage,
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    ) {
        match Pin::open(event_loop, image, position, size) {
            Ok(pin) => self.pins.push(pin),
            Err(err) => log::error!("{err:#}"),
        }
    }

    /// Copies the finished scrolling screenshot and pins it to look it over,
    /// or asks where to save the finished recording.
    fn finish_scroll(&mut self, event_loop: &ActiveEventLoop) {
        let image = match self.scroll.take().and_then(Scroll::finish) {
            Some(Finished::Image(image)) => image,
            Some(Finished::Video(video)) => {
                match self.output.save_video(&video, &self.settings) {
                    Ok(Some(path)) => log::info!("saved {}", path.display()),
                    Ok(None) => {}
                    Err(err) => log::error!("{err:#}"),
                }
                return;
            }
            None => return,
        };
        if let Err(err) = self.copy(&image) {
            log::error!("{err:#}");
        }
        self.remember(&image);
        let (position, size) = centered(event_loop, image.dimensions());
        self.pin(event_loop, image, position, size);
    }

    fn pin_event(&mut self, index: usize, event: WindowEvent) {
        let Some(action) = self.pins[index].window_event(event) else {
            return;
        };
        match action {
            PinAction::Close => {
                self.pins.remove(index);
            }
            PinAction::Copy(shot) => {
                if let Err(err) = self.copy(&shot) {
                    log::error!("{err:#}");
                }
            }
            PinAction::Save(shot) => {
                if self.save(&shot) {
                    self.save_settings();
                }
            }
        }
    }

    fn request(&mut self, event_loop: &ActiveEventLoop, request: Request) {
        match request {
            Request::Capture => self.start_capture(event_loop, None, false),
            Request::Pick => self.start_capture(event_loop, None, true),
            Request::Settings => self.open_settings(event_loop),
        }
    }

    fn run_command(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        match command {
            Command::Capture => self.start_capture(event_loop, None, false),
            Command::PickColor => self.start_capture(event_loop, None, true),
            Command::FullScreen => self.copy_full_screen(),
            Command::RepeatArea => self.start_capture(event_loop, self.last_area, false),
            Command::Delay(seconds) => self.capture_later(seconds),
            Command::History(index) => self.pin_from_history(event_loop, index),
            Command::ClearHistory => {
                self.history.clear();
                if let Some(tray) = &mut self.tray {
                    tray.set_history(self.history.entries());
                }
            }
            Command::OpenFolder => self.open_folder(),
            Command::Settings => self.open_settings(event_loop),
            Command::Update => self.start_update(),
            Command::Quit => event_loop.exit(),
        }
    }

    fn watch_updates(&mut self) {
        if !self.watching_updates {
            self.watching_updates = true;
            updater::watch(self.proxy.clone());
        }
    }

    /// Recreates the tray, e.g. after a language change or when an update is out.
    fn rebuild_tray(&mut self) {
        self.tray = None;
        let version = self.update.as_ref().map(|u| u.version.as_str());
        match Tray::new(&self.settings, version) {
            Ok(mut tray) => {
                tray.set_history(self.history.entries());
                tray.set_can_repeat(self.last_area.is_some());
                self.tray = Some(tray);
            }
            Err(err) => log::error!("failed to create the tray icon: {err:#}"),
        }
    }

    /// Downloads the update, or opens the download page when this kind of
    /// install can't update itself.
    fn start_update(&mut self) {
        let Some(update) = self.update.clone() else {
            return;
        };
        if update.asset.is_none() {
            if let Err(err) = open::that_detached(updater::RELEASES) {
                log::error!("failed to open the releases page: {err}");
            }
            return;
        }
        if self.update_status != UpdateStatus::Downloading {
            self.update_status = UpdateStatus::Downloading;
            updater::download(self.proxy.clone(), update);
            self.redraw_settings();
        }
    }

    fn redraw_settings(&self) {
        if let Some(window) = &self.settings_window {
            window.request_redraw();
        }
    }

    fn open_folder(&self) {
        let _ = std::fs::create_dir_all(&self.settings.folder);
        if let Err(err) = open::that_detached(&self.settings.folder) {
            log::error!("failed to open the screenshots folder: {err}");
        }
    }

    fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        if self.settings_window.is_none() {
            match SettingsWindow::open(event_loop) {
                Ok(window) => self.settings_window = Some(window),
                Err(err) => {
                    log::error!("{err:#}");
                    return;
                }
            }
        }
        if let Some(window) = &mut self.settings_window {
            window.show(&self.settings, &self.update_status);
        }
    }

    /// Applies a change made in the settings window.
    fn apply(&mut self, action: Action) {
        let mut notice = None;
        match action {
            Action::Close => {
                // The window is hidden, not destroyed; free its memory anyway.
                self.settings_window = None;
                platform::pause_system_keys(false);
                self.register_hotkeys();
                return;
            }
            Action::Language(lang) => {
                self.settings.language = lang;
                i18n::set(self.settings.lang());
                // Menu texts can't all be changed in place, so the tray is rebuilt.
                self.rebuild_tray();
                if let Some(window) = &self.settings_window {
                    window.retitle();
                }
            }
            Action::Autostart => {
                let on = !self.settings.autostart;
                match settings::set_autostart(on) {
                    Ok(()) => self.settings.autostart = on,
                    Err(err) => log::error!("failed to change start at login: {err:#}"),
                }
            }
            Action::ReplaceSystem => {
                let mode = match self.settings.replace_system {
                    ReplaceMode::Off if platform::REPLACE_MODES => ReplaceMode::Ask,
                    ReplaceMode::Off => ReplaceMode::Auto,
                    ReplaceMode::Ask | ReplaceMode::Auto => ReplaceMode::Off,
                };
                notice = self.set_replace_mode(mode);
            }
            Action::RestoreSystem => {
                if self.give_back() {
                    self.settings.replace_system = ReplaceMode::Off;
                } else {
                    notice = Some(Notice::ReplaceFailed);
                }
            }
            Action::ReplaceMode(mode) => {
                let was_on = self.settings.replace_system.is_on();
                notice = self.set_replace_mode(mode);
                // A choice made here is the user's go-ahead, so the links page
                // opens again if they still go to the system tool.
                if was_on && notice.is_none() {
                    platform::confirm_links();
                }
            }
            // While a field listens, the current shortcuts must not fire.
            Action::Recording(true) => {
                platform::pause_system_keys(true);
                for which in Shortcut::ALL {
                    self.unregister_hotkey(which);
                }
            }
            Action::Recording(false) => {
                platform::pause_system_keys(false);
                self.register_hotkeys();
            }
            Action::Hotkey(which, hotkey) => notice = self.set_hotkey(which, hotkey),
            Action::ResetHotkey(which) => {
                notice = self.set_hotkey(which, which.default_keys().into());
            }
            Action::ColorFormat(format) => self.settings.color_format = format,
            Action::Feature(feature) => {
                self.settings.toggle(feature);
                self.register_hotkey(Shortcut::Pick);
                self.rebuild_tray();
            }
            Action::ChangeFolder => {
                let picked = rfd::FileDialog::new()
                    .set_title(i18n::t(i18n::T::FolderTitle))
                    .set_directory(&self.settings.folder)
                    .pick_folder();
                if let Some(folder) = picked {
                    self.settings.folder = folder;
                }
            }
            Action::OpenFolder => self.open_folder(),
            Action::Format(format) => self.settings.format = format,
            Action::CopyAs(copy_as) => self.settings.copy_as = copy_as,
            Action::CopyOnSelect => self.settings.copy_on_select = !self.settings.copy_on_select,
            Action::WindowOpensEditor => {
                self.settings.window_opens_editor = !self.settings.window_opens_editor;
            }
            Action::QuickSave => self.settings.quick_save = !self.settings.quick_save,
            Action::SaveOnCopy => self.settings.save_on_copy = !self.settings.save_on_copy,
            Action::OpenAfterSave => {
                self.settings.open_after_save = !self.settings.open_after_save;
            }
            Action::AutoUpdate => {
                self.settings.auto_update = !self.settings.auto_update;
                if self.settings.auto_update {
                    self.watch_updates();
                }
            }
            Action::CheckUpdates => {
                self.update_status = UpdateStatus::Checking;
                updater::check_now(self.proxy.clone());
            }
            Action::InstallUpdate => self.start_update(),
            Action::Link(url) => {
                if let Err(err) = open::that_detached(url) {
                    log::error!("failed to open {url}: {err}");
                }
            }
            Action::Theme(theme) => self.settings.theme = theme,
            Action::FileName(name) => {
                let name = name.trim();
                self.settings.file_name = if name.is_empty() {
                    settings::DEFAULT_FILE_NAME.into()
                } else {
                    name.into()
                };
            }
            Action::History(limit) => self.set_history(limit),
            Action::Reset(section) => notice = self.reset(section),
        }
        self.save_settings();
        if let Some(window) = &mut self.settings_window {
            if notice.is_some() {
                window.notice = notice;
            }
            window.request_redraw();
        }
    }

    fn set_history(&mut self, limit: usize) {
        self.settings.history = limit;
        self.history.set_limit(limit);
        if let Some(tray) = &mut self.tray {
            tray.set_history(self.history.entries());
        }
    }

    /// Puts the settings of one section of the settings window back to the defaults.
    fn reset(&mut self, section: Section) -> Option<Notice> {
        let default = Settings::default();
        let s = &mut self.settings;
        let features = |list: &[Feature], s: &mut Settings| {
            for feature in list {
                if s.is_enabled(*feature) == default.disabled.contains(feature) {
                    s.toggle(*feature);
                }
            }
        };
        match section {
            Section::General => {
                s.theme = default.theme;
                if s.autostart {
                    self.apply(Action::Autostart);
                }
                if self.settings.language.is_some() {
                    self.apply(Action::Language(None));
                }
            }
            Section::Capture => {
                s.copy_on_select = default.copy_on_select;
                s.window_opens_editor = default.window_opens_editor;
                s.color_format = default.color_format;
            }
            Section::Saving => {
                s.folder = default.folder;
                s.format = default.format;
                s.file_name = default.file_name;
                s.quick_save = default.quick_save;
                s.save_on_copy = default.save_on_copy;
                s.open_after_save = default.open_after_save;
                s.copy_as = default.copy_as;
                self.set_history(default.history);
            }
            Section::Shortcuts => {
                // One at a time, so a key two shortcuts swapped is free when its turn comes.
                for which in Shortcut::ALL {
                    *self.settings.shortcut_mut(which) = String::new();
                }
                self.register_hotkeys();
                let mut notice = None;
                for which in Shortcut::ALL {
                    notice = notice.or(self.set_hotkey(which, which.default_keys().into()));
                }
                return notice;
            }
            Section::Features => {
                features(&FEATURES, s);
                self.register_hotkey(Shortcut::Pick);
                self.rebuild_tray();
            }
            Section::Editor => features(&TOOLS, s),
            Section::System | Section::About => {}
        }
        None
    }

    /// Switches `which` to `hotkey`, keeping the old one when the new one can't
    /// be registered.
    fn set_hotkey(&mut self, which: Shortcut, hotkey: String) -> Option<Notice> {
        // Recording unregistered both; bring the other one back too.
        self.register_hotkeys();
        let taken = Shortcut::ALL
            .into_iter()
            .any(|w| w != which && !hotkey.is_empty() && hotkey == self.settings.shortcut(w));
        if taken {
            return Some(Notice::ShortcutTaken);
        }
        let previous = std::mem::replace(self.settings.shortcut_mut(which), hotkey);
        self.settings.apply_features();
        if !self.register_hotkey(which) {
            *self.settings.shortcut_mut(which) = previous;
            self.register_hotkey(which);
            return Some(Notice::ShortcutTaken);
        }
        if which == Shortcut::Capture
            && self.settings.replace_system.is_on()
            && let Err(err) =
                platform::replace_system_screenshot(true, self.settings.shortcut(Shortcut::Capture))
        {
            log::warn!("failed to update the system shortcut: {err:#}");
        }
        None
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // On macOS the tray must be created once the event loop is running.
        if cause == StartCause::Init {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.rebuild_tray();
            if let Some(request) = self.start.take() {
                self.request(event_loop, request);
            }
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Hotkey(event) => {
                if event.state() != HotKeyState::Pressed {
                    return;
                }
                let which = Shortcut::ALL
                    .into_iter()
                    .find(|w| self.registered[*w as usize].is_some_and(|h| h.id() == event.id()));
                if let Some(which) = which {
                    self.run_shortcut(event_loop, which);
                }
            }
            // PrtSc runs whichever shortcut is set to it, a screenshot by default.
            #[cfg(target_os = "windows")]
            UserEvent::SystemKey(key) => {
                let which = match key {
                    crate::windows_shot::SystemKey::PrintScreen => Shortcut::ALL
                        .into_iter()
                        .filter(|w| w.is_global())
                        .find(|w| {
                            self.settings
                                .shortcut(*w)
                                .eq_ignore_ascii_case("PrintScreen")
                        })
                        .unwrap_or(Shortcut::Capture),
                    crate::windows_shot::SystemKey::WinShiftS => Shortcut::Capture,
                };
                self.run_shortcut(event_loop, which);
            }
            UserEvent::ReplaceAnswer(yes) => {
                if yes {
                    self.take_over(true);
                } else {
                    // No means the system tool keeps everything, and Snapt stops asking.
                    self.give_back();
                    self.settings.replace_system = ReplaceMode::Off;
                    self.save_settings();
                }
                self.redraw_settings();
            }
            UserEvent::Request(request) => self.request(event_loop, request),
            #[cfg(target_os = "linux")]
            UserEvent::PortalShortcut(hotkey, result) => self.portal_answered(hotkey, result),
            UserEvent::Recognized(result) => self.recognized(event_loop, result),
            UserEvent::ScrollGrew => {
                if let Some(scroll) = &self.scroll {
                    scroll.request_redraw();
                }
            }
            UserEvent::UpdateChecked(result, manual) => {
                // The daily check stays quiet unless the user turned it off meanwhile.
                if !manual && !self.settings.auto_update {
                    return;
                }
                match result {
                    Ok(Some(update)) => {
                        log::info!("Snapt {} is available", update.version);
                        self.update_status = UpdateStatus::Available(update.version.clone());
                        if self.update.as_ref() != Some(&update) {
                            self.update = Some(update);
                            self.rebuild_tray();
                        }
                    }
                    Ok(None) => self.update_status = UpdateStatus::UpToDate,
                    Err(err) => {
                        log::warn!("update check failed: {err}");
                        self.update_status = UpdateStatus::Failed;
                    }
                }
                self.redraw_settings();
            }
            UserEvent::UpdateDownloaded(Ok(path)) => match updater::install(&path) {
                // Quit so the installer can replace Snapt.
                Ok(()) => event_loop.exit(),
                Err(err) => {
                    log::error!("failed to install the update: {err:#}");
                    self.update_status = UpdateStatus::Failed;
                    self.redraw_settings();
                }
            },
            UserEvent::UpdateDownloaded(Err(err)) => {
                log::error!("failed to download the update: {err}");
                self.update_status = UpdateStatus::Failed;
                self.redraw_settings();
            }
            UserEvent::Menu(event) => {
                let command = self.tray.as_ref().and_then(|t| t.command(&event.id));
                if let Some(command) = command {
                    self.run_command(event_loop, command);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(window) = &mut self.settings_window
            && window.id() == id
        {
            for action in window.window_event(event, &self.settings, &self.update_status) {
                self.apply(action);
            }
            return;
        }
        if let Some(scroll) = &mut self.scroll
            && scroll.id() == id
        {
            match scroll.window_event(event) {
                Some(ScrollAction::Done) => self.finish_scroll(event_loop),
                Some(ScrollAction::Cancel) => self.scroll = None,
                None => {}
            }
            return;
        }
        if let Some(index) = self.pins.iter().position(|p| p.id() == id) {
            self.pin_event(index, event);
            return;
        }
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        if !overlay.owns(id) {
            return;
        }
        // A minimized overlay would linger as a bare "Snapt" title bar, so it closes instead.
        if let WindowEvent::Resized(size) = &event
            && (size.width == 0 || size.height == 0)
        {
            log::warn!("a screenshot window was minimized; closing the screenshot");
            self.finish_capture(event_loop, Outcome::Cancel);
            return;
        }
        let outcome = overlay.window_event(id, event);
        if let Some(image) = overlay.take_recognize() {
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let _ = proxy.send_event(UserEvent::Recognized(ocr::recognize(&image)));
            });
        }
        if let Some(text) = overlay.take_copied() {
            if let Err(err) = self.output.copy_text(&text) {
                log::error!("{err:#}");
            }
        }
        if let Some(outcome) = outcome {
            self.finish_capture(event_loop, outcome);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        platform::catch_system_keys(None);
        self.overlay = None;
        self.pins.clear();
        for which in Shortcut::ALL {
            self.unregister_hotkey(which);
        }
        log::info!("Snapt stopped");
    }
}

/// A spot for an image of `width` × `height` pixels in the middle of the main
/// screen, shrunk to fit when it is bigger than the screen.
fn centered(
    event_loop: &ActiveEventLoop,
    (width, height): (u32, u32),
) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let Some(monitor) = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
    else {
        return (
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(width, height),
        );
    };
    let (origin, screen) = (monitor.position(), monitor.size());
    let fit = (0.9 * screen.width as f64 / width as f64)
        .min(0.9 * screen.height as f64 / height as f64)
        .min(1.0);
    let w = ((width as f64 * fit).round() as u32).max(1);
    let h = ((height as f64 * fit).round() as u32).max(1);
    let x = origin.x + (screen.width as i32 - w as i32) / 2;
    let y = origin.y + (screen.height as i32 - h as i32) / 2;
    (PhysicalPosition::new(x, y), PhysicalSize::new(w, h))
}
