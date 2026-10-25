//! Keeps a log file and writes a crash report when Snapt panics.
//!
//! Release builds on Windows have no console, so without a file the log is
//! lost. Both live in a `logs` folder next to `settings.ini`:
//! `snapt.log` for this run, `snapt.old.log` for the one before, and one
//! `crash-<time>.txt` per panic, of which the newest few are kept.

use std::{
    fs::{self, File},
    io::{self, Write},
    panic::PanicHookInfo,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use crate::settings::Settings;

/// How many crash reports to keep.
const KEEP_CRASHES: usize = 5;

/// The folder with the log and the crash reports.
pub fn dir() -> Option<PathBuf> {
    Some(Settings::path()?.with_file_name("logs"))
}

/// The log file, once this copy knows it is the one that keeps running.
static FILE: OnceLock<Mutex<File>> = OnceLock::new();

/// Starts logging to stderr. Call [`keep`] to also log to `snapt.log`.
pub fn init() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(Tee)))
        .init();
}

/// Starts writing the log file and crash reports. Copies that only hand over
/// to a running one skip this, so they don't overwrite its log.
pub fn keep() {
    if let Some(file) = open_log() {
        let _ = FILE.set(Mutex::new(file));
    }
    install_hook();
}

fn open_log() -> Option<File> {
    let dir = dir()?;
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join("snapt.log");
    let _ = fs::rename(&path, dir.join("snapt.old.log"));
    File::create(path).ok()
}

/// Writes every log line to stderr and, once it is open, to the log file.
struct Tee;

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        if let Some(Ok(mut file)) = FILE.get().map(Mutex::lock) {
            file.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(Ok(mut file)) = FILE.get().map(Mutex::lock) {
            file.flush()?;
        }
        Ok(())
    }
}

fn install_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{}", summary(info));
        match write_report(&report(info)) {
            Some(path) => log::error!("crash report saved to {}", path.display()),
            None => log::error!("could not save the crash report"),
        }
        default(info);
    }));
}

/// The panic message and where it happened.
fn summary(info: &PanicHookInfo) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into());
    let place = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_default();
    format!("panic: {message} at {place}")
}

/// The text of a crash report: what to paste into a bug report.
fn report(info: &PanicHookInfo) -> String {
    let thread = std::thread::current();
    format!(
        "Snapt {} crashed\n\
         OS: {} {}\n\
         Time: {}\n\
         Thread: {}\n\
         {}\n\
         \n\
         Backtrace:\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z"),
        thread.name().unwrap_or("unnamed"),
        summary(info),
        std::backtrace::Backtrace::force_capture(),
    )
}

fn write_report(report: &str) -> Option<PathBuf> {
    let dir = dir()?;
    fs::create_dir_all(&dir).ok()?;
    let name = chrono::Local::now().format("crash-%Y%m%d-%H%M%S.txt");
    let path = dir.join(name.to_string());
    fs::write(&path, report).ok()?;
    prune(&dir);
    Some(path)
}

/// Deletes all but the newest crash reports. Their names sort by time.
fn prune(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut crashes: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    crashes.sort();
    let excess = crashes.len().saturating_sub(KEEP_CRASHES);
    for old in &crashes[..excess] {
        let _ = fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_crash_reports_are_kept() {
        let dir = std::env::temp_dir().join(format!("snapt-crash-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for i in 0..8 {
            fs::write(dir.join(format!("crash-20260101-00000{i}.txt")), "x").unwrap();
        }
        fs::write(dir.join("snapt.log"), "log").unwrap();
        prune(&dir);
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left.len(), KEEP_CRASHES + 1);
        assert_eq!(left[0], "crash-20260101-000003.txt");
        assert!(left.contains(&"snapt.log".to_string()));
    }
}
