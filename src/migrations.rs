use anyhow::{Context, Result, bail};
use rusqlite::{Connection, TransactionBehavior};

pub const CURRENT_VERSION: i64 = 1;

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
    tx.execute_batch(&format!("PRAGMA user_version = {CURRENT_VERSION};"))
        .context("cannot set schema version")?;
    tx.commit().context("cannot commit migration")?;
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
}
