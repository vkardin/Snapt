use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use chrono::{DateTime, Local};
use directories::UserDirs;
use image::RgbaImage;

/// Where finished screenshots go: the clipboard and a PNG file.
pub struct Output {
    dir: PathBuf,
    // On Linux the clipboard only serves its contents while its owner is alive,
    // so one instance is kept for the lifetime of the app.
    clipboard: Option<Clipboard>,
}

impl Output {
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new()
            .inspect_err(|err| log::warn!("clipboard unavailable: {err}"))
            .ok();
        Ok(Self {
            dir: default_dir()?,
            clipboard,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Copies the shot to the clipboard and saves it, returning the file path.
    pub fn deliver(&mut self, shot: &RgbaImage) -> Result<PathBuf> {
        if let Some(clipboard) = &mut self.clipboard {
            let data = ImageData {
                width: shot.width() as usize,
                height: shot.height() as usize,
                bytes: Cow::Borrowed(shot.as_raw()),
            };
            if let Err(err) = clipboard.set_image(data) {
                log::warn!("failed to copy the screenshot: {err}");
            }
        }

        fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create {}", self.dir.display()))?;
        let path = self.dir.join(file_name(Local::now()));
        shot.save(&path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        Ok(path)
    }
}

/// `~/Pictures/Snapt`, falling back to the home folder when there is no Pictures folder.
fn default_dir() -> Result<PathBuf> {
    let dirs = UserDirs::new().context("could not find the home folder")?;
    let base = dirs.picture_dir().unwrap_or(dirs.home_dir());
    Ok(base.join("Snapt"))
}

fn file_name(time: DateTime<Local>) -> String {
    time.format("Snapt_%Y-%m-%d_%H-%M-%S.png").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn file_name_is_sortable_and_filesystem_safe() {
        let time = Local.with_ymd_and_hms(2026, 3, 7, 9, 5, 2).unwrap();
        assert_eq!(file_name(time), "Snapt_2026-03-07_09-05-02.png");
    }
}
