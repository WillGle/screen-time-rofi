use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use zbus::blocking::{Connection, Proxy};

use crate::backoff::{FailureLog, next_delay};
use crate::capture::Event;

const BASE_RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);

const LOGIN1: &str = "org.freedesktop.login1";
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

#[derive(Debug, Clone)]
pub struct IdleProbe {
    pub available: bool,
    pub idle: Option<bool>,
    pub error: Option<String>,
}

pub fn probe() -> IdleProbe {
    match probe_inner() {
        Ok(idle) => IdleProbe {
            available: true,
            idle: Some(idle),
            error: None,
        },
        Err(error) => IdleProbe {
            available: false,
            idle: None,
            error: Some(format!("{error:#}")),
        },
    }
}

fn probe_inner() -> Result<bool> {
    let connection = Connection::system().context("cannot connect to the system D-Bus")?;
    let proxy = Proxy::new(&connection, LOGIN1, SESSION_PATH, SESSION_INTERFACE)
        .context("cannot open the login1 session proxy")?;
    proxy
        .get_property("IdleHint")
        .context("cannot read login1 IdleHint")
}

/// Publish the state observed on a new connection. Sending `false` matters:
/// an idle-to-active transition while disconnected must reach the tracker.
fn emit_current_idle(tx: &Sender<Event>, current: bool) {
    let _ = tx.send(Event::Idle(current));
}

pub fn run_listener(tx: Sender<Event>, stop: Arc<AtomicBool>) {
    if std::env::var("SCREEN_TIME_DISABLE_IDLE").as_deref() == Ok("1") {
        return;
    }
    let mut failures = 0u32;
    let mut log = FailureLog::default();
    while !stop.load(Ordering::Relaxed) {
        match listen(&tx, &stop) {
            Ok(()) => {
                failures = 0;
                if log.recovered() {
                    eprintln!("screen-time: idle capture: recovered");
                }
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                let message = format!("{error:#}");
                if log.should_log(&message) {
                    eprintln!("screen-time: idle capture: {message}");
                }
                thread::sleep(next_delay(failures, BASE_RETRY_DELAY, MAX_RETRY_DELAY));
            }
        }
    }
}

fn listen(tx: &Sender<Event>, stop: &Arc<AtomicBool>) -> Result<()> {
    let connection = Connection::system().context("cannot connect to the system D-Bus")?;
    let proxy = Proxy::new(&connection, LOGIN1, SESSION_PATH, SESSION_INTERFACE)
        .context("cannot open the login1 session proxy")?;
    let current: bool = proxy
        .get_property("IdleHint")
        .context("cannot read login1 IdleHint")?;
    emit_current_idle(tx, current);
    let changes = proxy.receive_property_changed::<bool>("IdleHint");
    for change in changes {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match change.get() {
            Ok(idle) => {
                let _ = tx.send(Event::Idle(idle));
            }
            Err(error) => {
                eprintln!("screen-time: cannot decode login1 IdleHint: {error}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_is_non_panicking() {
        let probe = probe();
        assert!(probe.available || probe.error.is_some());
    }

    #[test]
    fn reconnect_resync_emits_both_idle_states() {
        let (tx, rx) = std::sync::mpsc::channel();
        emit_current_idle(&tx, false);
        emit_current_idle(&tx, true);
        assert!(matches!(rx.recv().unwrap(), Event::Idle(false)));
        assert!(matches!(rx.recv().unwrap(), Event::Idle(true)));
    }
}
