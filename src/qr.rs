//! Reading QR codes in a screenshot.

use image::RgbaImage;

/// The text of every QR code in `image`, top to bottom, one per line;
/// `None` when there is none.
pub fn read(image: &RgbaImage) -> Option<String> {
    let (w, h) = image.dimensions();
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(w as usize, h as usize, |x, y| {
            let [r, g, b, _] = image.get_pixel(x as u32, y as u32).0;
            ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000) as u8
        });
    let mut found: Vec<(i32, String)> = prepared
        .detect_grids()
        .into_iter()
        .filter_map(|grid| {
            let top = grid.bounds.iter().map(|p| p.y).min().unwrap_or(0);
            let (_, text) = grid.decode().ok()?;
            (!text.is_empty()).then_some((top, text))
        })
        .collect();
    found.sort_by_key(|(top, _)| *top);
    found.dedup_by(|a, b| a.1 == b.1);
    (!found.is_empty()).then(|| {
        found
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` as a QR code, `module` pixels per square, on a gray page at (`x`, `y`).
    fn page(codes: &[(&str, u32, u32)], module: u32) -> RgbaImage {
        let mut page = RgbaImage::from_pixel(600, 600, image::Rgba([235, 235, 240, 255]));
        for (text, x, y) in codes {
            let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
            let n = code.width() as u32;
            let colors = code.to_colors();
            // The quiet zone around it is white.
            for py in 0..(n + 8) * module {
                for px in 0..(n + 8) * module {
                    let (mx, my) = ((px / module) as i32 - 4, (py / module) as i32 - 4);
                    let dark = mx >= 0
                        && my >= 0
                        && (mx as u32) < n
                        && (my as u32) < n
                        && colors[my as usize * n as usize + mx as usize] == qrcode::Color::Dark;
                    let v = if dark { 20 } else { 255 };
                    page.put_pixel(x + px, y + py, image::Rgba([v, v, v, 255]));
                }
            }
        }
        page
    }

    #[test]
    fn a_code_is_read() {
        let image = page(&[("https://vkardin.github.io/snapt/", 40, 60)], 5);
        assert_eq!(
            read(&image).as_deref(),
            Some("https://vkardin.github.io/snapt/")
        );
    }

    #[test]
    fn several_codes_come_top_to_bottom() {
        let image = page(&[("second", 300, 320), ("first", 20, 20)], 4);
        assert_eq!(read(&image).as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn no_code_is_none() {
        let image = RgbaImage::from_pixel(200, 120, image::Rgba([255, 255, 255, 255]));
        assert_eq!(read(&image), None);
    }
}
