use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Once;
use std::thread;
use std::time::{Duration, Instant};

use screen_time::db::Database;
use screen_time::paths::Paths;
use screen_time::snapshot;

fn init_tz() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: this test process only runs the daemon child with TZ=UTC.
        unsafe { std::env::set_var("TZ", "UTC") };
    });
}

fn setup_paths(root: &Path) -> Paths {
    let state = root.join("state");
    let runtime = root.join("run");
    let paths = Paths {
        state_dir: state.clone(),
        db_path: state.join("screen-time.db"),
        runtime_dir: runtime.clone(),
        live_path: runtime.join("live.json"),
        ui_state_path: runtime.join("ui.json"),
        backups_dir: state.join("backups"),
        migration_backups_dir: state.join("migrations"),
    };
    paths.ensure().unwrap();
    paths
}

fn fake_hyprland(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let event_socket = dir.join(".socket2.sock");
    let command_socket = dir.join(".socket.sock");
    let event_listener = UnixListener::bind(&event_socket).unwrap();
    let command_listener = UnixListener::bind(&command_socket).unwrap();

    thread::spawn(move || {
        for stream in command_listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut request = [0u8; 64];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(br#"{"class":"code","title":"ignored title"}"#);
        }
    });

    thread::spawn(move || {
        let Ok((mut stream, _)) = event_listener.accept() else {
            return;
        };
        thread::sleep(Duration::from_millis(300));
        let _ = stream.write_all(b"activewindow>>code,title one\n");
        thread::sleep(Duration::from_secs(3));
        let _ = stream.write_all(b"activewindow>>firefox,title two\n");
        thread::sleep(Duration::from_secs(3));
        let _ = stream.write_all(b"activewindow>>code,title three\n");
        thread::sleep(Duration::from_secs(30));
    });

    dir.to_path_buf()
}

struct DaemonGuard {
    child: Option<Child>,
}

impl DaemonGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn pid(&self) -> u32 {
        self.child.as_ref().expect("daemon already reaped").id()
    }

    fn wait(&mut self) -> ExitStatus {
        self.child
            .take()
            .expect("daemon already reaped")
            .wait()
            .expect("cannot wait for daemon")
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn daemon_captures_focus_switches_and_shuts_down_cleanly() {
    init_tz();
    let root = tempfile::tempdir().unwrap();
    let paths = setup_paths(root.path());
    let hypr = fake_hyprland(&root.path().join("hypr").join("sig"));

    let child = Command::new(env!("CARGO_BIN_EXE_screen-time"))
        .arg("daemon")
        .env("TZ", "UTC")
        .env("HOME", root.path())
        .env("SCREEN_TIME_STATE_DIR", &paths.state_dir)
        .env("SCREEN_TIME_RUNTIME_DIR", &paths.runtime_dir)
        .env("SCREEN_TIME_HYPRLAND_DIR", &hypr)
        .env("SCREEN_TIME_DISABLE_IDLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot launch daemon");
    let mut child = DaemonGuard::new(child);

    let deadline = Instant::now() + Duration::from_secs(12);
    let mut snapshot = None;
    while Instant::now() < deadline {
        if let Ok(Some(value)) = snapshot::read(&paths.live_path) {
            let closed_an_app = value.apps.values().any(|app| app.closed_seconds >= 1);
            let open_app = value
                .open
                .as_ref()
                .map(|open| open.app.as_str())
                .unwrap_or("");
            if closed_an_app && !open_app.is_empty() {
                snapshot = Some(value);
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    let snapshot = snapshot.expect("live snapshot did not observe both focus switches");
    assert_eq!(
        snapshot.date,
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    );
    assert!(snapshot.closed_active_seconds >= 1);
    assert!(matches!(
        snapshot.open.as_ref().unwrap().app.as_str(),
        "Code" | "Firefox"
    ));

    // SIGTERM must flush the batch, close the power session, and remove the
    // ephemeral snapshot.
    // SAFETY: kill is called with the child PID and a valid signal.
    unsafe { libc::kill(child.pid() as i32, libc::SIGTERM) };
    let status = child.wait();
    assert!(status.success(), "daemon exited with {status}");

    assert!(
        !paths.live_path.exists(),
        "clean shutdown must remove live.json"
    );
    let db = Database::open_read(&paths.db_path).unwrap();
    let sessions: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert!(
        sessions >= 2,
        "expected committed sessions, found {sessions}"
    );
    let (reason, closed): (String, i64) = db
        .connection()
        .query_row(
            "SELECT reason, end_ts FROM power_sessions ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(reason, "shutdown");
    assert!(closed > 0);
    assert_eq!(db.meta("clean_shutdown").unwrap().as_deref(), Some("1"));
    let (events, _) = db.counts().unwrap();
    assert!(events >= 4, "expected boot + focus + idle/shutdown events");
}
