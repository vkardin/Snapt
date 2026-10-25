use anyhow::{Context, Result};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use tray_icon::menu::MenuEvent;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    window::WindowId,
};

use crate::{
    capture, output,
    overlay::{Outcome, Overlay, Style},
    platform,
    settings::{self, Settings},
    tray::{Command, Tray},
};

/// Events sent into the winit loop from tray, hotkey and other-instance callbacks.
#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuEvent),
    Hotkey(GlobalHotKeyEvent),
    /// Another copy of Snapt was started and asks for a screenshot.
    Capture,
}

pub struct App {
    // The manager unregisters its hotkeys when dropped, so it must live as long as the app.
    hotkeys: GlobalHotKeyManager,
    capture_hotkey: Option<HotKey>,
    tray: Option<Tray>,
    overlay: Option<Overlay>,
    settings: Settings,
    style: Style,
    capture_now: bool,
    output: output::Output,
}

impl App {
    /// `capture_now` opens the overlay as soon as the app starts.
    pub fn new(capture_now: bool) -> Result<Self> {
        let hotkeys = GlobalHotKeyManager::new().context("failed to start the hotkey manager")?;
        let settings = Settings::load();
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
            settings,
            style,
            capture_now,
            output: output::Output::new()?,
        };
        app.register_hotkey();
        Ok(app)
    }

    /// (Re)registers the capture shortcut from the settings.
    fn register_hotkey(&mut self) {
        if let Some(old) = self.capture_hotkey.take() {
            let _ = self.hotkeys.unregister(old);
        }
        let hotkey: HotKey = match self.settings.hotkey.parse() {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("bad capture shortcut {:?}: {err}", self.settings.hotkey);
                return;
            }
        };
        match self.hotkeys.register(hotkey) {
            Ok(()) => self.capture_hotkey = Some(hotkey),
            // Another app may own the shortcut. Snapt still works from the tray menu.
            Err(err) => log::warn!("could not register {}: {err}", self.settings.hotkey),
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

    fn run_command(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        match command {
            Command::Capture => self.start_capture(event_loop),
            Command::OpenFolder => {
                let _ = std::fs::create_dir_all(&self.settings.folder);
                if let Err(err) = open::that_detached(&self.settings.folder) {
                    log::error!("failed to open the screenshots folder: {err}");
                }
            }
            Command::Hotkey(hotkey) => {
                self.settings.hotkey = hotkey;
                self.register_hotkey();
            }
            Command::Format(format) => self.settings.format = format,
            Command::ChooseFolder => {
                let picked = rfd::FileDialog::new()
                    .set_title("Screenshots folder")
                    .set_directory(&self.settings.folder)
                    .pick_folder();
                if let Some(folder) = picked {
                    self.settings.folder = folder;
                }
            }
            Command::QuickSave => self.settings.quick_save = !self.settings.quick_save,
            Command::Autostart => {
                let on = !self.settings.autostart;
                match settings::set_autostart(on) {
                    Ok(()) => self.settings.autostart = on,
                    Err(err) => log::error!("failed to change start at login: {err:#}"),
                }
            }
            Command::OpenSettings => {
                // Write the file first so there is something to open.
                self.save_settings();
                if let Some(path) = Settings::path()
                    && let Err(err) = open::that_detached(path)
                {
                    log::error!("failed to open the settings file: {err}");
                }
                return;
            }
            Command::Quit => {
                event_loop.exit();
                return;
            }
        }
        self.save_settings();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // On macOS the tray must be created once the event loop is running.
        if cause == StartCause::Init {
            event_loop.set_control_flow(ControlFlow::Wait);
            match Tray::new(&self.settings) {
                Ok(tray) => self.tray = Some(tray),
                Err(err) => log::error!("failed to create the tray icon: {err:#}"),
            }
            if platform::is_wayland() {
                log::info!(
                    "Wayland has no global shortcuts for apps: bind `snapt --capture` to a key in the system settings"
                );
            }
            if self.capture_now {
                self.start_capture(event_loop);
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
            UserEvent::Menu(event) => {
                let command = self.tray.as_ref().and_then(|t| t.command(&event.id));
                if let Some(command) = command {
                    self.run_command(event_loop, command);
                }
            }
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        if !overlay.owns(id) {
            return;
        }
        if let Some(outcome) = overlay.window_event(id, event) {
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
