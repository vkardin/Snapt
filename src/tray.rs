use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{AboutMetadata, CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu},
};

use crate::{
    icon,
    settings::{self, Format, Settings},
};

/// What a tray menu click asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Capture,
    OpenFolder,
    Hotkey(String),
    Format(Format),
    ChooseFolder,
    QuickSave,
    Autostart,
    OpenSettings,
    Quit,
}

/// The tray icon and its menu.
pub struct Tray {
    // Dropping the icon removes it from the tray.
    _icon: TrayIcon,
    capture: MenuItem,
    hotkeys: Vec<(String, CheckMenuItem)>,
    png: CheckMenuItem,
    jpeg: CheckMenuItem,
    quick_save: CheckMenuItem,
    autostart: CheckMenuItem,
    commands: Vec<(MenuId, Command)>,
}

impl Tray {
    pub fn new(settings: &Settings) -> Result<Self> {
        let capture = MenuItem::new("Take screenshot", true, None);
        let open_folder = MenuItem::new("Open screenshots folder", true, None);
        let quit = MenuItem::new("Quit Snapt", true, None);
        let about = PredefinedMenuItem::about(
            Some("About Snapt"),
            Some(AboutMetadata {
                name: Some("Snapt".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                comments: Some(env!("CARGO_PKG_DESCRIPTION").into()),
                website: Some(env!("CARGO_PKG_REPOSITORY").into()),
                license: Some(env!("CARGO_PKG_LICENSE").into()),
                ..Default::default()
            }),
        );

        // A shortcut set by hand in the settings file is listed too.
        let mut names: Vec<String> = settings::HOTKEYS.iter().map(|s| s.to_string()).collect();
        if !names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&settings.hotkey))
        {
            names.push(settings.hotkey.clone());
        }
        let hotkeys: Vec<(String, CheckMenuItem)> = names
            .into_iter()
            .map(|name| {
                let item = CheckMenuItem::new(label(&name), true, false, None);
                (name, item)
            })
            .collect();
        let hotkey_menu = Submenu::new("Capture shortcut", true);
        for (_, item) in &hotkeys {
            hotkey_menu
                .append(item)
                .context("failed to build the tray menu")?;
        }

        let png = CheckMenuItem::new("PNG", true, false, None);
        let jpeg = CheckMenuItem::new("JPEG", true, false, None);
        let format_menu = Submenu::with_items("Image format", true, &[&png, &jpeg])
            .context("failed to build the tray menu")?;
        let choose_folder = MenuItem::new("Choose screenshots folder…", true, None);
        let quick_save = CheckMenuItem::new("Save without asking", true, false, None);
        let autostart = CheckMenuItem::new("Start at login", true, false, None);
        let open_settings = MenuItem::new("Open settings file", true, None);
        let settings_menu = Submenu::with_items(
            "Settings",
            true,
            &[
                &hotkey_menu,
                &format_menu,
                &choose_folder,
                &quick_save,
                &PredefinedMenuItem::separator(),
                &autostart,
                &open_settings,
            ],
        )
        .context("failed to build the tray menu")?;

        let menu = Menu::new();
        menu.append_items(&[
            &capture,
            &open_folder,
            &PredefinedMenuItem::separator(),
            &settings_menu,
            &about,
            &quit,
        ])
        .context("failed to build the tray menu")?;

        let mut commands = vec![
            (capture.id().clone(), Command::Capture),
            (open_folder.id().clone(), Command::OpenFolder),
            (png.id().clone(), Command::Format(Format::Png)),
            (jpeg.id().clone(), Command::Format(Format::Jpeg)),
            (choose_folder.id().clone(), Command::ChooseFolder),
            (quick_save.id().clone(), Command::QuickSave),
            (autostart.id().clone(), Command::Autostart),
            (open_settings.id().clone(), Command::OpenSettings),
            (quit.id().clone(), Command::Quit),
        ];
        for (name, item) in &hotkeys {
            commands.push((item.id().clone(), Command::Hotkey(name.clone())));
        }

        let icon = Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE)
            .context("failed to load the tray icon")?;
        let tray = TrayIconBuilder::new()
            .with_id("snapt")
            .with_tooltip("Snapt")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .context("failed to create the tray icon")?;

        let tray = Self {
            _icon: tray,
            capture,
            hotkeys,
            png,
            jpeg,
            quick_save,
            autostart,
            commands,
        };
        tray.sync(settings);
        Ok(tray)
    }

    pub fn command(&self, id: &MenuId) -> Option<Command> {
        self.commands
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, c)| c.clone())
    }

    /// Shows the current settings in the menu. Check items toggle themselves
    /// when clicked, so this also puts radio-like groups back in order.
    pub fn sync(&self, settings: &Settings) {
        self.capture
            .set_text(format!("Take screenshot\t{}", label(&settings.hotkey)));
        for (name, item) in &self.hotkeys {
            item.set_checked(name.eq_ignore_ascii_case(&settings.hotkey));
        }
        self.png.set_checked(settings.format == Format::Png);
        self.jpeg.set_checked(settings.format == Format::Jpeg);
        self.quick_save.set_checked(settings.quick_save);
        self.autostart.set_checked(settings.autostart);
    }
}

/// A shortcut as people write it: `PrintScreen` reads as `PrtSc`.
fn label(hotkey: &str) -> String {
    hotkey.replace("PrintScreen", "PrtSc")
}
