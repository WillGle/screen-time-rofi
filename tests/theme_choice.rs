mod common;

use common::{Fixture, init_tz};

fn theme_background(fixture: &Fixture) -> String {
    let theme = fixture.render(&["render", "--format", "rofi-theme"]);
    let line = theme
        .lines()
        .find(|line| line.trim_start().starts_with("background-color:"))
        .expect("generated theme has no background");
    line.trim().to_owned()
}

#[test]
fn theme_shortcuts_persist_and_alt4_restores_config() {
    init_tz();
    let fixture = Fixture::new();
    fixture.write_config("[theme]\nmode = \"solarized\"\n");
    let config_path = fixture
        .config_home()
        .join("screen-time")
        .join("config.toml");
    let config_before = std::fs::read_to_string(&config_path).unwrap();
    let choice_path = fixture.paths.state_dir.join("theme-choice");
    let request_path = fixture.dir.path().join("theme-request");
    let request = request_path.to_str().unwrap();

    assert_eq!(theme_background(&fixture), "background-color: #fdf6e3;");

    std::fs::write(&request_path, "").unwrap();
    fixture.render_env(
        &["render", "--format", "rofi"],
        &[
            ("ROFI_RETV", "17"),
            ("SCREEN_TIME_ROFI_THEME_REQUEST", request),
        ],
    );
    assert_eq!(std::fs::read_to_string(&choice_path).unwrap(), "nord");
    assert_eq!(std::fs::read_to_string(&request_path).unwrap(), "reopen");
    assert_eq!(theme_background(&fixture), "background-color: #2e3440;");

    // A normal popup open resets the transient UI state, never the theme choice.
    fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert_eq!(theme_background(&fixture), "background-color: #2e3440;");

    // A hand-edited file with a trailing newline is still a valid choice.
    std::fs::write(&choice_path, "gruvbox\n").unwrap();
    assert_eq!(theme_background(&fixture), "background-color: #1d2021;");

    std::fs::write(&request_path, "").unwrap();
    fixture.render_env(
        &["render", "--format", "rofi"],
        &[
            ("ROFI_RETV", "20"),
            ("SCREEN_TIME_ROFI_THEME_REQUEST", request),
        ],
    );
    assert!(!choice_path.exists());
    assert_eq!(std::fs::read_to_string(&request_path).unwrap(), "reopen");
    assert_eq!(theme_background(&fixture), "background-color: #fdf6e3;");
    assert_eq!(
        std::fs::read_to_string(&config_path).unwrap(),
        config_before
    );
}
