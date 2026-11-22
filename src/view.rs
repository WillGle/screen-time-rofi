use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Local, NaiveDate};

use crate::db::{Database, DayData};
use crate::paths::{self, Paths};
use crate::sessions::{self, Burst, Session};
use crate::snapshot;
use crate::timeutil::{self, SLOT_COUNT};

pub const TOP_APPS: usize = 10;
/// The popup shows at most this many app rows at once. Larger lists are
/// paginated so the window does not grow with every tracked app and the last
/// row is not clipped by the popup height.
pub const APP_PAGE_SIZE: usize = 5;

#[derive(Debug, Clone)]
pub struct DayView {
    pub date: NaiveDate,
    pub today: NaiveDate,
    pub is_today: bool,
    pub stale: bool,
    pub active_seconds: i64,
    pub powered_seconds: i64,
    pub first_active_ts: Option<i64>,
    pub last_active_ts: Option<i64>,
    pub peak: Option<(i64, i64)>,
    pub delta_seconds: Option<i64>,
    pub usual_seconds: Option<i64>,
    pub history_days: usize,
    pub trend: Vec<Option<i64>>,
    pub trend_avg: Option<i64>,
    pub power_slots: [bool; SLOT_COUNT],
    pub rows: Vec<AppRow>,
    pub other: Option<OtherRow>,
    /// Zero-based app timeline page used by the Rofi view.
    pub page: usize,
    pub last_commit_at: Option<DateTime<Local>>,
}

#[derive(Debug, Clone)]
pub struct AppRow {
    pub name: String,
    pub seconds: i64,
    pub sessions: i64,
    pub slots: [bool; SLOT_COUNT],
}

#[derive(Debug, Clone)]
pub struct OtherRow {
    pub count: usize,
    pub seconds: i64,
    pub slots: [bool; SLOT_COUNT],
}

#[derive(Debug, Clone)]
pub struct DetailView {
    pub date: NaiveDate,
    pub today: NaiveDate,
    pub app: String,
    pub total_seconds: i64,
    pub day_seconds: i64,
    pub share_percent: i64,
    pub session_count: usize,
    pub median_seconds: i64,
    pub switches_per_hour: i64,
    pub sessions: Vec<Session>,
    pub bursts: Vec<Burst>,
    pub remaining_sessions: usize,
    pub remaining_seconds: i64,
    pub sessions_through: Option<DateTime<Local>>,
    pub stale: bool,
    pub trend: Vec<Option<i64>>,
    pub trend_avg: Option<i64>,
    pub db_error: Option<String>,
    pub has_open: bool,
}

pub fn load_main(paths: &Paths, date: NaiveDate, now: DateTime<Local>) -> Result<DayView> {
    let today = now.date_naive();
    let is_today = date == today;
    let now_ts = now.timestamp();
    let boot_id = paths::boot_id();

    let live = snapshot::read(&paths.live_path).ok().flatten();
    let live_today = live.as_ref().filter(|snapshot| {
        snapshot
            .date
            .parse::<NaiveDate>()
            .map(|live_date| live_date == date)
            .unwrap_or(false)
    });
    let live_stale = live_today
        .map(|snapshot| snapshot.is_stale(now_ts, &boot_id))
        .unwrap_or(true);

    let db = Database::open_read(&paths.db_path).ok();
    let db_day = db.as_ref().and_then(|db| db.load_day(date, now_ts).ok());

    let mut app_totals: BTreeMap<String, AppTotal> = BTreeMap::new();
    let total_slots;
    let power_slots_seconds;
    let active_seconds;
    let powered_seconds;
    let first_active_ts;
    let last_active_ts;

    if let Some(snapshot) = live_today {
        let stale = live_stale;
        active_seconds = snapshot.total_active(now_ts, stale);
        powered_seconds = snapshot.total_powered(now_ts, stale);
        first_active_ts = match (snapshot.first_active_ts, snapshot.open_start_ts()) {
            (Some(first), Some(open)) if !stale => Some(first.min(open)),
            (Some(first), _) => Some(first),
            (None, Some(open)) if !stale => Some(open),
            _ => None,
        };
        last_active_ts = if !stale && snapshot.open.is_some() {
            Some(now_ts)
        } else {
            snapshot.last_active_ts
        };
        for app in snapshot.apps.keys() {
            let app_slots = snapshot.slots_for_app(app, date, now_ts, stale);
            app_totals.insert(
                app.clone(),
                AppTotal {
                    seconds: snapshot.app_total(app, now_ts, stale),
                    sessions: snapshot.app_sessions(app, stale),
                    slots: app_slots,
                },
            );
        }
        if let Some(open) = &snapshot.open {
            app_totals
                .entry(open.app.clone())
                .or_insert_with(|| AppTotal {
                    seconds: snapshot.app_total(&open.app, now_ts, stale),
                    sessions: snapshot.app_sessions(&open.app, stale),
                    slots: snapshot.slots_for_app(&open.app, date, now_ts, stale),
                });
        }
        total_slots = snapshot.total_slots(date, now_ts, stale);
        power_slots_seconds = snapshot.total_power_slots(date, now_ts, stale);
    } else {
        let day = db_day.clone().unwrap_or_else(|| DayData::new(date));
        active_seconds = day.active_seconds();
        powered_seconds = day.powered_seconds;
        first_active_ts = day.first_active_ts();
        last_active_ts = day.last_active_ts();
        total_slots = day.slots;
        power_slots_seconds = day.power_slots;
        for (app, app_data) in &day.apps {
            app_totals.insert(
                app.clone(),
                AppTotal {
                    seconds: app_data.seconds,
                    sessions: app_data.sessions,
                    slots: app_data.slots,
                },
            );
        }
    }

    let mut totals: Vec<(String, AppTotal)> = app_totals.into_iter().collect();
    totals.sort_by(|(left_name, left), (right_name, right)| {
        right
            .seconds
            .cmp(&left.seconds)
            .then_with(|| left_name.cmp(right_name))
    });

    let mut rows: Vec<AppRow> = Vec::new();
    let mut other: Option<OtherRow> = None;
    let mut other_slots = [0i64; SLOT_COUNT];
    for (index, (name, total)) in totals.into_iter().enumerate() {
        if index < TOP_APPS {
            rows.push(AppRow {
                name,
                seconds: total.seconds.max(0),
                sessions: total.sessions.max(0),
                slots: slots_active(&total.slots),
            });
        } else {
            let other = other.get_or_insert(OtherRow {
                count: 0,
                seconds: 0,
                slots: [false; SLOT_COUNT],
            });
            other.count += 1;
            other.seconds += total.seconds.max(0);
            for (slot, seconds) in total.slots.iter().enumerate() {
                other_slots[slot] += (*seconds).max(0);
            }
        }
    }
    if let Some(other) = other.as_mut() {
        other.slots = slots_active(&other_slots);
    }

    let (trend, trend_avg, history_days) =
        trend_for(db.as_ref(), date, is_today.then_some(active_seconds), None);
    let stale = if is_today {
        match live_today {
            Some(_snapshot) => live_stale,
            None => true,
        }
    } else {
        false
    };

    let mut delta_seconds = None;
    let mut usual_seconds = None;
    if is_today && !stale && history_days >= 3 {
        let current_slot = timeutil::slot_index(now_ts);
        if let Some(db) = db.as_ref() {
            let mut samples = Vec::new();
            let dates = timeutil::week_dates(date, 7);
            for (index, prior) in dates.iter().enumerate() {
                if *prior >= date || trend.get(index).copied().flatten().is_none() {
                    continue;
                }
                if let Ok(seconds) = db.seconds_before_slot(*prior, current_slot) {
                    samples.push(seconds);
                }
            }
            if samples.len() >= 3 {
                let usual = samples.iter().sum::<i64>() / samples.len() as i64;
                usual_seconds = Some(usual);
                delta_seconds = Some(active_seconds - usual);
            }
        }
    }

    let peak = peak_window(&total_slots, date);
    let last_commit_at = if is_today {
        live_today
            .and_then(|snapshot| snapshot.last_commit_local())
            .or_else(|| {
                db.as_ref()
                    .and_then(|db| db.last_commit_at().ok().flatten())
                    .and_then(|value| timeutil::parse_rfc3339(&value))
            })
    } else {
        db.as_ref()
            .and_then(|db| db.last_commit_at().ok().flatten())
            .and_then(|value| timeutil::parse_rfc3339(&value))
    };

    Ok(DayView {
        date,
        today,
        is_today,
        stale,
        active_seconds,
        powered_seconds,
        first_active_ts,
        last_active_ts,
        peak,
        delta_seconds,
        usual_seconds,
        history_days,
        trend,
        trend_avg,
        power_slots: slots_active(&power_slots_seconds),
        rows,
        other,
        page: 0,
        last_commit_at,
    })
}

pub fn load_detail(
    paths: &Paths,
    date: NaiveDate,
    app: &str,
    now: DateTime<Local>,
) -> Result<DetailView> {
    let today = now.date_naive();
    if date > today {
        anyhow::bail!("future dates are not available");
    }
    let now_ts = now.timestamp();
    let boot_id = paths::boot_id();
    let live = snapshot::read(&paths.live_path).ok().flatten();
    let live_today = live.as_ref().filter(|snapshot| {
        snapshot
            .date
            .parse::<NaiveDate>()
            .map(|live_date| live_date == date)
            .unwrap_or(false)
    });
    let live_stale = live_today
        .map(|snapshot| snapshot.is_stale(now_ts, &boot_id))
        .unwrap_or(true);

    let db = Database::open_read(&paths.db_path);
    let mut db_error = None;
    let mut sessions = Vec::new();
    let mut day_seconds = 0;
    let mut trend = Vec::new();
    let mut trend_avg = None;
    let sessions_through;

    match &db {
        Ok(db) => {
            sessions = db.load_app_sessions(date, app)?;
            if let Ok(day) = db.load_day(date, now_ts) {
                day_seconds = day.active_seconds();
            }
            let totals = db.app_daily_totals(app, date, 7)?;
            let (built, average, _) = trend_from_totals(&totals, date, None);
            trend = built;
            trend_avg = average;
            sessions_through = live_today
                .and_then(|snapshot| snapshot.last_commit_local())
                .or_else(|| {
                    db.last_commit_at()
                        .ok()
                        .flatten()
                        .and_then(|value| timeutil::parse_rfc3339(&value))
                });
        }
        Err(error) => {
            db_error = Some(format!("{error:#}"));
            sessions_through = live_today.and_then(|snapshot| snapshot.last_commit_local());
        }
    }

    if let Some(snapshot) = live_today {
        let open_session =
            if !live_stale && snapshot.open.as_ref().is_some_and(|open| open.app == app) {
                snapshot.open_start_ts().and_then(|start| {
                    Session::new(app, date.format("%Y-%m-%d").to_string(), start, now_ts)
                })
            } else {
                None
            };
        if let Some(session) = open_session {
            sessions.push(session);
        }
    }

    sessions.sort_by_key(|session| session.start_ts);
    let session_count = sessions.len();
    let median = sessions::median_seconds(&sessions);
    let total_seconds: i64 = sessions.iter().map(|session| session.seconds).sum();
    let db_error = if db.is_err() {
        Some(db_error.unwrap_or_else(|| "database unavailable".to_owned()))
    } else {
        None
    };

    if let Some(snapshot) = live_today {
        if !live_stale {
            day_seconds = snapshot.total_active(now_ts, false);
        }
    }
    if trend.is_empty() && db.is_ok() {
        trend = vec![None; 7];
    }
    if let (Some(snapshot), true) = (live_today, date == today) {
        if !live_stale && snapshot.open.as_ref().is_some_and(|open| open.app == app) {
            if let Some(last) = trend.last_mut() {
                *last = Some(total_seconds);
            }
        }
    }

    let all_bursts = sessions::group_bursts(&sessions, sessions::BURST_GAP_SECONDS);
    let bursts: Vec<Burst> = all_bursts
        .into_iter()
        .filter(|burst| burst.count >= 2)
        .collect();
    let shown = bursts.len().min(5);
    let shown_sessions: usize = bursts.iter().take(shown).map(|burst| burst.count).sum();
    let shown_seconds: i64 = bursts.iter().take(shown).map(|burst| burst.seconds).sum();
    let remaining_sessions = session_count.saturating_sub(shown_sessions);
    let remaining_seconds = (total_seconds - shown_seconds).max(0);
    let has_open = live_today
        .map(|snapshot| !live_stale && snapshot.open.as_ref().is_some_and(|open| open.app == app))
        .unwrap_or(false);

    Ok(DetailView {
        date,
        today,
        app: app.to_owned(),
        total_seconds,
        day_seconds,
        share_percent: timeutil::percent(total_seconds, day_seconds),
        session_count,
        median_seconds: median,
        switches_per_hour: sessions::switches_per_hour(session_count, total_seconds),
        sessions,
        bursts,
        remaining_sessions,
        remaining_seconds,
        sessions_through,
        stale: live_today
            .map(|snapshot| snapshot.is_stale(now_ts, &boot_id))
            .unwrap_or(false),
        trend,
        trend_avg,
        db_error,
        has_open,
    })
}

#[derive(Debug, Clone)]
struct AppTotal {
    seconds: i64,
    sessions: i64,
    slots: [i64; SLOT_COUNT],
}

fn slots_active(slots: &[i64; SLOT_COUNT]) -> [bool; SLOT_COUNT] {
    let mut active = [false; SLOT_COUNT];
    for (index, seconds) in slots.iter().enumerate() {
        active[index] = *seconds >= 60;
    }
    active
}

fn peak_window(slots: &[i64; SLOT_COUNT], date: NaiveDate) -> Option<(i64, i64)> {
    if slots.iter().all(|seconds| *seconds <= 0) {
        return None;
    }
    let mut best = (0usize, 0i64);
    for start in 0..SLOT_COUNT - 2 {
        let sum = slots[start] + slots[start + 1] + slots[start + 2];
        if sum > best.1 {
            best = (start, sum);
        }
    }
    if best.1 < 60 {
        return None;
    }
    let start_ts = timeutil::slot_start_ts(date, best.0);
    let end_ts = if best.0 + 3 >= SLOT_COUNT {
        timeutil::day_bounds(date).1
    } else {
        timeutil::slot_start_ts(date, best.0 + 3)
    };
    Some((start_ts, end_ts))
}

fn trend_for(
    db: Option<&Database>,
    date: NaiveDate,
    today_override: Option<i64>,
    app: Option<&str>,
) -> (Vec<Option<i64>>, Option<i64>, usize) {
    let Some(db) = db else {
        let mut trend = vec![None; 7];
        if let Some(current) = today_override {
            if let Some(last) = trend.last_mut() {
                *last = Some(current);
            }
        }
        return (trend, None, 0);
    };
    let totals = match app {
        Some(app) => db.app_daily_totals(app, date, 7).unwrap_or_default(),
        None => db.daily_totals(date, 7).unwrap_or_default(),
    };
    trend_from_totals(&totals, date, today_override)
}

fn trend_from_totals(
    totals: &[crate::db::DailyTotal],
    date: NaiveDate,
    today_override: Option<i64>,
) -> (Vec<Option<i64>>, Option<i64>, usize) {
    let dates = timeutil::week_dates(date, 7);
    let map: BTreeMap<NaiveDate, i64> = totals
        .iter()
        .map(|total| (total.date, total.seconds))
        .collect();
    let mut trend = Vec::with_capacity(7);
    for day in &dates {
        if *day == date {
            trend.push(today_override.or_else(|| map.get(day).copied()));
        } else {
            trend.push(map.get(day).copied().filter(|seconds| *seconds > 0));
        }
    }
    let previous: Vec<i64> = trend.iter().take(6).filter_map(|value| *value).collect();
    let history_days = previous.len();
    let average = if history_days >= 3 {
        Some(previous.iter().sum::<i64>() / history_days as i64)
    } else {
        None
    };
    (trend, average, history_days)
}
