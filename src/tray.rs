use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem},
};

use crate::{
    i18n::{T, t},
    icon,
    settings::Settings,
};

/// What a tray menu click asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Capture,
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
    commands: Vec<(MenuId, Command)>,
}

impl Tray {
    /// `update` is the version to offer at the top of the menu, if one is out.
    pub fn new(settings: &Settings, update: Option<&str>) -> Result<Self> {
        let capture = MenuItem::new(t(T::TakeScreenshot), true, None);
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
        menu.append_items(&[
            &capture,
            &open_folder,
            &settings_item,
            &PredefinedMenuItem::separator(),
            &about,
            &quit,
        ])
        .context("failed to build the tray menu")?;

        commands.extend([
            (capture.id().clone(), Command::Capture),
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
            commands,
        };
        tray.sync(settings);
        Ok(tray)
    }

    pub fn command(&self, id: &MenuId) -> Option<Command> {
        self.commands.iter().find(|(i, _)| i == id).map(|(_, c)| *c)
    }

    /// Shows the current shortcut next to the capture item.
    pub fn sync(&self, settings: &Settings) {
        self.capture.set_text(format!(
            "{}\t{}",
            t(T::TakeScreenshot),
            hotkey_label(&settings.hotkey)
        ));
    }
}

/// A shortcut as people write it: `PrintScreen` reads as `PrtSc`.
pub fn hotkey_label(hotkey: &str) -> String {
    hotkey.replace("PrintScreen", "PrtSc")
}
