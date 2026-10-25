use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem},
};

use crate::icon;

/// The tray icon and the ids of its menu items.
pub struct Tray {
    // Dropping the icon removes it from the tray.
    _icon: TrayIcon,
    pub capture_id: MenuId,
    pub open_folder_id: MenuId,
    pub quit_id: MenuId,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let capture = MenuItem::new("Take screenshot\tPrtSc", true, None);
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

        let menu = Menu::new();
        menu.append_items(&[
            &capture,
            &open_folder,
            &PredefinedMenuItem::separator(),
            &about,
            &quit,
        ])
        .context("failed to build the tray menu")?;

        let icon = Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE)
            .context("failed to load the tray icon")?;
        let tray = TrayIconBuilder::new()
            .with_id("snapt")
            .with_tooltip("Snapt")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .context("failed to create the tray icon")?;

        Ok(Self {
            _icon: tray,
            capture_id: capture.id().clone(),
            open_folder_id: open_folder.id().clone(),
            quit_id: quit.id().clone(),
        })
    }
}
