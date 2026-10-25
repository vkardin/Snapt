//! Compares rendered pictures with the reference PNGs in `tests/snapshots`.
//!
//! A test renders into a pixel buffer and calls [`assert_matches`]. When the
//! drawing changes on purpose, regenerate the references with
//! `SNAPT_UPDATE_SNAPSHOTS=1 cargo test` and look at the new PNGs before
//! committing them. A failing test writes what it drew and a diff next to
//! each other in `target/snapshots`.

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use image::{Rgba, RgbaImage};

/// A channel that differs by more than this counts as a changed pixel, which
/// absorbs anti-aliasing differences between CPUs.
const CHANNEL_TOLERANCE: u8 = 8;
/// The share of pixels that may change before a snapshot fails.
const PIXEL_TOLERANCE: f64 = 0.002;

static LANGUAGE: Mutex<()> = Mutex::new(());

/// Switches the interface language for one test. Tests run in parallel and
/// the language is global, so the guard keeps others from switching it.
pub fn language(lang: crate::i18n::Lang) -> MutexGuard<'static, ()> {
    let guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    crate::i18n::set(lang);
    guard
}

/// Turns a `0RGB` buffer, as the windows show it, into an image.
pub fn from_0rgb(buf: &[u32], width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let p = buf[(y * width + x) as usize];
        Rgba([(p >> 16) as u8, (p >> 8) as u8, p as u8, 255])
    })
}

/// Fails the test when `image` differs from the reference called `name`.
pub fn assert_matches(name: &str, image: &RgbaImage) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reference = root.join("tests/snapshots").join(format!("{name}.png"));
    let update = std::env::var_os("SNAPT_UPDATE_SNAPSHOTS").is_some();
    if update || (!reference.exists() && std::env::var_os("CI").is_none()) {
        std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
        image.save(&reference).unwrap();
        eprintln!("wrote {}", reference.display());
        return;
    }
    let expected = image::open(&reference)
        .unwrap_or_else(|e| panic!("no reference {}: {e}", reference.display()))
        .to_rgba8();
    let Some(changed) = changed_pixels(&expected, image) else {
        let out = save_failure(root, name, image, None);
        panic!(
            "{name}: size {:?} instead of {:?}; see {}",
            image.dimensions(),
            expected.dimensions(),
            out.display()
        );
    };
    let share = changed.len() as f64 / (image.width() * image.height()) as f64;
    if share > PIXEL_TOLERANCE {
        let out = save_failure(root, name, image, Some(&changed));
        panic!(
            "{name}: {} pixels ({:.2}%) differ from the reference; see {}",
            changed.len(),
            share * 100.0,
            out.display()
        );
    }
}

/// The pixels that differ, or `None` when the sizes don't match.
fn changed_pixels(a: &RgbaImage, b: &RgbaImage) -> Option<Vec<(u32, u32)>> {
    if a.dimensions() != b.dimensions() {
        return None;
    }
    Some(
        a.enumerate_pixels()
            .filter(|(x, y, p)| {
                let q = b.get_pixel(*x, *y);
                p.0.iter()
                    .zip(q.0)
                    .any(|(&c, d)| c.abs_diff(d) > CHANNEL_TOLERANCE)
            })
            .map(|(x, y, _)| (x, y))
            .collect(),
    )
}

/// Saves the picture and a diff with the changed pixels in red.
fn save_failure(
    root: &Path,
    name: &str,
    image: &RgbaImage,
    changed: Option<&[(u32, u32)]>,
) -> PathBuf {
    let dir = root.join("target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(format!("{name}.actual.png"))).unwrap();
    if let Some(changed) = changed {
        let mut diff = image.clone();
        for p in diff.pixels_mut() {
            let gray = ((p[0] as u32 + p[1] as u32 + p[2] as u32) / 6) as u8;
            *p = Rgba([gray, gray, gray, 255]);
        }
        for &(x, y) in changed {
            diff.put_pixel(x, y, Rgba([255, 0, 0, 255]));
        }
        diff.save(dir.join(format!("{name}.diff.png"))).unwrap();
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_differences_are_tolerated() {
        let a = RgbaImage::from_pixel(4, 4, Rgba([100, 100, 100, 255]));
        let mut b = a.clone();
        b.put_pixel(0, 0, Rgba([104, 100, 100, 255]));
        assert_eq!(changed_pixels(&a, &b).unwrap().len(), 0);
        b.put_pixel(1, 1, Rgba([200, 100, 100, 255]));
        assert_eq!(changed_pixels(&a, &b).unwrap(), vec![(1, 1)]);
        assert!(changed_pixels(&a, &RgbaImage::new(2, 2)).is_none());
    }
}
