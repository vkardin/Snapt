//! The most recent screenshots, kept as PNG files in the cache folder so
//! they survive a restart. The tray menu lists them; picking one pins it.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use directories::ProjectDirs;
use image::RgbaImage;

/// How many screenshots the history keeps by default.
pub const DEFAULT_LIMIT: usize = 10;
const NAME_FORMAT: &str = "%Y%m%d-%H%M%S%.3f";

/// One remembered screenshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub time: DateTime<Local>,
    pub width: u32,
    pub height: u32,
}

impl Entry {
    /// `14:05:32 · 800 × 600`, with the date in front when it was not today.
    pub fn label(&self, now: DateTime<Local>) -> String {
        let time = if self.time.date_naive() == now.date_naive() {
            self.time.format("%H:%M:%S")
        } else {
            self.time.format("%d.%m %H:%M")
        };
        format!("{time}  ·  {} × {}", self.width, self.height)
    }
}

pub struct History {
    dir: Option<PathBuf>,
    /// Newest first.
    entries: Vec<Entry>,
    limit: usize,
}

impl History {
    /// Reads what earlier runs left. A `limit` of 0 turns the history off.
    pub fn load(limit: usize) -> Self {
        let dir = ProjectDirs::from("io.github", "vkardin", "Snapt")
            .map(|dirs| dirs.cache_dir().join("history"));
        let mut history = Self {
            entries: dir.as_deref().map(read_entries).unwrap_or_default(),
            dir,
            limit,
        };
        history.prune();
        history
    }

    /// Keeps at most `limit` screenshots from now on; 0 turns the history off.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.prune();
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Remembers `shot`. The file is written in the background so the
    /// screenshot itself is never slowed down.
    pub fn add(&mut self, shot: &RgbaImage) {
        let Some(dir) = self.dir.clone().filter(|_| self.limit > 0) else {
            return;
        };
        let time = Local::now();
        let path = dir.join(format!("{}.png", time.format(NAME_FORMAT)));
        self.entries.insert(
            0,
            Entry {
                path: path.clone(),
                time,
                width: shot.width(),
                height: shot.height(),
            },
        );
        self.entries.truncate(self.limit);
        let shot = shot.clone();
        let limit = self.limit;
        std::thread::spawn(move || {
            let result = fs::create_dir_all(&dir)
                .context("failed to create the history folder")
                .and_then(|()| {
                    shot.save(&path)
                        .with_context(|| format!("failed to write {}", path.display()))
                });
            if let Err(err) = result {
                log::warn!("{err:#}");
            }
            // Pruned only after writing, so a shot still being written is never left behind.
            prune_dir(&dir, limit);
        });
    }

    /// Opens the screenshot at `index`, newest first.
    pub fn open(&self, index: usize) -> Result<RgbaImage> {
        let entry = self
            .entries
            .get(index)
            .context("that screenshot is no longer in the history")?;
        Ok(image::open(&entry.path)
            .with_context(|| format!("failed to open {}", entry.path.display()))?
            .to_rgba8())
    }

    /// Forgets every screenshot and deletes the files.
    pub fn clear(&mut self) {
        for entry in self.entries.drain(..) {
            let _ = fs::remove_file(entry.path);
        }
    }

    fn prune(&mut self) {
        self.entries.truncate(self.limit);
        if let Some(dir) = &self.dir {
            prune_dir(dir, self.limit);
        }
    }
}

/// Deletes all but the newest `limit` screenshots in `dir`.
fn prune_dir(dir: &Path, limit: usize) {
    for entry in read_entries(dir).into_iter().skip(limit) {
        let _ = fs::remove_file(entry.path);
    }
}

/// The screenshots in `dir`, newest first. Other files are left alone.
fn read_entries(dir: &Path) -> Vec<Entry> {
    let Ok(files) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = files
        .flatten()
        .filter_map(|file| {
            let path = file.path();
            let time = parse_time(path.file_stem()?.to_str()?)?;
            if path.extension()? != "png" {
                return None;
            }
            // Only the header is read, so a long history still loads fast.
            let (width, height) = image::image_dimensions(&path).ok()?;
            Some(Entry {
                path,
                time,
                width,
                height,
            })
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.time));
    entries
}

fn parse_time(stem: &str) -> Option<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(stem, NAME_FORMAT).ok()?;
    Local.from_local_datetime(&naive).earliest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn temp_history(name: &str, limit: usize) -> History {
        let dir = std::env::temp_dir().join(format!("snapt-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        History {
            dir: Some(dir),
            entries: Vec::new(),
            limit,
        }
    }

    #[test]
    fn file_names_round_trip_to_times() {
        let time = Local.with_ymd_and_hms(2026, 10, 25, 18, 4, 9).unwrap();
        let stem = time.format(NAME_FORMAT).to_string();
        assert_eq!(stem, "20261025-180409.000");
        assert_eq!(parse_time(&stem), Some(time));
        assert_eq!(parse_time("Snapt_2026-10-25"), None);
    }

    #[test]
    fn labels_show_the_date_only_for_older_shots() {
        let now = Local.with_ymd_and_hms(2026, 10, 25, 18, 0, 0).unwrap();
        let mut entry = Entry {
            path: PathBuf::new(),
            time: Local.with_ymd_and_hms(2026, 10, 25, 9, 5, 2).unwrap(),
            width: 800,
            height: 600,
        };
        assert_eq!(entry.label(now), "09:05:02  ·  800 × 600");
        entry.time = Local.with_ymd_and_hms(2026, 10, 24, 9, 5, 2).unwrap();
        assert_eq!(entry.label(now), "24.10 09:05  ·  800 × 600");
    }

    #[test]
    fn keeps_only_the_newest_shots_and_reloads_them() {
        let mut history = temp_history("limit", 2);
        let shot = RgbaImage::from_pixel(3, 2, Rgba([1, 2, 3, 255]));
        let dir = history.dir.clone().unwrap();
        for _ in 0..3 {
            history.add(&shot);
            // Writing happens in the background; wait for it.
            let path = history.entries()[0].path.clone();
            for _ in 0..200 {
                if path.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(history.entries().len(), 2);
        let reloaded = read_entries(&dir);
        assert_eq!(reloaded.len(), 2);
        assert_eq!(reloaded[0].path, history.entries()[0].path);
        assert_eq!(history.open(1).unwrap().dimensions(), (3, 2));
        history.clear();
        assert!(read_entries(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zero_limit_keeps_nothing() {
        let mut history = temp_history("off", 0);
        history.add(&RgbaImage::new(1, 1));
        assert!(history.entries().is_empty());
    }
}
