use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu},
};

use crate::{
    history::Entry,
    i18n::{T, t},
    icon,
    settings::Settings,
};

/// The delays offered for a delayed screenshot, in seconds.
pub const DELAYS: [u64; 3] = [3, 5, 10];

/// What a tray menu click asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Capture,
    PickColor,
    /// Copy every screen at once, without the overlay.
    FullScreen,
    /// Open the overlay with the last selected area already selected.
    RepeatArea,
    /// Open the overlay after this many seconds.
    Delay(u64),
    /// Pin the screenshot at this index of the history, newest first.
    History(usize),
    ClearHistory,
    OpenFolder,
    Settings,
    Update,
    Quit,
}

/// The tray icon and its menu, in the current interface language.
pub struct Tray {
    // Dropping the icon removes it from the tray.
    _icon: TrayIcon,
    capture: MenuItem,
    pick: MenuItem,
    repeat: MenuItem,
    history: Submenu,
    commands: Vec<(MenuId, Command)>,
}

impl Tray {
    /// `update` is the version to offer at the top of the menu, if one is out.
    pub fn new(settings: &Settings, update: Option<&str>) -> Result<Self> {
        let capture = MenuItem::new(t(T::TakeScreenshot), true, None);
        let pick = MenuItem::new(t(T::PickColor), true, None);
        let full_screen = MenuItem::new(t(T::FullScreen), true, None);
        let repeat = MenuItem::new(t(T::RepeatArea), false, None);
        let delayed = Submenu::new(t(T::Delayed), true);
        let history = Submenu::new(t(T::History), true);
        let open_folder = MenuItem::new(t(T::OpenFolder), true, None);
        let settings_item = MenuItem::new(t(T::SettingsMenu), true, None);
        let quit = MenuItem::new(t(T::Quit), true, None);
        let about = PredefinedMenuItem::about(
            Some(t(T::About)),
            Some(AboutMetadata {
                name: Some("Snapt".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                comments: Some(t(T::Description).into()),
                website: Some(env!("CARGO_PKG_HOMEPAGE").into()),
                license: Some(env!("CARGO_PKG_LICENSE").into()),
                ..Default::default()
            }),
        );

        let menu = Menu::new();
        let mut commands = Vec::new();
        if let Some(version) = update {
            let item = MenuItem::new(t(T::TrayUpdate).replace("{}", version), true, None);
            menu.append_items(&[&item, &PredefinedMenuItem::separator()])
                .context("failed to build the tray menu")?;
            commands.push((item.id().clone(), Command::Update));
        }
        for seconds in DELAYS {
            let item = MenuItem::new(
                t(T::Seconds).replace("{}", &seconds.to_string()),
                true,
                None,
            );
            delayed
                .append(&item)
                .context("failed to build the tray menu")?;
            commands.push((item.id().clone(), Command::Delay(seconds)));
        }
        menu.append_items(&[
            &capture,
            &pick,
            &full_screen,
            &repeat,
            &delayed,
            &history,
            &PredefinedMenuItem::separator(),
            &open_folder,
            &settings_item,
            &PredefinedMenuItem::separator(),
            &about,
            &quit,
        ])
        .context("failed to build the tray menu")?;

        commands.extend([
            (capture.id().clone(), Command::Capture),
            (pick.id().clone(), Command::PickColor),
            (full_screen.id().clone(), Command::FullScreen),
            (repeat.id().clone(), Command::RepeatArea),
            (open_folder.id().clone(), Command::OpenFolder),
            (settings_item.id().clone(), Command::Settings),
            (quit.id().clone(), Command::Quit),
        ]);

        let icon = Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE)
            .context("failed to load the tray icon")?;
        let tray = TrayIconBuilder::new()
            .with_id("snapt")
            .with_tooltip(match update {
                Some(version) => t(T::TrayUpdate).replace("{}", version),
                None => "Snapt".into(),
            })
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .context("failed to create the tray icon")?;

        let tray = Self {
            _icon: tray,
            capture,
            pick,
            repeat,
            history,
            commands,
        };
        tray.sync(settings);
        Ok(tray)
    }

    /// Lists `entries` in the history submenu, newest first.
    pub fn set_history(&mut self, entries: &[Entry]) {
        while self.history.remove_at(0).is_some() {}
        self.commands
            .retain(|(_, c)| !matches!(c, Command::History(_) | Command::ClearHistory));
        let now = chrono::Local::now();
        let mut items = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let item = MenuItem::new(entry.label(now), true, None);
            self.commands.push((item.id().clone(), Command::History(i)));
            items.push(item);
        }
        if items.is_empty() {
            items.push(MenuItem::new(t(T::HistoryEmpty), false, None));
        }
        let clear = MenuItem::new(t(T::ClearHistory), !entries.is_empty(), None);
        self.commands
            .push((clear.id().clone(), Command::ClearHistory));
        let separator = PredefinedMenuItem::separator();
        let result = items
            .iter()
            .try_for_each(|item| self.history.append(item))
            .and_then(|()| self.history.append_items(&[&separator, &clear]));
        if let Err(err) = result {
            log::warn!("failed to update the history menu: {err}");
        }
    }

    /// Enables "Repeat last area" once there is an area to repeat.
    pub fn set_can_repeat(&self, on: bool) {
        self.repeat.set_enabled(on);
    }

    pub fn command(&self, id: &MenuId) -> Option<Command> {
        self.commands.iter().find(|(i, _)| i == id).map(|(_, c)| *c)
    }

    /// Shows the current shortcuts next to their items.
    pub fn sync(&self, settings: &Settings) {
        self.capture.set_text(format!(
            "{}\t{}",
            t(T::TakeScreenshot),
            hotkey_label(&settings.hotkey)
        ));
        self.pick.set_text(format!(
            "{}\t{}",
            t(T::PickColor),
            hotkey_label(&settings.pick_hotkey)
        ));
    }
}

/// A shortcut as people write it: `PrintScreen` reads as `PrtSc`.
pub fn hotkey_label(hotkey: &str) -> String {
    hotkey.replace("PrintScreen", "PrtSc")
}
