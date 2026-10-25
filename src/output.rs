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

    /// Opens the shot in the browser's print dialog.
    pub fn print(&mut self, shot: &RgbaImage) -> Result<()> {
        let page = format!(
            include_str!("pages/print.html"),
            image = png_data_url(shot)?
        );
        open_page("print", &page)
    }

    /// Searches Google Lens for similar images. The page uploads the shot
    /// from the browser, so Snapt itself needs no network code. The shot is
    /// also copied, so it can be pasted into Lens if the upload is blocked.
    pub fn search(&mut self, shot: &RgbaImage) -> Result<()> {
        if let Err(err) = self.copy(shot) {
            log::warn!("{err:#}");
        }
        let page = format!(
            include_str!("pages/search.html"),
            image = png_data_url(shot)?
        );
        open_page("search", &page)
    }
}

/// Writes `html` to a temporary file and opens it in the default browser.
fn open_page(name: &str, html: &str) -> Result<()> {
    let dir = std::env::temp_dir().join("snapt");
    fs::create_dir_all(&dir).context("failed to create a temporary folder")?;
    remove_old_pages(&dir);
    let path = dir.join(format!("{name}-{}.html", Local::now().format("%H%M%S%3f")));
    fs::write(&path, html).with_context(|| format!("failed to write {}", path.display()))?;
    open::that_detached(&path).context("failed to open the browser")
}

/// Pages hold screenshots, so they are removed once the browser surely has them loaded.
fn remove_old_pages(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age.as_secs() > 600));
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn png_data_url(shot: &RgbaImage) -> Result<String> {
    let mut png = Vec::new();
    shot.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("failed to encode the screenshot")?;
    Ok(format!("data:image/png;base64,{}", base64(&png)))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
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
    fn base64_matches_the_rfc_examples() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

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
