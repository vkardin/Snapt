//! Snapt: a fast, lightweight, open-source screenshot tool.
//!
//! The app lives in the system tray. A global hotkey (PrtSc by default)
//! freezes the screen and lets the user select an area to copy or save.

// Do not open a console window next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod crash;
mod font;
mod history;
mod i18n;
mod icon;
mod instance;
mod ocr;
mod output;
mod overlay;
mod paint;
mod pin;
mod platform;
#[cfg(target_os = "linux")]
mod portal;
mod record;
mod scroll;
mod settings;
mod settings_window;
#[cfg(test)]
mod snapshot;
mod tray;
mod updater;

use anyhow::Result;
use winit::event_loop::EventLoop;

use app::{App, UserEvent};

const HELP: &str = "Snapt: a fast, lightweight screenshot tool.

Usage: snapt [--capture | --pick | --settings | --logs]

  --capture   Take a screenshot right away. If Snapt is already running,
              that copy takes it. Bind this to a key where global
              shortcuts don't work, such as on some Wayland desktops.
  --pick      Pick a color from the screen and copy it.
  --settings  Open the settings window.
  --logs      Open the folder with the log and crash reports.
  --version   Print the version.";

fn main() -> Result<()> {
    crash::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("snapt {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--logs") {
        let dir = crash::dir().ok_or_else(|| anyhow::anyhow!("no config folder"))?;
        std::fs::create_dir_all(&dir)?;
        open::that_detached(&dir)?;
        return Ok(());
    }
    let start = if args.iter().any(|a| a == "--settings") {
        Some(instance::Request::Settings)
    } else if args.iter().any(|a| a == "--pick") {
        Some(instance::Request::Pick)
    } else if args.iter().any(|a| a == "--capture") {
        Some(instance::Request::Capture)
    } else {
        None
    };
    // A second copy hands over to the running one, which takes the screenshot.
    if instance::forward(start.unwrap_or(instance::Request::Capture)) {
        return Ok(());
    }
    crash::keep();
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

    instance::listen(event_loop.create_proxy());

    let mut app = App::new(start, event_loop.create_proxy())?;
    event_loop.run_app(&mut app)?;
    Ok(())
}
