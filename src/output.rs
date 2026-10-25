use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use chrono::{DateTime, Local};
use directories::UserDirs;
use image::{DynamicImage, RgbaImage};

/// Where finished screenshots go: the clipboard or an image file.
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

    pub fn copy(&mut self, shot: &RgbaImage) -> Result<()> {
        let clipboard = self.clipboard.as_mut().context("clipboard unavailable")?;
        let data = ImageData {
            width: shot.width() as usize,
            height: shot.height() as usize,
            bytes: Cow::Borrowed(shot.as_raw()),
        };
        clipboard
            .set_image(data)
            .context("failed to copy the screenshot")
    }

    /// Asks where to save the shot. Returns `None` when the user cancels.
    pub fn save_as(&mut self, shot: &RgbaImage) -> Result<Option<PathBuf>> {
        // Best effort: the dialog still works if the default folder can't be created.
        let _ = fs::create_dir_all(&self.dir);
        let picked = rfd::FileDialog::new()
            .set_title("Save screenshot")
            .set_directory(&self.dir)
            .set_file_name(file_name(Local::now()))
            .add_filter("PNG image", &["png"])
            .add_filter("JPEG image", &["jpg", "jpeg"])
            .save_file();
        let Some(path) = picked else { return Ok(None) };

        save(shot, &path)?;
        if let Some(parent) = path.parent() {
            // Next time, the dialog opens where the user saved last.
            self.dir = parent.to_path_buf();
        }
        Ok(Some(path))
    }
}

fn save(shot: &RgbaImage, path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let result = match ext.as_deref() {
        // JPEG has no alpha channel.
        Some("jpg" | "jpeg") => DynamicImage::ImageRgba8(shot.clone()).to_rgb8().save(path),
        Some(_) => shot.save(path),
        None => shot.save_with_format(path, image::ImageFormat::Png),
    };
    result.with_context(|| format!("failed to save {}", path.display()))
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
    use image::Rgba;

    #[test]
    fn file_name_is_sortable_and_filesystem_safe() {
        let time = Local.with_ymd_and_hms(2026, 3, 7, 9, 5, 2).unwrap();
        assert_eq!(file_name(time), "Snapt_2026-03-07_09-05-02.png");
    }

    #[test]
    fn saves_png_and_jpeg_by_extension() {
        let dir = std::env::temp_dir().join(format!("snapt-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let shot = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
        for name in ["a.png", "b.JPG", "c"] {
            let path = dir.join(name);
            save(&shot, &path).unwrap();
            let loaded = image::ImageReader::open(&path)
                .and_then(|r| r.with_guessed_format())
                .unwrap()
                .decode()
                .unwrap();
            assert_eq!((loaded.width(), loaded.height()), (4, 3));
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
