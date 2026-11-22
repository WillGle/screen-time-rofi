use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::sessions;

pub const CURRENT_VERSION: i64 = 2;

const V1_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS apps (
    id INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    first_seen_ts INTEGER NOT NULL,
    last_seen_ts INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    ts INTEGER NOT NULL,
    kind TEXT NOT NULL,
    app TEXT,
    state TEXT,
    boot_id TEXT NOT NULL DEFAULT 'unknown'
);
CREATE INDEX IF NOT EXISTS events_ts ON events(ts);
CREATE INDEX IF NOT EXISTS events_kind_ts ON events(kind, ts);

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    date TEXT NOT NULL,
    app TEXT NOT NULL,
    start_ts INTEGER NOT NULL,
    end_ts INTEGER NOT NULL,
    seconds INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'closed',
    confidence TEXT NOT NULL DEFAULT 'high'
);
CREATE INDEX IF NOT EXISTS sessions_date_app_start
    ON sessions(date, app, start_ts);
CREATE INDEX IF NOT EXISTS sessions_date_start
    ON sessions(date, start_ts);

CREATE TABLE IF NOT EXISTS daily_app (
    date TEXT NOT NULL,
    app TEXT NOT NULL,
    seconds INTEGER NOT NULL DEFAULT 0,
    sessions INTEGER NOT NULL DEFAULT 0,
    first_ts INTEGER,
    last_ts INTEGER,
    PRIMARY KEY (date, app)
);

CREATE TABLE IF NOT EXISTS daily_slots (
    date TEXT NOT NULL,
    app TEXT NOT NULL,
    slot INTEGER NOT NULL,
    seconds INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app, slot)
);
CREATE INDEX IF NOT EXISTS daily_slots_date ON daily_slots(date);

CREATE TABLE IF NOT EXISTS power_sessions (
    id INTEGER PRIMARY KEY,
    start_ts INTEGER NOT NULL,
    end_ts INTEGER,
    seconds INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    confidence TEXT NOT NULL DEFAULT 'high',
    boot_id TEXT NOT NULL DEFAULT 'unknown'
);
CREATE INDEX IF NOT EXISTS power_sessions_start ON power_sessions(start_ts);

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("cannot read schema version")?;

    if version > CURRENT_VERSION {
        bail!(
            "database schema version {version} is newer than this binary (supports {CURRENT_VERSION})"
        );
    }
    if version == CURRENT_VERSION {
        return Ok(());
    }

    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("cannot start migration transaction")?;
    if version == 0 {
        tx.execute_batch(V1_SCHEMA)
            .context("cannot create schema version 1")?;
    }
    if version < 2 {
        canonicalize_app_keys(&tx).context("cannot canonicalize app keys")?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {CURRENT_VERSION};"))
        .context("cannot set schema version")?;
    tx.commit().context("cannot commit migration")?;
    Ok(())
}

/// v2: collapse legacy WM_CLASS keys into the same readable keys the current
/// capture path stores (`Cs2` -> `CS2`, `Xdg-desktop-portal-gtk` -> `Portal`,
/// `.blueman-manager-wrapped` -> `Blueman Manager`). This runs inside the
/// migration transaction and keeps a pre-migration backup.
fn canonicalize_app_keys(tx: &Transaction<'_>) -> Result<()> {
    let mut keys = BTreeSet::new();
    for table in ["apps", "sessions", "daily_app", "daily_slots"] {
        let sql = match table {
            "apps" => "SELECT DISTINCT key FROM apps",
            _ => match table {
                "sessions" => "SELECT DISTINCT app FROM sessions",
                "daily_app" => "SELECT DISTINCT app FROM daily_app",
                _ => "SELECT DISTINCT app FROM daily_slots",
            },
        };
        let mut statement = tx.prepare(sql)?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            keys.insert(row?);
        }
    }
    let mut statement = tx.prepare("SELECT DISTINCT app FROM events WHERE app IS NOT NULL")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        keys.insert(row?);
    }

    for old in keys {
        let Some(new) = sessions::normalize_app(&old) else {
            continue;
        };
        if new != old {
            rename_app(tx, &old, &new)
                .with_context(|| format!("cannot rename app {old:?} to {new:?}"))?;
        }
    }
    Ok(())
}

type DailyRow = (String, i64, i64, Option<i64>, Option<i64>);

fn rename_app(tx: &Transaction<'_>, old: &str, new: &str) -> Result<()> {
    tx.execute(
        "UPDATE events SET app = ?1 WHERE app = ?2",
        params![new, old],
    )?;
    tx.execute(
        "UPDATE sessions SET app = ?1 WHERE app = ?2",
        params![new, old],
    )?;

    if let Some((first, last)) = tx
        .query_row(
            "SELECT first_seen_ts, last_seen_ts FROM apps WHERE key = ?1",
            params![old],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
    {
        tx.execute(
            "INSERT INTO apps(key, first_seen_ts, last_seen_ts) VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                 first_seen_ts = MIN(apps.first_seen_ts, excluded.first_seen_ts),
                 last_seen_ts = MAX(apps.last_seen_ts, excluded.last_seen_ts)",
            params![new, first, last],
        )?;
        tx.execute("DELETE FROM apps WHERE key = ?1", params![old])?;
    }

    let daily_rows: Vec<DailyRow> = {
        let mut statement = tx.prepare(
            "SELECT date, seconds, sessions, first_ts, last_ts
             FROM daily_app WHERE app = ?1",
        )?;
        let rows = statement.query_map(params![old], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (date, seconds, session_count, first, last) in daily_rows {
        tx.execute(
            "INSERT INTO daily_app(date, app, seconds, sessions, first_ts, last_ts)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(date, app) DO UPDATE SET
                 seconds = daily_app.seconds + excluded.seconds,
                 sessions = daily_app.sessions + excluded.sessions,
                 first_ts = CASE
                     WHEN daily_app.first_ts IS NULL THEN excluded.first_ts
                     WHEN excluded.first_ts IS NULL THEN daily_app.first_ts
                     ELSE MIN(daily_app.first_ts, excluded.first_ts)
                 END,
                 last_ts = CASE
                     WHEN daily_app.last_ts IS NULL THEN excluded.last_ts
                     WHEN excluded.last_ts IS NULL THEN daily_app.last_ts
                     ELSE MAX(daily_app.last_ts, excluded.last_ts)
                 END",
            params![date, new, seconds, session_count, first, last],
        )?;
    }
    tx.execute("DELETE FROM daily_app WHERE app = ?1", params![old])?;

    let slot_rows: Vec<(String, i64, i64)> = {
        let mut statement =
            tx.prepare("SELECT date, slot, seconds FROM daily_slots WHERE app = ?1")?;
        let rows = statement.query_map(params![old], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (date, slot, seconds) in slot_rows {
        tx.execute(
            "INSERT INTO daily_slots(date, app, slot, seconds)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(date, app, slot) DO UPDATE SET
                 seconds = daily_slots.seconds + excluded.seconds",
            params![date, new, slot, seconds],
        )?;
    }
    tx.execute("DELETE FROM daily_slots WHERE app = ?1", params![old])?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), CURRENT_VERSION);
        migrate(&mut conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), CURRENT_VERSION);
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        assert!(migrate(&mut conn).is_err());
    }

    #[test]
    fn v2_canonicalizes_and_merges_legacy_app_keys() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1_SCHEMA).unwrap();
        conn.execute_batch("PRAGMA user_version = 1;").unwrap();
        conn.execute_batch(
            "INSERT INTO apps(key, first_seen_ts, last_seen_ts) VALUES
                 ('Cs2', 100, 160),
                 ('CS2', 200, 230),
                 ('.blueman-manager-wrapped', 5, 6);
             INSERT INTO sessions(date, app, start_ts, end_ts, seconds)
                 VALUES('2026-11-21', 'Cs2', 100, 160, 60),
                       ('2026-11-21', 'CS2', 200, 230, 30),
                       ('2026-11-21', '.blueman-manager-wrapped', 5, 6, 1);
             INSERT INTO daily_app(date, app, seconds, sessions, first_ts, last_ts)
                 VALUES('2026-11-21', 'Cs2', 60, 1, 100, 160),
                       ('2026-11-21', 'CS2', 30, 1, 200, 230);
             INSERT INTO daily_slots(date, app, slot, seconds)
                 VALUES('2026-11-21', 'Cs2', 1, 60),
                       ('2026-11-21', 'CS2', 1, 30);
             INSERT INTO events(ts, kind, app) VALUES(100, 'focus', 'Cs2');",
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), CURRENT_VERSION);

        let apps: Vec<(String, i64, i64)> = {
            let mut statement = conn
                .prepare("SELECT key, first_seen_ts, last_seen_ts FROM apps ORDER BY key")
                .unwrap();
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        assert_eq!(
            apps,
            vec![
                ("Blueman Manager".to_owned(), 5, 6),
                ("CS2".to_owned(), 100, 230)
            ]
        );

        let sessions: Vec<(String, String)> = {
            let mut statement = conn
                .prepare("SELECT app, date FROM sessions ORDER BY start_ts")
                .unwrap();
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        assert_eq!(
            sessions,
            vec![
                ("Blueman Manager".to_owned(), "2026-11-21".to_owned()),
                ("CS2".to_owned(), "2026-11-21".to_owned()),
                ("CS2".to_owned(), "2026-11-21".to_owned()),
            ]
        );

        let daily: (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT seconds, sessions, first_ts, last_ts
                 FROM daily_app WHERE date = '2026-11-21' AND app = 'CS2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(daily, (90, 2, 100, 230));

        let slot_seconds: i64 = conn
            .query_row(
                "SELECT seconds FROM daily_slots
                 WHERE date = '2026-11-21' AND app = 'CS2' AND slot = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(slot_seconds, 90);

        let event_app: String = conn
            .query_row("SELECT app FROM events WHERE ts = 100", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(event_app, "CS2");
    }
}
