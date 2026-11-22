use std::sync::Once;

use chrono::NaiveDate;
use screen_time::timeutil;

static INIT_TZ: Once = Once::new();

/// This binary uses one zone only. America/Santiago skips 00:00-01:00 on
/// 2026-09-06; verified against the system zoneinfo before writing the test.
fn init_tz() {
    INIT_TZ.call_once(|| {
        // SAFETY: single-zone test binary; set before any Local conversion.
        unsafe { std::env::set_var("TZ", "America/Santiago") };
    });
}

#[test]
fn skipped_midnight_starts_at_the_first_valid_instant() {
    init_tz();
    let skipped = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
    let (start, end) = timeutil::day_bounds(skipped);
    assert!(start < end, "a midnight-gap day is not empty");
    assert_eq!(timeutil::format_hhmm(start), "01:00");
    assert_eq!(end - start, 23 * 3_600);

    let slot_starts: Vec<i64> = (0..48)
        .map(|slot| timeutil::slot_start_ts(skipped, slot))
        .collect();
    assert!(slot_starts.iter().all(|ts| *ts >= start && *ts < end));
    assert!(
        slot_starts.windows(2).all(|pair| pair[0] <= pair[1]),
        "gap slots may collapse but must stay ordered"
    );
}
