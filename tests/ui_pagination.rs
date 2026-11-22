mod common;

use common::Fixture;

fn list_lines(payload: &str) -> usize {
    payload
        .lines()
        .find_map(|line| {
            line.strip_prefix("\0theme\x1flistview { lines: ")
                .and_then(|value| value.strip_suffix("; }"))
                .and_then(|value| value.parse().ok())
        })
        .expect("missing listview lines metadata")
}

#[test]
fn main_view_pages_apps_and_survives_detail_round_trip() {
    let fixture = Fixture::new();

    let first = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);
    assert!(first.contains("apps 1-5 of 8"));
    assert!(first.contains("Code"));
    assert_eq!(first.matches("\0info\x1f").count(), 5);
    assert!(!first.contains("apps 6-8"));
    assert_eq!(list_lines(&first), 15);

    let second = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "21")]);
    assert!(second.contains("apps 6-8 of 8"));
    assert!(
        !second.contains("\0info\x1fCode"),
        "page 2 starts at item 6"
    );
    assert_eq!(second.matches("\0info\x1f").count(), 3);
    assert_eq!(list_lines(&second), 15, "page height stays stable");

    let detail = fixture.render_env(
        &["render", "--format", "rofi"],
        &[("ROFI_RETV", "1"), ("ROFI_INFO", "Code")],
    );
    assert!(detail.contains("APP ACTIVITY"));
    assert!(detail.contains("BURSTS"));

    let back = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "14")]);
    assert!(back.contains("APP TIMELINE"));
    assert!(
        back.contains("apps 6-8 of 8"),
        "page is preserved after detail"
    );
    assert_eq!(
        list_lines(&back),
        15,
        "returning from detail keeps main height"
    );
}

#[test]
fn page_down_and_up_stay_within_range() {
    let fixture = Fixture::new();
    fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "0")]);

    let last = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "21")]);
    let clamped = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "21")]);
    assert_eq!(last, clamped, "extra page-down is clamped to the last page");

    let back = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "22")]);
    assert!(back.contains("apps 1-5 of 8"));
    let back_again = fixture.render_env(&["render", "--format", "rofi"], &[("ROFI_RETV", "22")]);
    assert_eq!(
        back, back_again,
        "extra page-up is clamped to the first page"
    );
}
