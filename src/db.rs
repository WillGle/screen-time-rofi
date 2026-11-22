use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::config::RetentionConfig;
use crate::migrations::{self, CURRENT_VERSION};
use crate::sessions::Session;
use crate::timeutil::{self, SLOT_COUNT};

#[derive(Debug, Clone)]
pub struct EventRecord {
    pub ts: i64,
    pub kind: String,
    pub app: Option<String>,
    pub state: Option<String>,
    pub boot_id: String,
}

#[derive(Debug, Clone)]
pub struct PowerSession {
    pub id: i64,
    pub start_ts: i64,
    pub end_ts: Option<i64>,
    pub boot_id: String,
}

#[derive(Debug, Clone)]
pub struct DayApp {
    pub seconds: i64,
    pub sessions: i64,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    pub slots: [i64; SLOT_COUNT],
}

impl Default for DayApp {
    fn default() -> Self {
        Self {
            seconds: 0,
            sessions: 0,
            first_ts: None,
            last_ts: None,
            slots: [0; SLOT_COUNT],
        }
    }
}

#[derive(Debug, Clone)]
pub struct DayData {
    pub date: NaiveDate,
    pub apps: BTreeMap<String, DayApp>,
    pub slots: [i64; SLOT_COUNT],
    pub powered_seconds: i64,
    pub power_slots: [i64; SLOT_COUNT],
}

impl DayData {
    pub fn new(date: NaiveDate) -> Self {
        Self {
            date,
            apps: BTreeMap::new(),
            slots: [0; SLOT_COUNT],
            powered_seconds: 0,
            power_slots: [0; SLOT_COUNT],
        }
    }

    pub fn active_seconds(&self) -> i64 {
        self.apps.values().map(|app| app.seconds.max(0)).sum()
    }

    pub fn first_active_ts(&self) -> Option<i64> {
        self.apps.values().filter_map(|app| app.first_ts).min()
    }

    pub fn last_active_ts(&self) -> Option<i64> {
        self.apps.values().filter_map(|app| app.last_ts).max()
    }
}

#[derive(Debug, Clone)]
pub struct DailyTotal {
    pub date: NaiveDate,
    pub seconds: i64,
}

/// Resolved retention policy. `None` means keep that category forever.
/// This is the only thing `Database::retention` consults; an all-`None`
/// policy performs no SQL at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub events_days: Option<u32>,
    pub sessions_days: Option<u32>,
    pub aggregates_days: Option<u32>,
    pub power_days: Option<u32>,
}

impl RetentionPolicy {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn from_config(config: &RetentionConfig) -> Self {
        Self {
            events_days: config.events_days.filter(|days| *days > 0),
            sessions_days: config.sessions_days.filter(|days| *days > 0),
            aggregates_days: config.aggregates_days.filter(|days| *days > 0),
            power_days: config.power_days.filter(|days| *days > 0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.events_days.is_some()
            || self.sessions_days.is_some()
            || self.aggregates_days.is_some()
            || self.power_days.is_some()
    }

    pub fn summary(&self) -> String {
        if !self.is_enabled() {
            return "disabled".to_owned();
        }
        [
            ("events", self.events_days),
            ("sessions", self.sessions_days),
            ("aggregates", self.aggregates_days),
            ("power", self.power_days),
        ]
        .iter()
        .filter_map(|(name, days)| days.map(|days| format!("{name}={days}d")))
        .collect::<Vec<_>>()
        .join(",")
    }
}

/// Stable summary used to prove that an update did not lose data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataFingerprint {
    pub counts: BTreeMap<String, i64>,
    pub session_seconds: i64,
    pub app_keys: Vec<String>,
    pub app_key_hash: String,
}

pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open_write(path: &Path, backup_dir: Option<&Path>) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("cannot create database directory {}", parent.display())
            })?;
        }
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .with_context(|| format!("cannot open database {}", path.display()))?;
        configure_write(&conn)?;

        let version = migrations::schema_version(&conn)?;
        if version > 0 && version < CURRENT_VERSION {
            let backup_dir = backup_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).join("backups"));
            create_migration_backup(&conn, &backup_dir, version)?;
        }
        migrations::migrate(&mut conn)?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Open an existing database for `VACUUM INTO` snapshots. SQLite
    /// classifies `VACUUM INTO` as a write operation, so a read-only handle
    /// cannot be used; no statement other than the snapshot is executed.
    pub fn open_snapshot_source(path: &Path) -> Result<Self> {
        if !path.is_file() {
            bail!("database {} does not exist", path.display());
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .with_context(|| format!("cannot open database {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
        let version = migrations::schema_version(&conn)?;
        if version == 0 {
            bail!("database {} has no schema yet", path.display());
        }
        if version > CURRENT_VERSION {
            bail!(
                "database schema version {version} is newer than this binary (supports {CURRENT_VERSION})"
            );
        }
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn open_read(path: &Path) -> Result<Self> {
        if !path.is_file() {
            bail!("database {} does not exist", path.display());
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("cannot open database {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(2_000))?;
        conn.pragma_update(None, "query_only", "ON")?;
        let version = migrations::schema_version(&conn)?;
        if version == 0 {
            bail!("database {} has no schema yet", path.display());
        }
        if version > CURRENT_VERSION {
            bail!(
                "database schema version {version} is newer than this binary (supports {CURRENT_VERSION})"
            );
        }
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn commit_batch(
        &mut self,
        events: &[EventRecord],
        sessions: &[Session],
        last_commit_ts: i64,
    ) -> Result<()> {
        if events.is_empty() && sessions.is_empty() {
            self.set_meta("last_commit_at", &timeutil::rfc3339(last_commit_ts))?;
            return Ok(());
        }
        let tx = self
            .conn
            .transaction()
            .context("cannot start batch transaction")?;
        for event in events {
            insert_event(&tx, event)?;
            if let Some(app) = &event.app {
                touch_app(&tx, app, event.ts)?;
            }
        }
        for session in sessions {
            insert_session(&tx, session)?;
            touch_app(&tx, &session.app, session.end_ts)?;
        }
        tx.execute(
            "INSERT INTO meta(key, value) VALUES('last_commit_at', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![timeutil::rfc3339(last_commit_ts)],
        )?;
        tx.commit().context("cannot commit batch")?;
        Ok(())
    }

    pub fn load_day(&self, date: NaiveDate, now_ts: i64) -> Result<DayData> {
        self.load_day_internal(date, now_ts, true)
    }

    /// Load closed power sessions only. The daemon adds its live power
    /// interval separately, avoiding double counting.
    pub fn load_day_closed(&self, date: NaiveDate, now_ts: i64) -> Result<DayData> {
        self.load_day_internal(date, now_ts, false)
    }

    fn load_day_internal(
        &self,
        date: NaiveDate,
        now_ts: i64,
        include_open_power: bool,
    ) -> Result<DayData> {
        let mut data = DayData::new(date);
        let date_key = date.format("%Y-%m-%d").to_string();

        let mut statement = self.conn.prepare(
            "SELECT app, seconds, sessions, first_ts, last_ts
             FROM daily_app WHERE date = ?1 ORDER BY app",
        )?;
        let rows = statement.query_map(params![date_key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                DayApp {
                    seconds: row.get(1)?,
                    sessions: row.get(2)?,
                    first_ts: row.get(3)?,
                    last_ts: row.get(4)?,
                    slots: [0; SLOT_COUNT],
                },
            ))
        })?;
        for row in rows {
            let (app, app_data) = row?;
            data.apps.insert(app, app_data);
        }

        let mut statement = self.conn.prepare(
            "SELECT app, slot, seconds FROM daily_slots WHERE date = ?1 ORDER BY app, slot",
        )?;
        let rows = statement.query_map(params![date_key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (app, slot, seconds) = row?;
            if !(0..SLOT_COUNT as i64).contains(&slot) {
                continue;
            }
            let slot = slot as usize;
            data.apps.entry(app).or_default().slots[slot] += seconds.max(0);
            data.slots[slot] += seconds.max(0);
        }

        let (day_start, day_end) = timeutil::day_bounds(date);
        let sql = if include_open_power {
            "SELECT start_ts, end_ts FROM power_sessions
             WHERE start_ts < ?2 AND (end_ts IS NULL OR end_ts > ?1)
             ORDER BY start_ts"
        } else {
            "SELECT start_ts, end_ts FROM power_sessions
             WHERE start_ts < ?2 AND end_ts IS NOT NULL AND end_ts > ?1
             ORDER BY start_ts"
        };
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params![day_start, day_end], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        })?;
        for row in rows {
            let (start, end) = row?;
            let end = end.unwrap_or(now_ts);
            let clipped_start = start.max(day_start);
            let clipped_end = end.min(day_end);
            if clipped_end <= clipped_start {
                continue;
            }
            data.powered_seconds += clipped_end - clipped_start;
            timeutil::add_interval_to_slots(
                &mut data.power_slots,
                date,
                clipped_start,
                clipped_end,
            );
        }

        Ok(data)
    }

    pub fn load_app_sessions(&self, date: NaiveDate, app: &str) -> Result<Vec<Session>> {
        let date_key = date.format("%Y-%m-%d").to_string();
        let mut statement = self.conn.prepare(
            "SELECT date, app, start_ts, end_ts, seconds, state, confidence
             FROM sessions WHERE date = ?1 AND app = ?2 ORDER BY start_ts",
        )?;
        let rows = statement.query_map(params![date_key, app], |row| {
            Ok(Session {
                date: row.get(0)?,
                app: row.get(1)?,
                start_ts: row.get(2)?,
                end_ts: row.get(3)?,
                seconds: row.get(4)?,
                state: row.get(5)?,
                confidence: row.get(6)?,
            })
        })?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    pub fn daily_totals(&self, end_date: NaiveDate, days: usize) -> Result<Vec<DailyTotal>> {
        let start = end_date
            .checked_sub_days(chrono::Days::new((days.saturating_sub(1)) as u64))
            .unwrap_or(end_date);
        let mut statement = self.conn.prepare(
            "SELECT date, SUM(seconds) FROM daily_app
             WHERE date >= ?1 AND date <= ?2
             GROUP BY date ORDER BY date",
        )?;
        let rows = statement.query_map(
            params![
                start.format("%Y-%m-%d").to_string(),
                end_date.format("%Y-%m-%d").to_string()
            ],
            |row| {
                let date: String = row.get(0)?;
                let seconds: i64 = row.get(1)?;
                Ok((date, seconds))
            },
        )?;
        let mut totals = Vec::new();
        for row in rows {
            let (date, seconds) = row?;
            if let Ok(parsed) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                totals.push(DailyTotal {
                    date: parsed,
                    seconds,
                });
            }
        }
        Ok(totals)
    }

    pub fn app_daily_totals(
        &self,
        app: &str,
        end_date: NaiveDate,
        days: usize,
    ) -> Result<Vec<DailyTotal>> {
        let start = end_date
            .checked_sub_days(chrono::Days::new((days.saturating_sub(1)) as u64))
            .unwrap_or(end_date);
        let mut statement = self.conn.prepare(
            "SELECT date, seconds FROM daily_app
             WHERE app = ?1 AND date >= ?2 AND date <= ?3
             ORDER BY date",
        )?;
        let rows = statement.query_map(
            params![
                app,
                start.format("%Y-%m-%d").to_string(),
                end_date.format("%Y-%m-%d").to_string()
            ],
            |row| {
                let date: String = row.get(0)?;
                let seconds: i64 = row.get(1)?;
                Ok((date, seconds))
            },
        )?;
        let mut totals = Vec::new();
        for row in rows {
            let (date, seconds) = row?;
            if let Ok(parsed) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                totals.push(DailyTotal {
                    date: parsed,
                    seconds,
                });
            }
        }
        Ok(totals)
    }

    /// Seconds recorded before `slot` on the given date. Used by the
    /// same-clock-time comparison.
    pub fn seconds_before_slot(&self, date: NaiveDate, slot: usize) -> Result<i64> {
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(seconds), 0) FROM daily_slots WHERE date = ?1 AND slot < ?2",
            params![date.format("%Y-%m-%d").to_string(), slot as i64],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    pub fn last_event_ts(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT MAX(ts) FROM events", [], |row| row.get(0))
            .optional()?
            .flatten())
    }

    pub fn open_power_session(&self) -> Result<Option<PowerSession>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, start_ts, end_ts, boot_id FROM power_sessions
                 WHERE end_ts IS NULL ORDER BY start_ts DESC LIMIT 1",
                [],
                |row| {
                    Ok(PowerSession {
                        id: row.get(0)?,
                        start_ts: row.get(1)?,
                        end_ts: row.get(2)?,
                        boot_id: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn start_power_session(&mut self, start_ts: i64, boot_id: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO power_sessions(start_ts, end_ts, seconds, reason, confidence, boot_id)
             VALUES(?1, NULL, 0, NULL, 'high', ?2)",
            params![start_ts, boot_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn close_power_session(&mut self, id: i64, end_ts: i64, reason: &str) -> Result<()> {
        let start_ts: i64 = self
            .conn
            .query_row(
                "SELECT start_ts FROM power_sessions WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .context("power session disappeared")?;
        let seconds = (end_ts - start_ts).max(0);
        self.conn.execute(
            "UPDATE power_sessions SET end_ts = ?1, seconds = ?2, reason = ?3 WHERE id = ?4",
            params![end_ts, seconds, reason, id],
        )?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn last_commit_at(&self) -> Result<Option<String>> {
        self.meta("last_commit_at")
    }

    /// Apply only the categories enabled in `policy`. A disabled policy runs
    /// no `DELETE` statements and returns all-zero counts.
    pub fn retention(
        &mut self,
        policy: &RetentionPolicy,
        now_date: NaiveDate,
        now_ts: i64,
    ) -> Result<RetentionResult> {
        if !policy.is_enabled() {
            return Ok(RetentionResult::default());
        }
        let date_cutoff = |days: u32| {
            now_date
                .checked_sub_days(chrono::Days::new(days as u64))
                .unwrap_or(now_date)
                .format("%Y-%m-%d")
                .to_string()
        };
        let ts_cutoff = |days: u32| now_ts - i64::from(days) * 86_400;

        let tx = self.conn.transaction()?;
        let events = match policy.events_days {
            Some(days) => {
                tx.execute("DELETE FROM events WHERE ts < ?1", params![ts_cutoff(days)])?
            }
            None => 0,
        };
        let sessions = match policy.sessions_days {
            Some(days) => tx.execute(
                "DELETE FROM sessions WHERE date < ?1",
                params![date_cutoff(days)],
            )?,
            None => 0,
        };
        let (daily_app, daily_slots) = match policy.aggregates_days {
            Some(days) => {
                let cutoff = date_cutoff(days);
                let daily_app =
                    tx.execute("DELETE FROM daily_app WHERE date < ?1", params![cutoff])?;
                let daily_slots =
                    tx.execute("DELETE FROM daily_slots WHERE date < ?1", params![cutoff])?;
                (daily_app, daily_slots)
            }
            None => (0, 0),
        };
        let power = match policy.power_days {
            Some(days) => tx.execute(
                "DELETE FROM power_sessions
                 WHERE end_ts IS NOT NULL AND end_ts < ?1",
                params![ts_cutoff(days)],
            )?,
            None => 0,
        };
        tx.commit()?;
        Ok(RetentionResult {
            events,
            sessions,
            daily_app,
            daily_slots,
            power_sessions: power,
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        migrations::schema_version(&self.conn)
    }

    /// Row counts for every data table, in a stable order.
    pub fn table_counts(&self) -> Result<BTreeMap<String, i64>> {
        let mut counts = BTreeMap::new();
        for table in [
            "apps",
            "events",
            "sessions",
            "daily_app",
            "daily_slots",
            "power_sessions",
            "meta",
        ] {
            let count: i64 =
                self.conn
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })?;
            counts.insert(table.to_owned(), count);
        }
        Ok(counts)
    }

    pub fn total_session_seconds(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(seconds), 0) FROM sessions",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn app_keys(&self) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare("SELECT key FROM apps ORDER BY key")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut keys = Vec::new();
        for row in rows {
            keys.push(row?);
        }
        Ok(keys)
    }

    pub fn fingerprint(&self) -> Result<DataFingerprint> {
        let counts = self.table_counts()?;
        let session_seconds = self.total_session_seconds()?;
        let app_keys = self.app_keys()?;
        let app_key_hash = crate::sha256::hex_digest(app_keys.join("\n").as_bytes());
        Ok(DataFingerprint {
            counts,
            session_seconds,
            app_keys,
            app_key_hash,
        })
    }

    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .context("cannot checkpoint WAL")?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?)
    }

    pub fn journal_mode(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }

    pub fn counts(&self) -> Result<(i64, i64)> {
        let events: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        let sessions: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        Ok((events, sessions))
    }
}

#[derive(Debug, Clone, Default)]
pub struct RetentionResult {
    pub events: usize,
    pub sessions: usize,
    pub daily_app: usize,
    pub daily_slots: usize,
    pub power_sessions: usize,
}

fn configure_write(conn: &Connection) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(5_000))?;
    let journal: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        bail!("SQLite refused WAL mode (journal_mode={journal})");
    }
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA wal_autocheckpoint = 1000;",
    )?;
    Ok(())
}

fn create_migration_backup(conn: &Connection, backup_dir: &Path, version: i64) -> Result<PathBuf> {
    fs::create_dir_all(backup_dir).with_context(|| {
        format!(
            "cannot create migration backup directory {}",
            backup_dir.display()
        )
    })?;
    fs::set_permissions(backup_dir, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("cannot set permissions on {}", backup_dir.display()))?;
    if let Some(parent) = backup_dir.parent() {
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = backup_dir.join(format!("pre-v{version}-{stamp}.db"));
    let path_string = path.to_string_lossy().to_string();
    conn.execute("VACUUM INTO ?1", params![path_string])
        .with_context(|| format!("cannot create pre-migration backup {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("cannot set permissions on {}", path.display()))?;
    Ok(path)
}

fn insert_event(tx: &Transaction<'_>, event: &EventRecord) -> Result<()> {
    tx.execute(
        "INSERT INTO events(ts, kind, app, state, boot_id) VALUES(?1, ?2, ?3, ?4, ?5)",
        params![event.ts, event.kind, event.app, event.state, event.boot_id],
    )?;
    Ok(())
}

fn touch_app(tx: &Transaction<'_>, app: &str, ts: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO apps(key, first_seen_ts, last_seen_ts) VALUES(?1, ?2, ?2)
         ON CONFLICT(key) DO UPDATE SET
             first_seen_ts = MIN(apps.first_seen_ts, excluded.first_seen_ts),
             last_seen_ts = MAX(apps.last_seen_ts, excluded.last_seen_ts)",
        params![app, ts],
    )?;
    Ok(())
}

fn insert_session(tx: &Transaction<'_>, session: &Session) -> Result<()> {
    tx.execute(
        "INSERT INTO sessions(date, app, start_ts, end_ts, seconds, state, confidence)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session.date,
            session.app,
            session.start_ts,
            session.end_ts,
            session.seconds,
            session.state,
            session.confidence
        ],
    )?;

    tx.execute(
        "INSERT INTO daily_app(date, app, seconds, sessions, first_ts, last_ts)
         VALUES(?1, ?2, ?3, 1, ?4, ?5)
         ON CONFLICT(date, app) DO UPDATE SET
             seconds = daily_app.seconds + excluded.seconds,
             sessions = daily_app.sessions + 1,
             first_ts = MIN(COALESCE(daily_app.first_ts, excluded.first_ts), excluded.first_ts),
             last_ts = MAX(COALESCE(daily_app.last_ts, excluded.last_ts), excluded.last_ts)",
        params![
            session.date,
            session.app,
            session.seconds,
            session.start_ts,
            session.end_ts
        ],
    )?;

    let date = NaiveDate::parse_from_str(&session.date, "%Y-%m-%d")
        .with_context(|| format!("invalid session date {}", session.date))?;
    let mut slots = [0i64; SLOT_COUNT];
    timeutil::add_interval_to_slots(&mut slots, date, session.start_ts, session.end_ts);
    for (index, seconds) in slots.iter().enumerate() {
        if *seconds <= 0 {
            continue;
        }
        tx.execute(
            "INSERT INTO daily_slots(date, app, slot, seconds)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(date, app, slot) DO UPDATE SET
                 seconds = daily_slots.seconds + excluded.seconds",
            params![session.date, session.app, index as i64, seconds],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::migrate;
    use tempfile::tempdir;

    fn open_test_db() -> (tempfile::TempDir, Database) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let db = Database::open_write(&path, None).unwrap();
        (dir, db)
    }

    #[test]
    fn batch_commit_is_one_transaction() {
        let (_dir, mut db) = open_test_db();
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, _) = timeutil::day_bounds(date);
        let events = vec![EventRecord {
            ts: day_start + 1_000,
            kind: "focus".to_owned(),
            app: Some("Code".to_owned()),
            state: Some("begin".to_owned()),
            boot_id: "boot".to_owned(),
        }];
        let sessions = vec![
            Session::new(
                "Code",
                "2026-11-21".to_owned(),
                day_start + 1_000,
                day_start + 2_000,
            )
            .unwrap(),
        ];
        db.commit_batch(&events, &sessions, day_start + 2_000)
            .unwrap();
        let (events, sessions) = db.counts().unwrap();
        assert_eq!(events, 1);
        assert_eq!(sessions, 1);
        assert!(db.last_commit_at().unwrap().is_some());
        let day = db.load_day(date, day_start + 3_000).unwrap();
        assert_eq!(day.apps["Code"].seconds, 1_000);
        assert_eq!(day.apps["Code"].sessions, 1);
        assert_eq!(day.slots.iter().sum::<i64>(), 1_000);
    }

    #[test]
    fn retention_removes_only_old_rows() {
        let (_dir, mut db) = open_test_db();
        let old = timeutil::now().timestamp() - 40 * 86_400;
        let recent = timeutil::now().timestamp();
        let old_date = timeutil::date_of(old).format("%Y-%m-%d").to_string();
        let recent_date = timeutil::date_of(recent).format("%Y-%m-%d").to_string();
        db.conn
            .execute(
                "INSERT INTO events(ts, kind, boot_id) VALUES(?1, 'focus', 'b')",
                params![old],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO events(ts, kind, boot_id) VALUES(?1, 'focus', 'b')",
                params![recent],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO sessions(date, app, start_ts, end_ts, seconds)
                 VALUES(?1, 'Old', ?2, ?3, 60),
                        (?4, 'New', ?5, ?6, 60)",
                params![old_date, old, old + 60, recent_date, recent, recent + 60],
            )
            .unwrap();
        let policy = RetentionPolicy {
            events_days: Some(30),
            ..RetentionPolicy::disabled()
        };
        let result = db
            .retention(&policy, timeutil::today(timeutil::now()), recent)
            .unwrap();
        assert_eq!(result.events, 1);
        let remaining: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn disabled_retention_deletes_nothing() {
        let (_dir, mut db) = open_test_db();
        let now = timeutil::now().timestamp();
        let old = now - 900 * 86_400;
        let old_date = timeutil::date_of(old).format("%Y-%m-%d").to_string();
        db.conn
            .execute(
                "INSERT INTO events(ts, kind, boot_id) VALUES(?1, 'focus', 'b')",
                params![old],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO sessions(date, app, start_ts, end_ts, seconds)
                 VALUES(?1, 'Old', ?2, ?3, 60)",
                params![old_date, old, old + 60],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO daily_app(date, app, seconds, sessions) VALUES(?1, 'Old', 60, 1)",
                params![old_date],
            )
            .unwrap();
        let result = db
            .retention(
                &RetentionPolicy::disabled(),
                timeutil::today(timeutil::now()),
                now,
            )
            .unwrap();
        assert_eq!(result.events, 0);
        assert_eq!(result.sessions, 0);
        assert_eq!(result.daily_app, 0);
        assert_eq!(result.power_sessions, 0);
        assert_eq!(db.counts().unwrap(), (1, 1));
    }

    #[test]
    fn retention_categories_are_independent() {
        let (_dir, mut db) = open_test_db();
        let now = timeutil::now().timestamp();
        let old = now - 40 * 86_400;
        let old_date = timeutil::date_of(old).format("%Y-%m-%d").to_string();
        db.conn
            .execute(
                "INSERT INTO events(ts, kind, boot_id) VALUES(?1, 'focus', 'b')",
                params![old],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO sessions(date, app, start_ts, end_ts, seconds)
                 VALUES(?1, 'Old', ?2, ?3, 60)",
                params![old_date, old, old + 60],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO daily_app(date, app, seconds, sessions) VALUES(?1, 'Old', 60, 1)",
                params![old_date],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO daily_slots(date, app, slot, seconds) VALUES(?1, 'Old', 1, 60)",
                params![old_date],
            )
            .unwrap();
        let id = db.start_power_session(old, "old-boot").unwrap();
        db.close_power_session(id, old + 60, "shutdown").unwrap();

        let policy = RetentionPolicy {
            events_days: Some(30),
            ..RetentionPolicy::disabled()
        };
        let result = db
            .retention(&policy, timeutil::today(timeutil::now()), now)
            .unwrap();
        assert_eq!(result.events, 1);
        assert_eq!(result.sessions, 0);
        assert_eq!(result.daily_app, 0);
        assert_eq!(result.power_sessions, 0);
        assert_eq!(db.counts().unwrap(), (0, 1));
    }

    #[test]
    fn fingerprint_tracks_counts_seconds_and_app_keys() {
        let (_dir, mut db) = open_test_db();
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, _) = timeutil::day_bounds(date);
        let session = Session::new(
            "Code",
            "2026-11-21".to_owned(),
            day_start + 10,
            day_start + 70,
        )
        .unwrap();
        db.commit_batch(&[], &[session], day_start + 70).unwrap();
        let fingerprint = db.fingerprint().unwrap();
        assert_eq!(fingerprint.counts["sessions"], 1);
        assert_eq!(fingerprint.session_seconds, 60);
        assert_eq!(fingerprint.app_keys, vec!["Code".to_owned()]);
        assert_eq!(fingerprint.app_key_hash.len(), 64);
    }

    #[test]
    fn power_sessions_are_clipped_to_day() {
        let (_dir, mut db) = open_test_db();
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (start, end) = timeutil::day_bounds(date);
        let id = db.start_power_session(start - 3600, "boot").unwrap();
        db.close_power_session(id, end + 3600, "shutdown").unwrap();
        let day = db.load_day(date, end + 7200).unwrap();
        assert_eq!(day.powered_seconds, 86_400);
        assert_eq!(day.power_slots.iter().sum::<i64>(), 86_400);
    }

    #[test]
    fn schema_can_be_reopened() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            migrate(&mut conn).unwrap();
        }
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(migrations::schema_version(&conn).unwrap(), CURRENT_VERSION);
    }
}
