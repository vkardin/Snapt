//! The desktop portal (xdg-desktop-portal), which is how Wayland apps take
//! screenshots and get global shortcuts.
//!
//! Each portal call answers through a `Request` object: Snapt subscribes to
//! its `Response` signal, makes the call and waits for the answer, which can
//! take a while when the desktop asks the user for permission first.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use image::RgbaImage;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const APP_ID: &str = "io.github.vkardin.snapt";
const SHORTCUT_ID: &str = "capture";
/// How long to wait for an answer, including the user's permission dialog.
const TIMEOUT: Duration = Duration::from_secs(120);

type Results = HashMap<String, OwnedValue>;

fn connect() -> Result<Connection> {
    let conn = Connection::session().context("no D-Bus session bus")?;
    // Tells portals 1.19+ which app this is, as Snapt runs outside a sandbox.
    // It must be the first portal call on the connection; older portals lack it.
    if let Ok(registry) = Proxy::new(
        &conn,
        DESTINATION,
        PATH,
        "org.freedesktop.host.portal.Registry",
    ) {
        let options: HashMap<&str, Value> = HashMap::new();
        if let Err(err) = registry.call_method("Register", &(APP_ID, options)) {
            log::debug!("portal registry: {err}");
        }
    }
    Ok(conn)
}

/// A token unique within this process, for request and session paths.
fn token() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "snapt{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Calls `method` and waits for its `Response`. `call` gets the handle token
/// to put into the call's options.
fn request(
    conn: &Connection,
    interface: &str,
    call: impl FnOnce(&Proxy, &str) -> zbus::Result<zbus::message::Message>,
) -> Result<Results> {
    let token = token();
    let sender = conn
        .unique_name()
        .context("no D-Bus name")?
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{PATH}/request/{sender}/{token}");
    // Subscribe before the call, so a quick answer is not missed.
    let request = Proxy::new(conn, DESTINATION, path, "org.freedesktop.portal.Request")?;
    let mut responses = request.receive_signal("Response")?;
    let portal = Proxy::new(conn, DESTINATION, PATH, interface)?;
    call(&portal, &token).with_context(|| format!("{interface} call failed"))?;

    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(responses.next());
    });
    let message = receive
        .recv_timeout(TIMEOUT)
        .ok()
        .flatten()
        .context("the portal did not answer")?;
    let (code, results): (u32, Results) = message.body().deserialize()?;
    match code {
        0 => Ok(results),
        1 => bail!("cancelled by the user"),
        _ => bail!("the portal refused (code {code})"),
    }
}

/// Takes a screenshot of the whole desktop, without the portal's own dialog.
pub fn screenshot() -> Result<RgbaImage> {
    let conn = connect()?;
    let results = request(&conn, "org.freedesktop.portal.Screenshot", |p, token| {
        let options = HashMap::from([
            ("handle_token", Value::from(token)),
            ("interactive", Value::from(false)),
        ]);
        p.call_method("Screenshot", &("", options))
    })?;
    let uri = results
        .get("uri")
        .and_then(|v| v.downcast_ref::<String>().ok())
        .context("the portal sent no image")?;
    let path = file_path(&uri).with_context(|| format!("unexpected image location {uri}"))?;
    let image = image::open(&path).with_context(|| format!("failed to read {}", path.display()));
    // The portal saved the file for Snapt alone; don't leave it behind.
    let _ = std::fs::remove_file(&path);
    Ok(image?.to_rgba8())
}

/// `file:///a%20b.png` → `/a b.png`.
fn file_path(uri: &str) -> Option<std::path::PathBuf> {
    let path = uri.strip_prefix("file://")?;
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = path.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(std::ffi::OsString::from_vec(out).into())
}

/// A capture shortcut registered with the GlobalShortcuts portal. Dropping it
/// closes the portal session, which unbinds the shortcut.
pub struct Shortcut {
    conn: Connection,
    session: OwnedObjectPath,
    alive: Arc<AtomicBool>,
    pub trigger: String,
}

impl Shortcut {
    /// Asks the desktop for a shortcut that takes a screenshot, suggesting
    /// `hotkey` (as Snapt writes it, e.g. `Ctrl+Shift+S`). The desktop may
    /// ask the user to confirm or pick another key, and keeps the user's
    /// choice in its own keyboard settings.
    /// `pressed` runs on each press until it returns false.
    pub fn bind(
        hotkey: &str,
        description: &str,
        pressed: impl Fn() -> bool + Send + 'static,
    ) -> Result<Self> {
        let conn = connect()?;
        let interface = "org.freedesktop.portal.GlobalShortcuts";
        let portal = Proxy::new(&conn, DESTINATION, PATH, interface)?;
        let version: u32 = portal
            .get_property("version")
            .context("this desktop has no global shortcuts portal")?;
        log::debug!("GlobalShortcuts portal version {version}");

        let session_token = token();
        let results = request(&conn, interface, |p, token| {
            let options = HashMap::from([
                ("handle_token", Value::from(token)),
                ("session_handle_token", Value::from(session_token.as_str())),
            ]);
            p.call_method("CreateSession", &(options,))
        })?;
        let session = results
            .get("session_handle")
            .and_then(|v| v.downcast_ref::<String>().ok())
            .context("the portal sent no session")?;
        let session = OwnedObjectPath::try_from(session)?;

        // Listen before binding, so the first press is not missed.
        let mut activations = portal.receive_signal("Activated")?;
        let trigger = xdg_trigger(hotkey);
        let results = request(&conn, interface, |p, token| {
            let shortcut = HashMap::from([
                ("description", Value::from(description)),
                ("preferred_trigger", Value::from(trigger.as_str())),
            ]);
            let options = HashMap::from([("handle_token", Value::from(token))]);
            p.call_method(
                "BindShortcuts",
                &(
                    ObjectPath::from(&session),
                    vec![(SHORTCUT_ID, shortcut)],
                    "",
                    options,
                ),
            )
        })?;
        let bound = results
            .get("shortcuts")
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<(String, Results)>::try_from(v).ok())
            .unwrap_or_default();
        ensure!(
            bound.iter().any(|(id, _)| id == SHORTCUT_ID),
            "the shortcut was not bound"
        );
        let key = bound
            .iter()
            .find_map(|(_, info)| info.get("trigger_description"))
            .and_then(|v| v.downcast_ref::<String>().ok())
            .unwrap_or_default();
        log::info!("capture shortcut bound through the portal: {key}");

        let alive = Arc::new(AtomicBool::new(true));
        let listening = alive.clone();
        let own = session.clone();
        std::thread::spawn(move || {
            for message in &mut activations {
                if !listening.load(Ordering::Relaxed) {
                    break;
                }
                let Ok((session, id, _, _)) = message
                    .body()
                    .deserialize::<(OwnedObjectPath, String, u64, Results)>()
                else {
                    continue;
                };
                if session == own && id == SHORTCUT_ID && !pressed() {
                    break;
                }
            }
        });
        Ok(Self {
            conn,
            session,
            alive,
            trigger: hotkey.to_string(),
        })
    }
}

impl std::fmt::Debug for Shortcut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shortcut")
            .field("session", &self.session)
            .field("trigger", &self.trigger)
            .finish()
    }
}

impl Drop for Shortcut {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        if let Ok(session) = Proxy::new(
            &self.conn,
            DESTINATION,
            self.session.as_ref(),
            "org.freedesktop.portal.Session",
        ) {
            let _ = session.call_method("Close", &());
        }
    }
}

/// `Ctrl+Shift+KeyS` → `CTRL+SHIFT+s`, the format of the XDG shortcuts spec.
fn xdg_trigger(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(str::trim)
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "CTRL".to_string(),
            "alt" | "option" => "ALT".to_string(),
            "shift" => "SHIFT".to_string(),
            "super" | "cmd" | "command" | "meta" => "LOGO".to_string(),
            "printscreen" | "prtsc" | "print" => "Print".to_string(),
            key => {
                let key = key
                    .strip_prefix("key")
                    .or_else(|| key.strip_prefix("digit"))
                    .unwrap_or(key);
                if key.len() > 1 && key.starts_with('f') {
                    key.to_ascii_uppercase()
                } else if key.len() > 1 {
                    // Named keys keep their keysym spelling, e.g. `Insert`.
                    part.to_string()
                } else {
                    key.to_string()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_become_xdg_triggers() {
        assert_eq!(xdg_trigger("PrintScreen"), "Print");
        assert_eq!(xdg_trigger("Ctrl+Shift+S"), "CTRL+SHIFT+s");
        assert_eq!(xdg_trigger("Ctrl+Shift+KeyS"), "CTRL+SHIFT+s");
        assert_eq!(xdg_trigger("Super+Digit9"), "LOGO+9");
        assert_eq!(xdg_trigger("Alt+F9"), "ALT+F9");
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_path("file:///home/u/Pictures/Screenshot%20from%202026.png").unwrap(),
            std::path::PathBuf::from("/home/u/Pictures/Screenshot from 2026.png")
        );
        assert_eq!(
            file_path("file:///tmp/%D0%BA.png").unwrap(),
            std::path::PathBuf::from("/tmp/к.png")
        );
        assert!(file_path("https://example.com/a.png").is_none());
    }

    // These need a portal: see tests/portal-mock.py.
    #[test]
    #[ignore]
    fn portal_screenshot() {
        let image = screenshot().unwrap();
        assert_eq!(image.dimensions(), (64, 32));
    }

    #[test]
    #[ignore]
    fn portal_shortcut() {
        let (send, receive) = mpsc::channel();
        let shortcut = Shortcut::bind("Ctrl+Shift+S", "Take a screenshot", move || {
            send.send(()).is_ok()
        })
        .unwrap();
        assert_eq!(shortcut.trigger, "Ctrl+Shift+S");
        for _ in 0..2 {
            receive.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
}
