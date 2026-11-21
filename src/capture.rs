use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::sessions;

#[derive(Debug, Clone)]
pub enum Event {
    Focus(Option<String>),
    Idle(bool),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct HyprlandPaths {
    pub signature: String,
    pub event_socket: PathBuf,
    pub command_socket: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ActiveWindowReply {
    #[serde(default)]
    class: String,
}

pub fn discover() -> Option<HyprlandPaths> {
    if let Some(dir) = env::var_os("SCREEN_TIME_HYPRLAND_DIR") {
        let dir = PathBuf::from(dir);
        return paths_for_dir(&dir);
    }

    let mut bases = Vec::new();
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
        bases.push(PathBuf::from(runtime).join("hypr"));
    }
    bases.push(PathBuf::from("/tmp/hypr"));

    if let Ok(signature) = env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        for base in &bases {
            if let Some(paths) = paths_for_dir(&base.join(&signature)) {
                return Some(paths);
            }
        }
    }

    let mut candidates: Vec<(SystemTime, HyprlandPaths)> = Vec::new();
    for base in &bases {
        let Ok(entries) = fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Some(paths) = paths_for_dir(&dir) else {
                continue;
            };
            let modified = fs::metadata(&paths.event_socket)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            candidates.push((modified, paths));
        }
    }
    candidates.sort_by_key(|(modified, _)| *modified);
    candidates.pop().map(|(_, paths)| paths)
}

fn paths_for_dir(dir: &Path) -> Option<HyprlandPaths> {
    let event_socket = dir.join(".socket2.sock");
    if !event_socket.exists() {
        return None;
    }
    let command_socket = dir.join(".socket.sock");
    let signature = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_owned();
    Some(HyprlandPaths {
        signature,
        event_socket,
        command_socket,
    })
}

/// Parse one line of the Hyprland event socket. `None` means the line is not a
/// focus event; `Some(None)` means focus is now unknown/ignored.
pub fn parse_event_line(line: &str) -> Option<Option<String>> {
    let payload = line.strip_prefix("activewindow>>")?;
    let (class, _title) = payload.split_once(',').unwrap_or((payload, ""));
    Some(sessions::normalize_app(class))
}

pub fn query_active_window(command_socket: &Path) -> Result<Option<String>> {
    let mut stream = UnixStream::connect(command_socket)
        .with_context(|| format!("cannot connect to {}", command_socket.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .context("cannot set Hyprland command timeout")?;
    stream
        .write_all(b"j/activewindow")
        .context("cannot send Hyprland activewindow query")?;
    stream.flush()?;
    let mut response = String::new();
    let mut reader = BufReader::new(stream);
    reader
        .read_line(&mut response)
        .context("cannot read Hyprland activewindow reply")?;
    if response.trim().is_empty() {
        return Ok(None);
    }
    if let Ok(reply) = serde_json::from_str::<ActiveWindowReply>(&response) {
        return Ok(sessions::normalize_app(&reply.class));
    }
    Ok(None)
}

pub fn run_listener(tx: Sender<Event>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        let Some(paths) = discover() else {
            thread::sleep(Duration::from_secs(2));
            continue;
        };
        match connect_and_read(&paths, &tx, &stop) {
            Ok(()) => {}
            Err(error) => {
                eprintln!("screen-time: Hyprland capture: {error:#}");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn connect_and_read(
    paths: &HyprlandPaths,
    tx: &Sender<Event>,
    stop: &Arc<AtomicBool>,
) -> Result<()> {
    let stream = UnixStream::connect(&paths.event_socket)
        .with_context(|| format!("cannot connect to {}", paths.event_socket.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .context("cannot set Hyprland event timeout")?;
    if let Ok(current) = query_active_window(&paths.command_socket) {
        let _ = tx.send(Event::Focus(current));
    }

    let mut reader = BufReader::new(stream);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                if let Some(focus) = parse_event_line(line.trim_end_matches(['\r', '\n'])) {
                    let _ = tx.send(Event::Focus(focus));
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

    #[test]
    fn parses_focus_and_ignores_titles() {
        assert_eq!(
            parse_event_line("activewindow>>code,main.rs — screen-time"),
            Some(Some("Code".to_owned()))
        );
        assert_eq!(parse_event_line("activewindow>>rofi,rofi"), Some(None));
        assert_eq!(parse_event_line("activewindow>>,desktop"), Some(None));
        assert_eq!(parse_event_line("workspace>>2"), None);
        assert_eq!(parse_event_line("activewindowv2>>abc"), None);
    }

    #[test]
    fn discovers_socket_directory() {
        let dir = tempdir().unwrap();
        let sig = dir.path().join("sig");
        fs::create_dir(&sig).unwrap();
        fs::write(sig.join(".socket2.sock"), b"").unwrap();
        fs::write(sig.join(".socket.sock"), b"").unwrap();
        let paths = paths_for_dir(&sig).unwrap();
        assert_eq!(paths.signature, "sig");
        assert_eq!(paths.event_socket, sig.join(".socket2.sock"));
        assert_eq!(paths.command_socket, sig.join(".socket.sock"));
    }

    #[test]
    fn queries_active_window_over_command_socket() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join(".socket.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 64];
            use std::io::Read;
            let read = stream.read(&mut request).unwrap();
            assert_eq!(&request[..read], b"j/activewindow");
            stream
                .write_all(br#"{"class":"firefox","title":"secret title"}"#)
                .unwrap();
        });
        let class = query_active_window(&socket).unwrap();
        assert_eq!(class.as_deref(), Some("Firefox"));
        handle.join().unwrap();
    }
}
