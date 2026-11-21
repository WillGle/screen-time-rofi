use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::capture;
use crate::db::Database;
use crate::idle;
use crate::paths::{self, Paths};
use crate::snapshot;
use crate::timeutil;

pub fn run() -> Result<()> {
    let paths = Paths::resolve()?;
    let mut report = Report::default();
    report.section("screen-time doctor");
    report.ok(format!("version {}", env!("CARGO_PKG_VERSION")));
    report.ok(format!("state {}", paths.state_dir.display()));
    report.ok(format!("runtime {}", paths.runtime_dir.display()));

    if paths.state_dir.is_dir() {
        report.ok("state directory exists");
    } else {
        report.warn("state directory not created yet; start the daemon once");
    }
    if paths.runtime_dir.is_dir() {
        report.ok("runtime directory exists");
    } else {
        report.warn("runtime directory missing; live snapshot is unavailable");
    }

    match Database::open_read(&paths.db_path) {
        Ok(db) => {
            report.ok(format!("database {}", paths.db_path.display()));
            match db.integrity_check() {
                Ok(result) if result.eq_ignore_ascii_case("ok") => {
                    report.ok("database quick_check ok");
                }
                Ok(result) => report.fail(format!("database quick_check: {result}")),
                Err(error) => report.fail(format!("database quick_check: {error:#}")),
            }
            match db.journal_mode() {
                Ok(mode) if mode.eq_ignore_ascii_case("wal") => report.ok("journal_mode=wal"),
                Ok(mode) => report.warn(format!("journal_mode={mode} (expected wal)")),
                Err(error) => report.warn(format!("cannot read journal_mode: {error:#}")),
            }
            if let Ok((events, sessions)) = db.counts() {
                report.ok(format!("{events} raw events, {sessions} sessions"));
            }
            if let Some(commit) = db.last_commit_at().ok().flatten() {
                report.ok(format!("last batch commit {commit}"));
            }
        }
        Err(error) => {
            if paths.db_exists() {
                report.fail(format!("cannot open database: {error:#}"));
            } else {
                report.warn("database not created yet; start the daemon once");
            }
        }
    }

    match snapshot::read(&paths.live_path) {
        Ok(Some(snapshot)) => {
            let now_ts = timeutil::now().timestamp();
            let boot_id = paths::boot_id();
            let age = snapshot
                .as_of_local()
                .map(|as_of| now_ts - as_of.timestamp())
                .unwrap_or(i64::MAX);
            report.ok(format!(
                "live snapshot {} as_of={} age={}s",
                snapshot.date, snapshot.as_of, age
            ));
            if snapshot.boot_id != boot_id {
                report.warn(format!(
                    "live snapshot boot_id {} differs from current {}",
                    snapshot.boot_id, boot_id
                ));
            }
            if !paths::pid_alive(snapshot.pid) {
                report.warn(format!("live snapshot pid {} is not running", snapshot.pid));
            }
            if snapshot.is_stale(now_ts, &boot_id) {
                report.warn("live snapshot is stale (open intervals are not extended)");
            } else {
                report.ok("live snapshot is fresh");
            }
        }
        Ok(None) => report.warn("live snapshot missing; daemon is stopped or not ready"),
        Err(error) => report.fail(format!("live snapshot unreadable: {error:#}")),
    }

    match capture::discover() {
        Some(hypr) => {
            report.ok(format!(
                "Hyprland {} ({})",
                hypr.signature,
                hypr.event_socket.display()
            ));
            match capture::query_active_window(&hypr.command_socket) {
                Ok(Some(app)) => report.ok(format!("active app {app}")),
                Ok(None) => report.warn("Hyprland reports no active app window"),
                Err(error) => report.warn(format!("active-window query failed: {error:#}")),
            }
        }
        None => report.warn("Hyprland event socket not found; tracking is inactive"),
    }

    let idle_probe = idle::probe();
    if idle_probe.available {
        report.ok(format!(
            "logind IdleHint available (idle={})",
            idle_probe
                .idle
                .map(|idle| if idle { "yes" } else { "no" })
                .unwrap_or("unknown")
        ));
    } else {
        report.warn(format!(
            "logind IdleHint unavailable: {}",
            idle_probe
                .error
                .unwrap_or_else(|| "unknown error".to_owned())
        ));
    }

    match Command::new("rofi").arg("-version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            report.ok(format!("rofi {version}"));
        }
        Ok(_) => report.warn("rofi is installed but `rofi -version` failed"),
        Err(_) => report.warn("rofi is not installed or not in PATH"),
    }

    match Command::new("systemctl")
        .args(["--user", "is-active", "screen-time.service"])
        .output()
    {
        Ok(output) => {
            let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if state == "active" {
                report.ok("systemd user service active");
            } else {
                report.warn(format!(
                    "systemd user service is {} (journalctl --user -u screen-time)",
                    if state.is_empty() { "unknown" } else { &state }
                ));
            }
        }
        Err(error) => report.warn(format!("systemctl --user unavailable: {error}")),
    }

    check_writable(&paths.runtime_dir, &mut report);
    report.finish()
}

fn check_writable(path: &Path, report: &mut Report) {
    if !path.is_dir() {
        return;
    }
    let probe = path.join(format!(".doctor-{}", std::process::id()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            report.ok("runtime directory is writable");
        }
        Err(error) => report.fail(format!("runtime directory is not writable: {error}")),
    }
}

#[derive(Default)]
struct Report {
    failures: usize,
    warnings: usize,
}

impl Report {
    fn section(&self, title: &str) {
        println!("{title}");
    }

    fn ok(&self, message: impl AsRef<str>) {
        println!("  ok    {}", message.as_ref());
    }

    fn warn(&mut self, message: impl AsRef<str>) {
        self.warnings += 1;
        println!("  warn  {}", message.as_ref());
    }

    fn fail(&mut self, message: impl AsRef<str>) {
        self.failures += 1;
        println!("  fail  {}", message.as_ref());
    }

    fn finish(&self) -> Result<()> {
        println!(
            "\n{} failure(s), {} warning(s)",
            self.failures, self.warnings
        );
        if self.failures > 0 {
            anyhow::bail!("doctor found {} problem(s)", self.failures);
        }
        Ok(())
    }
}
