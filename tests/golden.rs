mod common;

use common::{Fixture, assert_or_update_golden, init_tz};
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
            line.chars().count() <= 116,
            "line exceeds 116 columns: {line:?}"
        );
    }
}

#[test]
fn main_screen_1920x1080() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi"]);
    assert_or_update_golden("main_1920x1080.txt", &actual);
    assert!(actual.contains("6h 48m active"));
    assert!(actual.contains("powered 12h 40m"));
    assert!(actual.contains("+3 apps"), "{actual}");
}

#[test]
fn detail_screen_1280x800() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi", "--app", "Code"]);
    assert_or_update_golden("detail_1280x800.txt", &actual);
    assert!(actual.contains("sessions through"));
    assert!(actual.contains("Bursts"));
}

#[test]
fn detail_screen_1920x1080() {
    init_tz();
    let fixture = Fixture::new();
    let actual = fixture.render(&["render", "--format", "rofi", "--app", "Code"]);
    assert_or_update_golden("detail_1920x1080.txt", &actual);
    assert!(actual.contains("2h 14m"));
    assert!(actual.contains("8 switches/h"));
    assert!(actual.contains("+ 4 longer sessions"));
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
    assert!(actual.contains("<span foreground="));
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
    assert!(actual.contains("6h 48m active"), "{actual}");
    assert!(actual.contains("stale"), "{actual}");
    assert!(!actual.contains("7h 03m active"));
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
