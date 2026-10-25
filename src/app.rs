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

use crate::{capture, output, tray::Tray};

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
            output: output::Output::new()?,
        })
    }

    fn take_screenshot(&mut self) {
        let result = capture::primary_monitor().and_then(|shot| self.output.deliver(&shot));
        match result {
            Ok(path) => log::info!("screenshot saved to {}", path.display()),
            Err(err) => log::error!("screenshot failed: {err:#}"),
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
                    self.take_screenshot();
                }
            }
            UserEvent::Menu(event) => {
                let Some(tray) = &self.tray else { return };
                if event.id == tray.capture_id {
                    self.take_screenshot();
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

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let _ = self.hotkeys.unregister(self.capture_hotkey);
        log::info!("Snapt stopped");
    }
}
