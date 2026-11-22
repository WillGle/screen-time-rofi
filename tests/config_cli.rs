use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use screen_time::config::{Config, LayoutMode, ThemeMode};
use tempfile::tempdir;

const TEMPLATE: &str = include_str!("../share/screen-time/config.example.toml");

fn run(home: &Path, config_home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_screen-time"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env_remove("ROFI_RETV")
        .env_remove("ROFI_INFO")
        .output()
        .expect("cannot run screen-time")
}

fn config_path(config_home: &Path) -> PathBuf {
    config_home.join("screen-time").join("config.toml")
}

#[test]
fn example_is_a_valid_parseable_template() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let config_home = dir.path().join("config");
    let output = run(&home, &config_home, &["config", "example"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text, TEMPLATE);
    let config: Config = toml::from_str(&text).unwrap();
    assert_eq!(config.theme_mode(), ThemeMode::Auto);
    assert_eq!(config.layout_mode(), LayoutMode::Auto);
    assert!(config.retention.is_disabled());
    assert_eq!(config.storage.warn_db_mib(), 250);
}

#[test]
fn path_reports_the_xdg_config_location() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let config_home = dir.path().join("config");
    let output = run(&home, &config_home, &["config", "path"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        config_path(&config_home).display().to_string()
    );
    assert!(
        !config_path(&config_home).exists(),
        "config path must not be created"
    );
}

#[test]
fn init_is_explicit_and_refuses_to_overwrite() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let config_home = dir.path().join("config");
    let path = config_path(&config_home);

    let output = run(&home, &config_home, &["config", "init"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(path.is_file());
    assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let output = run(&home, &config_home, &["config", "init"]);
    assert!(
        !output.status.success(),
        "init must refuse an existing config"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);

    fs::write(&path, "[theme]\nmode = \"nord\"\n").unwrap();
    let output = run(&home, &config_home, &["config", "init", "--force"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);
}
