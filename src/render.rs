use crate::sessions::display_name;
use crate::timeutil::{self, SLOT_COUNT};
use crate::view::{DayView, DetailView, OtherRow, TOP_APPS};

const PREFIX_WIDTH: usize = 20;
const WIDE_TIMELINE: usize = 96;
const NARROW_TIMELINE: usize = 48;
const ACCENT: &str = "#89b4fa";
const NEUTRAL: &str = "#bac2de";
const OTHER_SHADE: &str = "#6c7086";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Plain,
    Rofi,
}

#[derive(Debug, Clone, Copy)]
struct Layout {
    narrow: bool,
    session_count: bool,
}

impl Layout {
    fn from_env() -> Self {
        Self {
            narrow: std::env::var("SCREEN_TIME_NARROW").as_deref() == Ok("1"),
            session_count: cfg!(feature = "session-count")
                || std::env::var("SCREEN_TIME_SESSION_COUNT").as_deref() == Ok("1"),
        }
    }

    fn timeline_width(&self) -> usize {
        if self.narrow {
            NARROW_TIMELINE
        } else {
            WIDE_TIMELINE
        }
    }

    fn slot_width(&self) -> usize {
        if self.narrow { 1 } else { 2 }
    }

    fn prefix_width(&self) -> usize {
        if self.session_count {
            PREFIX_WIDTH + 2
        } else {
            PREFIX_WIDTH
        }
    }

    fn screen_width(&self) -> usize {
        self.prefix_width() + self.timeline_width()
    }

    fn top_apps(&self) -> usize {
        if self.narrow { 4 } else { TOP_APPS }
    }
}

pub fn render_main(view: &DayView, mode: OutputMode) -> String {
    let layout = Layout::from_env();
    let width = layout.screen_width();
    let mut out = String::new();
    if mode == OutputMode::Rofi {
        out.push_str("\0use-hot-keys\x1ftrue\n");
        out.push_str("\0markup-rows\x1ftrue\n");
        out.push_str("\0no-custom\x1ftrue\n");
        out.push_str("\0keep-selection\x1ftrue\n");
    }

    let date_label = timeutil::format_date_label(view.date, view.today);
    emit(
        &mut out,
        &pad_line(" SCREEN TIME", &date_label, width),
        RowMeta::NonSelectable,
        mode,
    );

    let mut active = format!(" {} active", timeutil::format_duration(view.active_seconds));
    if view.stale {
        active.push_str("  · stale");
    } else if let Some(delta) = view.delta_seconds {
        let minutes = delta.abs();
        if minutes < 60 {
            active.push_str("   on par with usual at this time");
        } else {
            let arrow = if delta >= 0 { "▲" } else { "▼" };
            active.push_str(&format!(
                "   {arrow} {} vs usual at this time",
                timeutil::format_minutes(minutes)
            ));
        }
    }
    emit(&mut out, &active, RowMeta::NonSelectable, mode);

    let mut details = vec![format!(
        "powered {}",
        timeutil::format_duration(view.powered_seconds)
    )];
    if let (Some(first), Some(last)) = (view.first_active_ts, view.last_active_ts) {
        details.push(format!("span {}", timeutil::format_span(first, last)));
    }
    if let Some((start, end)) = view.peak {
        details.push(format!("peak {}", timeutil::format_span(start, end)));
    }
    emit(
        &mut out,
        &format!(" {}", details.join(" · ")),
        RowMeta::NonSelectable,
        mode,
    );

    let trend_line = trend_line(&view.trend, view.trend_avg);
    emit(&mut out, &trend_line, RowMeta::NonSelectable, mode);
    emit(&mut out, "", RowMeta::NonSelectable, mode);

    let ruler = if layout.narrow {
        ruler_narrow()
    } else {
        ruler_wide()
    };
    emit(
        &mut out,
        &format!("{}{}", " ".repeat(layout.prefix_width()), ruler),
        RowMeta::NonSelectable,
        mode,
    );

    let power_strip = strip(&view.power_slots, "▓", layout.slot_width(), mode, NEUTRAL);
    emit(
        &mut out,
        &row_line(
            &layout,
            "POWER",
            view.powered_seconds,
            &power_strip,
            None,
            mode,
        ),
        RowMeta::NonSelectable,
        mode,
    );

    let visible_rows = view.rows.iter().take(layout.top_apps());
    let mut any_rows = false;
    for row in visible_rows {
        any_rows = true;
        let app_strip = strip(&row.slots, "█", layout.slot_width(), mode, ACCENT);
        let line = row_line(
            &layout,
            &row.name,
            row.seconds,
            &app_strip,
            layout.session_count.then_some(row.sessions),
            mode,
        );
        emit(&mut out, &line, RowMeta::App(row.name.clone()), mode);
    }

    let mut other_slots = [0i64; SLOT_COUNT];
    let mut other_count = 0usize;
    let mut other_seconds = 0i64;
    if let Some(other) = &view.other {
        other_count += other.count;
        other_seconds += other.seconds;
        for (index, active) in other.slots.iter().enumerate() {
            if *active {
                other_slots[index] += 60;
            }
        }
    }
    // The narrow layout has one fewer app row; fold the fifth row into the
    // aggregate without double-counting the rows already in `view.other`.
    for row in view.rows.iter().skip(layout.top_apps()) {
        other_count += 1;
        other_seconds += row.seconds;
        for (index, active) in row.slots.iter().enumerate() {
            if *active {
                other_slots[index] += 60;
            }
        }
    }
    if other_count > 0 {
        let other = OtherRow {
            count: other_count,
            seconds: other_seconds,
            slots: slots_active_i64(&other_slots),
        };
        let other_strip = strip(&other.slots, "▒", layout.slot_width(), mode, OTHER_SHADE);
        emit(
            &mut out,
            &other_line(&layout, &other, &other_strip),
            RowMeta::NonSelectable,
            mode,
        );
    }

    if !any_rows && view.rows.len() <= layout.top_apps() && view.active_seconds == 0 {
        emit(
            &mut out,
            " No activity recorded today",
            RowMeta::App("__none__".to_owned()),
            mode,
        );
    }

    emit(&mut out, "", RowMeta::NonSelectable, mode);
    let default_footer = " ←/→ day   T today   R refresh   ↵ detail   esc close";
    emit(&mut out, default_footer, RowMeta::NonSelectable, mode);
    out
}

pub fn render_detail(view: &DetailView, mode: OutputMode) -> String {
    let layout = Layout::from_env();
    let width = layout.screen_width().max(96);
    let mut out = String::new();
    if mode == OutputMode::Rofi {
        out.push_str("\0use-hot-keys\x1ftrue\n");
        out.push_str("\0markup-rows\x1ftrue\n");
        out.push_str("\0no-custom\x1ftrue\n");
        out.push_str("\0keep-selection\x1ftrue\n");
    }

    let right = format!(
        "{} · {}% of today",
        timeutil::format_duration(view.total_seconds),
        view.share_percent
    );
    emit(
        &mut out,
        &pad_line(&format!(" {}", escape_markup(&view.app)), &right, width),
        RowMeta::NonSelectable,
        mode,
    );

    let stats = format!(
        " {} sessions · median {} · {} switches/h",
        view.session_count,
        timeutil::format_minutes(view.median_seconds),
        view.switches_per_hour
    );
    emit(&mut out, &stats, RowMeta::NonSelectable, mode);

    let through = view
        .sessions_through
        .map(|value| value.format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_owned());
    let mut freshness = format!(" sessions through {through} · may lag one batch");
    if view.stale {
        freshness.push_str(" · stale live");
    }
    emit(&mut out, &freshness, RowMeta::NonSelectable, mode);
    emit(&mut out, "", RowMeta::NonSelectable, mode);

    if let Some(error) = &view.db_error {
        emit(
            &mut out,
            " Database unavailable",
            RowMeta::NonSelectable,
            mode,
        );
        emit(&mut out, &format!(" {error}"), RowMeta::NonSelectable, mode);
    } else {
        emit(&mut out, " Bursts", RowMeta::NonSelectable, mode);
        if view.sessions.is_empty() {
            emit(
                &mut out,
                " Sessions will appear after the first batch.",
                RowMeta::NonSelectable,
                mode,
            );
        } else {
            for burst in view.bursts.iter().take(5) {
                let line = format!(
                    " {}–{}   {:>2}×   {:>4}",
                    timeutil::format_hhmm(burst.start_ts),
                    timeutil::format_hhmm(burst.end_ts),
                    burst.count,
                    timeutil::format_minutes(burst.seconds)
                );
                emit(&mut out, &line, RowMeta::NonSelectable, mode);
            }
            if view.remaining_sessions > 0 {
                emit(
                    &mut out,
                    &format!(
                        " + {} longer sessions   {}",
                        view.remaining_sessions,
                        timeutil::format_duration(view.remaining_seconds)
                    ),
                    RowMeta::NonSelectable,
                    mode,
                );
            }
        }
    }

    emit(&mut out, "", RowMeta::NonSelectable, mode);
    emit(
        &mut out,
        &trend_line(&view.trend, view.trend_avg),
        RowMeta::NonSelectable,
        mode,
    );
    emit(&mut out, "", RowMeta::NonSelectable, mode);
    emit(
        &mut out,
        " ←/→ day   esc back",
        RowMeta::App("__back__".to_owned()),
        mode,
    );
    out
}

fn row_line(
    layout: &Layout,
    name: &str,
    seconds: i64,
    strip: &str,
    sessions: Option<i64>,
    mode: OutputMode,
) -> String {
    let mut fitted = display_name(name, 10);
    if mode == OutputMode::Rofi {
        fitted = escape_markup(&fitted);
    }
    let duration = timeutil::format_duration_compact(seconds);
    let prefix = if layout.session_count {
        format!(
            " {}{:>6} ×{:<2} ",
            fitted,
            duration,
            sessions.unwrap_or(0).min(99)
        )
    } else {
        format!(" {}{:>6}   ", fitted, duration)
    };
    format!("{prefix}{strip}")
}

fn other_line(layout: &Layout, other: &OtherRow, strip: &str) -> String {
    let label = format!("+{} apps", other.count);
    let mut fitted = display_name(&label, 10);
    if layout.session_count {
        fitted.push(' ');
    }
    let duration = timeutil::format_duration_compact(other.seconds);
    let prefix = format!(" {}{:>6}   ", fitted, duration);
    format!("{prefix}{strip}")
}

fn escape_markup(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn pad_line(left: &str, right: &str, width: usize) -> String {
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    let padding = width.saturating_sub(left_len + right_len);
    format!("{left}{}{right}", " ".repeat(padding))
}

fn ruler_wide() -> String {
    let mut line = [' '; WIDE_TIMELINE];
    for (position, label) in [
        (0usize, "00"),
        (24, "06"),
        (48, "12"),
        (72, "18"),
        (94, "24"),
    ] {
        for (offset, character) in label.chars().enumerate() {
            if position + offset < WIDE_TIMELINE {
                line[position + offset] = character;
            }
        }
    }
    line.iter().collect()
}

fn ruler_narrow() -> String {
    let mut line = [' '; NARROW_TIMELINE];
    for (position, label) in [
        (0usize, "00"),
        (12, "06"),
        (24, "12"),
        (36, "18"),
        (46, "24"),
    ] {
        for (offset, character) in label.chars().enumerate() {
            if position + offset < NARROW_TIMELINE {
                line[position + offset] = character;
            }
        }
    }
    line.iter().collect()
}

fn strip(
    slots: &[bool; SLOT_COUNT],
    glyph: &str,
    slot_width: usize,
    mode: OutputMode,
    color: &str,
) -> String {
    let mut value = String::with_capacity(SLOT_COUNT * slot_width);
    for active in slots.iter().take(SLOT_COUNT) {
        if *active {
            for _ in 0..slot_width {
                value.push_str(glyph);
            }
        } else {
            for _ in 0..slot_width {
                value.push(' ');
            }
        }
    }
    if mode == OutputMode::Rofi {
        format!("<span foreground=\"{color}\">{value}</span>")
    } else {
        value
    }
}

fn slots_active_i64(slots: &[i64; SLOT_COUNT]) -> [bool; SLOT_COUNT] {
    let mut active = [false; SLOT_COUNT];
    for (index, seconds) in slots.iter().enumerate() {
        active[index] = *seconds >= 60;
    }
    active
}

fn trend_line(trend: &[Option<i64>], average: Option<i64>) -> String {
    let values: Vec<Option<i64>> = trend.to_vec();
    if values.iter().all(Option::is_none) {
        return " 7d no history".to_owned();
    }
    let max = values.iter().filter_map(|value| *value).max().unwrap_or(0);
    let bars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut sparkline = String::new();
    for value in values {
        match value {
            Some(seconds) if seconds > 0 && max > 0 => {
                let index = ((seconds as f64 / max as f64) * 7.0).round() as usize;
                sparkline.push(bars[index.min(7)]);
            }
            _ => sparkline.push('·'),
        }
    }
    match average {
        Some(average) => format!(
            " 7d {sparkline}  avg {}",
            timeutil::format_duration(average)
        ),
        None => format!(" 7d {sparkline}"),
    }
}

enum RowMeta {
    NonSelectable,
    App(String),
}

fn emit(out: &mut String, line: &str, meta: RowMeta, mode: OutputMode) {
    out.push_str(line);
    if mode == OutputMode::Rofi {
        match meta {
            RowMeta::NonSelectable => out.push_str("\0nonselectable\x1ftrue"),
            RowMeta::App(app) => {
                let safe = app.replace(['\x1f', '\n'], " ");
                out.push_str("\0info\x1f");
                out.push_str(&safe);
            }
        }
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::AppRow;
    use chrono::NaiveDate;

    fn sample_view() -> DayView {
        let date = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let mut slots = [false; SLOT_COUNT];
        slots[18] = true;
        slots[19] = true;
        DayView {
            date,
            today: date,
            is_today: true,
            stale: false,
            active_seconds: 6 * 3600 + 48 * 60,
            powered_seconds: 12 * 3600 + 40 * 60,
            first_active_ts: None,
            last_active_ts: None,
            peak: None,
            delta_seconds: Some(38 * 60),
            usual_seconds: Some(6 * 3600 + 10 * 60),
            history_days: 4,
            trend: vec![Some(1000), None, Some(2000), None, None, None, Some(3000)],
            trend_avg: Some(6 * 3600 + 12 * 60),
            power_slots: slots,
            rows: vec![AppRow {
                name: "Code".to_owned(),
                seconds: 2 * 3600 + 14 * 60,
                sessions: 18,
                slots,
            }],
            other: None,
            last_commit_at: None,
        }
    }

    #[test]
    fn main_lines_are_116_columns_or_less() {
        let output = render_main(&sample_view(), OutputMode::Plain);
        for line in output.lines() {
            assert!(
                line.chars().count() <= 116,
                "line is too wide ({}): {line}",
                line.chars().count()
            );
        }
        assert!(output.contains("00"));
        assert!(output.contains("POWER"));
        assert!(output.contains("██"));
        assert!(output.contains("▓▓"));
    }

    #[test]
    fn wide_ruler_is_96_columns() {
        assert_eq!(ruler_wide().chars().count(), 96);
        assert_eq!(ruler_narrow().chars().count(), 48);
    }

    #[test]
    fn session_count_is_off_by_default() {
        let output = render_main(&sample_view(), OutputMode::Plain);
        assert!(!output.contains("×18"));
    }
}
