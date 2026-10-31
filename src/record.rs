//! Recording an area of the screen to GIF, and to MP4 when ffmpeg is installed.
//!
//! The worker captures the area ten times a second, like a scrolling
//! screenshot, and streams the frames to the encoders as they come: a GIF
//! encoder on its own thread, and ffmpeg through a pipe. Both write to
//! temporary files; when the user stops, they pick which one to keep.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use image::{Delay, Frame, RgbaImage, codecs::gif, imageops};
use winit::event_loop::EventLoopProxy;

use crate::{app::UserEvent, scroll::Region};

const FPS: u32 = 10;
/// Recording stops by itself after this long.
const MAX_SECONDS: u32 = 120;

/// A finished recording: temporary files, deleted when it is dropped.
pub struct Video {
    pub gif: PathBuf,
    pub mp4: Option<PathBuf>,
}

impl Video {
    /// Moves the file in the format of `target`'s extension there: MP4 for
    /// `.mp4` when there is one, GIF otherwise. Returns where it went.
    pub fn keep(&self, target: &Path) -> Result<PathBuf> {
        let mp4 = target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mp4"));
        let (source, target) = match &self.mp4 {
            Some(source) if mp4 => (source, target.to_path_buf()),
            _ if target
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gif")) =>
            {
                (&self.gif, target.to_path_buf())
            }
            _ => (&self.gif, target.with_extension("gif")),
        };
        // A rename fails across drives, so fall back to a copy.
        if std::fs::rename(source, &target).is_err() {
            std::fs::copy(source, &target)
                .with_context(|| format!("failed to save {}", target.display()))?;
        }
        Ok(target)
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.gif);
        if let Some(mp4) = &self.mp4 {
            let _ = std::fs::remove_file(mp4);
        }
    }
}

fn ffmpeg() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new("ffmpeg");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn temp_path(ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("snapt-recording-{}.{ext}", std::process::id()))
}

/// Records `region` until `stop` is set, counting the seconds in `seconds`.
pub fn capture_loop(
    region: Region,
    stop: &AtomicBool,
    seconds: &AtomicU32,
    proxy: &EventLoopProxy<UserEvent>,
) -> Option<Video> {
    let tick = || {
        let _ = proxy.send_event(UserEvent::ScrollGrew);
    };
    match record(region, stop, seconds, tick) {
        Ok(video) => Some(video),
        Err(err) => {
            log::error!("recording failed: {err:#}");
            None
        }
    }
}

/// `tick` runs each second, to show the time.
fn record(
    region: Region,
    stop: &AtomicBool,
    seconds: &AtomicU32,
    tick: impl Fn(),
) -> Result<Video> {
    let monitor = crate::scroll::find_monitor(region.monitor)?;
    // Video encoders want even sizes.
    let (w, h) = ((region.w & !1).max(2), (region.h & !1).max(2));
    let gif_path = temp_path("gif");
    let (frames, encoded) = mpsc::sync_channel::<(RgbaImage, Instant)>(4);
    let gif = {
        let path = gif_path.clone();
        std::thread::spawn(move || encode_gif(&path, encoded))
    };
    let mut mp4 = Mp4::start(w, h);

    let start = Instant::now();
    let interval = Duration::from_secs(1) / FPS;
    let mut next = start;
    // Waiting one interval first also gives the overlay time to disappear.
    while !stop.load(Ordering::Relaxed) {
        next += interval;
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let now = Instant::now();
        let frame = match monitor.capture_image() {
            Ok(image) => imageops::crop_imm(&image, region.x, region.y, w, h).to_image(),
            Err(err) => {
                log::warn!("recording frame skipped: {err}");
                continue;
            }
        };
        if let Some(encoder) = &mut mp4 {
            let due = (now - start).as_millis() as u64 * FPS as u64 / 1000 + 1;
            if !encoder.push(&frame, due) {
                mp4 = None;
            }
        }
        // A busy GIF encoder drops frames; the next one just stays longer.
        let _ = frames.try_send((frame, now));
        let elapsed = (now - start).as_secs() as u32;
        if elapsed != seconds.swap(elapsed, Ordering::Relaxed) {
            tick();
        }
        if elapsed >= MAX_SECONDS {
            break;
        }
    }
    drop(frames);
    let mp4 = mp4.and_then(Mp4::finish);
    join(gif)?;
    Ok(Video { gif: gif_path, mp4 })
}

fn join(worker: JoinHandle<Result<()>>) -> Result<()> {
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("the GIF encoder crashed"))?
}

/// Writes the frames as they come. Each one is written when the next arrives,
/// so its delay is known; repeated frames only lengthen the one before.
fn encode_gif(path: &Path, frames: mpsc::Receiver<(RgbaImage, Instant)>) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut encoder = gif::GifEncoder::new_with_speed(BufWriter::new(file), 10);
    encoder.set_repeat(gif::Repeat::Infinite)?;
    let mut held: Option<(RgbaImage, Instant)> = None;
    for (frame, at) in frames {
        match &held {
            Some((last, _)) if *last == frame => continue,
            Some((last, since)) => write_gif_frame(&mut encoder, last.clone(), at - *since)?,
            None => {}
        }
        held = Some((frame, at));
    }
    if let Some((last, _)) = held {
        write_gif_frame(&mut encoder, last, Duration::from_secs(1) / FPS)?;
    }
    Ok(())
}

fn write_gif_frame<W: Write>(
    encoder: &mut gif::GifEncoder<W>,
    image: RgbaImage,
    shown: Duration,
) -> Result<()> {
    // GIF delays are in hundredths of a second.
    let delay = Delay::from_saturating_duration(shown.max(Duration::from_millis(20)));
    encoder
        .encode_frame(Frame::from_parts(image, 0, 0, delay))
        .context("failed to write a GIF frame")
}

/// ffmpeg turning raw frames from its stdin into an H.264 MP4.
struct Mp4 {
    child: Child,
    path: PathBuf,
    written: u64,
}

impl Mp4 {
    fn start(w: u32, h: u32) -> Option<Self> {
        let path = temp_path("mp4");
        let child = ffmpeg()
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
            ])
            .args(["-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-"])
            .args([
                "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p",
            ])
            .args(["-movflags", "+faststart"])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(child) => Some(Self {
                child,
                path,
                written: 0,
            }),
            Err(err) => {
                log::info!("no MP4 without ffmpeg: {err}");
                None
            }
        }
    }

    /// Writes `frame` until `due` frames are in, so slow captures keep time.
    /// Returns false when ffmpeg is gone.
    fn push(&mut self, frame: &RgbaImage, due: u64) -> bool {
        let Some(stdin) = self.child.stdin.as_mut() else {
            return false;
        };
        while self.written < due {
            if stdin.write_all(frame.as_raw()).is_err() {
                log::warn!("ffmpeg stopped; only the GIF is kept");
                let _ = self.child.kill();
                return false;
            }
            self.written += 1;
        }
        true
    }

    fn finish(mut self) -> Option<PathBuf> {
        drop(self.child.stdin.take());
        let ok = self.child.wait().is_ok_and(|s| s.success());
        if !ok {
            let _ = std::fs::remove_file(&self.path);
        }
        ok.then_some(self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn repeated_frames_become_one_longer_gif_frame() {
        let path = std::env::temp_dir().join(format!("snapt-gif-test-{}.gif", std::process::id()));
        let (send, receive) = mpsc::sync_channel(8);
        let t = Instant::now();
        let red = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]));
        send.send((red.clone(), t)).unwrap();
        send.send((red, t + Duration::from_millis(100))).unwrap();
        send.send((blue, t + Duration::from_millis(300))).unwrap();
        drop(send);
        encode_gif(&path, receive).unwrap();
        let decoder = gif::GifDecoder::new(std::io::BufReader::new(File::open(&path).unwrap()));
        let frames = image::AnimationDecoder::into_frames(decoder.unwrap())
            .collect_frames()
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].delay().numer_denom_ms(), (300, 1));
    }

    #[test]
    fn the_chosen_extension_picks_the_file() {
        let dir = std::env::temp_dir().join(format!("snapt-keep-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = Video {
            gif: dir.join("a.gif"),
            mp4: None,
        };
        std::fs::write(&video.gif, "gif").unwrap();
        // Without an MP4, asking for one still saves the GIF.
        let kept = video.keep(&dir.join("clip.mp4")).unwrap();
        assert_eq!(kept, dir.join("clip.gif"));
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "gif");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn frames_become_an_mp4_when_ffmpeg_is_there() {
        let Some(mut mp4) = Mp4::start(16, 8) else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let frame = RgbaImage::from_pixel(16, 8, Rgba([0, 128, 255, 255]));
        assert!(mp4.push(&frame, 5));
        let path = mp4.finish().expect("ffmpeg made the video");
        let size = std::fs::metadata(&path).unwrap().len();
        std::fs::remove_file(&path).unwrap();
        assert!(size > 0);
    }

    // Needs a screen to capture, e.g. `xvfb-run cargo test -- --ignored recording`.
    #[test]
    #[ignore]
    fn recording_a_screen_area() {
        let monitor = xcap::Monitor::all().unwrap().remove(0);
        let region = Region {
            monitor: (monitor.x().unwrap(), monitor.y().unwrap()),
            scale: 1.0,
            x: 10,
            y: 10,
            w: 101,
            h: 61,
        };
        let stop = AtomicBool::new(false);
        let seconds = AtomicU32::new(0);
        let video = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(1500));
                stop.store(true, Ordering::Relaxed);
            });
            record(region, &stop, &seconds, || {}).unwrap()
        });
        let gif = gif::GifDecoder::new(std::io::BufReader::new(File::open(&video.gif).unwrap()));
        let (w, h) = image::ImageDecoder::dimensions(&gif.unwrap());
        assert_eq!((w, h), (100, 60));
        if let Some(mp4) = &video.mp4 {
            assert!(std::fs::metadata(mp4).unwrap().len() > 0);
        }
    }
}
