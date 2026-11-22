mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use common::{BOOT_ID, Fixture, NOW};
use screen_time::migrations::CURRENT_VERSION;
use serde_json::Value;

fn run(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    let config_home = fixture.config_home();
    fs::create_dir_all(&config_home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_screen-time"))
        .args(args)
        .env("TZ", "UTC")
        .env("HOME", fixture.dir.path())
        .env("XDG_CONFIG_HOME", config_home)
        .env("SCREEN_TIME_STATE_DIR", &fixture.paths.state_dir)
        .env("SCREEN_TIME_RUNTIME_DIR", &fixture.paths.runtime_dir)
        .env("SCREEN_TIME_NOW", NOW)
        .env("SCREEN_TIME_BOOT_ID", BOOT_ID)
        .env("SCREEN_TIME_IGNORE_PID", "1")
        .env_remove("ROFI_RETV")
        .env_remove("ROFI_INFO")
        .env_remove("ROFI_THEME")
        .output()
        .expect("cannot run screen-time")
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn doctor_json_has_a_stable_schema_and_no_runtime_mode() {
    let fixture = Fixture::new();
    let output = run(&fixture, &["doctor", "--json", "--no-runtime"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["ok"], true);
    assert_eq!(report["mode"], "no-runtime");
    assert_eq!(report["schema_version"], CURRENT_VERSION);
    assert_eq!(report["quick_check"], "ok");
    assert_eq!(report["retention"]["enabled"], false);
    assert_eq!(report["retention"]["summary"], "disabled");
    assert!(report["runtime"].is_null());
    assert!(
        report["runtime"].get("capture").is_none(),
        "capture is omitted with --no-runtime"
    );
    assert!(report["counts"]["sessions"].as_i64().unwrap() > 0);
    assert!(report["fingerprint"]["session_seconds"].as_i64().unwrap() > 0);
    assert!(
        report["fingerprint"]["app_key_hash"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(report["sizes"]["database_bytes"].as_u64().unwrap() > 0);
    assert!(report["sizes"]["update_backups_bytes"].is_u64());
    assert!(report["warnings"].is_array());
    assert!(report["failures"].is_array());
}

#[test]
fn doctor_full_runtime_reports_capture_liveness() {
    let fixture = Fixture::new();
    let output = run(&fixture, &["doctor", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["mode"], "full");
    // The fixture's live snapshot has no open interval, so no capture
    // mismatch can be confirmed even when the host has Hyprland.
    assert_eq!(report["runtime"]["capture"]["status"], "unknown");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|warning| !warning.as_str().unwrap().contains("not following Hyprland"))
    );
}

#[test]
fn doctor_json_reports_an_enabled_retention_policy() {
    let fixture = Fixture::new();
    fixture.write_config("[retention]\nevents_days = 1\nsessions_days = 0\n");
    let output = run(&fixture, &["doctor", "--json", "--no-runtime"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["retention"]["enabled"], true);
    assert_eq!(report["retention"]["events_days"], 1);
    assert_eq!(
        report["retention"]["sessions_days"],
        serde_json::Value::Null
    );
    assert_eq!(report["retention"]["summary"], "events=1d");
}

#[test]
fn doctor_json_falls_back_to_keep_all_on_invalid_config() {
    let fixture = Fixture::new();
    fixture.write_config("[retention\nevents_days = 1\n");
    let output = run(&fixture, &["doctor", "--json", "--no-runtime"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["retention"]["enabled"], false);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("invalid"))
    );
}

#[test]
fn doctor_json_reports_a_corrupt_database_as_failure() {
    let fixture = Fixture::new();
    fs::remove_file(&fixture.paths.db_path).unwrap();
    let _ = fs::remove_file(fixture.paths.db_path.with_extension("db-wal"));
    let _ = fs::remove_file(fixture.paths.db_path.with_extension("db-shm"));
    fs::write(&fixture.paths.db_path, b"this is not sqlite").unwrap();
    let output = run(&fixture, &["doctor", "--json", "--no-runtime"]);
    assert!(!output.status.success());
    let report = json(&output);
    assert_eq!(report["ok"], false);
    assert!(!report["failures"].as_array().unwrap().is_empty());
}

#[test]
fn doctor_json_reports_a_newer_schema_as_failure() {
    let fixture = Fixture::new();
    let conn = rusqlite::Connection::open(&fixture.paths.db_path).unwrap();
    conn.execute_batch("PRAGMA user_version = 99;").unwrap();
    drop(conn);
    let output = run(&fixture, &["doctor", "--json", "--no-runtime"]);
    assert!(!output.status.success());
    let report = json(&output);
    assert_eq!(report["ok"], false);
    assert!(
        report["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|failure| failure.as_str().unwrap().contains("newer"))
    );
}

#[test]
fn snapshot_command_writes_a_verifiable_manifest() {
    let fixture = Fixture::new();
    let to = fixture.dir.path().join("snapshot");
    let to_str = to.to_str().unwrap();
    let output = run(&fixture, &["snapshot", "--to", to_str, "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = json(&output);
    assert_eq!(manifest["quick_check"], "ok");
    assert_eq!(manifest["schema_version"], CURRENT_VERSION);
    assert!(manifest["fingerprint"]["session_seconds"].as_i64().unwrap() > 0);
    assert_eq!(
        manifest["sha256"],
        screen_time::sha256::file_digest(&to.join("screen-time.db")).unwrap()
    );
    assert!(to.join("manifest.json").is_file());
    let mode = fs::metadata(to.join("manifest.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);

    let again = run(&fixture, &["snapshot", "--to", to_str, "--json"]);
    assert!(
        !again.status.success(),
        "snapshot overwrite must be refused"
    );
}

#[test]
fn snapshot_refuses_a_read_only_target_parent() {
    let fixture = Fixture::new();
    let parent = fixture.dir.path().join("readonly");
    fs::create_dir_all(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).unwrap();
    let to = parent.join("snapshot");
    let output = run(
        &fixture,
        &["snapshot", "--to", to.to_str().unwrap(), "--json"],
    );
    let failed = !output.status.success();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed, "snapshot into a read-only parent must fail");
}

#[test]
fn data_check_detects_loss_and_supports_a_documented_migration() {
    let fixture = Fixture::new();
    let to = fixture.dir.path().join("snapshot");
    let output = run(
        &fixture,
        &["snapshot", "--to", to.to_str().unwrap(), "--json"],
    );
    assert!(output.status.success());

    let conn = rusqlite::Connection::open(&fixture.paths.db_path).unwrap();
    let lost = conn
        .execute(
            "DELETE FROM sessions WHERE id = (SELECT id FROM sessions LIMIT 1)",
            [],
        )
        .unwrap();
    assert_eq!(lost, 1);
    drop(conn);

    let output = run(
        &fixture,
        &["check-data", "--against", to.to_str().unwrap(), "--json"],
    );
    assert!(!output.status.success());
    let report = json(&output);
    assert_eq!(report["ok"], false);
    assert!(!report["differences"].as_array().unwrap().is_empty());

    let output = run(
        &fixture,
        &[
            "check-data",
            "--against",
            to.to_str().unwrap(),
            "--json",
            "--allow-migration",
        ],
    );
    assert!(output.status.success());
    let report = json(&output);
    assert_eq!(report["ok"], true);
    assert_eq!(report["allow_migration"], true);
}

#[test]
fn snapshot_database_can_be_opened_and_restored() {
    let fixture = Fixture::new();
    let to = fixture.dir.path().join("snapshot");
    let output = run(
        &fixture,
        &["snapshot", "--to", to.to_str().unwrap(), "--json"],
    );
    assert!(output.status.success());
    let snapshot_db: &Path = &to.join("screen-time.db");
    let db = screen_time::db::Database::open_read(snapshot_db).unwrap();
    assert_eq!(db.integrity_check().unwrap().to_lowercase(), "ok");
    assert!(db.fingerprint().unwrap().session_seconds > 0);
}
