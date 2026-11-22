use std::env;

use anyhow::{Context, Result, bail};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, SecondsFormat,
    TimeZone, Timelike, Weekday,
};

pub const SLOT_SECONDS: i64 = 30 * 60;
pub const SLOT_COUNT: usize = 48;

pub fn now() -> DateTime<Local> {
    if let Ok(value) = env::var("SCREEN_TIME_NOW") {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(&value) {
            return parsed.with_timezone(&Local);
        }
    }
    Local::now()
}

pub fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").with_context(|| format!("invalid date {value:?}"))
}

pub fn date_of(ts: i64) -> NaiveDate {
    local(ts).date_naive()
}

pub fn local(ts: i64) -> DateTime<Local> {
    let utc =
        DateTime::from_timestamp(ts, 0).unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    utc.with_timezone(&Local)
}

pub fn rfc3339(ts: i64) -> String {
    local(ts).to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn parse_rfc3339(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Local))
}

pub fn day_bounds(date: NaiveDate) -> (i64, i64) {
    let start = local_midnight(date);
    let next = local_midnight(date.succ_opt().unwrap_or(date));
    (start.timestamp(), next.timestamp())
}

fn local_midnight(date: NaiveDate) -> DateTime<Local> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight is valid");
    resolve_local(naive)
}

/// Resolve a naive local wall-clock time without panicking. Ambiguous times
/// take the earliest instant, matching the old `.earliest()` behavior. A time
/// inside a DST gap advances minute by minute to the first real instant. If no
/// wall-clock instant exists within 26 hours (a fully skipped date such as
/// Pacific/Apia 2011-12-30), the naive value is interpreted as UTC as a last
/// resort so callers still receive a total, comparable instant.
fn resolve_local(naive: NaiveDateTime) -> DateTime<Local> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(earliest, _) => earliest,
        LocalResult::None => {
            for minutes in 1..=(26 * 60) {
                let Some(candidate) = naive.checked_add_signed(Duration::minutes(minutes)) else {
                    break;
                };
                match Local.from_local_datetime(&candidate) {
                    LocalResult::Single(dt) => return dt,
                    LocalResult::Ambiguous(dt, _) => return dt,
                    LocalResult::None => {}
                }
            }
            Local.from_utc_datetime(&naive)
        }
    }
}

pub fn today(now: DateTime<Local>) -> NaiveDate {
    now.date_naive()
}

pub fn format_date_label(date: NaiveDate, today: NaiveDate) -> String {
    let base = date.format("%a %-d %b").to_string();
    if date == today {
        format!("{base} · Today")
    } else if date.succ_opt() == Some(today) {
        format!("{base} · Yesterday")
    } else {
        base
    }
}

pub fn format_weekday_date(date: NaiveDate) -> String {
    date.format("%a %-d %b").to_string()
}

pub fn format_hhmm(ts: i64) -> String {
    local(ts).format("%H:%M").to_string()
}

pub fn format_duration(seconds: i64) -> String {
    let minutes = rounded_minutes(seconds);
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}m")
    } else {
        format!("{mins}m")
    }
}

pub fn format_duration_compact(seconds: i64) -> String {
    let minutes = rounded_minutes(seconds);
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours > 0 {
        if mins == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h{mins:02}m")
        }
    } else {
        format!("{mins}m")
    }
}

pub fn format_minutes(seconds: i64) -> String {
    format!("{}m", rounded_minutes(seconds))
}

pub fn format_span(first: i64, last: i64) -> String {
    format!("{}–{}", format_hhmm(first), format_hhmm(last))
}

pub fn rounded_minutes(seconds: i64) -> i64 {
    let seconds = seconds.max(0);
    (seconds + 30) / 60
}

pub fn slot_index(ts: i64) -> usize {
    let dt = local(ts);
    let hour = dt.hour() as usize;
    let minute = dt.minute() as usize;
    (hour * 2 + minute / 30).min(SLOT_COUNT - 1)
}

pub fn slot_start_ts(date: NaiveDate, slot: usize) -> i64 {
    let hour = (slot / 2) as u32;
    let minute = if slot % 2 == 0 { 0 } else { 30 };
    let naive = date
        .and_hms_opt(hour.min(23), minute, 0)
        .expect("valid slot");
    resolve_local(naive).timestamp()
}

pub fn next_slot_boundary(ts: i64) -> i64 {
    // Walk real time in whole minutes until the local clock reaches :00 or
    // :30. Local wall-clock arithmetic alone is wrong around DST: a repeated
    // boundary can be ambiguous and a skipped one does not exist, while the
    // real distance to the next boundary stays at most 30 minutes.
    let mut candidate = ts - ts.rem_euclid(60) + 60;
    for _ in 0..(2 * 60) {
        let dt = local(candidate);
        if dt.second() == 0 && dt.minute() % 30 == 0 {
            return candidate;
        }
        candidate += 60;
    }
    ts + SLOT_SECONDS
}

/// Add the overlap of `[start, end)` with `date` to 48 half-hour slots.
pub fn add_interval_to_slots(slots: &mut [i64; SLOT_COUNT], date: NaiveDate, start: i64, end: i64) {
    let (day_start, day_end) = day_bounds(date);
    let mut cursor = start.max(day_start);
    let end = end.min(day_end);
    while cursor < end {
        let index = slot_index(cursor);
        let next = next_slot_boundary(cursor).min(end);
        if next <= cursor {
            break;
        }
        slots[index] += next - cursor;
        cursor = next;
    }
}

/// Split an interval at local midnight boundaries.
pub fn split_interval_by_date(start: i64, end: i64) -> Vec<(NaiveDate, i64, i64)> {
    let mut result = Vec::new();
    if end <= start {
        return result;
    }
    let mut cursor = start;
    while cursor < end {
        let date = date_of(cursor);
        let (_, day_end) = day_bounds(date);
        let segment_end = end.min(day_end);
        if segment_end <= cursor {
            break;
        }
        result.push((date, cursor, segment_end));
        cursor = segment_end;
    }
    result
}

pub fn week_dates(end: NaiveDate, count: usize) -> Vec<NaiveDate> {
    let mut dates = Vec::with_capacity(count);
    for offset in (0..count).rev() {
        if let Some(date) = end.checked_sub_days(chrono::Days::new(offset as u64)) {
            dates.push(date);
        }
    }
    dates
}

pub fn weekday_name(date: NaiveDate) -> Weekday {
    date.weekday()
}

pub fn percent(part: i64, whole: i64) -> i64 {
    if whole <= 0 {
        return 0;
    }
    ((part.max(0) as f64 / whole as f64) * 100.0).round() as i64
}

pub fn validate_past_or_today(date: NaiveDate, now: DateTime<Local>) -> Result<()> {
    if date > now.date_naive() {
        bail!("future dates are not available");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_rounded_and_compact() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(59), "1m");
        assert_eq!(format_duration(48 * 60), "48m");
        assert_eq!(format_duration(2 * 3600 + 14 * 60), "2h 14m");
        assert_eq!(format_duration_compact(12 * 3600 + 40 * 60), "12h40m");
        assert_eq!(format_duration_compact(3600), "1h");
    }

    #[test]
    fn slots_cover_the_day() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (start, end) = day_bounds(date);
        assert_eq!(date_of(start), date);
        assert_eq!(date_of(end - 1), date);
        assert_eq!(start + 86400, end);
    }

    #[test]
    fn interval_allocates_to_slots() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (day_start, _) = day_bounds(date);
        let mut slots = [0i64; SLOT_COUNT];
        add_interval_to_slots(
            &mut slots,
            date,
            day_start + 9 * 3600,
            day_start + 9 * 3600 + 3600,
        );
        assert_eq!(slots[18], 1800);
        assert_eq!(slots[19], 1800);
        assert_eq!(slots.iter().sum::<i64>(), 3600);
    }

    #[test]
    fn split_interval_at_midnight() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (start, end) = day_bounds(date);
        let parts = split_interval_by_date(start + 23 * 3600, end + 3600);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, date);
        assert_eq!(parts[1].0, date.succ_opt().unwrap());
    }
}
