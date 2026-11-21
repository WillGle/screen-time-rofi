use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::paths;
use crate::timeutil::{self, SLOT_COUNT};

pub const SNAPSHOT_VERSION: u32 = 1;
pub const STALE_AFTER_SECONDS: i64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveSnapshot {
    pub version: u32,
    pub date: String,
    pub as_of: String,
    pub last_commit_at: String,
    pub boot_id: String,
    pub pid: u32,
    pub closed_active_seconds: i64,
    pub powered_closed_seconds: i64,
    pub powered_open_start: Option<String>,
    pub first_active_ts: Option<i64>,
    pub last_active_ts: Option<i64>,
    pub open: Option<OpenInterval>,
    pub apps: BTreeMap<String, SnapshotApp>,
    pub slots: Vec<i64>,
    pub power_slots: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenInterval {
    pub app: String,
    pub start: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotApp {
    pub closed_seconds: i64,
    pub closed_sessions: i64,
    pub slots: Vec<i64>,
}

impl LiveSnapshot {
    pub fn empty(date: impl Into<String>, now_ts: i64, last_commit: &str, boot_id: &str) -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            date: date.into(),
            as_of: timeutil::rfc3339(now_ts),
            last_commit_at: last_commit.to_owned(),
            boot_id: boot_id.to_owned(),
            pid: std::process::id(),
            closed_active_seconds: 0,
            powered_closed_seconds: 0,
            powered_open_start: None,
            first_active_ts: None,
            last_active_ts: None,
            open: None,
            apps: BTreeMap::new(),
            slots: vec![0; SLOT_COUNT],
            power_slots: vec![0; SLOT_COUNT],
        }
    }

    pub fn as_of_local(&self) -> Option<DateTime<Local>> {
        timeutil::parse_rfc3339(&self.as_of)
    }

    pub fn last_commit_local(&self) -> Option<DateTime<Local>> {
        timeutil::parse_rfc3339(&self.last_commit_at)
    }

    pub fn open_start_ts(&self) -> Option<i64> {
        self.open
            .as_ref()
            .and_then(|open| timeutil::parse_rfc3339(&open.start))
            .map(|value| value.timestamp())
    }

    pub fn powered_open_start_ts(&self) -> Option<i64> {
        self.powered_open_start
            .as_ref()
            .and_then(|value| timeutil::parse_rfc3339(value))
            .map(|value| value.timestamp())
    }

    pub fn is_stale(&self, now_ts: i64, boot_id: &str) -> bool {
        if self.version != SNAPSHOT_VERSION || self.boot_id != boot_id {
            return true;
        }
        let ignore_pid = std::env::var_os("SCREEN_TIME_IGNORE_PID").is_some();
        if !ignore_pid && !paths::pid_alive(self.pid) {
            return true;
        }
        let Some(as_of) = self.as_of_local() else {
            return true;
        };
        let age = now_ts - as_of.timestamp();
        !(-30..=STALE_AFTER_SECONDS).contains(&age)
    }

    pub fn fresh_open_seconds(&self, now_ts: i64, stale: bool) -> i64 {
        if stale {
            return 0;
        }
        self.open_start_ts()
            .map(|start| (now_ts - start).max(0))
            .unwrap_or(0)
    }

    pub fn fresh_powered_open_seconds(&self, now_ts: i64, stale: bool) -> i64 {
        if stale {
            return 0;
        }
        self.powered_open_start_ts()
            .map(|start| (now_ts - start).max(0))
            .unwrap_or(0)
    }

    pub fn app_total(&self, app: &str, now_ts: i64, stale: bool) -> i64 {
        let closed = self
            .apps
            .get(app)
            .map(|value| value.closed_seconds)
            .unwrap_or(0);
        let open = if !stale && self.open.as_ref().is_some_and(|open| open.app == app) {
            self.fresh_open_seconds(now_ts, stale)
        } else {
            0
        };
        closed + open
    }

    pub fn app_sessions(&self, app: &str, stale: bool) -> i64 {
        let closed = self
            .apps
            .get(app)
            .map(|value| value.closed_sessions)
            .unwrap_or(0);
        let open = if !stale && self.open.as_ref().is_some_and(|open| open.app == app) {
            1
        } else {
            0
        };
        closed + open
    }

    pub fn total_active(&self, now_ts: i64, stale: bool) -> i64 {
        self.closed_active_seconds + self.fresh_open_seconds(now_ts, stale)
    }

    pub fn total_powered(&self, now_ts: i64, stale: bool) -> i64 {
        self.powered_closed_seconds + self.fresh_powered_open_seconds(now_ts, stale)
    }

    pub fn slots_for_app(
        &self,
        app: &str,
        date: chrono::NaiveDate,
        now_ts: i64,
        stale: bool,
    ) -> [i64; SLOT_COUNT] {
        let mut slots = [0i64; SLOT_COUNT];
        if let Some(app_data) = self.apps.get(app) {
            for (index, seconds) in app_data.slots.iter().take(SLOT_COUNT).enumerate() {
                slots[index] = (*seconds).max(0);
            }
        }
        if !stale && self.open.as_ref().is_some_and(|open| open.app == app) {
            if let Some(start) = self.open_start_ts() {
                timeutil::add_interval_to_slots(&mut slots, date, start, now_ts);
            }
        }
        slots
    }

    pub fn total_slots(
        &self,
        date: chrono::NaiveDate,
        now_ts: i64,
        stale: bool,
    ) -> [i64; SLOT_COUNT] {
        let mut slots = [0i64; SLOT_COUNT];
        for (index, seconds) in self.slots.iter().take(SLOT_COUNT).enumerate() {
            slots[index] = (*seconds).max(0);
        }
        if !stale {
            if let Some(start) = self.open_start_ts() {
                timeutil::add_interval_to_slots(&mut slots, date, start, now_ts);
            }
        }
        slots
    }

    pub fn total_power_slots(
        &self,
        date: chrono::NaiveDate,
        now_ts: i64,
        stale: bool,
    ) -> [i64; SLOT_COUNT] {
        let mut slots = [0i64; SLOT_COUNT];
        for (index, seconds) in self.power_slots.iter().take(SLOT_COUNT).enumerate() {
            slots[index] = (*seconds).max(0);
        }
        if !stale {
            if let Some(start) = self.powered_open_start_ts() {
                timeutil::add_interval_to_slots(&mut slots, date, start, now_ts);
            }
        }
        slots
    }
}

pub fn write_atomic(path: &Path, snapshot: &LiveSnapshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create snapshot directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec(snapshot).context("cannot serialize live snapshot")?;
    let temp_path = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("live.json"),
        std::process::id()
    ));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp_path)
            .with_context(|| format!("cannot create {}", temp_path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("cannot write {}", temp_path.display()))?;
        file.flush()?;
        drop(file);
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "cannot atomically replace {} with {}",
                path.display(),
                temp_path.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub fn read(path: &Path) -> Result<Option<LiveSnapshot>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", path.display()));
        }
    };
    let snapshot: LiveSnapshot = serde_json::from_slice(&bytes)
        .with_context(|| format!("cannot parse {}", path.display()))?;
    if snapshot.version != SNAPSHOT_VERSION {
        bail!(
            "unsupported live snapshot version {} (expected {})",
            snapshot.version,
            SNAPSHOT_VERSION
        );
    }
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_and_read_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("live.json");
        let mut snapshot =
            LiveSnapshot::empty("2026-11-21", 1_000, "2026-11-21T00:16:40+00:00", "boot");
        snapshot.closed_active_seconds = 100;
        snapshot.apps.insert(
            "Code".to_owned(),
            SnapshotApp {
                closed_seconds: 100,
                closed_sessions: 1,
                slots: vec![0; SLOT_COUNT],
            },
        );
        write_atomic(&path, &snapshot).unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), snapshot);
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn stale_guard_uses_age_and_boot_id() {
        let mut snapshot =
            LiveSnapshot::empty("2026-11-21", 1_000, "2026-11-21T00:16:40+00:00", "boot");
        snapshot.pid = std::process::id();
        snapshot.as_of = timeutil::rfc3339(1_000);
        assert!(!snapshot.is_stale(1_100, "boot"));
        assert!(snapshot.is_stale(1_300, "boot"));
        assert!(snapshot.is_stale(1_100, "other-boot"));
        snapshot.pid = 999_999;
        assert!(snapshot.is_stale(1_100, "boot"));
    }

    #[test]
    fn open_interval_is_not_extended_when_stale() {
        let mut snapshot =
            LiveSnapshot::empty("2026-11-21", 1_000, "2026-11-21T00:16:40+00:00", "boot");
        snapshot.pid = std::process::id();
        snapshot.open = Some(OpenInterval {
            app: "Code".to_owned(),
            start: timeutil::rfc3339(900),
        });
        snapshot.closed_active_seconds = 500;
        assert_eq!(snapshot.total_active(1_100, false), 700);
        assert_eq!(snapshot.total_active(1_100, true), 500);
    }

    #[test]
    fn slots_include_open_interval() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, _) = timeutil::day_bounds(date);
        let mut snapshot = LiveSnapshot::empty("2026-11-21", day_start, "", "boot");
        snapshot.pid = std::process::id();
        snapshot.open = Some(OpenInterval {
            app: "Code".to_owned(),
            start: timeutil::rfc3339(day_start + 9 * 3600),
        });
        let slots = snapshot.slots_for_app("Code", date, day_start + 9 * 3600 + 3600, false);
        assert_eq!(slots.iter().sum::<i64>(), 3600);
    }
}
