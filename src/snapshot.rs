use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::db::{DataFingerprint, Database};
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

/// Manifest written beside a database snapshot. It is deliberately
/// self-describing: the installer can verify an update backup without any
/// SQLite tooling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSnapshotManifest {
    pub version: u32,
    pub created_at: String,
    pub source_db: String,
    pub snapshot_db: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub schema_version: i64,
    pub quick_check: String,
    pub counts: BTreeMap<String, i64>,
    pub fingerprint: DataFingerprint,
    pub config_sha256: Option<String>,
}

pub const DATABASE_SNAPSHOT_VERSION: u32 = 1;

/// Create a consistent copy of the live database with `VACUUM INTO`.
///
/// The destination must not exist yet. The snapshot directory is created with
/// mode `0700`, the database and manifest with `0600`. The function verifies
/// `quick_check`, records counts and the data fingerprint, and fsyncs the
/// output before returning.
pub fn create_database_snapshot(
    source_db: &Path,
    to: &Path,
    include_config_hash: bool,
) -> Result<DatabaseSnapshotManifest> {
    if to.exists() {
        bail!("snapshot destination already exists: {}", to.display());
    }
    if !source_db.is_file() {
        bail!("database {} does not exist", source_db.display());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(to)
        .with_context(|| format!("cannot create snapshot directory {}", to.display()))?;
    let snapshot_db = to.join("screen-time.db");
    let manifest_path = to.join("manifest.json");

    let result = (|| -> Result<DatabaseSnapshotManifest> {
        let db = Database::open_snapshot_source(source_db)?;
        db.connection()
            .execute(
                "VACUUM INTO ?1",
                rusqlite::params![snapshot_db.to_string_lossy()],
            )
            .with_context(|| format!("cannot snapshot database into {}", snapshot_db.display()))?;
        drop(db);

        let snapshot = Database::open_read(&snapshot_db)?;
        let schema_version = snapshot.schema_version()?;
        let quick_check = snapshot.integrity_check()?;
        if !quick_check.eq_ignore_ascii_case("ok") {
            bail!("snapshot quick_check failed: {quick_check}");
        }
        let counts = snapshot.table_counts()?;
        let fingerprint = snapshot.fingerprint()?;
        drop(snapshot);

        fs::set_permissions(&snapshot_db, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot set permissions on {}", snapshot_db.display()))?;
        let sha256 = crate::sha256::file_digest(&snapshot_db)?;
        let size_bytes = fs::metadata(&snapshot_db)
            .with_context(|| format!("cannot stat {}", snapshot_db.display()))?
            .len();
        sync_file(&snapshot_db)?;

        let config_sha256 = if include_config_hash {
            let config = crate::config::default_config_path();
            if config.is_file() {
                Some(crate::sha256::file_digest(&config)?)
            } else {
                None
            }
        } else {
            None
        };

        let manifest = DatabaseSnapshotManifest {
            version: DATABASE_SNAPSHOT_VERSION,
            created_at: timeutil::rfc3339(timeutil::now().timestamp()),
            source_db: source_db.display().to_string(),
            snapshot_db: snapshot_db.display().to_string(),
            sha256,
            size_bytes,
            schema_version,
            quick_check,
            counts,
            fingerprint,
            config_sha256,
        };
        write_private_json(&manifest_path, &manifest)?;
        sync_dir(to)?;
        Ok(manifest)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(to);
    }
    result
}

/// Result of comparing the live database with a pre-update snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataCheckReport {
    pub ok: bool,
    pub expected_schema_version: i64,
    pub actual_schema_version: i64,
    pub expected_fingerprint: DataFingerprint,
    pub actual_fingerprint: DataFingerprint,
    pub expected_quick_check: String,
    pub actual_quick_check: String,
    pub allow_migration: bool,
    pub differences: Vec<String>,
}

/// Compare the live database with the fingerprint recorded in a snapshot
/// manifest. This fails closed: any unexpected count, second, or app-key
/// difference is an update failure.
pub fn check_database_against_snapshot(
    live_db: &Path,
    snapshot_dir: &Path,
    allow_migration: bool,
) -> Result<DataCheckReport> {
    let expected = read_database_snapshot_manifest(&snapshot_dir.join("manifest.json"))?;
    let actual_db = Database::open_read(live_db)?;
    let actual_fingerprint = actual_db.fingerprint()?;
    let actual_schema_version = actual_db.schema_version()?;
    let actual_quick_check = actual_db.integrity_check()?;
    let mut differences = Vec::new();

    if !actual_quick_check.eq_ignore_ascii_case("ok") {
        differences.push(format!("quick_check: {actual_quick_check}"));
    }
    if actual_schema_version != expected.schema_version && !allow_migration {
        differences.push(format!(
            "schema_version: expected {}, got {}",
            expected.schema_version, actual_schema_version
        ));
    }
    if actual_fingerprint != expected.fingerprint && !allow_migration {
        for (table, expected_count) in &expected.fingerprint.counts {
            let actual_count = actual_fingerprint.counts.get(table).copied().unwrap_or(0);
            if actual_count != *expected_count {
                differences.push(format!(
                    "count {table}: expected {expected_count}, got {actual_count}"
                ));
            }
        }
        if actual_fingerprint.session_seconds != expected.fingerprint.session_seconds {
            differences.push(format!(
                "session seconds: expected {}, got {}",
                expected.fingerprint.session_seconds, actual_fingerprint.session_seconds
            ));
        }
        if actual_fingerprint.app_key_hash != expected.fingerprint.app_key_hash {
            differences.push(format!(
                "app keys: expected {}, got {}",
                expected.fingerprint.app_key_hash, actual_fingerprint.app_key_hash
            ));
        }
    }
    Ok(DataCheckReport {
        ok: differences.is_empty(),
        expected_schema_version: expected.schema_version,
        actual_schema_version,
        expected_fingerprint: expected.fingerprint,
        actual_fingerprint,
        expected_quick_check: expected.quick_check,
        actual_quick_check,
        allow_migration,
        differences,
    })
}

pub fn read_database_snapshot_manifest(path: &Path) -> Result<DatabaseSnapshotManifest> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let manifest: DatabaseSnapshotManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("cannot parse {}", path.display()))?;
    if manifest.version != DATABASE_SNAPSHOT_VERSION {
        bail!(
            "unsupported snapshot manifest version {} (expected {})",
            manifest.version,
            DATABASE_SNAPSHOT_VERSION
        );
    }
    Ok(manifest)
}

/// Verify that a snapshot file still matches its manifest. Returns the
/// manifest on success.
pub fn verify_database_snapshot(path: &Path) -> Result<DatabaseSnapshotManifest> {
    let manifest = read_database_snapshot_manifest(&path.join("manifest.json"))?;
    let db_path = path.join("screen-time.db");
    let actual = crate::sha256::file_digest(&db_path)?;
    if actual != manifest.sha256 {
        bail!(
            "snapshot {} checksum mismatch: expected {}, got {}",
            db_path.display(),
            manifest.sha256,
            actual
        );
    }
    let size = fs::metadata(&db_path)
        .with_context(|| format!("cannot stat {}", db_path.display()))?
        .len();
    if size != manifest.size_bytes {
        bail!(
            "snapshot {} size mismatch: expected {}, got {}",
            db_path.display(),
            manifest.size_bytes,
            size
        );
    }
    Ok(manifest)
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("cannot serialize manifest")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write {}", path.display()))?;
    file.write_all(b"\n")?;
    file.sync_all()
        .with_context(|| format!("cannot fsync {}", path.display()))?;
    Ok(())
}

fn sync_file(path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot fsync {}", path.display()))?;
    Ok(())
}

fn sync_dir(path: &Path) -> Result<()> {
    let dir = fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    dir.sync_all()
        .with_context(|| format!("cannot fsync {}", path.display()))?;
    Ok(())
}

pub fn backup_usage(root: &Path) -> (u64, usize) {
    let mut bytes = 0u64;
    let mut count = 0usize;
    let Ok(entries) = fs::read_dir(root) else {
        return (bytes, count);
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            count += 1;
            bytes += directory_size(&path);
        }
    }
    (bytes, count)
}

pub fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return total;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.metadata() {
            Ok(metadata) if metadata.is_dir() => total += directory_size(&path),
            Ok(metadata) => total += metadata.len(),
            Err(_) => {}
        }
    }
    total
}

pub fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
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
    fn database_snapshot_round_trip_and_refuses_overwrite() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("live.db");
        {
            let mut db = Database::open_write(&db_path, None).unwrap();
            let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
            let (start, _) = timeutil::day_bounds(date);
            let session = crate::sessions::Session::new(
                "Code",
                "2026-11-21".to_owned(),
                start + 10,
                start + 70,
            )
            .unwrap();
            db.commit_batch(&[], &[session], start + 70).unwrap();
        }
        let to = dir.path().join("snapshot");
        let manifest = create_database_snapshot(&db_path, &to, false).unwrap();
        assert_eq!(manifest.counts["sessions"], 1);
        assert_eq!(manifest.fingerprint.session_seconds, 60);
        assert_eq!(manifest.quick_check, "ok");
        assert_eq!(
            manifest.sha256,
            crate::sha256::file_digest(&to.join("screen-time.db")).unwrap()
        );
        assert!(verify_database_snapshot(&to).is_ok());
        assert!(create_database_snapshot(&db_path, &to, false).is_err());

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&to).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let mode = std::fs::metadata(to.join("manifest.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let mode = std::fs::metadata(to.join("screen-time.db"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
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
