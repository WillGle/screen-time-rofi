use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct Paths {
    pub state_dir: PathBuf,
    pub db_path: PathBuf,
    pub runtime_dir: PathBuf,
    pub live_path: PathBuf,
    pub ui_state_path: PathBuf,
    pub backups_dir: PathBuf,
    pub migration_backups_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let state_dir = state_dir()?;
        let runtime_dir = runtime_dir(&state_dir);
        let db_path = env::var_os("SCREEN_TIME_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("screen-time.db"));
        Ok(Self {
            live_path: runtime_dir.join("live.json"),
            ui_state_path: runtime_dir.join("ui.json"),
            backups_dir: state_dir.join("backups").join("updates"),
            migration_backups_dir: state_dir.join("backups").join("migrations"),
            state_dir,
            db_path,
            runtime_dir,
        })
    }

    pub fn ensure(&self) -> Result<()> {
        create_private_dir(&self.state_dir)?;
        create_private_dir(&self.runtime_dir)?;
        Ok(())
    }

    pub fn ensure_backup_dirs(&self) -> Result<()> {
        create_private_dir(&self.backups_dir)?;
        create_private_dir(&self.migration_backups_dir)?;
        Ok(())
    }

    pub fn db_exists(&self) -> bool {
        self.db_path.is_file()
    }
}

fn home_dir() -> Result<PathBuf> {
    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home);
        if !path.as_os_str().is_empty() {
            return Ok(path);
        }
    }
    bail!("HOME is not set")
}

fn state_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("SCREEN_TIME_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return Ok(path.join("screen-time"));
        }
    }
    Ok(home_dir()?.join(".local").join("state").join("screen-time"))
}

fn runtime_dir(state_dir: &Path) -> PathBuf {
    if let Some(path) = env::var_os("SCREEN_TIME_RUNTIME_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = env::var_os("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path.join("screen-time");
        }
    }
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let run_user = PathBuf::from(format!("/run/user/{uid}"));
    if run_user.is_dir() {
        run_user.join("screen-time")
    } else {
        state_dir.join("run")
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("cannot create {}", path.display()))?;
    let metadata = fs::metadata(path).with_context(|| format!("cannot stat {}", path.display()))?;
    if !metadata.is_dir() {
        bail!("{} is not a directory", path.display());
    }
    let mut perms = metadata.permissions();
    let mode = perms.mode();
    if mode & 0o777 != 0o700 {
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)
            .with_context(|| format!("cannot set permissions on {}", path.display()))?;
    }
    Ok(())
}

/// Current boot identity, or `unknown` when the kernel file is unavailable.
pub fn boot_id() -> String {
    if let Ok(value) = env::var("SCREEN_TIME_BOOT_ID") {
        if !value.is_empty() {
            return value;
        }
    }
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|value| value.trim().to_owned())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

pub fn pid_alive(pid: u32) -> bool {
    pid != 0 && Path::new(&format!("/proc/{pid}")).exists()
}
