use std::sync::Once;

use chrono::NaiveDate;
use screen_time::timeutil;

static INIT_TZ: Once = Once::new();

/// Pacific/Apia skipped all of 2011-12-30 when it crossed the date line;
/// verified against the system zoneinfo. One zone per test binary.
fn init_tz() {
    INIT_TZ.call_once(|| {
        // SAFETY: single-zone test binary; set before any Local conversion.
        unsafe { std::env::set_var("TZ", "Pacific/Apia") };
    });
}

#[test]
fn fully_skipped_date_collapses_to_an_empty_day() {
    init_tz();
    let skipped = NaiveDate::from_ymd_opt(2011, 12, 30).unwrap();
    let (start, end) = timeutil::day_bounds(skipped);
    assert!(
        start >= end,
        "a fully skipped date must be an empty day, not a panic"
    );

    let slot_starts: Vec<i64> = (0..48)
        .map(|slot| timeutil::slot_start_ts(skipped, slot))
        .collect();
    assert!(
        slot_starts.windows(2).all(|pair| pair[0] <= pair[1]),
        "skipped-date slots resolve forward but stay ordered"
    );

    let next = NaiveDate::from_ymd_opt(2011, 12, 31).unwrap();
    let (next_start, next_end) = timeutil::day_bounds(next);
    assert_eq!(next_end - next_start, 86_400);
}
