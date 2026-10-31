use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{
        AboutMetadata, Icon as MenuIcon, IconMenuItem, Menu, MenuId, PredefinedMenuItem, Submenu,
    },
};

use crate::{
    history::Entry,
    i18n::{T, t},
    icon,
    settings::{Feature, Settings, Shortcut},
    settings_window::UpdateStatus,
    tray_menu::{Glyph, NATIVE_ICON, glyph_rgba, update_row},
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
    /// Look for a new version now.
    CheckUpdates,
    Update,
    /// Show the version and the website; the system menus have their own item for it.
    About,
    Quit,
}

/// Menu icons are drawn at twice their size on a Mac, for Retina screens.
#[cfg(target_os = "macos")]
const ICON_PX: u32 = 32;
#[cfg(not(target_os = "macos"))]
const ICON_PX: u32 = 16;

fn menu_icon(glyph: Glyph) -> Option<MenuIcon> {
    MenuIcon::from_rgba(glyph_rgba(glyph, ICON_PX, NATIVE_ICON), ICON_PX, ICON_PX).ok()
}

fn item(glyph: Glyph, text: &str, enabled: bool) -> IconMenuItem {
    IconMenuItem::new(text, enabled, menu_icon(glyph), None)
}

/// The tray icon and its system menu, in the current interface language.
///
/// On Windows the system menu only stands in when Snapt's own menu (see
/// `tray_menu`) can't open; a click on the icon opens that one instead.
pub struct Tray {
    // Dropping the icon removes it from the tray.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    icon: TrayIcon,
    capture: IconMenuItem,
    pick: IconMenuItem,
    full_screen: IconMenuItem,
    repeat: IconMenuItem,
    history: Submenu,
    update: IconMenuItem,
    commands: Vec<(MenuId, Command)>,
}

impl Tray {
    /// `status` decides the update item: a new version goes at the top of the menu.
    pub fn new(settings: &Settings, status: &UpdateStatus) -> Result<Self> {
        let capture = item(Glyph::Capture, t(T::TakeScreenshot), true);
        let pick = item(
            Glyph::Picker,
            t(T::PickColor),
            settings.is_enabled(Feature::Picker),
        );
        let full_screen = item(Glyph::FullScreen, t(T::FullScreen), true);
        let repeat = item(Glyph::Repeat, t(T::RepeatArea), false);
        let delayed = Submenu::new(t(T::Delayed), true);
        delayed.set_icon(menu_icon(Glyph::Timer));
        let history = Submenu::new(t(T::History), true);
        history.set_icon(menu_icon(Glyph::History));
        let open_folder = item(Glyph::Folder, t(T::OpenFolder), true);
        let settings_item = item(Glyph::Settings, t(T::SettingsMenu), true);
        let row = update_row(status);
        let update = item(row.glyph, &row.text, row.command.is_some());
        let quit = item(Glyph::Quit, t(T::Quit), true);
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
        let failed = "failed to build the tray menu";
        if row.top {
            menu.append_items(&[&update, &PredefinedMenuItem::separator()])
                .context(failed)?;
        }
        for seconds in DELAYS {
            let item = item(
                Glyph::Timer,
                &t(T::Seconds).replace("{}", &seconds.to_string()),
                true,
            );
            delayed.append(&item).context(failed)?;
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
        ])
        .context(failed)?;
        if !row.top {
            menu.append(&update).context(failed)?;
        }
        menu.append_items(&[&PredefinedMenuItem::separator(), &about, &quit])
            .context(failed)?;

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
        let tooltip = match status {
            UpdateStatus::Available(version) => t(T::TrayUpdate).replace("{}", version),
            _ => "Snapt".into(),
        };
        let builder = TrayIconBuilder::new()
            .with_id("snapt")
            .with_tooltip(tooltip)
            .with_icon(icon)
            .with_menu(Box::new(menu));
        // Clicks open Snapt's own menu; the app shows this one only if that fails.
        #[cfg(target_os = "windows")]
        let builder = builder
            .with_menu_on_left_click(false)
            .with_menu_on_right_click(false);
        let tray = builder.build().context("failed to create the tray icon")?;

        let mut tray = Self {
            icon: tray,
            capture,
            pick,
            full_screen,
            repeat,
            history,
            update,
            commands,
        };
        tray.set_update(status);
        tray.sync(settings);
        Ok(tray)
    }

    /// Opens the system menu at the tray icon.
    #[cfg(target_os = "windows")]
    pub fn show_menu(&self) {
        self.icon.show_menu();
    }

    /// Shows how the update check went on the update item.
    pub fn set_update(&mut self, status: &UpdateStatus) {
        let row = update_row(status);
        self.update.set_text(&row.text);
        self.update.set_icon(menu_icon(row.glyph));
        self.update.set_enabled(row.command.is_some());
        let id = self.update.id().clone();
        self.commands.retain(|(i, _)| *i != id);
        if let Some(command) = row.command {
            self.commands.push((id, command));
        }
    }

    /// Lists `entries` in the history submenu, newest first.
    pub fn set_history(&mut self, entries: &[Entry]) {
        while self.history.remove_at(0).is_some() {}
        self.commands
            .retain(|(_, c)| !matches!(c, Command::History(_) | Command::ClearHistory));
        let now = chrono::Local::now();
        let mut items = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let item = item(Glyph::Image, &entry.label(now), true);
            self.commands.push((item.id().clone(), Command::History(i)));
            items.push(item);
        }
        if items.is_empty() {
            items.push(IconMenuItem::new(t(T::HistoryEmpty), false, None, None));
        }
        let clear = item(Glyph::Clear, t(T::ClearHistory), !entries.is_empty());
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
        for (item, text, which) in [
            (&self.capture, T::TakeScreenshot, Shortcut::Capture),
            (&self.pick, T::PickColor, Shortcut::Pick),
            (&self.full_screen, T::FullScreen, Shortcut::FullScreen),
            (&self.repeat, T::RepeatArea, Shortcut::RepeatArea),
        ] {
            let keys = settings.shortcut(which);
            if keys.is_empty() {
                item.set_text(t(text));
            } else {
                item.set_text(format!("{}\t{}", t(text), hotkey_label(keys)));
            }
        }
    }
}

/// A shortcut as people write it: `PrintScreen` reads as `PrtSc`.
pub fn hotkey_label(hotkey: &str) -> String {
    hotkey.replace("PrintScreen", "PrtSc")
}
