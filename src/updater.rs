//! Updates from GitHub Releases, with no server of Snapt's own.
//!
//! The latest release is read from the GitHub API and the installer for this
//! system is downloaded and started. Network access goes through the system's
//! `curl` (built into Windows 10+, macOS and most Linux desktops), so Snapt
//! carries no HTTP or TLS code. A download runs only once its SHA-256 matches
//! the release's `SHA256SUMS.txt`.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

const LATEST: &str = "https://api.github.com/repos/vkardin/snapt/releases/latest";
pub const RELEASES: &str = "https://github.com/vkardin/snapt/releases/latest";
/// How often a running Snapt looks for a new version.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// A newer release than the running one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    /// The installer for this system, or `None` when it has to be installed by
    /// hand, as with the `.deb` package.
    pub asset: Option<String>,
    /// The release's `SHA256SUMS.txt`.
    pub checksums: Option<String>,
}

/// Checks for updates in the background: shortly after start, then daily.
pub fn watch(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            match check() {
                Ok(update) => {
                    if proxy
                        .send_event(UserEvent::UpdateChecked(Ok(update), false))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(err) => log::info!("update check failed: {err:#}"),
            }
            std::thread::sleep(INTERVAL);
        }
    });
}

/// Checks now, on the user's request; the result comes back as an event.
pub fn check_now(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let result = check().map_err(|err| format!("{err:#}"));
        let _ = proxy.send_event(UserEvent::UpdateChecked(result, true));
    });
}

/// Downloads `update` in the background; `UpdateDownloaded` carries the file.
pub fn download(proxy: EventLoopProxy<UserEvent>, update: Update) {
    std::thread::spawn(move || {
        let result = update
            .asset
            .as_deref()
            .context("no installer for this system")
            .and_then(|asset| {
                let sums = update
                    .checksums
                    .as_deref()
                    .context("the release has no checksums")?;
                fetch_installer(asset, sums)
            })
            .map_err(|err| format!("{err:#}"));
        let _ = proxy.send_event(UserEvent::UpdateDownloaded(result));
    });
}

fn check() -> Result<Option<Update>> {
    // A Flatpak is updated by Flatpak, and can't replace its own files anyway.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Ok(None);
    }
    let json = curl_text(LATEST)?;
    let tag = json_string(&json, "tag_name").context("no tag in the latest release")?;
    let version = tag.trim_start_matches('v').to_string();
    if !is_newer(&version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let assets = json_strings(&json, "browser_download_url");
    Ok(Some(Update {
        asset: pick_asset(&assets).map(str::to_string),
        checksums: assets
            .iter()
            .find(|u| u.ends_with("/SHA256SUMS.txt"))
            .cloned(),
        version,
    }))
}

/// Whether `candidate` (like `1.2.0`) is a later version than `current`.
fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        // Pre-releases such as `1.2.0-beta` never reach /releases/latest; drop the suffix anyway.
        let core = v.split(['-', '+']).next()?;
        core.split('.').map(|n| n.parse().ok()).collect()
    };
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The installer that fits this system, by the names the release workflow gives them.
fn pick_asset(urls: &[String]) -> Option<&str> {
    let suffix = if cfg!(target_os = "windows") {
        "_x64-setup.exe"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "_aarch64.dmg"
        } else {
            "_x64.dmg"
        }
    } else if std::env::var_os("APPIMAGE").is_some() {
        ".AppImage"
    } else {
        // A .deb install is updated through the package manager.
        return None;
    };
    urls.iter()
        .map(String::as_str)
        .find(|u| u.ends_with(suffix))
}

fn fetch_installer(url: &str, sums_url: &str) -> Result<PathBuf> {
    let name = url.rsplit('/').next().unwrap_or("snapt-update");
    let sums = curl_text(sums_url)?;
    let expected = expected_sum(&sums, name)
        .with_context(|| format!("no checksum for {name} in the release"))?;
    let dir = std::env::temp_dir().join("snapt-update");
    std::fs::create_dir_all(&dir).context("failed to create a download folder")?;
    let path = dir.join(name);
    let status = curl()
        .args(["-fsSL", "--max-time", "600", "-o"])
        .arg(&path)
        .arg(url)
        .status()
        .context("failed to run curl")?;
    ensure!(status.success(), "download failed: {status}");
    let actual = file_sum(&path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(&path);
        bail!("the download of {name} is damaged: its SHA-256 does not match the release");
    }
    Ok(path)
}

/// The hash of `name` in `sha256sum` output: `<hex>  <name>`, or `<hex> *<name>` for binary mode.
fn expected_sum<'a>(sums: &'a str, name: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then_some(hash)
    })
}

fn file_sum(path: &Path) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).context("failed to open the download")?;
    let mut hasher = crate::sha256::Sha256::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buf).context("failed to read the download")?;
        if n == 0 {
            return Ok(hasher.hex());
        }
        hasher.update(&buf[..n]);
    }
}

/// Starts installing the downloaded file. Snapt quits right after, so the
/// installer can replace it; the new version starts by itself where it can.
pub fn install(path: &Path) -> Result<()> {
    platform_install(path)
}

#[cfg(target_os = "windows")]
fn platform_install(path: &Path) -> Result<()> {
    // The NSIS installer closes nothing by itself, which is why Snapt exits first.
    // It installs per user, so no administrator prompt appears.
    Command::new(path)
        .spawn()
        .context("failed to start the installer")?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_install(path: &Path) -> Result<()> {
    // .../Snapt.app/Contents/MacOS/snapt -> .../Snapt.app
    let exe = std::env::current_exe()?;
    let bundle = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .context("Snapt is not running from an app bundle")?
        .to_path_buf();
    // Once Snapt has quit: mount the image, swap the bundle, unmount, relaunch.
    let script = r#"
        while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
        mount=$(mktemp -d)
        hdiutil attach -nobrowse -quiet -mountpoint "$mount" "$2" || exit 1
        app=$(ls -d "$mount"/*.app | head -n 1)
        rm -rf "$3.old" && mv "$3" "$3.old" && cp -R "$app" "$3" && rm -rf "$3.old"
        hdiutil detach -quiet "$mount"
        open "$3"
    "#;
    Command::new("/bin/sh")
        .args(["-c", script, "snapt-update"])
        .arg(std::process::id().to_string())
        .arg(path)
        .arg(&bundle)
        .spawn()
        .context("failed to start the update")?;
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_install(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let Some(target) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        bail!("only the AppImage updates itself");
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    // Copy next to the target first: a rename is atomic only within one file system.
    let staged = target.with_extension("AppImage.new");
    std::fs::copy(path, &staged).context("failed to stage the update")?;
    std::fs::rename(&staged, &target).context("failed to replace the AppImage")?;
    let _ = std::fs::remove_file(path);
    // Running files can be replaced on Linux; start the new one once this one quits.
    Command::new("/bin/sh")
        .args([
            "-c",
            r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2""#,
            "snapt-update",
        ])
        .arg(std::process::id().to_string())
        .arg(&target)
        .spawn()
        .context("failed to restart Snapt")?;
    Ok(())
}

fn curl() -> Command {
    let mut cmd = Command::new(if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    });
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.args(["--user-agent", concat!("Snapt/", env!("CARGO_PKG_VERSION"))]);
    cmd
}

fn curl_text(url: &str) -> Result<String> {
    let out = curl()
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            url,
        ])
        .output()
        .context("failed to run curl")?;
    if !out.status.success() {
        bail!(
            "curl {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8(out.stdout).context("the reply is not UTF-8")
}

/// The first string value of `key` in `json`. Enough for GitHub's flat fields
/// without pulling in a JSON parser.
fn json_string(json: &str, key: &str) -> Option<String> {
    json_strings(json, key).into_iter().next()
}

fn json_strings(json: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(i) = rest.find(&needle) {
        rest = &rest[i + needle.len()..];
        let Some(value) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('"') else {
            continue;
        };
        let mut s = String::new();
        let mut chars = value.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => {
                    if let Some(e) = chars.next() {
                        s.push(if e == 'n' { '\n' } else { e });
                    }
                }
                c => s.push(c),
            }
        }
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.1.0", "1.1.0"));
        assert!(!is_newer("1.0.9", "1.1.0"));
        assert!(!is_newer("garbage", "1.1.0"));
    }

    #[test]
    fn reads_release_fields() {
        let json = r#"{"url":"x","tag_name": "v1.2.0","assets":[
            {"name":"a","browser_download_url":"https://e/snapt_1.2.0_x64-setup.exe"},
            {"browser_download_url":"https://e/Snapt_1.2.0_aarch64.dmg"},
            {"browser_download_url":"https://e/Snapt_1.2.0_x64.dmg"},
            {"browser_download_url":"https://e/snapt_1.2.0_x86_64.AppImage"},
            {"browser_download_url":"https://e/snapt_1.2.0_amd64.deb"}],
            "body":"Quote \"inside\""}"#;
        assert_eq!(json_string(json, "tag_name").as_deref(), Some("v1.2.0"));
        assert_eq!(
            json_string(json, "body").as_deref(),
            Some("Quote \"inside\"")
        );
        let urls = json_strings(json, "browser_download_url");
        assert_eq!(urls.len(), 5);
        if cfg!(target_os = "windows") {
            assert!(pick_asset(&urls).unwrap().ends_with("setup.exe"));
        }
    }

    #[test]
    fn finds_the_checksum_of_an_asset() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums =
            format!("{a}  snapt_2.1.0_x64-setup.exe\n{b} *Snapt_2.1.0_x64.dmg\nbroken line\n");
        assert_eq!(
            expected_sum(&sums, "snapt_2.1.0_x64-setup.exe"),
            Some(a.as_str())
        );
        assert_eq!(expected_sum(&sums, "Snapt_2.1.0_x64.dmg"), Some(b.as_str()));
        assert_eq!(expected_sum(&sums, "x64-setup.exe"), None);
        assert_eq!(expected_sum("short  file", "file"), None);
    }

    #[test]
    fn hashes_a_file() {
        let path = std::env::temp_dir().join(format!("snapt-sum-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let sum = file_sum(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            sum,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
