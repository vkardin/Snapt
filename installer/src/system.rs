//! What the installer does to the system: files, shortcuts, the Apps entry and autostart.
//!
//! The layout matches the NSIS installer Snapt shipped before, so this one
//! updates those installs in place: `%LOCALAPPDATA%\Snapt` per user, or
//! `%ProgramFiles%\Snapt` for everyone, with the same `Snapt` uninstall key.

use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("SNAPT_VERSION");

/// The Snapt binary, deflated by build.rs. Empty in builds without `SNAPT_EXE`.
static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));

/// A step of the work, shown under the title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Closing,
    Copying,
    Shortcuts,
    Registering,
    Removing,
}

/// An install found on this computer.
#[derive(Debug, Clone)]
pub struct Existing {
    pub all_users: bool,
    pub dir: PathBuf,
    pub version: String,
    pub desktop: bool,
}

/// What to install.
#[derive(Debug, Clone)]
pub struct Plan {
    pub dir: PathBuf,
    pub all_users: bool,
    pub desktop: bool,
}

pub type Progress<'a> = &'a dyn Fn(f32, Step);

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(windows))]
pub use preview::*;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::{fs, os::windows::process::CommandExt, process::Command, thread, time::Duration};
    use windows::{
        Win32::{
            Foundation::ERROR_FILE_NOT_FOUND,
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, CoTaskMemFree, IPersistFile,
                },
                Registry::{
                    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_DWORD,
                    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
                    RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
                },
            },
            UI::{
                Shell::{
                    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_LocalAppData,
                    FOLDERID_ProgramFiles, FOLDERID_Programs, FOLDERID_PublicDesktop, IShellLinkW,
                    IsUserAnAdmin, KF_FLAG_DEFAULT, SEE_MASK_NOASYNC, SHCNE_ASSOCCHANGED,
                    SHCNF_IDLIST, SHChangeNotify, SHELLEXECUTEINFOW, SHGetKnownFolderPath,
                    ShellExecuteExW, ShellLink,
                },
                WindowsAndMessaging::SW_SHOWNORMAL,
            },
        },
        core::{GUID, Interface, PCWSTR, w},
    };

    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Snapt";
    /// Where the NSIS installer kept its install folder and language.
    const NSIS_KEY: &str = r"Software\vkardin\Snapt";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// The link class Snapt registers while it replaces the Snipping Tool.
    const SCREENCLIP_KEY: &str = r"Software\Classes\Snapt.ScreenClip";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn root(all_users: bool) -> HKEY {
        if all_users {
            HKEY_LOCAL_MACHINE
        } else {
            HKEY_CURRENT_USER
        }
    }

    fn known_folder(id: &GUID) -> Result<PathBuf, String> {
        unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)
                .map_err(|e| format!("a system folder is missing: {e}"))?;
            let path = p.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(p.0 as _));
            path.map(PathBuf::from)
        }
    }

    fn programs(all_users: bool) -> Result<PathBuf, String> {
        known_folder(if all_users {
            &FOLDERID_CommonPrograms
        } else {
            &FOLDERID_Programs
        })
    }

    fn desktop(all_users: bool) -> Result<PathBuf, String> {
        known_folder(if all_users {
            &FOLDERID_PublicDesktop
        } else {
            &FOLDERID_Desktop
        })
    }

    pub fn install_dir(all_users: bool) -> PathBuf {
        let base = known_folder(if all_users {
            &FOLDERID_ProgramFiles
        } else {
            &FOLDERID_LocalAppData
        });
        base.unwrap_or_else(|_| PathBuf::from(r"C:\Program Files"))
            .join("Snapt")
    }

    pub fn is_admin() -> bool {
        unsafe { IsUserAnAdmin().as_bool() }
    }

    fn read_string(root: HKEY, key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let err = unsafe {
            RegGetValueW(
                root,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        err.ok().ok()?;
        let chars = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
    }

    enum Value<'a> {
        Str(&'a str),
        Dword(u32),
    }

    fn write_values(root: HKEY, key: &str, values: &[(&str, Value)]) -> Result<(), String> {
        let key = wide(key);
        let mut hkey = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(key.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            )
            .ok()
            .map_err(|e| format!("failed to write to the registry: {e}"))?;
            let mut result = Ok(());
            for (name, value) in values {
                let name = wide(name);
                let err = match value {
                    Value::Str(s) => {
                        let data: Vec<u8> = wide(s).iter().flat_map(|c| c.to_le_bytes()).collect();
                        RegSetValueExW(hkey, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data))
                    }
                    Value::Dword(n) => RegSetValueExW(
                        hkey,
                        PCWSTR(name.as_ptr()),
                        None,
                        REG_DWORD,
                        Some(&n.to_le_bytes()),
                    ),
                };
                if let Err(e) = err.ok() {
                    result = Err(format!("failed to write to the registry: {e}"));
                }
            }
            let _ = RegCloseKey(hkey);
            result
        }
    }

    fn delete_key(root: HKEY, key: &str) {
        let key = wide(key);
        unsafe {
            // Deletes the key itself too, with everything in it.
            let _ = RegDeleteTreeW(root, PCWSTR(key.as_ptr()));
        }
    }

    fn delete_value(root: HKEY, key: &str, name: &str) {
        let (key, name) = (wide(key), wide(name));
        unsafe {
            let err = RegDeleteKeyValueW(root, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()));
            if err != ERROR_FILE_NOT_FOUND {
                let _ = err.ok();
            }
        }
    }

    pub fn existing() -> Option<Existing> {
        // A per-user install wins: that is the one the user's own Snapt runs from.
        [false, true].into_iter().find_map(|all_users| {
            let root = root(all_users);
            let dir = read_string(root, UNINSTALL_KEY, "InstallLocation")?;
            let dir = PathBuf::from(dir.trim_matches('"'));
            let version = read_string(root, UNINSTALL_KEY, "DisplayVersion").unwrap_or_default();
            let desktop = desktop(all_users)
                .map(|d| d.join("Snapt.lnk").exists())
                .unwrap_or(false);
            Some(Existing {
                all_users,
                dir,
                version,
                desktop,
            })
        })
    }

    /// Stops every running Snapt; returns whether one was running.
    fn close_snapt() -> bool {
        let closed = Command::new("taskkill")
            .args(["/F", "/IM", "snapt.exe"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|o| o.status.success());
        if closed {
            // Give Windows a moment to release the file.
            thread::sleep(Duration::from_millis(300));
        }
        closed
    }

    /// Writes `data` to `path`, moving a locked old file out of the way first.
    fn replace_file(path: &Path, data: &[u8]) -> Result<(), String> {
        let staged = path.with_extension("exe.new");
        fs::write(&staged, data)
            .map_err(|e| format!("failed to write {}: {e}", staged.display()))?;
        let old = path.with_extension("exe.old");
        for attempt in 0..30 {
            match fs::rename(&staged, path) {
                Ok(()) => {
                    let _ = fs::remove_file(&old);
                    return Ok(());
                }
                // A running file can't be overwritten, but it can be renamed.
                Err(_) if attempt == 10 && path.exists() => {
                    let _ = fs::remove_file(&old);
                    let _ = fs::rename(path, &old);
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
        let _ = fs::remove_file(&staged);
        Err(format!(
            "{} is in use. Close Snapt and try again.",
            path.display()
        ))
    }

    fn shortcut(lnk: &Path, target: &Path) -> Result<(), String> {
        let fail = |e: windows::core::Error| format!("failed to create {}: {e}", lnk.display());
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(fail)?;
            let target_w = wide(&target.display().to_string());
            link.SetPath(PCWSTR(target_w.as_ptr())).map_err(fail)?;
            if let Some(dir) = target.parent() {
                let dir = wide(&dir.display().to_string());
                link.SetWorkingDirectory(PCWSTR(dir.as_ptr()))
                    .map_err(fail)?;
            }
            link.SetDescription(w!("Screenshot tool")).map_err(fail)?;
            let file: IPersistFile = link.cast().map_err(fail)?;
            let lnk_w = wide(&lnk.display().to_string());
            file.Save(PCWSTR(lnk_w.as_ptr()), true).map_err(fail)?;
        }
        Ok(())
    }

    /// Removes the Start menu entries of this installer and of the NSIS one.
    fn remove_start_menu(all_users: bool) {
        if let Ok(programs) = programs(all_users) {
            let _ = fs::remove_file(programs.join("Snapt.lnk"));
            let _ = fs::remove_file(programs.join("Snapt").join("Snapt.lnk"));
            let _ = fs::remove_dir(programs.join("Snapt"));
        }
    }

    pub fn install(plan: &Plan, progress: Progress) -> Result<bool, String> {
        if PAYLOAD.is_empty() {
            return Err("This build of the installer has no Snapt inside.".into());
        }
        progress(0.05, Step::Closing);
        let was_running = close_snapt();

        progress(0.2, Step::Copying);
        let dir = &plan.dir;
        fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        let exe = miniz_oxide::inflate::decompress_to_vec(PAYLOAD)
            .map_err(|e| format!("the installer is damaged: {e:?}"))?;
        progress(0.45, Step::Copying);
        let snapt = dir.join("snapt.exe");
        replace_file(&snapt, &exe)?;
        progress(0.65, Step::Copying);
        // The uninstaller is this program, run with --uninstall.
        let uninstaller = dir.join("uninstall.exe");
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        if me != uninstaller {
            let data = fs::read(&me).map_err(|e| format!("failed to read the installer: {e}"))?;
            replace_file(&uninstaller, &data)?;
        }

        progress(0.8, Step::Shortcuts);
        remove_start_menu(plan.all_users);
        let programs = programs(plan.all_users)?;
        shortcut(&programs.join("Snapt.lnk"), &snapt)?;
        let desktop_lnk = desktop(plan.all_users)?.join("Snapt.lnk");
        if plan.desktop {
            shortcut(&desktop_lnk, &snapt)?;
        } else {
            let _ = fs::remove_file(&desktop_lnk);
        }

        progress(0.92, Step::Registering);
        let root = root(plan.all_users);
        delete_key(root, UNINSTALL_KEY);
        delete_key(HKEY_CURRENT_USER, NSIS_KEY);
        let size = [&snapt, &uninstaller]
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum::<u64>()
            / 1024;
        let quoted = |p: &Path| format!("\"{}\"", p.display());
        write_values(
            root,
            UNINSTALL_KEY,
            &[
                ("DisplayName", Value::Str("Snapt")),
                ("DisplayVersion", Value::Str(VERSION)),
                ("DisplayIcon", Value::Str(&quoted(&snapt))),
                ("Publisher", Value::Str("vkardin")),
                (
                    "URLInfoAbout",
                    Value::Str("https://vkardin.github.io/snapt/"),
                ),
                (
                    "HelpLink",
                    Value::Str("https://github.com/vkardin/snapt/issues"),
                ),
                ("InstallLocation", Value::Str(&dir.display().to_string())),
                (
                    "UninstallString",
                    Value::Str(&format!("{} --uninstall", quoted(&uninstaller))),
                ),
                (
                    "QuietUninstallString",
                    Value::Str(&format!("{} --uninstall --silent", quoted(&uninstaller))),
                ),
                ("EstimatedSize", Value::Dword(size as u32)),
                ("NoModify", Value::Dword(1)),
                ("NoRepair", Value::Dword(1)),
            ],
        )?;
        progress(1.0, Step::Registering);
        Ok(was_running)
    }

    /// Where an uninstaller run from the install folder copies itself, so it can delete that folder.
    pub fn uninstall_copy() -> PathBuf {
        std::env::temp_dir().join("snapt-uninstall.exe")
    }

    pub fn uninstall(dir: &Path, progress: Progress) -> Result<(), String> {
        let all_users = needs_admin(dir);
        progress(0.1, Step::Closing);
        close_snapt();

        progress(0.35, Step::Shortcuts);
        remove_start_menu(all_users);
        if let Ok(desktop) = desktop(all_users) {
            let _ = fs::remove_file(desktop.join("Snapt.lnk"));
        }
        // Snapt's own autostart switch writes this value for the current user.
        if read_string(HKEY_CURRENT_USER, RUN_KEY, "Snapt").is_some_and(|v| {
            v.to_lowercase()
                .contains(&dir.display().to_string().to_lowercase())
        }) {
            delete_value(HKEY_CURRENT_USER, RUN_KEY, "Snapt");
        }

        // Snapt's "Replace system screenshots" option: the Snipping Tool gets
        // PrtSc and the screenshot links back. The link class marks that it was on.
        if read_string(HKEY_CURRENT_USER, SCREENCLIP_KEY, "").is_some() {
            let _ = write_values(
                HKEY_CURRENT_USER,
                r"Control Panel\Keyboard",
                &[("PrintScreenKeyForSnippingEnabled", Value::Dword(1))],
            );
            delete_value(
                HKEY_CURRENT_USER,
                r"Software\RegisteredApplications",
                "Snapt",
            );
            delete_key(HKEY_CURRENT_USER, r"Software\Snapt");
            delete_key(HKEY_CURRENT_USER, SCREENCLIP_KEY);
            unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        }

        progress(0.6, Step::Removing);
        for name in [
            "snapt.exe",
            "snapt.exe.old",
            "snapt.exe.new",
            "uninstall.exe",
        ] {
            let path = dir.join(name);
            for _ in 0..20 {
                if !path.exists() || fs::remove_file(&path).is_ok() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = fs::remove_dir(dir);
        if dir.join("snapt.exe").exists() {
            return Err(format!("{} is in use.", dir.join("snapt.exe").display()));
        }

        progress(0.9, Step::Registering);
        delete_key(root(all_users), UNINSTALL_KEY);
        delete_key(HKEY_CURRENT_USER, NSIS_KEY);
        progress(1.0, Step::Removing);
        Ok(())
    }

    /// Whether changing `dir` takes administrator rights.
    pub fn needs_admin(dir: &Path) -> bool {
        dir.starts_with(
            install_dir(true)
                .parent()
                .unwrap_or(Path::new(r"C:\Program Files")),
        )
    }

    /// Starts `exe` with `args`, asking for administrator rights when `elevate` is set.
    pub fn run(exe: &Path, args: &[String], elevate: bool) -> Result<(), String> {
        if !elevate {
            return Command::new(exe)
                .args(args)
                .spawn()
                .map(drop)
                .map_err(|e| e.to_string());
        }
        let file = wide(&exe.display().to_string());
        let params = wide(
            &args
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(" "),
        );
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.message())
    }

    /// Starts Snapt as the signed-in user, even from an installer running as administrator.
    pub fn launch(dir: &Path) {
        let snapt = dir.join("snapt.exe");
        let result = if is_admin() {
            // Explorer runs without administrator rights and so does what it starts.
            Command::new("explorer.exe").arg(&snapt).spawn()
        } else {
            Command::new(&snapt).spawn()
        };
        if let Err(e) = result {
            eprintln!("failed to start Snapt: {e}");
        }
    }

    pub fn open_url(url: &str) {
        let _ = Command::new("explorer.exe").arg(url).spawn();
    }

    /// Deletes this program's file a moment after it quits.
    pub fn delete_self_later() {
        if let Ok(me) = std::env::current_exe() {
            let _ = Command::new("cmd.exe")
                .raw_arg(format!(
                    "/C ping -n 3 127.0.0.1 >NUL & del /F /Q \"{}\"",
                    me.display()
                ))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
    }
}

/// Lets the window be built and looked at on other systems; nothing is installed.
#[cfg(not(windows))]
mod preview {
    use super::*;
    use std::{thread, time::Duration};

    pub fn install_dir(all_users: bool) -> PathBuf {
        PathBuf::from(if all_users {
            r"C:\Program Files\Snapt"
        } else {
            r"C:\Users\you\AppData\Local\Snapt"
        })
    }

    pub fn is_admin() -> bool {
        false
    }

    pub fn existing() -> Option<Existing> {
        None
    }

    fn pretend(steps: &[(f32, Step)], progress: Progress) {
        for &(at, step) in steps {
            progress(at, step);
            thread::sleep(Duration::from_millis(300));
        }
    }

    pub fn install(plan: &Plan, progress: Progress) -> Result<bool, String> {
        let _ = (PAYLOAD, plan.all_users, plan.desktop);
        pretend(
            &[
                (0.05, Step::Closing),
                (0.45, Step::Copying),
                (0.8, Step::Shortcuts),
                (1.0, Step::Registering),
            ],
            progress,
        );
        Ok(false)
    }

    pub fn uninstall_copy() -> PathBuf {
        std::env::temp_dir().join("snapt-uninstall")
    }

    pub fn uninstall(_dir: &Path, progress: Progress) -> Result<(), String> {
        pretend(&[(0.3, Step::Closing), (1.0, Step::Removing)], progress);
        Ok(())
    }

    pub fn needs_admin(_dir: &Path) -> bool {
        false
    }

    pub fn run(_exe: &Path, _args: &[String], _elevate: bool) -> Result<(), String> {
        Err("the installer only runs on Windows".into())
    }

    pub fn launch(_dir: &Path) {}

    pub fn open_url(url: &str) {
        eprintln!("would open {url}");
    }

    pub fn delete_self_later() {}
}
