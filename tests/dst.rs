use std::sync::Once;

use chrono::NaiveDate;
use screen_time::timeutil::{self, SLOT_COUNT};

static INIT_TZ: Once = Once::new();

/// The whole test binary uses one zone. All tests call this first; only the
/// first call mutates the process environment.
fn init_tz() {
    INIT_TZ.call_once(|| {
        // SAFETY: this integration test binary sets one fixed zone before any
        // timestamp is converted, and no test changes it afterwards.
        unsafe { std::env::set_var("TZ", "America/New_York") };
    });
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn ts(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp()
}

fn assert_no_zero_length_segments(parts: &[(NaiveDate, i64, i64)]) {
    assert!(!parts.is_empty());
    for (index, (_, start, end)) in parts.iter().enumerate() {
        assert!(end > start, "zero-length segment at index {index}");
        if index > 0 {
            assert_eq!(*start, parts[index - 1].2, "segments are contiguous");
        }
    }
}

#[test]
fn spring_forward_day_is_23_real_hours() {
    init_tz();
    let day = date(2026, 3, 8);
    let (start, end) = timeutil::day_bounds(day);
    assert_eq!(end - start, 82_800);

    let mut slots = [0i64; SLOT_COUNT];
    timeutil::add_interval_to_slots(&mut slots, day, start, end);
    assert_eq!(slots.iter().sum::<i64>(), 82_800);
    assert!(slots.iter().all(|seconds| *seconds >= 0));

    // 01:30 EST to 03:30 EDT is one real hour even though the local clock
    // jumps over 02:00.
    let crossing_start = ts("2026-03-08T01:30:00-05:00");
    let crossing_end = ts("2026-03-08T03:30:00-04:00");
    assert_eq!(crossing_end - crossing_start, 3_600);
    let mut crossing = [0i64; SLOT_COUNT];
    timeutil::add_interval_to_slots(&mut crossing, day, crossing_start, crossing_end);
    assert_eq!(crossing.iter().sum::<i64>(), 3_600);
    assert_eq!(crossing[3], 1_800, "01:30-02:00 local");
    assert_eq!(crossing[6], 1_800, "03:00-03:30 local");
    assert!(crossing.iter().all(|seconds| *seconds >= 0));
}

#[test]
fn fall_back_day_is_25_real_hours() {
    init_tz();
    let day = date(2026, 11, 1);
    let (start, end) = timeutil::day_bounds(day);
    assert_eq!(end - start, 90_000);

    let mut slots = [0i64; SLOT_COUNT];
    timeutil::add_interval_to_slots(&mut slots, day, start, end);
    assert_eq!(slots.iter().sum::<i64>(), 90_000);
    assert!(slots.iter().all(|seconds| *seconds >= 0));

    // The repeated 01:00 hour counts exactly twice, never more.
    let repeated_start = ts("2026-11-01T01:00:00-04:00");
    let repeated_end = ts("2026-11-01T02:00:00-05:00");
    assert_eq!(repeated_end - repeated_start, 7_200);
    let mut repeated = [0i64; SLOT_COUNT];
    timeutil::add_interval_to_slots(&mut repeated, day, repeated_start, repeated_end);
    assert_eq!(repeated.iter().sum::<i64>(), 7_200);
    assert_eq!(repeated[2], 3_600, "01:00 local, both occurrences");
    assert_eq!(repeated[3], 3_600, "01:30 local, both occurrences");
    assert!(repeated.iter().all(|seconds| *seconds >= 0));
}

#[test]
fn split_interval_uses_dst_aware_local_midnights() {
    init_tz();

    let spring_start = ts("2026-03-07T22:00:00-05:00");
    let spring_end = ts("2026-03-09T02:00:00-04:00");
    let parts = timeutil::split_interval_by_date(spring_start, spring_end);
    assert_eq!(
        parts.iter().map(|(day, _, _)| *day).collect::<Vec<_>>(),
        vec![date(2026, 3, 7), date(2026, 3, 8), date(2026, 3, 9)]
    );
    assert_no_zero_length_segments(&parts);
    assert_eq!(parts[0].2 - parts[0].1, 2 * 3_600);
    assert_eq!(parts[1].1, timeutil::day_bounds(date(2026, 3, 8)).0);
    assert_eq!(parts[1].2, timeutil::day_bounds(date(2026, 3, 8)).1);
    assert_eq!(parts[1].2 - parts[1].1, 82_800);
    assert_eq!(parts[2].2 - parts[2].1, 2 * 3_600);

    let fall_start = ts("2026-10-31T22:00:00-04:00");
    let fall_end = ts("2026-11-02T02:00:00-05:00");
    let parts = timeutil::split_interval_by_date(fall_start, fall_end);
    assert_eq!(
        parts.iter().map(|(day, _, _)| *day).collect::<Vec<_>>(),
        vec![date(2026, 10, 31), date(2026, 11, 1), date(2026, 11, 2)]
    );
    assert_no_zero_length_segments(&parts);
    assert_eq!(parts[0].2 - parts[0].1, 2 * 3_600);
    assert_eq!(parts[1].1, timeutil::day_bounds(date(2026, 11, 1)).0);
    assert_eq!(parts[1].2, timeutil::day_bounds(date(2026, 11, 1)).1);
    assert_eq!(parts[1].2 - parts[1].1, 90_000);
    assert_eq!(parts[2].2 - parts[2].1, 2 * 3_600);
}
