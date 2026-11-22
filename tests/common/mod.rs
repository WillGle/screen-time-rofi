#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

use chrono::{NaiveDate, TimeZone, Utc};
use tempfile::TempDir;

use screen_time::db::Database;
use screen_time::paths::Paths;
use screen_time::sessions::Session;
use screen_time::snapshot::{self, LiveSnapshot, SnapshotApp};
use screen_time::timeutil::{self, SLOT_COUNT};

pub const BOOT_ID: &str = "test-boot";
pub const NOW: &str = "2026-11-21T17:15:00+00:00";

pub struct Fixture {
    pub dir: TempDir,
    pub paths: Paths,
}

impl Fixture {
    /// Same temp paths as [`Fixture::new`], but without a database or live
    /// snapshot. Used to pin empty and not-yet-started states.
    pub fn empty() -> Self {
        init_tz();
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state").join("screen-time");
        let runtime_dir = dir.path().join("run").join("screen-time");
        let paths = Paths {
            state_dir: state_dir.clone(),
            db_path: state_dir.join("screen-time.db"),
            runtime_dir: runtime_dir.clone(),
            live_path: runtime_dir.join("live.json"),
            ui_state_path: runtime_dir.join("ui.json"),
            backups_dir: state_dir.join("backups").join("updates"),
            migration_backups_dir: state_dir.join("backups").join("migrations"),
        };
        paths.ensure().unwrap();
        Self { dir, paths }
    }

    pub fn new() -> Self {
        let fixture = Self::empty();
        build_database(&fixture.paths);
        write_snapshot(&fixture.paths);
        fixture
    }

    /// Add closed sessions to an otherwise empty fixture. This creates the
    /// database on first use.
    pub fn add_sessions(&self, sessions: &[Session]) {
        let mut db = Database::open_write(&self.paths.db_path, None).unwrap();
        db.commit_batch(&[], sessions, 0).unwrap();
    }

    pub fn render(&self, args: &[&str]) -> String {
        self.render_env(args, &[])
    }

    pub fn config_home(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    pub fn write_config(&self, text: &str) {
        let path = self.config_home().join("screen-time").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub fn render_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> String {
        let output = self.render_env_output(args, extra_env);
        assert!(
            output.status.success(),
            "screen-time failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("render output is not UTF-8")
    }

    pub fn render_env_output(
        &self,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_screen-time"));
        let config_home = self.dir.path().join("config");
        std::fs::create_dir_all(&config_home).unwrap();
        command
            .args(args)
            .env("TZ", "UTC")
            .env("HOME", self.dir.path())
            .env("XDG_CONFIG_HOME", config_home)
            .env("SCREEN_TIME_STATE_DIR", &self.paths.state_dir)
            .env("SCREEN_TIME_RUNTIME_DIR", &self.paths.runtime_dir)
            .env("SCREEN_TIME_NOW", NOW)
            .env("SCREEN_TIME_BOOT_ID", BOOT_ID)
            .env("SCREEN_TIME_IGNORE_PID", "1")
            .env_remove("ROFI_RETV")
            .env_remove("ROFI_INFO")
            .env_remove("ROFI_THEME")
            .env_remove("SCREEN_TIME_SESSION_COUNT")
            .env_remove("SCREEN_TIME_NARROW")
            .env_remove("SCREEN_TIME_SCREEN_WIDTH");
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().expect("cannot run screen-time")
    }
}

pub fn init_tz() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: integration tests run before any renderer child is spawned.
        unsafe { std::env::set_var("TZ", "UTC") };
    });
}

fn at(month_day: NaiveDate, hour: u32, minute: u32, second: u32) -> i64 {
    Utc.with_ymd_and_hms(
        month_day.year(),
        month_day.month(),
        month_day.day(),
        hour,
        minute,
        second,
    )
    .single()
    .unwrap()
    .timestamp()
}

use chrono::Datelike;

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 21).unwrap()
}

fn session(day: NaiveDate, app: &str, start_min: u32, end_min: u32) -> Session {
    let start = at(day, 7 + start_min / 60, start_min % 60, 0);
    let end = at(day, 7 + end_min / 60, end_min % 60, 0);
    Session::new(app, day.format("%Y-%m-%d").to_string(), start, end).unwrap()
}

fn clock_session(day: NaiveDate, app: &str, start: (u32, u32), end: (u32, u32)) -> Session {
    Session::new(
        app,
        day.format("%Y-%m-%d").to_string(),
        at(day, start.0, start.1, 0),
        at(day, end.0, end.1, 0),
    )
    .unwrap()
}

pub fn today_sessions() -> Vec<Session> {
    let day = date();
    let mut sessions = Vec::new();
    // Code: five multi-session bursts (18 sessions total, 2h14m) and four
    // longer single sessions (93m).
    for (start, end) in [
        ((9, 12, 0), (9, 13, 30)),
        ((9, 14, 0), (9, 15, 30)),
        ((9, 16, 0), (9, 17, 30)),
        ((9, 18, 0), (9, 19, 30)),
    ] {
        sessions.push(clock_session_seconds(day, "Code", start, end));
    }
    for (start, end) in [((9, 30), (9, 32)), ((9, 33), (9, 35))] {
        sessions.push(clock_session(day, "Code", start, end));
    }
    for (start, end) in [((10, 2), (10, 5)), ((10, 6), (10, 9)), ((10, 10), (10, 13))] {
        sessions.push(clock_session(day, "Code", start, end));
    }
    for (start, end) in [((13, 5), (13, 7)), ((13, 8), (13, 10))] {
        sessions.push(clock_session(day, "Code", start, end));
    }
    for (start, end) in [
        ((14, 2), (14, 8)),
        ((14, 9), (14, 15)),
        ((14, 16), (14, 22)),
    ] {
        sessions.push(clock_session(day, "Code", start, end));
    }
    for (start, end) in [
        ((7, 5), (7, 20)),
        ((11, 0), (11, 20)),
        ((15, 0), (15, 18)),
        ((16, 35), (17, 15)),
    ] {
        sessions.push(clock_session(day, "Code", start, end));
    }

    sessions.push(clock_session(day, "Firefox", (8, 0), (8, 40)));
    sessions.push(clock_session(day, "Firefox", (10, 30), (11, 0)));
    sessions.push(clock_session(day, "Firefox", (12, 0), (12, 22)));
    sessions.push(clock_session(day, "Terminal", (9, 0), (9, 24)));
    sessions.push(clock_session(day, "Terminal", (13, 30), (13, 54)));
    sessions.push(clock_session(day, "Slack", (11, 10), (11, 26)));
    sessions.push(clock_session(day, "Slack", (15, 40), (15, 56)));
    sessions.push(clock_session(day, "Music", (12, 30), (12, 52)));
    sessions.push(clock_session(day, "Gimp", (11, 26), (11, 46)));
    sessions.push(clock_session(day, "Zoom", (14, 30), (14, 50)));
    sessions.push(clock_session(day, "Notes", (7, 20), (8, 0)));
    sessions
}

fn clock_session_seconds(
    day: NaiveDate,
    app: &str,
    start: (u32, u32, u32),
    end: (u32, u32, u32),
) -> Session {
    Session::new(
        app,
        day.format("%Y-%m-%d").to_string(),
        at(day, start.0, start.1, start.2),
        at(day, end.0, end.1, end.2),
    )
    .unwrap()
}

fn prior_sessions() -> Vec<Session> {
    let totals = [300i64, 360, 360, 390, 420, 402];
    let mut sessions = Vec::new();
    for (offset, total) in totals.iter().enumerate() {
        let day = date()
            .checked_sub_days(chrono::Days::new((6 - offset) as u64))
            .unwrap();
        let code = 108;
        let firefox = 60;
        let terminal = 45;
        let slack = 30;
        let remainder = total - code - firefox - terminal - slack;
        let mut cursor = 0u32;
        for (app, minutes) in [
            ("Code", code),
            ("Firefox", firefox),
            ("Terminal", terminal),
            ("Slack", slack),
            ("Files", remainder),
        ] {
            sessions.push(session(day, app, cursor, cursor + minutes as u32));
            cursor += minutes as u32;
        }
    }
    sessions
}

fn build_database(paths: &Paths) {
    let mut db = Database::open_write(&paths.db_path, None).unwrap();
    let mut sessions = prior_sessions();
    sessions.extend(today_sessions());
    db.commit_batch(&[], &sessions, 0).unwrap();
    let day = db.load_day(date(), at(date(), 17, 15, 0)).unwrap();
    assert_eq!(day.active_seconds(), 408 * 60);
}

fn write_snapshot(paths: &Paths) {
    let db = Database::open_read(&paths.db_path).unwrap();
    let day = db.load_day(date(), at(date(), 17, 15, 0)).unwrap();
    let mut snapshot = LiveSnapshot::empty(
        "2026-11-21",
        at(date(), 17, 14, 40),
        "2026-11-21T17:14:00+00:00",
        BOOT_ID,
    );
    snapshot.closed_active_seconds = day.active_seconds();
    snapshot.powered_closed_seconds = 2 * 3600;
    snapshot.powered_open_start = Some("2026-11-21T06:35:00+00:00".to_owned());
    snapshot.first_active_ts = day.first_active_ts();
    snapshot.last_active_ts = day.last_active_ts();
    for (app, data) in &day.apps {
        snapshot.apps.insert(
            app.clone(),
            SnapshotApp {
                closed_seconds: data.seconds,
                closed_sessions: data.sessions,
                slots: data.slots.to_vec(),
            },
        );
    }
    snapshot.slots = day.slots.to_vec();
    let mut power_slots = [0i64; SLOT_COUNT];
    timeutil::add_interval_to_slots(
        &mut power_slots,
        date(),
        at(date(), 4, 35, 0),
        at(date(), 6, 35, 0),
    );
    snapshot.power_slots = power_slots.to_vec();
    snapshot::write_atomic(&paths.live_path, &snapshot).unwrap();
}

pub fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

pub fn assert_or_update_golden(name: &str, actual: &str) {
    let path = golden(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden {}; run UPDATE_GOLDEN=1", path.display()));
    assert_eq!(expected, actual, "golden mismatch for {}", path.display());
}
