use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use crate::capture;
use crate::config::{Config, StorageConfig};
use crate::db::{DataFingerprint, Database, RetentionPolicy};
use crate::idle;
use crate::paths::{self, Paths};
use crate::snapshot::{self, backup_usage, directory_size, file_size};
use crate::timeutil;

pub struct Options {
    pub json: bool,
    pub no_runtime: bool,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub ok: bool,
    pub mode: &'static str,
    pub state_dir: String,
    pub runtime_dir: String,
    pub db_path: String,
    pub db_exists: bool,
    pub schema_version: Option<i64>,
    pub quick_check: Option<String>,
    pub counts: BTreeMap<String, i64>,
    pub fingerprint: Option<DataFingerprint>,
    pub retention: RetentionReport,
    pub sizes: SizeReport,
    pub service_state: String,
    pub snapshot: SnapshotReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeReport>,
    pub warnings: Vec<String>,
    pub failures: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RetentionReport {
    pub enabled: bool,
    pub events_days: Option<u32>,
    pub sessions_days: Option<u32>,
    pub aggregates_days: Option<u32>,
    pub power_days: Option<u32>,
    pub summary: String,
}

#[derive(Debug, Serialize)]
pub struct SizeReport {
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub shm_bytes: u64,
    pub update_backups_bytes: u64,
    pub update_backups_count: usize,
    pub migration_backups_bytes: u64,
    pub migration_backups_count: usize,
    pub total_bytes: u64,
    pub warn_threshold_bytes: u64,
    pub hard_threshold_bytes: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct SnapshotReport {
    pub present: bool,
    pub fresh: bool,
    pub date: Option<String>,
    pub as_of: Option<String>,
    pub age_seconds: Option<i64>,
    pub pid: Option<u32>,
    pub boot_id_matches: Option<bool>,
}

#[derive(Debug, Default, Serialize)]
pub struct RuntimeReport {
    pub hyprland_available: bool,
    pub hyprland_signature: Option<String>,
    pub active_app: Option<String>,
    pub idle_available: bool,
    pub idle: Option<bool>,
    pub rofi_available: bool,
    pub rofi_version: Option<String>,
    pub runtime_writable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture: Option<CaptureReport>,
}

#[derive(Debug, Serialize)]
pub struct CaptureReport {
    /// "ok", "mismatch", or "unknown".
    pub status: String,
    pub active_app: Option<String>,
    pub tracked_app: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureStatus {
    Ok,
    Mismatch,
    Unknown,
}

impl CaptureStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Mismatch => "mismatch",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Default)]
struct CaptureSample {
    snapshot_ready: bool,
    active_app: Option<String>,
    tracked_app: Option<String>,
}

fn capture_decision(
    hyprland_available: bool,
    idle: Option<bool>,
    sample: &CaptureSample,
) -> CaptureStatus {
    if !hyprland_available || idle != Some(false) || !sample.snapshot_ready {
        return CaptureStatus::Unknown;
    }
    match (sample.active_app.as_deref(), sample.tracked_app.as_deref()) {
        (Some(active), Some(tracked)) if active == tracked => CaptureStatus::Ok,
        (Some(_), Some(_)) => CaptureStatus::Mismatch,
        _ => CaptureStatus::Unknown,
    }
}

/// A mismatch is only reported when a second sample confirms it. This tests
/// the same decision over two injected samples; the caller owns the delay and
/// re-read.
fn confirm_capture(
    hyprland_available: bool,
    idle: Option<bool>,
    first: &CaptureSample,
    second: &CaptureSample,
) -> CaptureStatus {
    let first_status = capture_decision(hyprland_available, idle, first);
    if first_status != CaptureStatus::Mismatch {
        return first_status;
    }
    capture_decision(hyprland_available, idle, second)
}

pub fn run(options: &Options) -> Result<()> {
    let paths = Paths::resolve()?;
    let mut warnings = Vec::new();
    let mut failures = Vec::new();

    let (config, config_error) = Config::load_default_lenient();
    if let Some(error) = config_error {
        warnings.push(format!(
            "config is invalid; using keep-all retention: {error:#}"
        ));
    }
    let retention_policy = RetentionPolicy::from_config(&config.retention);

    let db_exists = paths.db_path.is_file();
    let mut schema_version = None;
    let mut quick_check = None;
    let mut counts = BTreeMap::new();
    let mut fingerprint = None;

    match Database::open_read(&paths.db_path) {
        Ok(db) => {
            schema_version = db.schema_version().ok();
            match db.integrity_check() {
                Ok(result) => {
                    if result.eq_ignore_ascii_case("ok") {
                        quick_check = Some(result);
                    } else {
                        quick_check = Some(result.clone());
                        failures.push(format!("database quick_check: {result}"));
                    }
                }
                Err(error) => failures.push(format!("database quick_check: {error:#}")),
            }
            match db.journal_mode() {
                Ok(mode) if mode.eq_ignore_ascii_case("wal") => {}
                Ok(mode) => warnings.push(format!("journal_mode={mode} (expected wal)")),
                Err(error) => warnings.push(format!("cannot read journal_mode: {error:#}")),
            }
            match db.table_counts() {
                Ok(value) => counts = value,
                Err(error) => failures.push(format!("cannot count database rows: {error:#}")),
            }
            match db.fingerprint() {
                Ok(value) => fingerprint = Some(value),
                Err(error) => failures.push(format!("cannot fingerprint database: {error:#}")),
            }
        }
        Err(error) => {
            if db_exists {
                failures.push(format!("cannot open database: {error:#}"));
            } else {
                warnings.push("database not created yet; start the daemon once".to_owned());
            }
        }
    }

    let sizes = size_report(&paths, &config.storage);
    if sizes.database_bytes >= sizes.hard_threshold_bytes {
        warnings.push(format!(
            "database is {} MiB, above the {} MiB hard warning threshold",
            sizes.database_bytes / (1024 * 1024),
            sizes.hard_threshold_bytes / (1024 * 1024)
        ));
    } else if sizes.database_bytes >= sizes.warn_threshold_bytes {
        warnings.push(format!(
            "database is {} MiB, above the {} MiB warning threshold",
            sizes.database_bytes / (1024 * 1024),
            sizes.warn_threshold_bytes / (1024 * 1024)
        ));
    }
    if retention_policy.is_enabled() {
        warnings.push(format!(
            "retention is enabled ({}); history older than these cutoffs is deleted on startup and rollover",
            retention_policy.summary()
        ));
    }

    let snapshot = snapshot_report(&paths, &mut warnings);
    let service_state = service_state();
    if service_state != "active" {
        warnings.push(format!(
            "systemd user service is {} (journalctl --user -u screen-time)",
            if service_state.is_empty() {
                "unknown"
            } else {
                &service_state
            }
        ));
    }

    let runtime = if options.no_runtime {
        None
    } else {
        Some(runtime_report(&paths, &mut warnings, &mut failures))
    };

    let report = DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        ok: failures.is_empty(),
        mode: if options.no_runtime {
            "no-runtime"
        } else {
            "full"
        },
        state_dir: paths.state_dir.display().to_string(),
        runtime_dir: paths.runtime_dir.display().to_string(),
        db_path: paths.db_path.display().to_string(),
        db_exists,
        schema_version,
        quick_check,
        counts,
        fingerprint,
        retention: RetentionReport {
            enabled: retention_policy.is_enabled(),
            events_days: retention_policy.events_days,
            sessions_days: retention_policy.sessions_days,
            aggregates_days: retention_policy.aggregates_days,
            power_days: retention_policy.power_days,
            summary: retention_policy.summary(),
        },
        sizes,
        service_state,
        snapshot,
        runtime,
        warnings,
        failures,
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }
    if !report.ok {
        anyhow::bail!("doctor found {} problem(s)", report.failures.len());
    }
    Ok(())
}

fn size_report(paths: &Paths, storage: &StorageConfig) -> SizeReport {
    let (update_backups_bytes, update_backups_count) = backup_usage(&paths.backups_dir);
    let (migration_backups_bytes, migration_backups_count) =
        backup_usage(&paths.migration_backups_dir);
    let database_bytes = file_size(&paths.db_path);
    let wal_path = std::path::PathBuf::from(format!("{}-wal", paths.db_path.display()));
    let shm_path = std::path::PathBuf::from(format!("{}-shm", paths.db_path.display()));
    let wal_bytes = file_size(&wal_path);
    let shm_bytes = file_size(&shm_path);
    SizeReport {
        database_bytes,
        wal_bytes,
        shm_bytes,
        update_backups_bytes,
        update_backups_count,
        migration_backups_bytes,
        migration_backups_count,
        total_bytes: database_bytes
            + wal_bytes
            + shm_bytes
            + update_backups_bytes
            + migration_backups_bytes,
        warn_threshold_bytes: storage.warn_db_mib() * 1024 * 1024,
        hard_threshold_bytes: storage.hard_db_mib() * 1024 * 1024,
    }
}

fn snapshot_report(paths: &Paths, warnings: &mut Vec<String>) -> SnapshotReport {
    match snapshot::read(&paths.live_path) {
        Ok(Some(snapshot)) => {
            let now_ts = timeutil::now().timestamp();
            let age = snapshot
                .as_of_local()
                .map(|as_of| now_ts - as_of.timestamp())
                .unwrap_or(i64::MAX);
            let boot_id = paths::boot_id();
            let boot_id_matches = snapshot.boot_id == boot_id;
            if !boot_id_matches {
                warnings.push(format!(
                    "live snapshot boot_id {} differs from current {}",
                    snapshot.boot_id, boot_id
                ));
            }
            if !paths::pid_alive(snapshot.pid) {
                warnings.push(format!("live snapshot pid {} is not running", snapshot.pid));
            }
            let fresh = !snapshot.is_stale(now_ts, &boot_id);
            if !fresh {
                warnings
                    .push("live snapshot is stale (open intervals are not extended)".to_owned());
            }
            SnapshotReport {
                present: true,
                fresh,
                date: Some(snapshot.date.clone()),
                as_of: Some(snapshot.as_of.clone()),
                age_seconds: (age != i64::MAX).then_some(age),
                pid: Some(snapshot.pid),
                boot_id_matches: Some(boot_id_matches),
            }
        }
        Ok(None) => {
            warnings.push("live snapshot missing; daemon is stopped or not ready".to_owned());
            SnapshotReport::default()
        }
        Err(error) => {
            warnings.push(format!("live snapshot unreadable: {error:#}"));
            SnapshotReport::default()
        }
    }
}

fn service_state() -> String {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", "screen-time.service"])
        .output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        Err(_) => "unknown".to_owned(),
    }
}

fn runtime_report(
    paths: &Paths,
    warnings: &mut Vec<String>,
    failures: &mut Vec<String>,
) -> RuntimeReport {
    let mut report = RuntimeReport::default();

    if paths.state_dir.is_dir() {
        // Nothing to report: a missing state directory is a warning below.
    } else {
        warnings.push("state directory not created yet; start the daemon once".to_owned());
    }
    if paths.runtime_dir.is_dir() {
        report.runtime_writable = Some(check_writable(&paths.runtime_dir, failures));
    } else {
        warnings.push("runtime directory missing; live snapshot is unavailable".to_owned());
        report.runtime_writable = Some(false);
    }

    let hyprland = capture::discover();
    let mut active_app = None;
    match &hyprland {
        Some(hypr) => {
            report.hyprland_available = true;
            report.hyprland_signature = Some(hypr.signature.clone());
            match capture::query_active_window(&hypr.command_socket) {
                Ok(Some(app)) => {
                    active_app = Some(app.clone());
                    report.active_app = Some(app);
                }
                Ok(None) => warnings.push("Hyprland reports no active app window".to_owned()),
                Err(error) => warnings.push(format!("active-window query failed: {error:#}")),
            }
        }
        None => warnings.push("Hyprland event socket not found; tracking is inactive".to_owned()),
    }

    let idle_probe = idle::probe();
    report.idle_available = idle_probe.available;
    report.idle = idle_probe.idle;
    if !idle_probe.available {
        warnings.push(format!(
            "logind IdleHint unavailable: {}",
            idle_probe
                .error
                .unwrap_or_else(|| "unknown error".to_owned())
        ));
    }
    report.capture = Some(capture_report(
        paths,
        hyprland.as_ref(),
        idle_probe.idle,
        active_app,
        warnings,
    ));

    match Command::new("rofi").arg("-version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            report.rofi_available = true;
            report.rofi_version = Some(version);
        }
        Ok(_) => warnings.push("rofi is installed but `rofi -version` failed".to_owned()),
        Err(_) => warnings.push("rofi is not installed or not in PATH".to_owned()),
    }
    report
}

fn capture_report(
    paths: &Paths,
    hyprland: Option<&capture::HyprlandPaths>,
    idle: Option<bool>,
    first_active: Option<String>,
    warnings: &mut Vec<String>,
) -> CaptureReport {
    let first_sample = capture_sample(paths, first_active);
    let first_status = capture_decision(hyprland.is_some(), idle, &first_sample);
    let (status, sample) = if first_status == CaptureStatus::Mismatch {
        // Give the daemon one second to persist the focus change before
        // declaring it stuck.
        thread::sleep(Duration::from_secs(1));
        let active = hyprland.and_then(|hypr| {
            capture::query_active_window(&hypr.command_socket)
                .ok()
                .flatten()
        });
        let second_sample = capture_sample(paths, active);
        let status = confirm_capture(hyprland.is_some(), idle, &first_sample, &second_sample);
        (status, second_sample)
    } else {
        (first_status, first_sample)
    };
    if status == CaptureStatus::Mismatch {
        if let (Some(active), Some(tracked)) = (&sample.active_app, &sample.tracked_app) {
            warnings.push(format!(
                "daemon is not following Hyprland focus (active \"{active}\", tracked \"{tracked}\"); check journalctl --user -u screen-time"
            ));
        }
    }
    CaptureReport {
        status: status.as_str().to_owned(),
        active_app: sample.active_app,
        tracked_app: sample.tracked_app,
    }
}

fn capture_sample(paths: &Paths, active_app: Option<String>) -> CaptureSample {
    let boot_id = paths::boot_id();
    let live = snapshot::read(&paths.live_path).ok().flatten();
    let tracked_app = live
        .as_ref()
        .and_then(|snapshot| snapshot.open.as_ref().map(|open| open.app.clone()));
    let now_ts = timeutil::now().timestamp();
    let snapshot_ready = live.as_ref().is_some_and(|snapshot| {
        snapshot.boot_id == boot_id
            && paths::pid_alive(snapshot.pid)
            && !snapshot.is_stale(now_ts, &boot_id)
    });
    CaptureSample {
        snapshot_ready,
        active_app,
        tracked_app,
    }
}

fn check_writable(path: &Path, failures: &mut Vec<String>) -> bool {
    if !path.is_dir() {
        return false;
    }
    let probe = path.join(format!(".doctor-{}", std::process::id()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(error) => {
            failures.push(format!(
                "runtime directory {} is not writable: {error}",
                path.display()
            ));
            false
        }
    }
}

fn print_text(report: &DoctorReport) {
    println!("screen-time doctor");
    println!("  ok    version {}", report.version);
    println!("  ok    state {}", report.state_dir);
    println!("  ok    runtime {}", report.runtime_dir);
    if report.db_exists {
        println!("  ok    database {}", report.db_path);
        if let Some(schema) = report.schema_version {
            println!("  ok    schema version {schema}");
        }
        if let Some(check) = &report.quick_check {
            println!("  ok    database quick_check {check}");
        }
        println!(
            "  ok    {} raw events, {} sessions",
            report.counts.get("events").copied().unwrap_or(0),
            report.counts.get("sessions").copied().unwrap_or(0)
        );
        if let Some(fingerprint) = &report.fingerprint {
            println!(
                "  ok    {} total session seconds, app-key hash {}",
                fingerprint.session_seconds, fingerprint.app_key_hash
            );
        }
    } else {
        println!("  warn  database not created yet; start the daemon once");
    }
    println!(
        "  ok    retention={}, database {} B, wal {} B, shm {} B",
        report.retention.summary,
        report.sizes.database_bytes,
        report.sizes.wal_bytes,
        report.sizes.shm_bytes
    );
    println!(
        "  ok    update backups {} B in {} dir(s); migration backups {} B in {} dir(s)",
        report.sizes.update_backups_bytes,
        report.sizes.update_backups_count,
        report.sizes.migration_backups_bytes,
        report.sizes.migration_backups_count
    );
    if report.snapshot.present {
        println!(
            "  ok    live snapshot {} as_of={} age={}s",
            report.snapshot.date.as_deref().unwrap_or("?"),
            report.snapshot.as_of.as_deref().unwrap_or("?"),
            report
                .snapshot
                .age_seconds
                .map(|age| age.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        );
    }
    println!("  ok    service_state={}", report.service_state);
    if let Some(runtime) = &report.runtime {
        if runtime.hyprland_available {
            println!(
                "  ok    Hyprland {}",
                runtime.hyprland_signature.as_deref().unwrap_or("")
            );
        }
        if runtime.rofi_available {
            println!(
                "  ok    rofi {}",
                runtime.rofi_version.as_deref().unwrap_or("")
            );
        }
    }
    for warning in &report.warnings {
        println!("  warn  {warning}");
    }
    for failure in &report.failures {
        println!("  fail  {failure}");
    }
    println!(
        "\n{} failure(s), {} warning(s)",
        report.failures.len(),
        report.warnings.len()
    );
}

/// Recursively measure a directory. Kept public for restore drills and tests.
pub fn dir_size(path: &Path) -> u64 {
    directory_size(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(active: Option<&str>, tracked: Option<&str>) -> CaptureSample {
        CaptureSample {
            snapshot_ready: true,
            active_app: active.map(str::to_owned),
            tracked_app: tracked.map(str::to_owned),
        }
    }

    #[test]
    fn capture_decision_skips_without_enough_evidence() {
        let matching = sample(Some("Code"), Some("Code"));
        assert_eq!(
            capture_decision(false, Some(false), &matching),
            CaptureStatus::Unknown
        );
        assert_eq!(
            capture_decision(true, Some(true), &matching),
            CaptureStatus::Unknown
        );
        assert_eq!(
            capture_decision(true, None, &matching),
            CaptureStatus::Unknown
        );
        let mut stale = sample(Some("Code"), Some("Code"));
        stale.snapshot_ready = false;
        assert_eq!(
            capture_decision(true, Some(false), &stale),
            CaptureStatus::Unknown
        );
        assert_eq!(
            capture_decision(true, Some(false), &sample(None, Some("Code"))),
            CaptureStatus::Unknown
        );
        assert_eq!(
            capture_decision(true, Some(false), &sample(Some("Code"), None)),
            CaptureStatus::Unknown
        );
        assert_eq!(
            capture_decision(true, Some(false), &sample(None, None)),
            CaptureStatus::Unknown
        );
    }

    #[test]
    fn capture_decision_matches_or_flags_focus() {
        assert_eq!(
            capture_decision(true, Some(false), &sample(Some("Code"), Some("Code"))),
            CaptureStatus::Ok
        );
        assert_eq!(
            capture_decision(true, Some(false), &sample(Some("Firefox"), Some("Code"))),
            CaptureStatus::Mismatch
        );
    }

    #[test]
    fn mismatch_is_reported_only_when_the_second_sample_confirms_it() {
        let first = sample(Some("Firefox"), Some("Code"));
        let resolved = sample(Some("Firefox"), Some("Firefox"));
        let still_mismatched = sample(Some("Firefox"), Some("Code"));
        let unknown = sample(None, Some("Code"));
        assert_eq!(
            confirm_capture(true, Some(false), &first, &resolved),
            CaptureStatus::Ok
        );
        assert_eq!(
            confirm_capture(true, Some(false), &first, &still_mismatched),
            CaptureStatus::Mismatch
        );
        assert_eq!(
            confirm_capture(true, Some(false), &first, &unknown),
            CaptureStatus::Unknown
        );
        assert_eq!(
            confirm_capture(true, Some(false), &resolved, &still_mismatched),
            CaptureStatus::Ok,
            "an already-matching first sample needs no confirmation"
        );
    }

    #[test]
    fn runtime_report_omits_capture_when_not_checked() {
        let value = serde_json::to_value(RuntimeReport::default()).unwrap();
        assert!(value.get("capture").is_none());
        assert_eq!(CaptureStatus::Ok.as_str(), "ok");
        assert_eq!(CaptureStatus::Mismatch.as_str(), "mismatch");
        assert_eq!(CaptureStatus::Unknown.as_str(), "unknown");
    }
}
