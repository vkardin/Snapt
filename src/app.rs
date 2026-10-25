use anyhow::{Context, Result};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use tray_icon::menu::MenuEvent;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy},
    window::WindowId,
};

use crate::{
    capture,
    i18n::{self, T, t},
    ocr, output,
    overlay::{Outcome, Overlay, Style},
    platform,
    settings::{self, DEFAULT_HOTKEY, Settings},
    settings_window::{Action, Notice, SettingsWindow, UpdateStatus},
    tray::{Command, Tray},
    updater::{self, Update},
};

/// Events sent into the winit loop from tray, hotkey and other-instance callbacks.
#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuEvent),
    Hotkey(GlobalHotKeyEvent),
    /// Another copy of Snapt was started and asks for a screenshot.
    Capture,
    /// Another copy of Snapt was started with `--settings`.
    Settings,
    /// An update check finished; the flag tells whether the user asked for it.
    UpdateChecked(Result<Option<Update>, String>, bool),
    /// The installer of an update was downloaded.
    UpdateDownloaded(Result<std::path::PathBuf, String>),
    /// The desktop portal answered a request for the capture shortcut.
    #[cfg(target_os = "linux")]
    PortalShortcut(String, Result<crate::portal::Shortcut, String>),
    /// Text recognition on a selection finished.
    Recognized(Result<String, ocr::Error>),
}

pub struct App {
    // The manager unregisters its hotkeys when dropped, so it must live as long as the app.
    hotkeys: GlobalHotKeyManager,
    capture_hotkey: Option<HotKey>,
    tray: Option<Tray>,
    overlay: Option<Overlay>,
    settings_window: Option<SettingsWindow>,
    settings: Settings,
    style: Style,
    capture_now: bool,
    settings_now: bool,
    output: output::Output,
    proxy: EventLoopProxy<UserEvent>,
    update: Option<Update>,
    update_status: UpdateStatus,
    watching_updates: bool,
    /// The capture shortcut on Wayland, bound through the desktop portal.
    #[cfg(target_os = "linux")]
    portal: Portal,
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

impl App {
    /// `capture_now` opens the overlay as soon as the app starts, `settings_now`
    /// the settings window.
    pub fn new(
        capture_now: bool,
        settings_now: bool,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self> {
        let hotkeys = GlobalHotKeyManager::new().context("failed to start the hotkey manager")?;
        let settings = Settings::load();
        i18n::set(settings.lang());
        let style = Style {
            tool: None,
            color: settings.color,
            stroke: settings.stroke,
            text_size: settings.text_size,
        };
        let mut app = Self {
            hotkeys,
            capture_hotkey: None,
            tray: None,
            overlay: None,
            settings_window: None,
            settings,
            style,
            capture_now,
            settings_now,
            output: output::Output::new()?,
            proxy,
            update: None,
            update_status: UpdateStatus::Idle,
            watching_updates: false,
            #[cfg(target_os = "linux")]
            portal: Portal::default(),
        };
        app.register_hotkey();
        if app.settings.auto_update {
            app.watch_updates();
        }
        Ok(app)
    }

    /// (Re)registers the capture shortcut from the settings. Returns false when
    /// the shortcut is invalid or another app owns it; Snapt then still works
    /// from the tray menu.
    fn register_hotkey(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        if platform::is_wayland() && !self.portal.failed {
            self.bind_portal_shortcut();
            return true;
        }
        self.register_global_hotkey()
    }

    fn register_global_hotkey(&mut self) -> bool {
        self.unregister_hotkey();
        let hotkey: HotKey = match self.settings.hotkey.parse() {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("bad capture shortcut {:?}: {err}", self.settings.hotkey);
                return false;
            }
        };
        match self.hotkeys.register(hotkey) {
            Ok(()) => {
                self.capture_hotkey = Some(hotkey);
                true
            }
            Err(err) => {
                log::warn!("could not register {}: {err}", self.settings.hotkey);
                false
            }
        }
    }

    /// Asks the desktop portal for the capture shortcut in the background, as
    /// the desktop may first ask the user. The answer comes back as
    /// [`UserEvent::PortalShortcut`].
    #[cfg(target_os = "linux")]
    fn bind_portal_shortcut(&mut self) {
        let hotkey = self.settings.hotkey.clone();
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
            let pressed = move || events.send_event(UserEvent::Capture).is_ok();
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
                self.register_global_hotkey();
            }
        }
    }

    fn unregister_hotkey(&mut self) {
        if let Some(old) = self.capture_hotkey.take() {
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

    /// Freezes the screens and opens the selection overlay.
    fn start_capture(&mut self, event_loop: &ActiveEventLoop) {
        if self.overlay.is_some() {
            return;
        }
        if !platform::screen_access() {
            return;
        }
        let result =
            capture::all_monitors().and_then(|shots| Overlay::open(event_loop, shots, self.style));
        match result {
            Ok(overlay) => self.overlay = Some(overlay),
            Err(err) => log::error!("screenshot failed: {err:#}"),
        }
    }

    fn finish_capture(&mut self, outcome: Outcome) {
        // Close the overlay first so it is not in the way of the save dialog.
        if let Some(overlay) = self.overlay.take() {
            self.style = overlay.style();
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

        let result = match outcome {
            Outcome::Cancel => Ok(()),
            Outcome::Copy(shot) => self.output.copy(&shot).map(|()| {
                log::info!("copied {}×{} to the clipboard", shot.width(), shot.height());
            }),
            Outcome::Save(shot) => {
                let folder = self.settings.folder.clone();
                let saved = self.output.save(&shot, &mut self.settings);
                changed |= folder != self.settings.folder;
                saved.map(|path| {
                    if let Some(path) = path {
                        log::info!("saved {}", path.display());
                    }
                })
            }
            Outcome::Print(shot) => self.output.print(&shot),
            Outcome::Search(shot) => self.output.search(&shot),
        };
        if let Err(err) = result {
            log::error!("{err:#}");
        }
        if changed {
            self.save_settings();
        }
    }

    /// Copies recognized text and closes the overlay, or says in it why there is none.
    fn recognized(&mut self, result: Result<String, ocr::Error>) {
        let status = match result {
            Ok(text) if !text.is_empty() => {
                match self.output.copy_text(&text) {
                    Ok(()) => log::info!("copied {} recognized characters", text.chars().count()),
                    Err(err) => log::error!("{err:#}"),
                }
                if self.overlay.is_some() {
                    self.finish_capture(Outcome::Cancel);
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

    fn run_command(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        match command {
            Command::Capture => self.start_capture(event_loop),
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
            Ok(tray) => self.tray = Some(tray),
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
                self.register_hotkey();
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
                let on = !self.settings.replace_system;
                match platform::replace_system_screenshot(on, &self.settings.hotkey) {
                    Ok(()) => {
                        self.settings.replace_system = on;
                        // The system tool may have held the key until now.
                        self.register_hotkey();
                    }
                    Err(err) => {
                        log::error!("failed to replace the system screenshot tool: {err:#}");
                        notice = Some(Notice::ReplaceFailed);
                    }
                }
            }
            // While the field listens, the current shortcut must not fire.
            Action::Recording(true) => self.unregister_hotkey(),
            Action::Recording(false) => {
                self.register_hotkey();
            }
            Action::Hotkey(hotkey) => notice = self.set_hotkey(hotkey),
            Action::ResetHotkey => notice = self.set_hotkey(DEFAULT_HOTKEY.into()),
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
            Action::QuickSave => self.settings.quick_save = !self.settings.quick_save,
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
            Action::Website => {
                if let Err(err) = open::that_detached(env!("CARGO_PKG_HOMEPAGE")) {
                    log::error!("failed to open the website: {err}");
                }
            }
        }
        self.save_settings();
        if let Some(window) = &mut self.settings_window {
            if notice.is_some() {
                window.notice = notice;
            }
            window.request_redraw();
        }
    }

    /// Switches to `hotkey`, keeping the old one when the new one can't be registered.
    fn set_hotkey(&mut self, hotkey: String) -> Option<Notice> {
        let previous = std::mem::replace(&mut self.settings.hotkey, hotkey);
        if !self.register_hotkey() {
            self.settings.hotkey = previous;
            self.register_hotkey();
            return Some(Notice::ShortcutTaken);
        }
        if self.settings.replace_system
            && let Err(err) = platform::replace_system_screenshot(true, &self.settings.hotkey)
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
            if self.capture_now {
                self.start_capture(event_loop);
            }
            if self.settings_now {
                self.open_settings(event_loop);
            }
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Hotkey(event) => {
                if Some(event.id()) == self.capture_hotkey.map(|h| h.id())
                    && event.state() == HotKeyState::Pressed
                {
                    self.start_capture(event_loop);
                }
            }
            UserEvent::Capture => self.start_capture(event_loop),
            #[cfg(target_os = "linux")]
            UserEvent::PortalShortcut(hotkey, result) => self.portal_answered(hotkey, result),
            UserEvent::Recognized(result) => self.recognized(result),
            UserEvent::Settings => self.open_settings(event_loop),
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

    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(window) = &mut self.settings_window
            && window.id() == id
        {
            if let Some(action) = window.window_event(event, &self.settings, &self.update_status) {
                self.apply(action);
            }
            return;
        }
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        if !overlay.owns(id) {
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
            self.finish_capture(outcome);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.overlay = None;
        if let Some(hotkey) = self.capture_hotkey.take() {
            let _ = self.hotkeys.unregister(hotkey);
        }
        log::info!("Snapt stopped");
    }
}
