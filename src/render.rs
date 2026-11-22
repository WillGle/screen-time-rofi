use crate::config::{Config, LayoutMode};
use crate::sessions::{display_label, display_name};
use crate::theme::Theme;
use crate::timeutil::{self, SLOT_COUNT};
use crate::view::{APP_PAGE_SIZE, AppRow, DayView, DetailView, OtherRow, TOP_APPS};

const NAME_WIDTH: usize = 16;
const PREFIX_WIDTH: usize = 24;
const STATS_WIDTH: usize = 24;
const SESSION_STATS_WIDTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Plain,
    Rofi,
}

#[derive(Debug, Clone, Copy)]
struct Layout {
    /// Columns used to draw one half-hour timeline slot.
    cell_width: usize,
    session_count: bool,
}

impl Layout {
    fn from_theme(theme: &Theme) -> Self {
        let session_count = cfg!(feature = "session-count")
            || std::env::var("SCREEN_TIME_SESSION_COUNT").as_deref() == Ok("1");
        let config = Config::load_default().unwrap_or_default();
        let screen_width = std::env::var("SCREEN_TIME_SCREEN_WIDTH")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|width| *width >= 640)
            .unwrap_or(1280);
        let cell_width = if std::env::var("SCREEN_TIME_NARROW").as_deref() == Ok("1") {
            1
        } else {
            match config.layout_mode() {
                LayoutMode::Full => 2,
                LayoutMode::Compact => 1,
                LayoutMode::Auto => auto_cell_width(theme.font_size, screen_width, session_count),
            }
        };
        Self {
            cell_width,
            session_count,
        }
    }

    fn timeline_width(&self) -> usize {
        SLOT_COUNT * self.cell_width
    }

    fn stats_width(&self) -> usize {
        STATS_WIDTH
            + if self.session_count {
                SESSION_STATS_WIDTH
            } else {
                0
            }
    }

    fn screen_width(&self) -> usize {
        PREFIX_WIDTH + self.timeline_width() + self.stats_width()
    }

    fn top_apps(&self) -> usize {
        TOP_APPS
    }
}

/// Pick the widest timeline that still fits the focused monitor. One column
/// per half-hour slot is the compact fallback; wider monitors get two columns
/// per slot so the timeline uses the available width instead of staying small.
/// Explicit full/compact modes still override auto detection.
fn auto_cell_width(font_size: u8, screen_width: u32, session_count: bool) -> usize {
    (1..=2)
        .rev()
        .find(|cell_width| fits_layout(*cell_width, font_size, screen_width, session_count))
        .unwrap_or(1)
}

fn fits_layout(cell_width: usize, font_size: u8, screen_width: u32, session_count: bool) -> bool {
    let columns = PREFIX_WIDTH
        + SLOT_COUNT * cell_width
        + STATS_WIDTH
        + if session_count {
            SESSION_STATS_WIDTH
        } else {
            0
        };
    let char_px = f64::from(font_size) * (96.0 / 72.0) * 0.75;
    columns as f64 * char_px + 64.0 <= f64::from(screen_width) * 0.94
}

pub(crate) fn grid_columns(theme: &Theme) -> usize {
    Layout::from_theme(theme).screen_width()
}

pub fn render_main(view: &DayView, mode: OutputMode) -> String {
    render_main_with_theme(view, mode, &Theme::load())
}

pub(crate) fn render_main_with_theme(view: &DayView, mode: OutputMode, theme: &Theme) -> String {
    let layout = Layout::from_theme(theme);
    let width = layout.screen_width();
    let page_count = view.rows.len().div_ceil(APP_PAGE_SIZE).max(1);
    let paginated = mode == OutputMode::Rofi && page_count > 1;
    let page = if mode == OutputMode::Rofi {
        view.page.min(page_count - 1)
    } else {
        0
    };
    let page_size = if mode == OutputMode::Rofi {
        APP_PAGE_SIZE
    } else {
        layout.top_apps()
    };
    let page_start = page * page_size;
    let page_end = (page_start + page_size).min(view.rows.len());
    let page_rows = &view.rows[page_start..page_end];
    // Keep the main popup at a stable five-row height while paginated, even
    // when the last page has fewer apps. This prevents a detail view from
    // leaving the main view cropped after returning.
    let list_slots = if paginated {
        APP_PAGE_SIZE
    } else {
        page_rows.len()
    };
    let show_other = view.other.is_some() && page_end == view.rows.len();
    let mut out = String::new();
    if mode == OutputMode::Rofi {
        out.push_str("\0use-hot-keys\x1ftrue\n");
        out.push_str("\0markup-rows\x1ftrue\n");
        out.push_str("\0no-custom\x1ftrue\n");
        out.push_str("\0keep-selection\x1ftrue\n");
    }

    let total = timeutil::format_duration(view.active_seconds);
    let title = styled_span("SCREEN TIME", &theme.accent, Some("700"), mode);
    let total_line = if mode == OutputMode::Rofi {
        format!(" <span size=\"26pt\" weight=\"700\">{total}</span>")
    } else {
        format!(" {total} active")
    };

    let long_date = view.date.format("%A, %-d %B %Y").to_string();
    let relative = if view.is_today {
        "Today · "
    } else if view.date.succ_opt() == Some(view.today) {
        "Yesterday · "
    } else {
        ""
    };
    let stale = if view.stale { " · stale" } else { "" };
    let date = role_span(&format!("{relative}{long_date}{stale}"), &theme.muted, mode);

    let peak = view
        .peak
        .map(|(start, end)| timeutil::format_span(start, end))
        .unwrap_or_else(|| "no peak".to_owned());
    let summary = if let Some(top) = view.rows.first() {
        let category = AppCategory::from_app(&top.name);
        let dot = role_span("●", category.color(theme), mode);
        let top_label = display_label(&top.name);
        let name = styled_span(
            &markup_text(&top_label, mode),
            &theme.text,
            Some("600"),
            mode,
        );
        let duration = styled_span(
            &timeutil::format_duration_compact(top.seconds),
            &theme.text,
            Some("600"),
            mode,
        );
        format!(
            " {} {dot} {name} {duration} {}",
            role_span("Top app", &theme.muted, mode),
            role_span(&format!("· peak {peak}"), &theme.muted, mode)
        )
    } else {
        format!(" No activity recorded · peak {peak}")
    };
    if mode == OutputMode::Rofi {
        // The message widget measures mixed font sizes independently of list rows.
        out.push_str(&format!(
            "\0message\x1f <span size=\"{}pt\">{title}</span>    {total_line}    <span size=\"{}pt\">{date}</span>\n",theme.font_size,theme.font_size
        ));
        let extra = usize::from(show_other || list_slots == 0);
        out.push_str(&format!("\0active\x1f2-5,7-{}\n", 7 + list_slots + extra));
        out.push_str(&format!(
            "\0theme\x1flistview {{ lines: {}; }}\n",
            10 + list_slots + extra
        ));
        emit(&mut out, &summary, RowMeta::NonSelectable, mode);
    } else {
        out.push_str(&format!(" {title}\n{total_line}\n {date}\n{summary}\n"));
    }
    emit_spacer(&mut out, mode);

    emit(
        &mut out,
        &section_header("DAY ACTIVITY", &format!("peak {peak}"), width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );
    let ruler_line = format!(
        "{}{}",
        " ".repeat(PREFIX_WIDTH),
        ruler(layout.timeline_width())
    );
    emit(
        &mut out,
        &panel_line(&ruler_line, width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );

    // The shared strip uses the dominant (highest-ranked) app category in
    // each slot. Every row below uses the same origin and slot width.
    let other = aggregate_other(view, layout.top_apps());
    let day_strip = day_strip(view, &other, theme, &layout, mode);
    let day_line = format!("{}{}", " ".repeat(PREFIX_WIDTH), day_strip);
    emit(
        &mut out,
        &panel_line(&day_line, width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );
    emit(
        &mut out,
        &panel_line(&power_line(view, theme, &layout, mode), width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );
    emit_spacer(&mut out, mode);

    let app_count = view.rows.len() + view.other.as_ref().map_or(0, |row| row.count);
    let timeline_meta = if paginated {
        format!(
            "apps {}-{} of {} · {}",
            page_start + 1,
            page_end.max(page_start + 1),
            view.rows.len(),
            total
        )
    } else {
        format!(
            "top {} of {} apps · {}",
            view.rows.len().min(layout.top_apps()),
            app_count,
            total
        )
    };
    emit(
        &mut out,
        &section_header("APP TIMELINE", &timeline_meta, width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );

    for (offset, row) in page_rows.iter().enumerate() {
        let index = page_start + offset;
        emit(
            &mut out,
            &panel_line(
                &app_line(index + 1, row, view.active_seconds, theme, &layout, mode),
                width,
                theme,
                mode,
            ),
            RowMeta::App(row.name.clone()),
            mode,
        );
    }
    if show_other {
        if let Some(other) = &other {
            emit(
                &mut out,
                &panel_line(
                    &other_line(other, view.active_seconds, theme, &layout, mode),
                    width,
                    theme,
                    mode,
                ),
                RowMeta::NonSelectable,
                mode,
            );
        }
    }
    if view.rows.is_empty() && view.active_seconds == 0 {
        emit(
            &mut out,
            &panel_line(" No activity recorded today", width, theme, mode),
            RowMeta::App("__none__".to_owned()),
            mode,
        );
    }

    emit_spacer(&mut out, mode);
    emit(
        &mut out,
        &format!(
            " {}",
            role_span(
                if mode == OutputMode::Rofi && paginated {
                    "←/→ day   Alt+↓/↑ page   T today   R refresh   ↵ detail   esc close   Alt+1–4 theme"
                } else if mode == OutputMode::Rofi {
                    "←/→ day   T today   R refresh   ↵ detail   esc close   Alt+1–4 theme"
                } else {
                    "←/→ day   T today   R refresh   ↵ detail   esc close"
                },
                &theme.muted,
                mode,
            )
        ),
        RowMeta::NonSelectable,
        mode,
    );
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppCategory {
    Browser,
    Work,
    Communication,
    Entertainment,
    System,
    Unknown,
}

impl AppCategory {
    fn from_app(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        let matches = |apps: &[&str]| {
            apps.iter().any(|app| {
                name == *app
                    || name
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .any(|part| part == *app)
            })
        };
        if matches(&[
            "brave",
            "firefox",
            "chrome",
            "chromium",
            "librewolf",
            "floorp",
            "zen browser",
        ]) {
            Self::Browser
        } else if matches(&[
            "signal",
            "slack",
            "discord",
            "zoom",
            "teams",
            "telegram",
            "thunderbird",
        ]) {
            Self::Communication
        } else if matches(&[
            "steam", "cs2", "music", "spotify", "vlc", "mpv", "gimp", "game",
        ]) {
            Self::Entertainment
        } else if matches(&[
            "code",
            "vscode",
            "zed",
            "foot",
            "terminal",
            "kitty",
            "alacritty",
            "wezterm",
            "neovim",
            "libreoffice",
            "zotero",
        ]) {
            Self::Work
        } else if matches(&[
            "files",
            "nautilus",
            "portal",
            "xdg",
            "dialog",
            "zenity",
            "settings",
            "blueman",
            "bluetooth",
            "system monitor",
            "task manager",
        ]) {
            Self::System
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Browser => "Browser",
            Self::Work => "Work",
            Self::Communication => "Comm",
            Self::Entertainment => "Leisure",
            Self::System => "System",
            Self::Unknown => "Unknown",
        }
    }

    fn color(self, theme: &Theme) -> &str {
        match self {
            Self::Browser => &theme.browser,
            Self::Work => &theme.work,
            Self::Communication => &theme.communication,
            Self::Entertainment => &theme.entertainment,
            Self::System => &theme.muted,
            Self::Unknown => &theme.other,
        }
    }
}

fn aggregate_other(view: &DayView, visible: usize) -> Option<OtherRow> {
    let mut count = view.other.as_ref().map_or(0, |row| row.count);
    let mut seconds = view.other.as_ref().map_or(0, |row| row.seconds);
    let mut slots_i64 = [0i64; SLOT_COUNT];
    if let Some(other) = &view.other {
        for (index, active) in other.slots.iter().enumerate() {
            if *active {
                slots_i64[index] += 60;
            }
        }
    }
    for row in view.rows.iter().skip(visible) {
        count += 1;
        seconds += row.seconds;
        for (index, active) in row.slots.iter().enumerate() {
            if *active {
                slots_i64[index] += 60;
            }
        }
    }
    (count > 0).then(|| OtherRow {
        count,
        seconds,
        slots: slots_active_i64(&slots_i64),
    })
}

fn day_strip(
    view: &DayView,
    other: &Option<OtherRow>,
    theme: &Theme,
    layout: &Layout,
    mode: OutputMode,
) -> String {
    let mut out = String::new();
    for slot in 0..SLOT_COUNT {
        let category = view
            .rows
            .iter()
            .take(layout.top_apps())
            .find(|row| row.slots.get(slot).copied().unwrap_or(false))
            .map(|row| AppCategory::from_app(&row.name));
        let (glyph, color) = if let Some(category) = category {
            ("█", category.color(theme))
        } else if other
            .as_ref()
            .is_some_and(|row| row.slots.get(slot).copied().unwrap_or(false))
        {
            ("▒", theme.other.as_str())
        } else {
            (" ", theme.other.as_str())
        };
        let value = glyph.repeat(layout.cell_width);
        if mode == OutputMode::Rofi && glyph != " " {
            out.push_str(&format!(
                "<span background=\"{color}\">{}</span>",
                " ".repeat(layout.cell_width)
            ));
        } else {
            out.push_str(&value);
        }
    }
    out
}

fn power_line(view: &DayView, theme: &Theme, layout: &Layout, mode: OutputMode) -> String {
    let dot = role_span("■", &theme.power, mode);
    let name = role_span(
        &format!("{:<width$}", "POWER", width = NAME_WIDTH),
        &theme.muted,
        mode,
    );
    let power_strip = strip(&view.power_slots, "▓", layout, mode, &theme.power);
    let duration = styled_span(
        &format!(
            "{:>7}",
            timeutil::format_duration_compact(view.powered_seconds)
        ),
        &theme.text,
        Some("600"),
        mode,
    );
    let session_padding = if layout.session_count {
        " ".repeat(SESSION_STATS_WIDTH)
    } else {
        String::new()
    };
    format!(
        "    {dot} {name}  {power_strip}  {duration}{session_padding}  {}  {}",
        " ".repeat(4),
        " ".repeat(7)
    )
}

fn app_line(
    rank: usize,
    row: &AppRow,
    total: i64,
    theme: &Theme,
    layout: &Layout,
    mode: OutputMode,
) -> String {
    let category = AppCategory::from_app(&row.name);
    let color = category.color(theme);
    let rank = role_span(&format!("{rank:>2}"), &theme.muted, mode);
    let dot = role_span("●", color, mode);
    let label = display_label(&row.name);
    let name = format!(
        "{:<width$}",
        display_name(&label, NAME_WIDTH),
        width = NAME_WIDTH
    );
    let name = styled_span(&markup_text(&name, mode), &theme.text, Some("600"), mode);
    let spark = strip(&row.slots, "█", layout, mode, color);
    let share = percentage(row.seconds, total);
    let duration = styled_span(
        &format!("{:>7}", timeutil::format_duration_compact(row.seconds)),
        &theme.text,
        Some("600"),
        mode,
    );
    let sessions = if layout.session_count {
        role_span(
            &format!(" ×{:<3}", row.sessions.min(99)),
            &theme.muted,
            mode,
        )
    } else {
        String::new()
    };
    let share = role_span(&format!("{share:>3}%"), &theme.muted, mode);
    let category_label = role_span(&format!("{:>7}", category.label()), color, mode);
    format!(" {rank} {dot} {name}  {spark}  {duration}{sessions}  {share}  {category_label}")
}

fn other_line(
    other: &OtherRow,
    total: i64,
    theme: &Theme,
    layout: &Layout,
    mode: OutputMode,
) -> String {
    let label = if other.count == 1 {
        "+1 more app".to_owned()
    } else {
        format!("+{} more apps", other.count)
    };
    let dot = role_span("●", &theme.other, mode);
    let name = role_span(
        &format!(
            "{:<width$}",
            display_name(&label, NAME_WIDTH),
            width = NAME_WIDTH
        ),
        &theme.muted,
        mode,
    );
    let spark = strip(&other.slots, "▒", layout, mode, &theme.other);
    let share = percentage(other.seconds, total);
    let duration = styled_span(
        &format!("{:>7}", timeutil::format_duration_compact(other.seconds)),
        &theme.text,
        Some("600"),
        mode,
    );
    let session_padding = if layout.session_count {
        " ".repeat(SESSION_STATS_WIDTH)
    } else {
        String::new()
    };
    let share = role_span(&format!("{share:>3}%"), &theme.muted, mode);
    let category = role_span("  Mixed", &theme.other, mode);
    format!("    {dot} {name}  {spark}  {duration}{session_padding}  {share}  {category}")
}

fn percentage(seconds: i64, total: i64) -> i64 {
    if total > 0 {
        ((seconds.max(0) as f64 / total as f64) * 100.0).round() as i64
    } else {
        0
    }
}

pub fn render_detail(view: &DetailView, mode: OutputMode) -> String {
    render_detail_with_theme(view, mode, &Theme::load())
}

pub(crate) fn render_detail_with_theme(
    view: &DetailView,
    mode: OutputMode,
    theme: &Theme,
) -> String {
    let layout = Layout::from_theme(theme);
    let width = layout.screen_width();
    let category = AppCategory::from_app(&view.app);
    let color = category.color(theme);
    let label = display_label(&view.app);
    let app = display_name(&label, if layout.cell_width == 1 { 16 } else { 24 });
    let name = markup_text(app.trim_end(), mode);
    let title = styled_span(&format!("● {name}"), color, Some("700"), mode);
    let total = timeutil::format_duration(view.total_seconds);
    let share = format!("{}% of active time", view.share_percent);
    let date = timeutil::format_date_label(view.date, view.today);
    let subtitle = format!("{} · {share} · {date}", category.label());

    let mut body = String::new();
    let stats = format!(
        " {} sessions    median {}    {} switches/h",
        view.session_count,
        timeutil::format_minutes(view.median_seconds),
        view.switches_per_hour
    );
    emit(&mut body, &stats, RowMeta::NonSelectable, mode);

    let through = view
        .sessions_through
        .map(|value| value.format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_owned());
    let stale = if view.stale { " · stale live" } else { "" };
    emit(
        &mut body,
        &format!(
            " {}",
            role_span(
                &format!("sessions through {through} · may lag one batch{stale}"),
                &theme.muted,
                mode
            )
        ),
        RowMeta::NonSelectable,
        mode,
    );
    emit_spacer(&mut body, mode);

    let activity_start = body.lines().count();
    emit(
        &mut body,
        &section_header("APP ACTIVITY", "", width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );
    let mut slots = [0; SLOT_COUNT];
    for session in &view.sessions {
        timeutil::add_interval_to_slots(&mut slots, view.date, session.start_ts, session.end_ts);
    }
    emit(
        &mut body,
        &format!(
            "{}{}",
            " ".repeat(PREFIX_WIDTH),
            ruler(layout.timeline_width())
        ),
        RowMeta::NonSelectable,
        mode,
    );
    let activity = if slots.iter().any(|&seconds| seconds > 0) {
        let label = role_span(" Sessions", &theme.muted, mode);
        format!(
            "{label}{}{}",
            " ".repeat(PREFIX_WIDTH - " Sessions".len()),
            strip(&slots.map(|seconds| seconds > 0), "█", &layout, mode, color)
        )
    } else {
        " No sessions recorded".to_owned()
    };
    emit(&mut body, &activity, RowMeta::NonSelectable, mode);
    let activity_end = body.lines().count() - 1;
    emit_spacer(&mut body, mode);

    let burst_start = body.lines().count();
    emit(
        &mut body,
        &section_header("BURSTS", "", width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );
    if let Some(error) = &view.db_error {
        emit(
            &mut body,
            " Database unavailable",
            RowMeta::NonSelectable,
            mode,
        );
        emit(
            &mut body,
            &format!(" {}", markup_text(error, mode)),
            RowMeta::NonSelectable,
            mode,
        );
    } else {
        let bursts = view.bursts.iter().take(5);
        if let Some(longest) = bursts.clone().map(|burst| burst.seconds).max() {
            emit(
                &mut body,
                &format!(
                    " {}",
                    role_span(
                        "Time range      Sessions  Active     Relative duration",
                        &theme.muted,
                        mode
                    )
                ),
                RowMeta::NonSelectable,
                mode,
            );
            for burst in bursts {
                let length = (burst.seconds * 28 / longest.max(1)).clamp(1, 28) as usize;
                let bar = if mode == OutputMode::Rofi {
                    format!("<span background=\"{color}\">{}</span>", " ".repeat(length))
                } else {
                    "█".repeat(length)
                };
                let line = format!(
                    " {}–{}      {:>3}×    {:>7}     {bar}",
                    timeutil::format_hhmm(burst.start_ts),
                    timeutil::format_hhmm(burst.end_ts),
                    burst.count,
                    timeutil::format_duration_compact(burst.seconds),
                );
                emit(&mut body, &line, RowMeta::NonSelectable, mode);
            }
        } else {
            let message = if view.sessions.is_empty() {
                " Sessions will appear after the first batch."
            } else {
                " No bursts of repeated sessions."
            };
            emit(&mut body, message, RowMeta::NonSelectable, mode);
        }
        if view.remaining_sessions > 0 && !view.bursts.is_empty() {
            emit(
                &mut body,
                &format!(
                    " + {} other sessions   {}",
                    view.remaining_sessions,
                    timeutil::format_duration(view.remaining_seconds)
                ),
                RowMeta::NonSelectable,
                mode,
            );
        }
    }
    let burst_end = body.lines().count() - 1;
    emit_spacer(&mut body, mode);

    let trend_start = body.lines().count();
    let average = view.trend_avg.map_or_else(String::new, |average| {
        format!(
            "avg {} · previous days with usage",
            timeutil::format_duration(average)
        )
    });
    emit(
        &mut body,
        &section_header("7 DAY", &average, width, theme, mode),
        RowMeta::NonSelectable,
        mode,
    );
    if let Some(bars) = sparkline(&view.trend) {
        let dates = timeutil::week_dates(view.date, view.trend.len());
        let cell_width = (width - 2) / dates.len();
        let mut labels = String::from(" ");
        let mut graph = String::from(" ");
        let mut durations = String::from(" ");
        for ((day, seconds), bar) in dates.iter().zip(&view.trend).zip(bars.chars()) {
            let ink = if *day == view.date {
                color
            } else {
                &theme.muted
            };
            let day = day.format("%a %d").to_string();
            let total = seconds.map_or_else(|| "—".to_owned(), timeutil::format_duration_compact);
            labels.push_str(&role_span(&format!("{day:^cell_width$}"), ink, mode));
            let bar_color = if bar == '·' { &theme.muted } else { color };
            graph.push_str(&role_span(
                &format!("{:^cell_width$}", bar.to_string().repeat(3)),
                bar_color,
                mode,
            ));
            durations.push_str(&role_span(&format!("{total:^cell_width$}"), ink, mode));
        }
        for line in [labels, graph, durations] {
            emit(&mut body, &line, RowMeta::NonSelectable, mode);
        }
    } else {
        emit(
            &mut body,
            " No recorded history",
            RowMeta::NonSelectable,
            mode,
        );
    }
    let trend_end = body.lines().count() - 1;
    emit_spacer(&mut body, mode);
    emit(
        &mut body,
        if mode == OutputMode::Rofi {
            " ←/→ day   Backspace back   Esc close   Alt+1–4 theme"
        } else {
            " ←/→ day   Backspace back   Esc close"
        },
        RowMeta::App("__back__".to_owned()),
        mode,
    );

    if mode == OutputMode::Rofi {
        let font_size = theme.font_size;
        let lines = body.lines().count();
        format!(
            "\0message\x1f <span size=\"{font_size}pt\">{title}</span>  <span weight=\"700\">{total}</span>  <span size=\"{font_size}pt\" foreground=\"{}\">{subtitle}</span>\n\0theme\x1flistview {{ lines: {lines}; }}\n\0use-hot-keys\x1ftrue\n\0markup-rows\x1ftrue\n\0no-custom\x1ftrue\n\0active\x1f{activity_start}-{activity_end},{burst_start}-{burst_end},{trend_start}-{trend_end}\n\0keep-selection\x1ftrue\n\0new-selection\x1f{}\n{body}",
            theme.muted,
            lines - 1
        )
    } else {
        format!(" {title}   {total}   {subtitle}\n{body}")
    }
}

fn escape_markup(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markup_text(value: &str, mode: OutputMode) -> String {
    if mode == OutputMode::Rofi {
        escape_markup(value)
    } else {
        value.to_owned()
    }
}

fn styled_span(value: &str, color: &str, weight: Option<&str>, mode: OutputMode) -> String {
    if mode == OutputMode::Rofi {
        let weight = weight
            .map(|weight| format!(" weight=\"{weight}\""))
            .unwrap_or_default();
        format!("<span foreground=\"{color}\"{weight}>{value}</span>")
    } else {
        value.to_owned()
    }
}

fn panel_line(line: &str, width: usize, _theme: &Theme, mode: OutputMode) -> String {
    if mode == OutputMode::Rofi {
        return line.to_owned();
    }
    let padding = width.saturating_sub(visible_len(line));
    format!("{line}{}", " ".repeat(padding))
}

fn section_header(
    label: &str,
    meta: &str,
    width: usize,
    theme: &Theme,
    mode: OutputMode,
) -> String {
    let left_plain = format!(" ▌ {label}");
    let padding = width.saturating_sub(visible_len(&left_plain) + visible_len(meta));
    let marker = styled_span("▌", &theme.accent, Some("700"), mode);
    let title = styled_span(label, &theme.accent, Some("700"), mode);
    let meta = role_span(meta, &theme.muted, mode);
    let line = format!(" {marker} {title}{}{meta}", " ".repeat(padding));
    panel_line(&line, width, theme, mode)
}

/// Display width for our small Pango markup subset. Tags add no columns and
/// entities such as `&amp;` count as one character.
fn visible_len(value: &str) -> usize {
    let mut count = 0usize;
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '<' => {
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
            }
            '&' => {
                for next in chars.by_ref() {
                    if next == ';' {
                        break;
                    }
                }
                count += 1;
            }
            _ => count += 1,
        }
    }
    count
}

fn ruler(width: usize) -> String {
    let mut line = vec![' '; width + 2];
    for hour in (0..=24).step_by(2) {
        let position = width * hour / 24;
        let label = format!("{hour:02}");
        for (offset, character) in label.chars().enumerate() {
            line[position + offset] = character;
        }
    }
    line.iter().collect()
}

fn strip(
    slots: &[bool; SLOT_COUNT],
    glyph: &str,
    layout: &Layout,
    mode: OutputMode,
    color: &str,
) -> String {
    let mut value = String::with_capacity(layout.timeline_width());
    for active in slots.iter().take(SLOT_COUNT) {
        if !active {
            value.push_str(&" ".repeat(layout.cell_width));
        } else if mode == OutputMode::Rofi {
            value.push_str(&format!(
                "<span background=\"{color}\">{}</span>",
                " ".repeat(layout.cell_width)
            ));
        } else {
            value.push_str(&glyph.repeat(layout.cell_width));
        }
    }
    value
}

fn slots_active_i64(slots: &[i64; SLOT_COUNT]) -> [bool; SLOT_COUNT] {
    let mut active = [false; SLOT_COUNT];
    for (index, seconds) in slots.iter().enumerate() {
        active[index] = *seconds >= 60;
    }
    active
}

fn sparkline(trend: &[Option<i64>]) -> Option<String> {
    if trend.iter().all(Option::is_none) {
        return None;
    }
    let max = trend.iter().filter_map(|value| *value).max().unwrap_or(0);
    let bars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut value = String::new();
    for seconds in trend {
        match seconds {
            Some(seconds) if *seconds > 0 && max > 0 => {
                let index = ((*seconds as f64 / max as f64) * 7.0).round() as usize;
                value.push(bars[index.min(7)]);
            }
            _ => value.push('·'),
        }
    }
    Some(value)
}

fn role_span(value: &str, color: &str, mode: OutputMode) -> String {
    styled_span(value, color, None, mode)
}

enum RowMeta {
    NonSelectable,
    App(String),
}

fn emit_spacer(out: &mut String, mode: OutputMode) {
    let spacer = if mode == OutputMode::Rofi {
        "<span size=\"4pt\"> </span>"
    } else {
        ""
    };
    emit(out, spacer, RowMeta::NonSelectable, mode);
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
            page: 0,
            last_commit_at: None,
        }
    }

    fn sample_view_with_rows(count: usize) -> DayView {
        let mut view = sample_view();
        view.rows = (1..=count)
            .map(|index| AppRow {
                name: format!("App{index}"),
                seconds: (count + 1 - index) as i64 * 60,
                sessions: 1,
                slots: [false; SLOT_COUNT],
            })
            .collect();
        view.other = None;
        view
    }

    fn list_lines(output: &str) -> usize {
        output
            .lines()
            .find_map(|line| {
                line.strip_prefix("\0theme\x1flistview { lines: ")
                    .and_then(|value| value.strip_suffix("; }"))
                    .and_then(|value| value.parse().ok())
            })
            .expect("missing listview lines metadata")
    }

    #[test]
    fn app_timeline_is_paginated_four_rows_at_a_time() {
        let theme = Theme::resolve(&Config::default());
        let mut view = sample_view_with_rows(8);

        view.page = 0;
        let first = render_main_with_theme(&view, OutputMode::Rofi, &theme);
        for index in 1..=4 {
            assert!(
                first.contains(&format!("App{index}")),
                "page 1 missing App{index}"
            );
        }
        assert!(!first.contains("App5"));
        assert!(first.contains("apps 1-4 of 8"));
        assert_eq!(list_lines(&first), 14);

        view.page = 1;
        let second = render_main_with_theme(&view, OutputMode::Rofi, &theme);
        assert_eq!(
            second.matches("App1").count(),
            1,
            "page 2 must not repeat the first row"
        );
        for index in 5..=8 {
            assert!(
                second.contains(&format!("App{index}")),
                "page 2 missing App{index}"
            );
        }
        assert!(second.contains("apps 5-8 of 8"));
        assert_eq!(list_lines(&second), 14, "page height stays stable");
    }

    #[test]
    fn single_page_uses_exact_height_and_plain_output_lists_all_rows() {
        let theme = Theme::resolve(&Config::default());
        let view = sample_view_with_rows(3);
        let rofi = render_main_with_theme(&view, OutputMode::Rofi, &theme);
        assert_eq!(list_lines(&rofi), 13);
        assert!(!rofi.contains("Alt+↓/↑ page"));

        let plain = render_main_with_theme(&sample_view_with_rows(8), OutputMode::Plain, &theme);
        for index in 1..=8 {
            assert!(
                plain.contains(&format!("App{index}")),
                "plain missing App{index}"
            );
        }
    }

    #[test]
    fn main_lines_fit_the_default_1280_layout() {
        let output = render_main(&sample_view(), OutputMode::Plain);
        let max_width = if cfg!(feature = "session-count") {
            101
        } else {
            96
        };
        for line in output.lines() {
            assert!(
                line.chars().count() <= max_width,
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
    fn ruler_matches_each_timeline_width() {
        for width in [48, 96] {
            let ticks = ruler(width);
            assert_eq!(ticks.len(), width + 2);
            for hour in (0..=24).step_by(2) {
                let start = width * hour / 24;
                assert_eq!(&ticks[start..start + 2], format!("{hour:02}"));
            }
        }
    }

    #[test]
    fn compact_cells_merge_half_hour_slots() {
        let slots = sample_view().rows[0].slots;
        let compact = Layout {
            cell_width: 1,
            session_count: false,
        };
        let full = Layout {
            cell_width: 2,
            session_count: false,
        };
        assert_eq!(
            strip(&slots, "█", &compact, OutputMode::Plain, "#fff")
                .chars()
                .count(),
            48
        );
        assert_eq!(
            strip(&slots, "█", &full, OutputMode::Plain, "#fff")
                .chars()
                .count(),
            96
        );
    }

    #[test]
    fn app_categories_cover_common_fixture_apps() {
        assert_eq!(AppCategory::from_app("Firefox"), AppCategory::Browser);
        assert_eq!(AppCategory::from_app("Code"), AppCategory::Work);
        assert_eq!(AppCategory::from_app("Slack"), AppCategory::Communication);
        assert_eq!(AppCategory::from_app("Zoom"), AppCategory::Communication);
        assert_eq!(AppCategory::from_app("Music"), AppCategory::Entertainment);
        assert_eq!(AppCategory::from_app("Gimp"), AppCategory::Entertainment);
        assert_eq!(AppCategory::from_app("Files"), AppCategory::System);
        assert_eq!(AppCategory::from_app("Portal"), AppCategory::System);
        assert_eq!(AppCategory::from_app("System Dialog"), AppCategory::System);
        assert_eq!(AppCategory::from_app("Signal"), AppCategory::Communication);
        assert_eq!(AppCategory::from_app("CS2"), AppCategory::Entertainment);
        assert_eq!(AppCategory::from_app("Football"), AppCategory::Unknown);
        assert_eq!(
            AppCategory::from_app("org.mozilla.firefox"),
            AppCategory::Browser
        );
        assert_eq!(AppCategory::from_app("Notes"), AppCategory::Unknown);
    }

    #[test]
    fn auto_layout_prefers_full_slots_then_falls_back_to_compact() {
        assert_eq!(auto_cell_width(12, 1920, false), 2);
        assert_eq!(auto_cell_width(12, 2048, false), 2);
        assert_eq!(auto_cell_width(12, 2560, false), 2);
        assert_eq!(auto_cell_width(12, 1280, false), 1);
    }

    #[test]
    #[cfg(not(feature = "session-count"))]
    fn session_count_is_off_by_default() {
        let output = render_main(&sample_view(), OutputMode::Plain);
        assert!(!output.contains("×18"));
    }

    #[test]
    #[cfg(feature = "session-count")]
    fn session_count_feature_is_rendered() {
        let output = render_main(&sample_view(), OutputMode::Plain);
        assert!(output.contains("×18"));
    }
}
