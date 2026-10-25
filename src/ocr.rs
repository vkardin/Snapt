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

/// The text in `image`, one recognized line per line.
pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
    // Screen text is small for OCR engines tuned on photos and scans; doubling it helps.
    let image = if image.width() * image.height() <= 2_000_000 {
        imageops::resize(
            image,
            image.width() * 2,
            image.height() * 2,
            imageops::FilterType::Triangle,
        )
    } else {
        image.clone()
    };
    let text = platform::recognize(&image)?;
    Ok(tidy(&text))
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
    use super::Error;
    use image::RgbaImage;
    use windows::{
        Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
    };

    pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
        // Each OCR run gets its own thread; it may already be initialized, which is fine.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        // The user's languages that have OCR installed: English, Russian, Ukrainian and so on.
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| Error::Missing)?;
        let max = OcrEngine::MaxImageDimension()?;
        let image = if image.width() > max || image.height() > max {
            let k = max as f32 / image.width().max(image.height()) as f32;
            image::imageops::resize(
                image,
                (image.width() as f32 * k) as u32,
                (image.height() as f32 * k) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            image.clone()
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
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut text = String::new();
        for line in result.Lines()? {
            text.push_str(&line.Text()?.to_string_lossy());
            text.push('\n');
        }
        Ok(text)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Error;
    use image::{ImageFormat, RgbaImage};
    use objc2::{AnyThread, rc::Retained};
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };

    pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
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
        let mut text = String::new();
        for observation in request.results().unwrap_or_default().iter() {
            if let Some(best) = observation.topCandidates(1).firstObject() {
                text.push_str(&best.string().to_string());
                text.push('\n');
            }
        }
        Ok(text)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::Error;
    use image::{ImageFormat, RgbaImage};
    use std::process::Command;

    pub fn recognize(image: &RgbaImage) -> Result<String, Error> {
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

    #[test]
    fn blank_lines_and_trailing_spaces_go() {
        assert_eq!(tidy("  Hello  \n\n \nWorld\n"), "  Hello\nWorld");
    }
}
