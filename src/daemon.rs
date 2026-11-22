use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, NaiveDate};

use crate::capture::{self, Event};
use crate::config::Config;
use crate::db::{Database, EventRecord, RetentionPolicy};
use crate::idle;
use crate::paths::{self, Paths};
use crate::sessions::{self, Session};
use crate::snapshot::{self, LiveSnapshot, OpenInterval, SnapshotApp};
use crate::timeutil;
use crate::ui;

const BATCH_INTERVAL_SECONDS: i64 = 60;
const BATCH_EVENT_LIMIT: usize = 100;
const HEARTBEAT_SECONDS: i64 = 60;
const MAX_PENDING_EVENTS: usize = 10_000;
const MAX_PENDING_SESSIONS: usize = 1_000;

/// Held for the whole daemon run; flock releases automatically on exit or
/// crash, so no stale-lock cleanup is needed.
#[derive(Debug)]
struct DaemonLock {
    _file: File,
}

fn acquire_daemon_lock(paths: &Paths) -> Result<DaemonLock> {
    let path = paths.state_dir.join("daemon.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    // SAFETY: the File keeps a valid descriptor alive in the guard.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            bail!(
                "another daemon is already running (lock {}); stop the systemd service or the other process first",
                path.display()
            );
        }
        return Err(error).with_context(|| format!("cannot lock {}", path.display()));
    }
    Ok(DaemonLock { _file: file })
}

pub fn run() -> Result<()> {
    let paths = Paths::resolve()?;
    paths.ensure()?;
    let _daemon_lock = acquire_daemon_lock(&paths)?;
    let mut db = Database::open_write(&paths.db_path, Some(&paths.migration_backups_dir))?;
    let boot_id = paths::boot_id();
    let now = timeutil::now();
    let now_ts = now.timestamp();
    let today = now.date_naive();

    let (config, config_error) = Config::load_default_lenient();
    if let Some(error) = config_error {
        eprintln!("screen-time: warning: {error:#}; using keep-all retention");
    }
    let retention = RetentionPolicy::from_config(&config.retention);
    if retention.is_enabled() {
        println!("screen-time: retention {}", retention.summary());
    } else {
        eprintln!("screen-time: retention disabled (keep all history)");
    }

    recover_previous_power(&mut db, &boot_id, now_ts)?;
    let power_id = db.start_power_session(now_ts, &boot_id)?;
    db.set_meta("boot_id", &boot_id)?;
    db.set_meta("daemon_pid", &std::process::id().to_string())?;
    db.set_meta("daemon_started_at", &timeutil::rfc3339(now_ts))?;
    db.set_meta("clean_shutdown", "0")?;
    match db.retention(&retention, today, now_ts) {
        Ok(result) if retention.is_enabled() => eprintln!(
            "screen-time: retention removed {} events, {} sessions, {} daily-app rows, {} daily-slot rows, {} power sessions",
            result.events,
            result.sessions,
            result.daily_app,
            result.daily_slots,
            result.power_sessions
        ),
        Ok(_) => {}
        Err(error) => eprintln!("screen-time: retention failed: {error:#}"),
    }
    db.commit_batch(
        &[EventRecord {
            ts: now_ts,
            kind: "boot".to_owned(),
            app: None,
            state: Some("begin".to_owned()),
            boot_id: boot_id.clone(),
        }],
        &[],
        now_ts,
    )?;
    db.set_meta("last_commit_at", &timeutil::rfc3339(now_ts))?;

    let day = db.load_day_closed(today, now_ts)?;
    let mut tracker = Tracker::new(today, day, now_ts, boot_id.clone(), now_ts, retention);
    tracker.write_snapshot(&paths, now_ts);

    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    {
        let tx = tx.clone();
        let stop = stop.clone();
        thread::spawn(move || capture::run_listener(tx, stop));
    }
    {
        let tx = tx.clone();
        let stop = stop.clone();
        thread::spawn(move || idle::run_listener(tx, stop));
    }
    {
        let tx = tx.clone();
        thread::spawn(move || run_signal_listener(tx));
    }

    loop {
        let now = timeutil::now();
        let now_ts = now.timestamp();
        let deadline = tracker.next_deadline(now_ts);
        let timeout = (deadline - now_ts).max(1) as u64;
        match rx.recv_timeout(Duration::from_secs(timeout)) {
            Ok(Event::Shutdown) => break,
            Ok(Event::Focus(app)) => {
                let now_ts = timeutil::now().timestamp();
                tracker.ensure_day(&paths, &mut db, now_ts)?;
                let changed = tracker.on_focus(app, now_ts);
                tracker.after_event(&paths, &mut db, now_ts, changed)?;
            }
            Ok(Event::Idle(idle)) => {
                let now_ts = timeutil::now().timestamp();
                tracker.ensure_day(&paths, &mut db, now_ts)?;
                let changed = tracker.on_idle(idle, now_ts);
                tracker.after_event(&paths, &mut db, now_ts, changed)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                let now_ts = timeutil::now().timestamp();
                tracker.tick(&paths, &mut db, now_ts)?;
            }
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!("screen-time: event channel disconnected; shutting down");
                break;
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    let now = timeutil::now();
    tracker.shutdown(&paths, &mut db, power_id, now)?;
    Ok(())
}

fn run_signal_listener(tx: mpsc::Sender<Event>) {
    let mut signals = match signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ]) {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("screen-time: cannot install signal handlers: {error}");
            return;
        }
    };
    if signals.forever().next().is_some() {
        let _ = tx.send(Event::Shutdown);
    }
}

fn recover_previous_power(db: &mut Database, boot_id: &str, now_ts: i64) -> Result<()> {
    let Some(open) = db.open_power_session()? else {
        return Ok(());
    };
    let last_event = db.last_event_ts()?.unwrap_or(open.start_ts);
    let end = last_event.max(open.start_ts).min(now_ts);
    db.close_power_session(open.id, end, "unclean")?;
    db.commit_batch(
        &[EventRecord {
            ts: now_ts,
            kind: "recovery".to_owned(),
            app: None,
            state: Some(format!(
                "previous power interval closed at {}",
                timeutil::rfc3339(end)
            )),
            boot_id: boot_id.to_owned(),
        }],
        &[],
        now_ts,
    )?;
    Ok(())
}

/// First-failure/changed-error/recovery logging. A local copy keeps this
/// task independent of the listener backoff work.
#[derive(Debug, Default)]
struct RetryLog {
    previous: Option<String>,
}

impl RetryLog {
    fn should_log(&mut self, message: &str) -> bool {
        if self.previous.as_deref() == Some(message) {
            return false;
        }
        self.previous = Some(message.to_owned());
        true
    }

    fn recovered(&mut self) -> bool {
        self.previous.take().is_some()
    }
}

struct OpenSession {
    app: String,
    start_ts: i64,
}

struct Tracker {
    date: NaiveDate,
    day: crate::db::DayData,
    open: Option<OpenSession>,
    idle: bool,
    last_focus: Option<String>,
    pending_events: Vec<EventRecord>,
    pending_sessions: Vec<Session>,
    last_commit_error: RetryLog,
    snapshot_error: RetryLog,
    events_drop_warned: bool,
    sessions_drop_warned: bool,
    last_commit_ts: i64,
    last_snapshot_ts: i64,
    boot_id: String,
    power_start_ts: i64,
    retention: RetentionPolicy,
}

impl Tracker {
    fn new(
        date: NaiveDate,
        day: crate::db::DayData,
        last_commit_ts: i64,
        boot_id: String,
        power_start_ts: i64,
        retention: RetentionPolicy,
    ) -> Self {
        Self {
            date,
            day,
            open: None,
            idle: false,
            last_focus: None,
            pending_events: Vec::new(),
            pending_sessions: Vec::new(),
            last_commit_error: RetryLog::default(),
            snapshot_error: RetryLog::default(),
            events_drop_warned: false,
            sessions_drop_warned: false,
            last_commit_ts,
            last_snapshot_ts: 0,
            boot_id,
            power_start_ts,
            retention,
        }
    }

    fn on_focus(&mut self, app: Option<String>, ts: i64) -> bool {
        if self.idle {
            if app.as_deref() != self.last_focus.as_deref() {
                self.last_focus = app.clone();
            }
            return false;
        }
        if self.open.as_ref().map(|open| open.app.as_str()) == app.as_deref() {
            self.last_focus = app;
            return false;
        }
        self.close_open(ts);
        self.last_focus = app.clone();
        if let Some(app) = app {
            self.open = Some(OpenSession {
                app: app.clone(),
                start_ts: ts,
            });
            self.record_focus_event(ts, Some(app), "begin");
        } else {
            self.open = None;
            self.record_focus_event(ts, None, "none");
        }
        true
    }

    fn on_idle(&mut self, idle: bool, ts: i64) -> bool {
        if idle == self.idle {
            return false;
        }
        if idle {
            self.close_open(ts);
            self.idle = true;
            self.record_event(ts, "idle", self.last_focus.clone(), "begin");
        } else {
            self.idle = false;
            self.record_event(ts, "idle", self.last_focus.clone(), "end");
            if let Some(app) = self.last_focus.clone() {
                self.open = Some(OpenSession { app, start_ts: ts });
            }
        }
        true
    }

    fn close_open(&mut self, ts: i64) {
        let Some(open) = self.open.take() else {
            return;
        };
        if ts <= open.start_ts {
            // A backward clock jump would otherwise drop the interval without
            // any trace in the raw event log.
            self.record_event(ts, "clock", Some(open.app), "backward");
            return;
        }
        for session in sessions::split_session(&open.app, open.start_ts, ts) {
            self.record_closed_session(session);
        }
    }

    fn record_closed_session(&mut self, session: Session) {
        if let Some(previous) = self.pending_sessions.last_mut() {
            let gap = session.start_ts - previous.end_ts;
            if previous.app == session.app
                && previous.date == session.date
                && (0..=sessions::SESSION_MERGE_GAP_SECONDS).contains(&gap)
            {
                let old_end = previous.end_ts;
                previous.end_ts = previous.end_ts.max(session.end_ts);
                previous.seconds = previous.end_ts - previous.start_ts;
                if session.confidence == "low" {
                    previous.confidence = "low".to_owned();
                }
                if let Some(extension) =
                    Session::new(&session.app, session.date.clone(), old_end, previous.end_ts)
                {
                    self.add_to_day(&extension, true);
                }
                return;
            }
        }
        self.add_to_day(&session, false);
        self.pending_sessions.push(session);
        if cap_pending(&mut self.pending_sessions, MAX_PENDING_SESSIONS) > 0
            && !self.sessions_drop_warned
        {
            self.sessions_drop_warned = true;
            eprintln!(
                "screen-time: pending session queue is full; dropping oldest sessions (max {MAX_PENDING_SESSIONS})"
            );
        }
    }

    fn add_to_day(&mut self, session: &Session, extension_only: bool) {
        if session.date != self.date.format("%Y-%m-%d").to_string() {
            return;
        }
        let entry = self.day.apps.entry(session.app.clone()).or_default();
        entry.seconds += session.seconds;
        if !extension_only {
            entry.sessions += 1;
        }
        entry.first_ts = Some(
            entry
                .first_ts
                .map(|value| value.min(session.start_ts))
                .unwrap_or(session.start_ts),
        );
        entry.last_ts = Some(
            entry
                .last_ts
                .map(|value| value.max(session.end_ts))
                .unwrap_or(session.end_ts),
        );
        timeutil::add_interval_to_slots(
            &mut entry.slots,
            self.date,
            session.start_ts,
            session.end_ts,
        );
        timeutil::add_interval_to_slots(
            &mut self.day.slots,
            self.date,
            session.start_ts,
            session.end_ts,
        );
    }

    fn record_focus_event(&mut self, ts: i64, app: Option<String>, state: &str) {
        self.record_event(ts, "focus", app, state);
    }

    fn record_event(&mut self, ts: i64, kind: &str, app: Option<String>, state: &str) {
        self.pending_events.push(EventRecord {
            ts,
            kind: kind.to_owned(),
            app,
            state: Some(state.to_owned()),
            boot_id: self.boot_id.clone(),
        });
        if cap_pending(&mut self.pending_events, MAX_PENDING_EVENTS) > 0 && !self.events_drop_warned
        {
            self.events_drop_warned = true;
            eprintln!(
                "screen-time: pending event queue is full; dropping oldest events (max {MAX_PENDING_EVENTS})"
            );
        }
    }

    fn after_event(
        &mut self,
        paths: &Paths,
        db: &mut Database,
        now_ts: i64,
        changed: bool,
    ) -> Result<()> {
        if !changed {
            return Ok(());
        }
        if self.pending_events.len() >= BATCH_EVENT_LIMIT
            || now_ts - self.last_commit_ts >= BATCH_INTERVAL_SECONDS
        {
            self.flush(paths, db, now_ts)?;
        } else {
            self.write_snapshot(paths, now_ts);
        }
        Ok(())
    }

    fn ensure_day(&mut self, paths: &Paths, db: &mut Database, now_ts: i64) -> Result<()> {
        let now_date = timeutil::date_of(now_ts);
        if now_date != self.date {
            self.rollover(paths, db, now_date, now_ts)?;
        }
        Ok(())
    }

    fn tick(&mut self, paths: &Paths, db: &mut Database, now_ts: i64) -> Result<()> {
        self.ensure_day(paths, db, now_ts)?;
        if !self.pending_events.is_empty() && now_ts - self.last_commit_ts >= BATCH_INTERVAL_SECONDS
        {
            self.flush(paths, db, now_ts)?;
        }
        if now_ts - self.last_snapshot_ts >= HEARTBEAT_SECONDS {
            self.write_snapshot(paths, now_ts);
        }
        Ok(())
    }

    fn next_deadline(&self, now_ts: i64) -> i64 {
        let mut deadline = timeutil::day_bounds(self.date).1;
        if !self.pending_events.is_empty() {
            deadline = deadline.min(self.last_commit_ts + BATCH_INTERVAL_SECONDS);
        }
        deadline = deadline.min(self.last_snapshot_ts + HEARTBEAT_SECONDS);
        deadline.max(now_ts + 1)
    }

    fn flush(&mut self, paths: &Paths, db: &mut Database, now_ts: i64) -> Result<()> {
        if self.pending_events.is_empty() && self.pending_sessions.is_empty() {
            return Ok(());
        }
        match db.commit_batch(&self.pending_events, &self.pending_sessions, now_ts) {
            Ok(()) => {
                self.pending_events.clear();
                self.pending_sessions.clear();
                self.last_commit_ts = now_ts;
                if self.last_commit_error.recovered() {
                    eprintln!("screen-time: batch commit recovered");
                }
                self.events_drop_warned = false;
                self.sessions_drop_warned = false;
                self.write_snapshot(paths, now_ts);
                Ok(())
            }
            Err(error) => {
                let message = format!("{error:#}");
                if self.last_commit_error.should_log(&message) {
                    eprintln!("screen-time: batch commit failed (will retry): {message}");
                }
                Ok(())
            }
        }
    }

    fn rollover(
        &mut self,
        paths: &Paths,
        db: &mut Database,
        new_date: NaiveDate,
        now_ts: i64,
    ) -> Result<()> {
        let old_date = self.date;
        let (_, old_end) = timeutil::day_bounds(old_date);
        let resume = self.open.as_ref().map(|open| open.app.clone());
        self.close_open(old_end);
        self.record_event(old_end, "rollover", None, "day_end");
        self.flush(paths, db, old_end)?;

        self.date = new_date;
        self.day = db.load_day_closed(new_date, now_ts)?;
        if !self.idle {
            if let (Some(app), true) = (resume, old_date.succ_opt() == Some(new_date)) {
                self.open = Some(OpenSession {
                    app,
                    start_ts: old_end,
                });
            }
        }
        self.record_event(now_ts, "rollover", None, "begin");
        let (config, config_error) = Config::load_default_lenient();
        if let Some(error) = config_error {
            eprintln!("screen-time: warning: {error:#}; using keep-all retention");
        }
        self.retention = RetentionPolicy::from_config(&config.retention);
        match db.retention(&self.retention, new_date, now_ts) {
            Ok(result) if self.retention.is_enabled() => eprintln!(
                "screen-time: retention removed {} events, {} sessions, {} daily-app rows, {} daily-slot rows, {} power sessions",
                result.events,
                result.sessions,
                result.daily_app,
                result.daily_slots,
                result.power_sessions
            ),
            Ok(_) => {}
            Err(error) => eprintln!("screen-time: retention failed during rollover: {error:#}"),
        }
        self.write_snapshot(paths, now_ts);
        Ok(())
    }

    fn shutdown(
        &mut self,
        paths: &Paths,
        db: &mut Database,
        power_id: i64,
        now: DateTime<Local>,
    ) -> Result<()> {
        let now_ts = now.timestamp();
        self.close_open(now_ts);
        self.record_event(now_ts, "shutdown", None, "clean");
        self.flush(paths, db, now_ts)?;
        if let Err(error) = db.close_power_session(power_id, now_ts, "shutdown") {
            eprintln!("screen-time: cannot close power session: {error:#}");
        }
        db.set_meta("clean_shutdown", "1")?;
        db.set_meta("last_shutdown_at", &timeutil::rfc3339(now_ts))?;
        db.set_meta("last_commit_at", &timeutil::rfc3339(now_ts))?;
        let _ = db.checkpoint();
        let _ = std::fs::remove_file(&paths.live_path);
        ui::remove_state(paths);
        Ok(())
    }

    fn snapshot(&self, now_ts: i64) -> LiveSnapshot {
        let mut snapshot = LiveSnapshot::empty(
            self.date.format("%Y-%m-%d").to_string(),
            now_ts,
            &timeutil::rfc3339(self.last_commit_ts),
            &self.boot_id,
        );
        snapshot.closed_active_seconds = self.day.active_seconds();
        snapshot.powered_closed_seconds = self.day.powered_seconds;
        let (day_start, _) = timeutil::day_bounds(self.date);
        snapshot.powered_open_start = Some(timeutil::rfc3339(self.power_start_ts.max(day_start)));
        snapshot.first_active_ts = self.day.first_active_ts();
        snapshot.last_active_ts = self.day.last_active_ts();
        snapshot.open = self.open.as_ref().map(|open| OpenInterval {
            app: open.app.clone(),
            start: timeutil::rfc3339(open.start_ts),
        });
        for (app, data) in &self.day.apps {
            snapshot.apps.insert(
                app.clone(),
                SnapshotApp {
                    closed_seconds: data.seconds,
                    closed_sessions: data.sessions,
                    slots: data.slots.to_vec(),
                },
            );
        }
        snapshot.slots = self.day.slots.to_vec();
        snapshot.power_slots = self.day.power_slots.to_vec();
        snapshot
    }

    fn write_snapshot(&mut self, paths: &Paths, now_ts: i64) {
        let snapshot = self.snapshot(now_ts);
        match snapshot::write_atomic(&paths.live_path, &snapshot) {
            Ok(()) => {
                self.last_snapshot_ts = now_ts;
                if self.snapshot_error.recovered() {
                    eprintln!("screen-time: live snapshot writes recovered");
                }
            }
            Err(error) => {
                // Keep the heartbeat cadence after a failure; otherwise the
                // deadline stays in the past and retries every second.
                self.last_snapshot_ts = now_ts;
                let message = format!("{error:#}");
                if self.snapshot_error.should_log(&message) {
                    eprintln!("screen-time: cannot write live snapshot: {message}");
                }
            }
        }
    }
}

/// Drop the oldest entries above `cap`, returning how many were discarded.
fn cap_pending<T>(queue: &mut Vec<T>, cap: usize) -> usize {
    if queue.len() <= cap {
        return 0;
    }
    let excess = queue.len() - cap;
    queue.drain(..excess);
    excess
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sessions::Session;
    use crate::timeutil::SLOT_COUNT;
    use rusqlite::params;
    use std::fs;
    use tempfile::tempdir;

    fn base() -> i64 {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        timeutil::day_bounds(date).0
    }

    fn tracker() -> Tracker {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        Tracker::new(
            date,
            crate::db::DayData::new(date),
            base(),
            "boot".to_owned(),
            base(),
            RetentionPolicy::disabled(),
        )
    }

    fn test_paths(dir: &tempfile::TempDir) -> Paths {
        let paths = Paths {
            state_dir: dir.path().to_path_buf(),
            db_path: dir.path().join("screen-time.db"),
            runtime_dir: dir.path().to_path_buf(),
            live_path: dir.path().join("live.json"),
            ui_state_path: dir.path().join("ui.json"),
            backups_dir: dir.path().join("backups"),
            migration_backups_dir: dir.path().join("migration-backups"),
        };
        paths.ensure().unwrap();
        paths
    }

    #[test]
    fn focus_switch_closes_and_opens() {
        let mut tracker = tracker();
        assert!(tracker.on_focus(Some("Code".to_owned()), base() + 1_000));
        assert!(tracker.on_focus(Some("Firefox".to_owned()), base() + 1_600));
        assert_eq!(tracker.pending_sessions.len(), 1);
        assert_eq!(tracker.pending_sessions[0].app, "Code");
        assert_eq!(tracker.pending_sessions[0].seconds, 600);
        assert_eq!(tracker.day.apps["Code"].seconds, 600);
    }

    #[test]
    fn title_level_same_app_does_not_create_events() {
        let mut tracker = tracker();
        assert!(tracker.on_focus(Some("Code".to_owned()), base() + 1_000));
        assert!(!tracker.on_focus(Some("Code".to_owned()), base() + 1_100));
        assert_eq!(tracker.pending_events.len(), 1);
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, base() + 1_000);
    }

    #[test]
    fn idle_closes_and_resumes() {
        let mut tracker = tracker();
        tracker.on_focus(Some("Code".to_owned()), base() + 1_000);
        assert!(tracker.on_idle(true, base() + 1_600));
        assert!(tracker.open.is_none());
        assert_eq!(tracker.day.apps["Code"].seconds, 600);
        assert!(tracker.on_idle(false, base() + 2_000));
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, base() + 2_000);
    }

    #[test]
    fn idle_false_resync_unblocks_focus_tracking() {
        let mut tracker = tracker();
        let start = base();
        tracker.idle = true;
        assert!(!tracker.on_focus(Some("Code".to_owned()), start + 100));
        assert!(tracker.open.is_none());
        assert_eq!(tracker.last_focus.as_deref(), Some("Code"));
        assert!(tracker.pending_sessions.is_empty());

        assert!(tracker.on_idle(false, start + 200));
        assert_eq!(tracker.open.as_ref().unwrap().app, "Code");
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, start + 200);

        assert!(tracker.on_focus(Some("Firefox".to_owned()), start + 300));
        assert_eq!(tracker.open.as_ref().unwrap().app, "Firefox");
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, start + 300);
        assert_eq!(tracker.pending_sessions.len(), 1);
        assert_eq!(tracker.pending_sessions[0].app, "Code");
        assert_eq!(tracker.pending_sessions[0].seconds, 100);
    }

    #[test]
    fn merge_within_batch() {
        let mut tracker = tracker();
        tracker.record_closed_session(
            Session::new(
                "Code",
                "2026-11-21".to_owned(),
                base() + 1_000,
                base() + 1_100,
            )
            .unwrap(),
        );
        tracker.record_closed_session(
            Session::new(
                "Code",
                "2026-11-21".to_owned(),
                base() + 1_130,
                base() + 1_200,
            )
            .unwrap(),
        );
        assert_eq!(tracker.pending_sessions.len(), 1);
        assert_eq!(tracker.pending_sessions[0].seconds, 200);
        assert_eq!(tracker.day.apps["Code"].seconds, 200);
        assert_eq!(tracker.day.apps["Code"].sessions, 1);
    }

    #[test]
    fn snapshot_contains_open_interval_and_totals() {
        let mut tracker = tracker();
        tracker.on_focus(Some("Code".to_owned()), base() + 1_000);
        tracker.record_closed_session(
            Session::new("Code", "2026-11-21".to_owned(), 500, 800).unwrap(),
        );
        let snapshot = tracker.snapshot(1_100);
        assert_eq!(snapshot.open.as_ref().unwrap().app, "Code");
        assert_eq!(snapshot.closed_active_seconds, 300);
        assert_eq!(snapshot.apps["Code"].closed_seconds, 300);
        assert!(snapshot.power_slots.len() == SLOT_COUNT);
    }

    #[test]
    fn snapshot_clips_open_power_to_viewed_day() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, _) = timeutil::day_bounds(date);
        let tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            day_start,
            "boot".to_owned(),
            day_start - 3_600,
            RetentionPolicy::disabled(),
        );
        let snapshot = tracker.snapshot(day_start + 60);
        let start = snapshot.powered_open_start_ts().unwrap();
        assert!(start >= day_start);
        assert_eq!(start, day_start);
    }

    #[test]
    fn flush_commits_pending_data() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            state_dir: dir.path().to_path_buf(),
            db_path: dir.path().join("screen-time.db"),
            runtime_dir: dir.path().to_path_buf(),
            live_path: dir.path().join("live.json"),
            ui_state_path: dir.path().join("ui.json"),
            backups_dir: dir.path().join("backups"),
            migration_backups_dir: dir.path().join("migration-backups"),
        };
        paths.ensure().unwrap();
        let mut db = Database::open_write(&paths.db_path, None).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (start, _) = timeutil::day_bounds(date);
        let mut tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            start,
            "boot".to_owned(),
            start,
            RetentionPolicy::disabled(),
        );
        tracker.record_closed_session(
            Session::new("Code", "2026-11-21".to_owned(), start + 100, start + 200).unwrap(),
        );
        tracker.record_event(start + 200, "focus", Some("Code".to_owned()), "begin");
        tracker.flush(&paths, &mut db, start + 300).unwrap();
        assert!(tracker.pending_sessions.is_empty());
        let (events, sessions) = db.counts().unwrap();
        assert_eq!(events, 1);
        assert_eq!(sessions, 1);
    }
    #[test]
    fn recovery_closes_previous_power_at_last_event() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("screen-time.db");
        let mut db = Database::open_write(&path, None).unwrap();
        let base = base();
        let id = db.start_power_session(base - 3_600, "old-boot").unwrap();
        db.commit_batch(
            &[EventRecord {
                ts: base - 600,
                kind: "focus".to_owned(),
                app: Some("Code".to_owned()),
                state: Some("begin".to_owned()),
                boot_id: "old-boot".to_owned(),
            }],
            &[],
            base - 600,
        )
        .unwrap();
        recover_previous_power(&mut db, "new-boot", base).unwrap();
        assert!(db.open_power_session().unwrap().is_none());
        let (end, reason): (i64, String) = db
            .connection()
            .query_row(
                "SELECT end_ts, reason FROM power_sessions WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(end, base - 600);
        assert_eq!(reason, "unclean");
    }

    #[test]
    fn rollover_closes_old_day_and_resumes_open_app() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            state_dir: dir.path().to_path_buf(),
            db_path: dir.path().join("screen-time.db"),
            runtime_dir: dir.path().to_path_buf(),
            live_path: dir.path().join("live.json"),
            ui_state_path: dir.path().join("ui.json"),
            backups_dir: dir.path().join("backups"),
            migration_backups_dir: dir.path().join("migration-backups"),
        };
        paths.ensure().unwrap();
        let mut db = Database::open_write(&paths.db_path, None).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, day_end) = timeutil::day_bounds(date);
        let mut tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            day_start,
            "boot".to_owned(),
            day_start,
            RetentionPolicy::disabled(),
        );
        tracker.on_focus(Some("Code".to_owned()), day_end - 600);
        let next = date.succ_opt().unwrap();
        tracker.ensure_day(&paths, &mut db, day_end + 60).unwrap();
        assert_eq!(tracker.date, next);
        let sessions = db.load_app_sessions(date, "Code").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].end_ts, day_end);
        assert_eq!(tracker.open.as_ref().unwrap().app, "Code");
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, day_end);
    }

    #[test]
    fn rollover_applies_enabled_retention() {
        struct EnvGuard {
            home: Option<std::ffi::OsString>,
            config_home: Option<std::ffi::OsString>,
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                // SAFETY: this test owns these variables for its duration.
                unsafe {
                    match &self.home {
                        Some(value) => std::env::set_var("HOME", value),
                        None => std::env::remove_var("HOME"),
                    }
                    match &self.config_home {
                        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                        None => std::env::remove_var("XDG_CONFIG_HOME"),
                    }
                }
            }
        }

        let dir = tempdir().unwrap();
        let config_home = dir.path().join("config");
        fs::create_dir_all(config_home.join("screen-time")).unwrap();
        fs::write(
            config_home.join("screen-time").join("config.toml"),
            "[retention]\nevents_days = 1\n",
        )
        .unwrap();
        let _guard = EnvGuard {
            home: std::env::var_os("HOME"),
            config_home: std::env::var_os("XDG_CONFIG_HOME"),
        };
        // SAFETY: no other test in this binary reads these variables.
        unsafe {
            std::env::set_var("HOME", dir.path());
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
        }

        let paths = Paths {
            state_dir: dir.path().to_path_buf(),
            db_path: dir.path().join("screen-time.db"),
            runtime_dir: dir.path().to_path_buf(),
            live_path: dir.path().join("live.json"),
            ui_state_path: dir.path().join("ui.json"),
            backups_dir: dir.path().join("backups"),
            migration_backups_dir: dir.path().join("migration-backups"),
        };
        paths.ensure().unwrap();
        let mut db = Database::open_write(&paths.db_path, None).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, day_end) = timeutil::day_bounds(date);
        db.commit_batch(
            &[EventRecord {
                ts: day_start - 2 * 86_400,
                kind: "focus".to_owned(),
                app: Some("Old".to_owned()),
                state: Some("begin".to_owned()),
                boot_id: "boot".to_owned(),
            }],
            &[],
            day_start - 2 * 86_400,
        )
        .unwrap();
        let mut tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            day_start,
            "boot".to_owned(),
            day_start,
            RetentionPolicy::disabled(),
        );
        tracker.ensure_day(&paths, &mut db, day_end + 60).unwrap();
        let remaining: i64 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind = 'focus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn pending_event_queue_drops_oldest_and_stays_bounded() {
        let mut tracker = tracker();
        let start = base();
        for offset in 0..MAX_PENDING_EVENTS + 5 {
            tracker.record_event(
                start + offset as i64,
                "focus",
                Some("Code".to_owned()),
                "begin",
            );
        }
        assert_eq!(tracker.pending_events.len(), MAX_PENDING_EVENTS);
        assert_eq!(tracker.pending_events.first().unwrap().ts, start + 5);
        assert_eq!(
            tracker.pending_events.last().unwrap().ts,
            start + MAX_PENDING_EVENTS as i64 + 4
        );
        assert!(tracker.events_drop_warned);
    }

    #[test]
    fn pending_session_queue_drops_oldest_without_touching_day_totals() {
        let mut tracker = tracker();
        let start = base();
        let total = MAX_PENDING_SESSIONS + 5;
        for offset in 0..total {
            let app = format!("App{offset}");
            let session_start = start + offset as i64 * 2;
            tracker.record_closed_session(
                Session::new(
                    &app,
                    "2026-11-21".to_owned(),
                    session_start,
                    session_start + 1,
                )
                .unwrap(),
            );
        }
        assert_eq!(tracker.pending_sessions.len(), MAX_PENDING_SESSIONS);
        assert_eq!(tracker.pending_sessions.first().unwrap().app, "App5");
        assert_eq!(tracker.pending_sessions.last().unwrap().app, "App1004");
        assert_eq!(tracker.day.apps.len(), total);
        assert_eq!(tracker.day.active_seconds(), total as i64);
        assert!(tracker.sessions_drop_warned);
    }

    #[test]
    fn failed_commit_then_success_leaves_no_queued_duplicates() {
        let dir = tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut write_db = Database::open_write(&paths.db_path, None).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (start, _) = timeutil::day_bounds(date);
        let mut tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            start,
            "boot".to_owned(),
            start,
            RetentionPolicy::disabled(),
        );
        tracker.record_closed_session(
            Session::new("Code", "2026-11-21".to_owned(), start + 100, start + 200).unwrap(),
        );
        tracker.record_event(start + 200, "focus", Some("Code".to_owned()), "begin");

        let mut read_only = Database::open_read(&paths.db_path).unwrap();
        tracker.flush(&paths, &mut read_only, start + 250).unwrap();
        assert_eq!(tracker.pending_events.len(), 1);
        assert_eq!(tracker.pending_sessions.len(), 1);
        assert_eq!(read_only.counts().unwrap(), (0, 0));

        tracker.flush(&paths, &mut write_db, start + 300).unwrap();
        assert!(tracker.pending_events.is_empty());
        assert!(tracker.pending_sessions.is_empty());
        assert_eq!(write_db.counts().unwrap(), (1, 1));
    }

    #[test]
    fn cap_pending_drops_only_the_oldest_entries() {
        let mut queue = vec![1, 2, 3, 4, 5];
        assert_eq!(cap_pending(&mut queue, 3), 2);
        assert_eq!(queue, vec![3, 4, 5]);
        assert_eq!(cap_pending(&mut queue, 3), 0);
        assert_eq!(queue, vec![3, 4, 5]);
    }

    #[test]
    fn retry_log_suppresses_identical_errors_and_rearms() {
        let mut log = RetryLog::default();
        assert!(log.should_log("disk full"));
        assert!(!log.should_log("disk full"));
        assert!(log.should_log("read-only"));
        assert!(log.recovered());
        assert!(!log.recovered());
        assert!(log.should_log("disk full"));
    }

    #[test]
    fn failed_snapshot_writes_keep_the_heartbeat_cadence() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        fs::write(&blocker, b"x").unwrap();
        let paths = Paths {
            state_dir: dir.path().to_path_buf(),
            db_path: dir.path().join("screen-time.db"),
            runtime_dir: blocker.clone(),
            live_path: blocker.join("live.json"),
            ui_state_path: dir.path().join("ui.json"),
            backups_dir: dir.path().join("backups"),
            migration_backups_dir: dir.path().join("migration-backups"),
        };
        let mut tracker = tracker();
        let ts = base() + 12 * 3_600;

        tracker.write_snapshot(&paths, ts);
        assert_eq!(tracker.last_snapshot_ts, ts);
        assert_eq!(tracker.next_deadline(ts), ts + HEARTBEAT_SECONDS);
        tracker.write_snapshot(&paths, ts + 1);
        assert_eq!(tracker.last_snapshot_ts, ts + 1);
        assert!(!paths.live_path.exists());

        let mut repaired = paths.clone();
        repaired.live_path = dir.path().join("live.json");
        tracker.write_snapshot(&repaired, ts + 2);
        assert_eq!(tracker.last_snapshot_ts, ts + 2);
        assert!(repaired.live_path.is_file());
    }

    #[test]
    fn daemon_lock_is_exclusive_until_the_guard_drops() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let paths = test_paths(&dir);
        let first = acquire_daemon_lock(&paths).unwrap();
        let mode = fs::metadata(paths.state_dir.join("daemon.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let error = acquire_daemon_lock(&paths).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("another daemon is already running"),
            "{error:#}"
        );

        drop(first);
        acquire_daemon_lock(&paths).expect("lock is released when the guard drops");
    }

    #[test]
    fn backward_clock_jump_discards_interval_and_records_one_clock_event() {
        let mut tracker = tracker();
        let start = base();
        assert!(tracker.on_focus(Some("Code".to_owned()), start + 1_000));
        assert!(tracker.on_focus(Some("Firefox".to_owned()), start + 900));

        assert!(tracker.pending_sessions.is_empty());
        assert_eq!(tracker.day.active_seconds(), 0);
        assert!(tracker.day.slots.iter().all(|seconds| *seconds >= 0));
        assert_eq!(tracker.open.as_ref().unwrap().app, "Firefox");
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, start + 900);

        let clock_events: Vec<_> = tracker
            .pending_events
            .iter()
            .filter(|event| event.kind == "clock")
            .collect();
        assert_eq!(clock_events.len(), 1);
        assert_eq!(clock_events[0].ts, start + 900);
        assert_eq!(clock_events[0].state.as_deref(), Some("backward"));
        assert_eq!(clock_events[0].app.as_deref(), Some("Code"));

        // Every discarded interval gets exactly one diagnostic event.
        assert!(tracker.on_focus(Some("Terminal".to_owned()), start + 850));
        let clock_events: Vec<_> = tracker
            .pending_events
            .iter()
            .filter(|event| event.kind == "clock")
            .collect();
        assert_eq!(clock_events.len(), 2);
        assert_eq!(clock_events[1].app.as_deref(), Some("Firefox"));
        assert_eq!(tracker.day.active_seconds(), 0);
        assert!(tracker.day.slots.iter().all(|seconds| *seconds >= 0));
    }

    #[test]
    fn tick_after_midnight_closes_old_day_at_midnight_and_resumes() {
        let dir = tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut db = Database::open_write(&paths.db_path, None).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, day_end) = timeutil::day_bounds(date);
        let mut tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            day_start,
            "boot".to_owned(),
            day_start,
            RetentionPolicy::disabled(),
        );
        let focus_ts = day_end - 10 * 60;
        tracker.on_focus(Some("Code".to_owned()), focus_ts);
        tracker.tick(&paths, &mut db, day_end + 10 * 60).unwrap();

        assert_eq!(tracker.date, date.succ_opt().unwrap());
        let sessions = db.load_app_sessions(date, "Code").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].start_ts, focus_ts);
        assert_eq!(sessions[0].end_ts, day_end);
        assert_eq!(sessions[0].seconds, 10 * 60);
        assert_eq!(tracker.open.as_ref().unwrap().app, "Code");
        assert_eq!(tracker.open.as_ref().unwrap().start_ts, day_end);

        tracker.close_open(day_end + 10 * 60);
        assert_eq!(tracker.day.apps["Code"].seconds, 10 * 60);
        assert_eq!(tracker.day.active_seconds(), 10 * 60);
        assert_eq!(
            sessions[0].seconds + tracker.day.active_seconds(),
            20 * 60,
            "the interval is split exactly at local midnight"
        );
    }

    #[test]
    fn rollover_while_idle_does_not_resume_an_open_interval() {
        let dir = tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut db = Database::open_write(&paths.db_path, None).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, day_end) = timeutil::day_bounds(date);
        let mut tracker = Tracker::new(
            date,
            crate::db::DayData::new(date),
            day_start,
            "boot".to_owned(),
            day_start,
            RetentionPolicy::disabled(),
        );
        tracker.on_focus(Some("Code".to_owned()), day_end - 10 * 60);
        // Simulate an idle signal that raced the rollover: the open focus
        // interval still exists, but tracking is already marked idle.
        tracker.idle = true;
        tracker.tick(&paths, &mut db, day_end + 10 * 60).unwrap();

        assert_eq!(tracker.date, date.succ_opt().unwrap());
        assert!(tracker.open.is_none());
        let sessions = db.load_app_sessions(date, "Code").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].end_ts, day_end);
    }

    #[test]
    fn multi_day_gap_does_not_resume_and_closes_monday_at_midnight() {
        let dir = tempdir().unwrap();
        let paths = test_paths(&dir);
        let mut db = Database::open_write(&paths.db_path, None).unwrap();
        let monday = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let (monday_start, monday_end) = timeutil::day_bounds(monday);
        let mut tracker = Tracker::new(
            monday,
            crate::db::DayData::new(monday),
            monday_start,
            "boot".to_owned(),
            monday_start,
            RetentionPolicy::disabled(),
        );
        tracker.on_focus(Some("Code".to_owned()), monday_start + 10 * 3_600);
        let wednesday = monday.checked_add_days(chrono::Days::new(2)).unwrap();
        let wednesday_start = timeutil::day_bounds(wednesday).0;
        tracker
            .tick(&paths, &mut db, wednesday_start + 10 * 3_600)
            .unwrap();

        assert_eq!(tracker.date, wednesday);
        assert!(tracker.open.is_none());
        let sessions = db.load_app_sessions(monday, "Code").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].end_ts, monday_end);
        let tuesday = monday.succ_opt().unwrap();
        assert!(db.load_app_sessions(tuesday, "Code").unwrap().is_empty());
    }
}
