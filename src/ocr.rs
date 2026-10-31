//! Text recognition with what the system already has: Windows.Media.Ocr on
//! Windows, the Vision framework on macOS and Tesseract on Linux, so Snapt
//! ships no OCR models of its own.

use image::{RgbaImage, imageops};

/// Why no text came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The system has no recognizer: no OCR language on Windows, no Tesseract on Linux.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Missing,
    Failed(String),
}

impl<E: std::fmt::Display> From<E> for Error {
    fn from(err: E) -> Self {
        Error::Failed(err.to_string())
    }
}

/// One recognized word and where it is, in the pixels of the image given.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    /// Words with the same number are on one line, left to right.
    pub line: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The text in `image`, one recognized line per line.
pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
    let (image, _) = enlarge(image);
    let text = platform::recognize(&image)?;
    Ok(tidy(&text))
}

/// The words in `image` with their boxes.
pub fn words(image: &RgbaImage) -> Result<Vec<Word>, Error> {
    let (enlarged, k) = enlarge(image);
    let mut words = platform::words(&enlarged)?;
    for word in &mut words {
        word.x /= k;
        word.y /= k;
        word.w /= k;
        word.h /= k;
    }
    words.retain(|w| !w.text.trim().is_empty());
    Ok(words)
}

/// Screen text is small for OCR engines tuned on photos and scans; doubling
/// it helps. Returns the image and how much it grew.
fn enlarge(image: &RgbaImage) -> (RgbaImage, f32) {
    if image.width() * image.height() <= 2_000_000 {
        let big = imageops::resize(
            image,
            image.width() * 2,
            image.height() * 2,
            imageops::FilterType::Triangle,
        );
        (big, 2.0)
    } else {
        (image.clone(), 1.0)
    }
}

/// Trims each line and drops blank ones.
fn tidy(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{Error, Word};
    use image::RgbaImage;
    use windows::{
        Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::{OcrEngine, OcrResult},
        Storage::Streams::DataWriter,
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
    };

    pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
        let (result, _) = run(image)?;
        let mut text = String::new();
        for line in result.Lines()? {
            text.push_str(&line.Text()?.to_string_lossy());
            text.push('\n');
        }
        Ok(text)
    }

    pub fn words(image: &RgbaImage) -> Result<Vec<Word>, Error> {
        let (result, k) = run(image)?;
        let mut words = Vec::new();
        for (line, l) in result.Lines()?.into_iter().enumerate() {
            for word in l.Words()? {
                let r = word.BoundingRect()?;
                words.push(Word {
                    text: word.Text()?.to_string_lossy(),
                    line,
                    x: r.X / k,
                    y: r.Y / k,
                    w: r.Width / k,
                    h: r.Height / k,
                });
            }
        }
        Ok(words)
    }

    /// Recognizes `image`, shrunk to what the engine takes; returns how much it shrank.
    fn run(image: &RgbaImage) -> Result<(OcrResult, f32), Error> {
        // Each OCR run gets its own thread; it may already be initialized, which is fine.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        // The user's languages that have OCR installed: English, Russian, Ukrainian and so on.
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| Error::Missing)?;
        let max = OcrEngine::MaxImageDimension()?;
        let (image, k) = if image.width() > max || image.height() > max {
            let k = max as f32 / image.width().max(image.height()) as f32;
            let small = image::imageops::resize(
                image,
                (image.width() as f32 * k) as u32,
                (image.height() as f32 * k) as u32,
                image::imageops::FilterType::Triangle,
            );
            (small, k)
        } else {
            (image.clone(), 1.0)
        };
        let mut bgra = image.as_raw().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buffer = writer.DetachBuffer()?;
        let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            image.width() as i32,
            image.height() as i32,
            BitmapAlphaMode::Premultiplied,
        )?;
        Ok((engine.RecognizeAsync(&bitmap)?.join()?, k))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Error, Word};
    use image::{ImageFormat, RgbaImage};
    use objc2::{AnyThread, msg_send, rc::Retained};
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSRange, NSString};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedText, VNRectangleObservation,
        VNRequest, VNRequestTextRecognitionLevel,
    };

    pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
        let mut text = String::new();
        for best in run(image)? {
            text.push_str(&best.string().to_string());
            text.push('\n');
        }
        Ok(text)
    }

    pub fn words(image: &RgbaImage) -> Result<Vec<Word>, Error> {
        let (w, h) = (image.width() as f64, image.height() as f64);
        let mut words = Vec::new();
        for (line, best) in run(image)?.iter().enumerate() {
            let text = best.string().to_string();
            // Vision measures ranges in UTF-16 units.
            let mut at = 0;
            for piece in text.split(' ') {
                let len = piece.encode_utf16().count();
                let range = NSRange::new(at, len);
                at += len + 1;
                if piece.is_empty() {
                    continue;
                }
                // SAFETY: the range lies inside the candidate's own string.
                let found: Result<Retained<VNRectangleObservation>, Retained<NSError>> =
                    unsafe { msg_send![&**best, boundingBoxForRange: range, error: _] };
                let Ok(found) = found else { continue };
                // SAFETY: a plain getter; normalized, with the origin bottom left.
                let r = unsafe { found.boundingBox() };
                words.push(Word {
                    text: piece.to_string(),
                    line,
                    x: (r.origin.x * w) as f32,
                    y: ((1.0 - r.origin.y - r.size.height) * h) as f32,
                    w: (r.size.width * w) as f32,
                    h: (r.size.height * h) as f32,
                });
            }
        }
        Ok(words)
    }

    /// The best reading of each line of text in `image`.
    fn run(image: &RgbaImage) -> Result<Vec<Retained<VNRecognizedText>>, Error> {
        let mut png = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)?;
        let data = NSData::with_bytes(&png);
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &NSDictionary::new(),
        );
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        // Only languages this macOS knows; an unknown one fails the whole request.
        // SAFETY: a plain query on a request this thread owns.
        let supported = unsafe { request.supportedRecognitionLanguagesAndReturnError() }
            .map(|langs| langs.iter().map(|l| l.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        let wanted: Vec<Retained<NSString>> = ["en-US", "ru-RU", "uk-UA"]
            .iter()
            .filter(|l| supported.iter().any(|s| s == *l))
            .map(|l| NSString::from_str(l))
            .collect();
        if !wanted.is_empty() {
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&wanted));
        }
        let as_request: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[as_request]))
            .map_err(|err| Error::Failed(err.localizedDescription().to_string()))?;
        Ok(request
            .results()
            .unwrap_or_default()
            .iter()
            .filter_map(|observation| observation.topCandidates(1).firstObject())
            .collect())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::{Error, Word};
    use image::{ImageFormat, RgbaImage};
    use std::process::Command;

    pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
        run(image, &[])
    }

    pub fn words(image: &RgbaImage) -> Result<Vec<Word>, Error> {
        Ok(parse_tsv(&run(image, &["tsv"])?))
    }

    /// Tesseract's table output: one row per word with its block, line and box.
    pub fn parse_tsv(tsv: &str) -> Vec<Word> {
        let mut words = Vec::new();
        let mut lines: Vec<(u32, u32, u32, u32)> = Vec::new();
        for row in tsv.lines().skip(1) {
            let cols: Vec<&str> = row.split('\t').collect();
            if cols.len() < 12 || cols[0] != "5" {
                continue;
            }
            let n = |i: usize| cols[i].trim().parse::<u32>().unwrap_or(0);
            let key = (n(1), n(2), n(3), n(4));
            let line = match lines.iter().position(|l| *l == key) {
                Some(i) => i,
                None => {
                    lines.push(key);
                    lines.len() - 1
                }
            };
            words.push(Word {
                text: cols[11..].join("\t"),
                line,
                x: n(6) as f32,
                y: n(7) as f32,
                w: n(8) as f32,
                h: n(9) as f32,
            });
        }
        words
    }

    fn run(image: &RgbaImage, config: &[&str]) -> Result<String, Error> {
        let langs = Command::new("tesseract")
            .arg("--list-langs")
            .output()
            .map_err(|_| Error::Missing)?;
        let installed = String::from_utf8_lossy(&langs.stdout);
        let wanted: Vec<&str> = ["eng", "rus", "ukr"]
            .into_iter()
            .filter(|l| installed.lines().any(|i| i.trim() == *l))
            .collect();
        let path = std::env::temp_dir().join(format!("snapt-ocr-{}.png", std::process::id()));
        image.save_with_format(&path, ImageFormat::Png)?;
        let mut cmd = Command::new("tesseract");
        cmd.arg(&path).arg("stdout");
        if !wanted.is_empty() {
            cmd.args(["-l", &wanted.join("+")]);
        }
        cmd.args(config);
        let out = cmd.output();
        let _ = std::fs::remove_file(&path);
        let out = out?;
        if !out.status.success() {
            return Err(Error::Failed(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn tesseract_tables_become_words_on_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   4\t1\t1\t1\t1\t0\t10\t10\t200\t20\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t10\t10\t50\t20\t96\tHello\n\
                   5\t1\t1\t1\t1\t2\t70\t10\t60\t20\t95\tworld\n\
                   5\t1\t1\t1\t2\t1\t10\t40\t80\t20\t91\tnext\n";
        let words = platform::parse_tsv(tsv);
        assert_eq!(words.len(), 3);
        assert_eq!(
            (words[1].text.as_str(), words[1].line, words[1].x),
            ("world", 0, 70.0)
        );
        assert_eq!(
            (words[2].text.as_str(), words[2].line, words[2].y),
            ("next", 1, 40.0)
        );
    }

    #[test]
    fn blank_lines_and_trailing_spaces_go() {
        assert_eq!(tidy("  Hello  \n\n \nWorld\n"), "  Hello\nWorld");
    }
}
