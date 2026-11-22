mod common;

use common::{Fixture, assert_or_update_golden, init_tz};
use screen_time::sessions::Session;
use screen_time::snapshot;
use screen_time::timeutil;

#[test]
fn main_screen_1280x800() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi"]);
    assert_or_update_golden("main_1280x800.txt", &actual);
    for line in actual.lines() {
        assert!(
            line.chars().count() <= 96,
            "line exceeds 96 columns at 1280x800: {line:?}"
        );
    }
}

#[test]
fn main_screen_1920x1080() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render_env(
        &["render", "--format", "rofi"],
        &[("SCREEN_TIME_SCREEN_WIDTH", "1920")],
    );
    assert_or_update_golden("main_1920x1080.txt", &actual);
    for line in actual.lines() {
        assert!(
            line.chars().count() <= 144,
            "line exceeds 144 columns at 1920x1080: {line:?}"
        );
    }
    assert!(actual.contains("6h 48m"));
    assert!(actual.contains("POWER"));
    assert!(actual.contains("DAY ACTIVITY"));
    assert!(actual.contains("APP TIMELINE"));
    assert!(actual.contains("Top app"));
    assert!(actual.contains("●"));
    assert!(actual.contains("%"));
}

#[test]
fn detail_screen_1280x800() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi", "--app", "Code"]);
    assert_or_update_golden("detail_1280x800.txt", &actual);
    assert!(actual.contains("sessions through"));
    assert!(actual.contains("▌ BURSTS"));
    assert!(actual.contains("▌ 7 DAY"));
}

#[test]
fn detail_screen_1920x1080() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render_env(
        &["render", "--format", "rofi", "--app", "Code"],
        &[("SCREEN_TIME_SCREEN_WIDTH", "1920")],
    );
    assert_or_update_golden("detail_1920x1080.txt", &actual);
    assert!(actual.contains("2h 14m"));
    assert!(actual.contains("8 switches/h"));
    assert!(actual.contains("+ 4 other sessions"));
    assert!(actual.contains("▌ BURSTS"));
    assert!(actual.contains("▌ 7 DAY"));
}

#[test]
fn plain_output_has_no_rofi_metadata() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi"]);
    assert!(!actual.contains('\0'));
    assert!(!actual.contains("\x1f"));
    assert!(!actual.contains("\\0"));
}

#[test]
fn rofi_protocol_marks_rows_and_carries_app_info() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert!(actual.contains("\0use-hot-keys\x1ftrue"));
    assert!(actual.contains("\0markup-rows\x1ftrue"));
    assert!(actual.contains("\0nonselectable\x1ftrue"));
    assert!(actual.contains("\0info\x1fCode"));
    assert!(actual.contains("SCREEN TIME"));
    assert!(actual.contains("DAY ACTIVITY"));
    assert!(actual.contains("APP TIMELINE"));
    assert!(actual.contains("<span foreground=\"#"));
    assert!(actual.contains("●"));
}

#[test]
fn stale_snapshot_does_not_extend_the_open_interval() {
    init_tz();
    let fixture = Fixture::new();
    let mut snapshot = snapshot::read(&fixture.paths.live_path).unwrap().unwrap();
    snapshot.open = Some(snapshot::OpenInterval {
        app: "Code".to_owned(),
        start: timeutil::rfc3339(
            timeutil::parse_rfc3339("2026-11-21T16:00:00+00:00")
                .unwrap()
                .timestamp(),
        ),
    });
    snapshot.as_of = "2026-11-21T16:00:00+00:00".to_owned();
    snapshot::write_atomic(&fixture.paths.live_path, &snapshot).unwrap();

    let actual = fixture.render(&["render", "--format", "rofi"]);
    assert!(actual.contains("6h 48m"), "{actual}");
    assert!(actual.contains("stale"), "{actual}");
    assert!(!actual.contains("7h 03m"));
}

#[test]
fn session_count_experiment_is_off_and_can_be_enabled() {
    init_tz();
    let fixture = Fixture::new();
    let plain = fixture.render(&["render", "--format", "rofi"]);
    assert!(!plain.contains("×18"));
    let experiment = fixture.render_env(
        &["render", "--format", "rofi"],
        &[("SCREEN_TIME_SESSION_COUNT", "1")],
    );
    assert!(experiment.contains("×18"), "{experiment}");
}

#[test]
fn matugen_palette_updates_section_data_colors_without_restart() {
    init_tz();
    let fixture = Fixture::new();
    let dir = fixture.dir.path();
    let palette = dir.join("palette.json");
    let static_env = dir.join("static.env");
    let rofi_theme = dir.join("user.rasi");
    std::fs::write(&rofi_theme, "* { }\n").unwrap();
    std::fs::write(
        &palette,
        r##"{
          "colors": {
            "surface": {"default": {"color": "#101010"}},
            "surface_container": {"default": {"color": "#202020"}},
            "on_surface": {"default": {"color": "#f0f0f0"}},
            "on_surface_variant": {"default": {"color": "#909090"}},
            "primary": {"default": {"color": "#aaccff"}}
          },
          "palettes": {"secondary": {"70": {"color": "#66aacc"}}}
        }"##,
    )
    .unwrap();
    std::fs::write(
        &static_env,
        "THEME_STATIC_BASE='#111111'\nTHEME_STATIC_MANTLE='#222222'\nTHEME_STATIC_TEXT='#eeeeee'\nTHEME_STATIC_SUBTEXT='#999999'\nTHEME_STATIC_ACCENT='#3333ff'\nTHEME_STATIC_CYAN='#33cccc'\nTHEME_MONO_FONT='Fallback Mono'\nTHEME_ROFI_FONT_SIZE=10\n",
    )
    .unwrap();
    fixture.write_config(&format!(
        "[theme]\npalette = \"{}\"\nstatic_env = \"{}\"\nrofi_theme = \"{}\"\n\n[font]\nfamily = \"Palette Mono\"\nsize = 14\n",
        palette.display(),
        static_env.display(),
        rofi_theme.display()
    ));
    let actual = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert!(actual.contains("foreground=\"#aaccff\""), "{actual}");
    assert!(actual.contains("foreground=\"#66aacc\""), "{actual}");
    let theme = fixture.render(&["render", "--format", "rofi-theme"]);
    assert!(theme.contains("@theme \""), "{theme}");
    assert!(theme.contains("font: \"Palette Mono 14\""), "{theme}");
    assert!(theme.contains("background-color: #101010"), "{theme}");

    // Palette changes apply on the next render; no daemon cache/restart.
    std::fs::write(
        &palette,
        r##"{
          "colors": {
            "surface": {"default": {"color": "#101010"}},
            "surface_container": {"default": {"color": "#202020"}},
            "on_surface": {"default": {"color": "#f0f0f0"}},
            "on_surface_variant": {"default": {"color": "#909090"}},
            "primary": {"default": {"color": "#aabbcc"}}
          },
          "palettes": {"secondary": {"70": {"color": "#ddeeff"}}}
        }"##,
    )
    .unwrap();
    let updated = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert!(updated.contains("foreground=\"#aabbcc\""), "{updated}");
    assert!(updated.contains("foreground=\"#ddeeff\""), "{updated}");
}

#[test]
fn config_font_size_auto_compacts_and_full_overrides() {
    init_tz();
    let fixture = Fixture::new();
    fixture.write_config("[font]\nfamily = \"Big Mono\"\nsize = 16\n[layout]\nmode = \"auto\"\n");
    let compact = fixture.render(&["render", "--format", "rofi"]);
    let ruler = compact
        .lines()
        .find(|line| line.contains("00") && line.contains("06") && line.contains("12"))
        .expect("ruler");
    assert_eq!(
        ruler.chars().count(),
        96,
        "compact layout must fit 1280 at 16pt: {ruler:?}"
    );
    assert_eq!(
        compact
            .lines()
            .filter(|line| line.contains('●') && line.contains('%'))
            .count(),
        8,
        "compact layout must keep every app row: {compact}"
    );
    let theme = fixture.render(&["render", "--format", "rofi-theme"]);
    assert!(theme.contains("Big Mono 16"), "{theme}");

    let wide = fixture.render_env(
        &["render", "--format", "rofi"],
        &[("SCREEN_TIME_SCREEN_WIDTH", "3440")],
    );
    let ruler = wide
        .lines()
        .find(|line| line.contains("00") && line.contains("06") && line.contains("12"))
        .expect("ruler");
    assert_eq!(ruler.chars().count(), 144, "3440 ruler: {ruler:?}");
    assert_eq!(
        wide.lines()
            .filter(|line| line.contains('●') && line.contains('%'))
            .count(),
        8,
        "3440 must show all eight fixture app rows: {wide}"
    );

    fixture.write_config("[font]\nfamily = \"Big Mono\"\nsize = 16\n[layout]\nmode = \"full\"\n");
    let full = fixture.render(&["render", "--format", "rofi"]);
    let ruler = full
        .lines()
        .find(|line| line.contains("00") && line.contains("06") && line.contains("12"))
        .expect("ruler");
    assert_eq!(ruler.chars().count(), 144, "full ruler: {ruler:?}");
}

#[test]
fn responsive_screen_geometry_selects_slot_and_window_size() {
    init_tz();
    let fixture = Fixture::new();
    let expanded = fixture.render_env(
        &["render", "--format", "rofi"],
        &[
            ("SCREEN_TIME_SCREEN_WIDTH", "2560"),
            ("SCREEN_TIME_SCREEN_HEIGHT", "1280"),
        ],
    );
    let ruler = expanded
        .lines()
        .find(|line| line.contains("00") && line.contains("06") && line.contains("12"))
        .expect("expanded ruler");
    assert_eq!(ruler.chars().count(), 144, "2560 ruler: {ruler:?}");
    let theme = fixture.render_env(
        &["render", "--format", "rofi-theme"],
        &[
            ("SCREEN_TIME_SCREEN_WIDTH", "2560"),
            ("SCREEN_TIME_SCREEN_HEIGHT", "1280"),
        ],
    );
    assert!(theme.contains("144ch + 64px"), "{theme}");

    let compact = fixture.render_env(
        &["render", "--format", "rofi"],
        &[
            ("SCREEN_TIME_SCREEN_WIDTH", "1280"),
            ("SCREEN_TIME_SCREEN_HEIGHT", "720"),
        ],
    );
    let ruler = compact
        .lines()
        .find(|line| line.contains("00") && line.contains("06") && line.contains("12"))
        .expect("compact ruler");
    assert_eq!(ruler.chars().count(), 96, "1280 ruler: {ruler:?}");
    assert!(compact.contains("top 8 of 8 apps"), "{compact}");
    let theme = fixture.render_env(
        &["render", "--format", "rofi-theme"],
        &[("SCREEN_TIME_SCREEN_HEIGHT", "720")],
    );
    assert!(theme.contains("96ch + 64px"), "{theme}");
}

#[test]
fn static_env_font_and_colors_are_used_when_matugen_is_missing() {
    init_tz();
    let fixture = Fixture::new();
    let theme_dir = fixture.dir.path().join(".config").join("theme");
    std::fs::create_dir_all(&theme_dir).unwrap();
    std::fs::write(
        theme_dir.join("static.env"),
        "THEME_STATIC_BASE='#111111'\nTHEME_STATIC_MANTLE='#222222'\nTHEME_STATIC_TEXT='#eeeeee'\nTHEME_STATIC_SUBTEXT='#999999'\nTHEME_STATIC_ACCENT='#3333ff'\nTHEME_STATIC_CYAN='#33cccc'\nTHEME_MONO_FONT='Static Mono'\nTHEME_ROFI_FONT_SIZE=13\n",
    )
    .unwrap();
    let actual = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert!(actual.contains("foreground=\"#3333ff\""), "{actual}");
    assert!(actual.contains("foreground=\"#33cccc\""), "{actual}");
    let theme = fixture.render(&["render", "--format", "rofi-theme"]);
    assert!(theme.contains("font: \"Static Mono 13\""), "{theme}");
    assert!(theme.contains("background-color: #111111"), "{theme}");
}

#[test]
fn main_empty_day_renders_intentional_zero_state() {
    init_tz();
    let fixture = Fixture::empty();
    let actual = fixture.render(&["render", "--format", "rofi"]);
    assert_or_update_golden("main_empty_day.txt", &actual);
    for line in actual.lines() {
        assert!(
            line.chars().count() <= 96,
            "line exceeds 96 columns: {line:?}"
        );
    }
    assert!(actual.contains("DAY ACTIVITY"), "{actual}");
    assert!(actual.contains("APP TIMELINE"), "{actual}");
    assert!(actual.contains("No activity recorded today"), "{actual}");
    assert!(actual.contains("0m"), "{actual}");
}

#[test]
fn main_no_data_day_renders_headers_without_rows() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi", "--date", "2026-01-01"]);
    assert_or_update_golden("main_no_data_day.txt", &actual);
    for line in actual.lines() {
        assert!(
            line.chars().count() <= 96,
            "line exceeds 96 columns: {line:?}"
        );
    }
    assert!(actual.contains("Thursday, 1 January 2026"), "{actual}");
    assert!(actual.contains("DAY ACTIVITY"), "{actual}");
    assert!(actual.contains("APP TIMELINE"), "{actual}");
    assert!(actual.contains("No activity recorded today"), "{actual}");
}

#[test]
fn main_degenerate_rows_truncate_long_utf8_names() {
    init_tz();
    let fixture = Fixture::empty();
    let at = |value: &str| timeutil::parse_rfc3339(value).unwrap().timestamp();
    let long_name = "日本語🎨長いアプリケーション名 with ASCII tail 0123456789";
    assert!(long_name.chars().count() > 40);
    fixture.add_sessions(&[
        Session::new(
            "Code",
            "2026-11-21".to_owned(),
            at("2026-11-21T12:00:00+00:00"),
            at("2026-11-21T12:00:01+00:00"),
        )
        .unwrap(),
        Session::new(
            long_name,
            "2026-11-21".to_owned(),
            at("2026-11-21T12:01:00+00:00"),
            at("2026-11-21T12:01:01+00:00"),
        )
        .unwrap(),
    ]);
    let actual = fixture.render(&["render", "--format", "rofi"]);
    assert_or_update_golden("main_degenerate_rows.txt", &actual);
    assert!(std::str::from_utf8(actual.as_bytes()).is_ok());
    assert!(actual.contains("0m"), "{actual}");
    assert!(actual.contains("50%"), "{actual}");
    assert!(!actual.contains("NaN"), "{actual}");
    let truncated = actual
        .lines()
        .find(|line| line.contains('…'))
        .expect("long name is truncated with an ellipsis");
    assert!(
        truncated.chars().count() <= 96,
        "truncated row exceeds the column budget: {truncated:?}"
    );
}

#[test]
fn detail_unknown_app_shows_an_empty_detail() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi", "--app", "NoSuchApp"]);
    assert_or_update_golden("detail_unknown_app.txt", &actual);
    assert!(actual.contains("NoSuchApp"), "{actual}");
    assert!(actual.contains("No sessions recorded"), "{actual}");
}

#[test]
fn future_date_plain_render_fails_fast() {
    init_tz();
    let fixture = Fixture::new();
    for args in [
        vec!["render", "--date", "2026-11-22"],
        vec!["render", "--date", "2026-11-22", "--app", "Code"],
    ] {
        let output = fixture.render_env_output(&args, &[]);
        assert!(!output.status.success(), "future date must fail: {args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("future dates are not available"),
            "unexpected stderr for {args:?}: {stderr}"
        );
    }
}

#[test]
fn corrupt_palette_falls_back_without_invisible_text() {
    init_tz();
    let fixture = Fixture::new();
    let dir = fixture.dir.path();
    let palette = dir.join("bad-palette.json");
    let static_env = dir.join("static.env");
    std::fs::write(&palette, "{ not json").unwrap();
    std::fs::write(
        &static_env,
        "THEME_STATIC_BASE='#111111'\nTHEME_STATIC_MANTLE='#222222'\nTHEME_STATIC_TEXT='#eeeeee'\nTHEME_STATIC_SUBTEXT='#999999'\nTHEME_STATIC_ACCENT='#3333ff'\nTHEME_STATIC_CYAN='#33cccc'\nTHEME_MONO_FONT='Fallback Mono'\nTHEME_ROFI_FONT_SIZE=12\n",
    )
    .unwrap();
    fixture.write_config(&format!(
        "[theme]\npalette = \"{}\"\nstatic_env = \"{}\"\n",
        palette.display(),
        static_env.display()
    ));
    let actual = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert!(actual.contains("foreground=\"#3333ff\""), "{actual}");
    assert!(actual.contains("foreground=\"#33cccc\""), "{actual}");
    assert!(!actual.contains("foreground=\"#000000\""), "{actual}");
}
