//! Snapt: a fast, lightweight, open-source screenshot tool.
//!
//! The app lives in the system tray. A global hotkey (PrtSc by default)
//! freezes the screen and lets the user select an area to copy or save.

// Do not open a console window next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod font;
mod icon;
mod output;
mod overlay;
mod tray;

use anyhow::Result;
use winit::event_loop::EventLoop;

use app::{App, UserEvent};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Snapt {} starting", env!("CARGO_PKG_VERSION"));

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        // Keep Snapt out of the Dock: it is a menu bar app.
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build()?;

    // Forward tray menu and hotkey events into the event loop so it wakes up on each one.
    let proxy = event_loop.create_proxy();
    tray_icon::menu::MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));
    let proxy = event_loop.create_proxy();
    global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Hotkey(event));
    }));

    let mut app = App::new(std::env::args().any(|a| a == "--capture"))?;
    event_loop.run_app(&mut app)?;
    Ok(())
}
