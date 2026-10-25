//! Keeps Snapt to one running copy.
//!
//! The first copy listens on a localhost port and records it in the config
//! folder. Starting Snapt again, e.g. `snapt --capture` from a desktop shortcut
//! on Wayland where global hotkeys are not available, asks that copy to take a
//! screenshot and exits.

use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    time::Duration,
};

use winit::event_loop::EventLoopProxy;

use crate::{app::UserEvent, settings::Settings};

const REQUEST: &[u8] = b"snapt capture\n";
const REPLY: &str = "snapt ok";

fn port_file() -> Option<PathBuf> {
    Some(Settings::path()?.with_file_name("instance.port"))
}

/// Asks a running copy to take a screenshot. Returns false when none answers.
pub fn forward() -> bool {
    let Some(port) = port_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<u16>().ok())
    else {
        return false;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
    if stream.write_all(REQUEST).is_err() {
        return false;
    }
    // Another program may have taken the port since; only Snapt answers like this.
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == REPLY
}

/// Listens for later copies of Snapt in a background thread.
pub fn listen(proxy: EventLoopProxy<UserEvent>) {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
        Ok(listener) => listener,
        Err(err) => {
            log::warn!("single-instance listener unavailable: {err}");
            return;
        }
    };
    let (Some(path), Ok(addr)) = (port_file(), listener.local_addr()) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(err) = std::fs::write(&path, addr.port().to_string()) {
        log::warn!("failed to record the instance port: {err}");
        return;
    }
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
            let mut line = String::new();
            let mut reader = BufReader::new(&stream);
            if reader.read_line(&mut line).is_ok() && line.as_bytes() == REQUEST {
                let _ = (&stream).write_all(format!("{REPLY}\n").as_bytes());
                if proxy.send_event(UserEvent::Capture).is_err() {
                    break;
                }
            }
        }
    });
}
