//! Taking over from the Windows screenshot tool (the Snipping Tool).
//!
//! Windows reaches the Snipping Tool three ways, and each is handled the
//! way Windows allows:
//!
//! - PrtSc: a per-user switch in the registry turns the Snipping Tool's use
//!   of the key off. It only takes effect after signing in again, so while
//!   Snapt runs the key is also caught by the keyboard hook below.
//! - Win+Shift+S: a shell shortcut that can't be reassigned. A low-level
//!   keyboard hook catches it before the shell does. Nothing in the system is
//!   changed, so the shortcut is the Snipping Tool's again as soon as the
//!   option is turned off or Snapt quits.
//! - `ms-screenclip:` and `ms-screensketch:` links, opened by the Snipping
//!   Tool's own shortcuts and by other apps. Snapt registers itself as an app
//!   that can open them, but only the user can make it the default: Windows
//!   protects that choice, so Snapt opens the page where it is made.
//!
//! Turning the option off undoes all of it, and the Snipping Tool itself is
//! never changed.

use std::{
    ffi::c_void,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
                REG_VALUE_TYPE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
                RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
            },
            Threading::GetCurrentThreadId,
        },
        UI::{
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
                KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
                VK_SHIFT, VK_SNAPSHOT,
            },
            Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify},
            WindowsAndMessaging::{
                CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
                PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
                WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    },
    core::PCWSTR,
};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

/// A system screenshot key that Snapt caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKey {
    PrintScreen,
    /// Win+Shift+S, the Snipping Tool's area capture.
    WinShiftS,
}

const KEYBOARD_KEY: &str = r"Control Panel\Keyboard";
const PRTSC_VALUE: &str = "PrintScreenKeyForSnippingEnabled";
/// The links the Snipping Tool opens, and the name Snapt opens them under.
const LINKS: [&str; 2] = ["ms-screenclip", "ms-screensketch"];
const PROG_ID: &str = "Snapt.ScreenClip";
const CLASSES_KEY: &str = r"Software\Classes\Snapt.ScreenClip";
const APP_KEY: &str = r"Software\Snapt";
const CAPABILITIES_KEY: &str = r"Software\Snapt\Capabilities";
const REGISTERED_KEY: &str = r"Software\RegisteredApplications";
/// The name under `RegisteredApplications`, which the Settings page link uses.
const APP_NAME: &str = "Snapt";
const CHOICE_KEY: &str = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations";

/// Hands PrtSc and the screenshot links to Snapt (`on`) or back to the Snipping Tool.
pub fn apply(on: bool) -> Result<()> {
    set_dword(HKEY_CURRENT_USER, KEYBOARD_KEY, PRTSC_VALUE, u32::from(!on))?;
    if on {
        register_links()?;
    } else {
        unregister_links();
    }
    // Tells Explorer and the Settings app that the list of link handlers changed.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    Ok(())
}

/// Whether PrtSc and both links go to Snapt, i.e. nothing is left to ask about.
pub fn in_place() -> bool {
    read_dword(HKEY_CURRENT_USER, KEYBOARD_KEY, PRTSC_VALUE) == Some(0)
        && read_string(HKEY_CURRENT_USER, CLASSES_KEY, "").is_some()
        && links_are_snapt()
}

/// Whether the user made Snapt the default for both screenshot links.
pub fn links_are_snapt() -> bool {
    LINKS.iter().all(|link| {
        let key = format!(r"{CHOICE_KEY}\{link}\UserChoice");
        read_string(HKEY_CURRENT_USER, &key, "ProgId").as_deref() == Some(PROG_ID)
    })
}

/// Opens the Windows page where the user picks Snapt for the screenshot links.
/// Windows 11 opens Snapt's own page; older versions open the default apps list.
pub fn open_link_choice() {
    let url = format!("ms-settings:defaultapps?registeredAppUser={APP_NAME}");
    if let Err(err) = open::that_detached(url) {
        log::warn!("failed to open the default apps page: {err}");
    }
}

/// Opens the page where the Snipping Tool gets the links back, for when Snapt
/// had them and the option was turned off.
pub fn open_snipping_tool_choice() {
    let url = "ms-settings:defaultapps?registeredAUMID=Microsoft.ScreenSketch_8wekyb3d8bbwe!App";
    if let Err(err) = open::that_detached(url) {
        log::warn!("failed to open the default apps page: {err}");
    }
}

fn register_links() -> Result<()> {
    let exe = crate::settings::launch_path()?;
    let exe = exe.display();
    let command = format!("\"{exe}\" --capture \"%1\"");
    let icon = format!("\"{exe}\",0");
    write_strings(
        CLASSES_KEY,
        &[("", "URL:Snapt screenshot"), ("URL Protocol", "")],
    )?;
    write_strings(&format!(r"{CLASSES_KEY}\DefaultIcon"), &[("", &icon)])?;
    write_strings(
        &format!(r"{CLASSES_KEY}\Application"),
        &[("ApplicationName", "Snapt"), ("ApplicationIcon", &icon)],
    )?;
    write_strings(
        &format!(r"{CLASSES_KEY}\shell\open\command"),
        &[("", &command)],
    )?;
    write_strings(
        CAPABILITIES_KEY,
        &[
            ("ApplicationName", "Snapt"),
            (
                "ApplicationDescription",
                "A fast, lightweight screenshot tool.",
            ),
        ],
    )?;
    let links: Vec<(&str, &str)> = LINKS.iter().map(|l| (*l, PROG_ID)).collect();
    write_strings(&format!(r"{CAPABILITIES_KEY}\URLAssociations"), &links)?;
    write_strings(REGISTERED_KEY, &[(APP_NAME, CAPABILITIES_KEY)])?;
    Ok(())
}

fn unregister_links() {
    delete_value(HKEY_CURRENT_USER, REGISTERED_KEY, APP_NAME);
    delete_tree(HKEY_CURRENT_USER, APP_KEY);
    delete_tree(HKEY_CURRENT_USER, CLASSES_KEY);
}

// The keyboard hook. It runs on its own thread, which only pumps the
// messages the hook needs, so a busy Snapt never delays the keyboard.

/// The thread running the hook, 0 when none.
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static HOOK_PROXY: Mutex<Option<EventLoopProxy<UserEvent>>> = Mutex::new(None);
/// Set while a shortcut field records keys, so PrtSc reaches it.
static PAUSED: AtomicBool = AtomicBool::new(false);
/// The key whose press was caught, so its repeats and release are caught too.
static CAUGHT: AtomicU32 = AtomicU32::new(0);
/// An unassigned key: tapping it while Win is held keeps the Start menu from
/// opening when Win is let go, since Win was used for a shortcut.
const MASK_KEY: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// Starts catching the system screenshot keys and sending them to `proxy`.
pub fn start_hook(proxy: EventLoopProxy<UserEvent>) {
    *HOOK_PROXY.lock().unwrap_or_else(|e| e.into_inner()) = Some(proxy);
    if HOOK_THREAD.load(Ordering::SeqCst) != 0 {
        return;
    }
    let (ready, wait) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("snapt-keys".into())
        .spawn(move || {
            // SAFETY: the hook procedure is a plain function that lives as long as the
            // program, and the hook is removed on this same thread.
            let hook = unsafe {
                let module = GetModuleHandleW(PCWSTR::null()).ok();
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module.map(Into::into), 0)
            };
            let hook = match hook {
                Ok(hook) => hook,
                Err(err) => {
                    log::error!("failed to catch the screenshot keys: {err}");
                    let _ = ready.send(0);
                    return;
                }
            };
            let _ = ready.send(unsafe { GetCurrentThreadId() });
            let mut msg = MSG::default();
            // SAFETY: a plain message loop on this thread; WM_QUIT ends it.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
            let _ = unsafe { UnhookWindowsHookEx(hook) };
            log::info!("stopped catching the screenshot keys");
        });
    if spawned.is_err() {
        log::error!("failed to start the keyboard thread");
        return;
    }
    if let Ok(id) = wait.recv()
        && id != 0
    {
        HOOK_THREAD.store(id, Ordering::SeqCst);
        log::info!("catching PrtSc and Win+Shift+S");
    }
}

/// Gives the keys back to the system.
pub fn stop_hook() {
    let id = HOOK_THREAD.swap(0, Ordering::SeqCst);
    if id != 0 {
        // SAFETY: posts WM_QUIT to the hook thread's own message loop.
        let _ = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

/// Lets every key through while `paused`, e.g. while a shortcut is being recorded.
pub fn pause_hook(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

fn held(key: VIRTUAL_KEY) -> bool {
    // SAFETY: reads the key state; no pointers involved.
    unsafe { GetAsyncKeyState(i32::from(key.0)) as u16 & 0x8000 != 0 }
}

/// The screenshot key that `vk` presses with the modifiers held right now.
fn system_key(vk: u32) -> Option<SystemKey> {
    let win = held(VK_LWIN) || held(VK_RWIN);
    let (shift, ctrl, alt) = (held(VK_SHIFT), held(VK_CONTROL), held(VK_MENU));
    if vk == u32::from(VK_SNAPSHOT.0) && !win && !shift && !ctrl && !alt {
        Some(SystemKey::PrintScreen)
    } else if vk == u32::from(b'S') && win && shift && !ctrl && !alt {
        Some(SystemKey::WinShiftS)
    } else {
        None
    }
}

fn tap_mask_key() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: MASK_KEY,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: the inputs are fully initialized and the size is that of INPUT.
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && !PAUSED.load(Ordering::Relaxed) {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;
        if !info.flags.contains(LLKHF_INJECTED) {
            let vk = info.vkCode;
            if CAUGHT.load(Ordering::Relaxed) == vk && (down || up) {
                if up {
                    CAUGHT.store(0, Ordering::Relaxed);
                }
                return LRESULT(1);
            }
            if down && let Some(key) = system_key(vk) {
                CAUGHT.store(vk, Ordering::Relaxed);
                if key == SystemKey::WinShiftS {
                    tap_mask_key();
                }
                if let Some(proxy) = HOOK_PROXY.lock().ok().and_then(|p| p.clone()) {
                    let _ = proxy.send_event(UserEvent::SystemKey(key));
                }
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

// Registry helpers, all under HKEY_CURRENT_USER: nothing here needs admin rights.

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn read_dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
    let (key, name) = (wide(key), wide(name));
    let mut value = 0u32;
    let mut len = 4u32;
    // SAFETY: the buffer is a u32 and len says so.
    let err = unsafe {
        RegGetValueW(
            root,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast::<c_void>()),
            Some(&mut len),
        )
    };
    err.ok().ok().map(|()| value)
}

fn read_string(root: HKEY, key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(key), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: len is the buffer's size in bytes.
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

fn write_value(root: HKEY, key: &str, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> Result<()> {
    let (key_w, name) = (wide(key), wide(name));
    let mut hkey = HKEY::default();
    // SAFETY: the key handle is closed below; the data slice outlives the call.
    unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(key_w.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
        .ok()
        .with_context(|| format!("failed to open {key}"))?;
        let result = RegSetValueExW(hkey, PCWSTR(name.as_ptr()), None, kind, Some(data)).ok();
        let _ = RegCloseKey(hkey);
        result.with_context(|| format!("failed to write to {key}"))
    }
}

fn set_dword(root: HKEY, key: &str, name: &str, value: u32) -> Result<()> {
    write_value(root, key, name, REG_DWORD, &value.to_le_bytes())
}

fn write_strings(key: &str, values: &[(&str, &str)]) -> Result<()> {
    for (name, value) in values {
        let data: Vec<u8> = wide(value).iter().flat_map(|c| c.to_le_bytes()).collect();
        write_value(HKEY_CURRENT_USER, key, name, REG_SZ, &data)?;
    }
    Ok(())
}

fn delete_tree(root: HKEY, key: &str) {
    let key = wide(key);
    // SAFETY: deletes only the named key under root.
    unsafe {
        let _ = RegDeleteTreeW(root, PCWSTR(key.as_ptr()));
    }
}

fn delete_value(root: HKEY, key: &str, name: &str) {
    let (key, name) = (wide(key), wide(name));
    // SAFETY: deletes only the named value.
    let err = unsafe { RegDeleteKeyValueW(root, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr())) };
    if err != ERROR_FILE_NOT_FOUND {
        let _ = err.ok();
    }
}
