use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};
use tray_icon::menu::MenuEvent;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    window::WindowId,
};

use crate::{
    capture, output,
    overlay::{Outcome, Overlay},
    tray::Tray,
};

/// Events sent into the winit loop from tray and hotkey callbacks.
#[derive(Debug)]
pub enum UserEvent {
    Menu(MenuEvent),
    Hotkey(GlobalHotKeyEvent),
}

pub struct App {
    // The manager unregisters its hotkeys when dropped, so it must live as long as the app.
    hotkeys: GlobalHotKeyManager,
    capture_hotkey: HotKey,
    tray: Option<Tray>,
    overlay: Option<Overlay>,
    output: output::Output,
}

impl App {
    pub fn new() -> Result<Self> {
        let hotkeys = GlobalHotKeyManager::new().context("failed to start the hotkey manager")?;
        let capture_hotkey = HotKey::new(None, Code::PrintScreen);
        if let Err(err) = hotkeys.register(capture_hotkey) {
            // Another app may already own PrtSc. Snapt still works from the tray menu.
            log::warn!("could not register PrtSc: {err}");
        }

        Ok(Self {
            hotkeys,
            capture_hotkey,
            tray: None,
            overlay: None,
            output: output::Output::new()?,
        })
    }

    /// Freezes the screens and opens the selection overlay.
    fn start_capture(&mut self, event_loop: &ActiveEventLoop) {
        if self.overlay.is_some() {
            return;
        }
        let result = capture::all_monitors().and_then(|shots| Overlay::open(event_loop, shots));
        match result {
            Ok(overlay) => self.overlay = Some(overlay),
            Err(err) => log::error!("screenshot failed: {err:#}"),
        }
    }

    fn finish_capture(&mut self, outcome: Outcome) {
        // Close the overlay first so it is not in the way of the save dialog.
        self.overlay = None;
        match outcome {
            Outcome::Cancel => {}
            Outcome::Copy(shot) => match self.output.copy(&shot) {
                Ok(()) => log::info!("copied {}×{} to the clipboard", shot.width(), shot.height()),
                Err(err) => log::error!("{err:#}"),
            },
            Outcome::Save(shot) => match self.output.save_as(&shot) {
                Ok(Some(path)) => log::info!("saved {}", path.display()),
                Ok(None) => {}
                Err(err) => log::error!("{err:#}"),
            },
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // On macOS the tray must be created once the event loop is running.
        if cause == StartCause::Init {
            event_loop.set_control_flow(ControlFlow::Wait);
            match Tray::new() {
                Ok(tray) => self.tray = Some(tray),
                Err(err) => log::error!("failed to create the tray icon: {err:#}"),
            }
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Hotkey(event) => {
                if event.id() == self.capture_hotkey.id() && event.state() == HotKeyState::Pressed {
                    self.start_capture(event_loop);
                }
            }
            UserEvent::Menu(event) => {
                let Some(tray) = &self.tray else { return };
                if event.id == tray.capture_id {
                    self.start_capture(event_loop);
                } else if event.id == tray.open_folder_id {
                    if let Err(err) = open::that(self.output.dir()) {
                        log::error!("failed to open the screenshots folder: {err}");
                    }
                } else if event.id == tray.quit_id {
                    event_loop.exit();
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
        let _ = self.hotkeys.unregister(self.capture_hotkey);
        log::info!("Snapt stopped");
    }
}
