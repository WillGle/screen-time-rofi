use crate::timeutil;

pub const SESSION_MERGE_GAP_SECONDS: i64 = 60;
pub const BURST_GAP_SECONDS: i64 = 2 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub date: String,
    pub app: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub seconds: i64,
    pub state: String,
    pub confidence: String,
}

impl Session {
    pub fn new(app: &str, date: String, start_ts: i64, end_ts: i64) -> Option<Self> {
        let seconds = end_ts - start_ts;
        if app.is_empty() || seconds <= 0 {
            return None;
        }
        Some(Self {
            date,
            app: app.to_owned(),
            start_ts,
            end_ts,
            seconds,
            state: "closed".to_owned(),
            confidence: "high".to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Burst {
    pub start_ts: i64,
    pub end_ts: i64,
    pub count: usize,
    pub seconds: i64,
}

/// Split one focus interval into per-local-day session rows.
pub fn split_session(app: &str, start_ts: i64, end_ts: i64) -> Vec<Session> {
    timeutil::split_interval_by_date(start_ts, end_ts)
        .into_iter()
        .filter_map(|(date, start, end)| {
            Session::new(app, date.format("%Y-%m-%d").to_string(), start, end)
        })
        .collect()
}

/// Merge adjacent same-app sessions separated by at most `max_gap` seconds.
/// Sessions must already be in chronological order for each app.
pub fn merge_adjacent(sessions: &mut Vec<Session>, max_gap: i64) {
    sessions.sort_by(|a, b| {
        (a.date.as_str(), a.app.as_str(), a.start_ts).cmp(&(
            b.date.as_str(),
            b.app.as_str(),
            b.start_ts,
        ))
    });

    let mut merged: Vec<Session> = Vec::with_capacity(sessions.len());
    for session in sessions.drain(..) {
        if let Some(previous) = merged.last_mut() {
            let gap = session.start_ts - previous.end_ts;
            if previous.app == session.app
                && previous.date == session.date
                && (0..=max_gap).contains(&gap)
            {
                previous.end_ts = previous.end_ts.max(session.end_ts);
                previous.seconds = previous.end_ts - previous.start_ts;
                if session.confidence == "low" {
                    previous.confidence = "low".to_owned();
                }
                continue;
            }
        }
        merged.push(session);
    }
    *sessions = merged;
}

/// Group sessions into readable bursts. The burst duration is the sum of its
/// session durations, not the wall-clock span (gaps under two minutes are not
/// counted as usage).
pub fn group_bursts(sessions: &[Session], max_gap: i64) -> Vec<Burst> {
    let mut sorted: Vec<&Session> = sessions.iter().collect();
    sorted.sort_by_key(|session| session.start_ts);
    let mut bursts: Vec<Burst> = Vec::new();

    for session in sorted {
        if let Some(last) = bursts.last_mut() {
            let gap = session.start_ts - last.end_ts;
            if (0..=max_gap).contains(&gap) {
                last.end_ts = last.end_ts.max(session.end_ts);
                last.count += 1;
                last.seconds += session.seconds;
                continue;
            }
        }
        bursts.push(Burst {
            start_ts: session.start_ts,
            end_ts: session.end_ts,
            count: 1,
            seconds: session.seconds,
        });
    }
    bursts
}

pub fn median_seconds(sessions: &[Session]) -> i64 {
    if sessions.is_empty() {
        return 0;
    }
    let mut values: Vec<i64> = sessions.iter().map(|session| session.seconds).collect();
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2
    } else {
        values[middle]
    }
}

pub fn switches_per_hour(session_count: usize, active_seconds: i64) -> i64 {
    if active_seconds <= 0 {
        return 0;
    }
    ((session_count as f64) / (active_seconds as f64 / 3600.0)).round() as i64
}

pub fn ignored_class(class: &str) -> bool {
    let lower = class.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "" | "desktop"
            | "hyprland"
            | "rofi"
            | "wofi"
            | "hyprlock"
            | "swaylock"
            | "gtklock"
            | "i3lock"
            | "xscreensaver"
            | "org.gnome.screensaver"
            | "lock"
    )
}

/// Convert a Hyprland WM_CLASS value to a stable display key. Window titles
/// never reach this function.
pub fn normalize_app(class: &str) -> Option<String> {
    let trimmed = class.trim();
    if trimmed.is_empty() || ignored_class(trimmed) {
        return None;
    }

    let mut name = trimmed.to_owned();
    loop {
        let before = name.clone();
        for suffix in ["-url-handler", "-wrapped", ".desktop"] {
            if let Some(stripped) = name.strip_suffix(suffix) {
                name = stripped.to_owned();
            }
        }
        name = name
            .trim_end_matches(|character: char| {
                character == '.' || character == '-' || character == '_'
            })
            .trim()
            .to_owned();
        if name == before {
            break;
        }
    }
    name = name
        .trim_start_matches(|character: char| {
            character == '.' || character == '-' || character == '_'
        })
        .trim()
        .to_owned();
    if name.is_empty() {
        return None;
    }

    let canonical = match name.to_ascii_lowercase().as_str() {
        "code" | "code - oss" | "code-oss" => Some("Code"),
        "vscodium" => Some("VSCodium"),
        "firefox" | "firefox-developer-edition" | "firefox-esr" | "org.mozilla.firefox" => {
            Some("Firefox")
        }
        "chromium" | "chromium-browser" | "org.chromium.chromium" => Some("Chromium"),
        "google-chrome" | "google-chrome-stable" | "com.google.chrome" => Some("Chrome"),
        "brave-browser" | "brave" | "com.brave.browser" => Some("Brave"),
        "kitty" => Some("Kitty"),
        "alacritty" => Some("Alacritty"),
        "foot" => Some("Foot"),
        "wezterm" | "org.wezfurlong.wezterm" => Some("WezTerm"),
        "org.gnome.terminal" | "gnome-terminal" | "gnome-terminal-server" => Some("Terminal"),
        "konsole" => Some("Konsole"),
        "slack" => Some("Slack"),
        "discord" | "com.discordapp.discord" => Some("Discord"),
        "signal" | "signal-desktop" | "org.signal.signal" => Some("Signal"),
        "telegram-desktop" | "telegram" | "org.telegram.desktop" => Some("Telegram"),
        "spotify" | "com.spotify.client" => Some("Spotify"),
        "mpv" => Some("mpv"),
        "vlc" => Some("VLC"),
        "obsidian" | "md.obsidian.obsidian" => Some("Obsidian"),
        "thunderbird" => Some("Thunderbird"),
        "nautilus" | "org.gnome.nautilus" => Some("Files"),
        "dolphin" => Some("Dolphin"),
        "zathura" => Some("Zathura"),
        "cs2" => Some("CS2"),
        "zenity" => Some("System Dialog"),
        "blueman-manager" | "blueman-applet" | "blueman-manager-wrapped" => Some("Blueman Manager"),
        "gnome-control-center" | "systemsettings" | "systemsettings5" | "org.gnome.settings" => {
            Some("Settings")
        }
        "org.gnome.calculator" | "gnome-calculator" => Some("Calculator"),
        "org.gnome.console" | "kgx" => Some("Terminal"),
        "org.gnome.texteditor" | "gnome-text-editor" => Some("Text Editor"),
        "org.gnome.eog" | "eog" => Some("Image Viewer"),
        "org.gnome.file-roller" | "file-roller" => Some("Archive Manager"),
        "org.gnome.diskutility" | "gnome-disks" => Some("Disks"),
        "org.gnome.systemmonitor" | "gnome-system-monitor" => Some("System Monitor"),
        "xdg-desktop-portal"
        | "xdg-desktop-portal-gtk"
        | "xdg-desktop-portal-gnome"
        | "xdg-desktop-portal-kde"
        | "xdg-desktop-portal-lxqt"
        | "xdg-desktop-portal-wlr"
        | "xdg-desktop-portal-hyprland" => Some("Portal"),
        _ => None,
    };
    if let Some(canonical) = canonical {
        return Some(canonical.to_owned());
    }

    // Reverse-DNS IDs such as `org.gnome.Calculator` use the last component
    // as the readable fallback.
    if let Some(last) = name.rsplit('.').next() {
        name = last.to_owned();
    }
    let words: Vec<&str> = name
        .split(|character: char| {
            character == '-' || character == '_' || character == '.' || character.is_whitespace()
        })
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return None;
    }
    let readable = words
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut capitalized = first.to_uppercase().to_string();
                    capitalized.push_str(chars.as_str());
                    capitalized
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if readable.is_empty() {
        None
    } else {
        Some(readable)
    }
}

/// User-facing label for stored app keys. Older databases contain WM_CLASS
/// names that were normalized before newer aliases existed.
pub fn display_label(app: &str) -> String {
    normalize_app(app).unwrap_or_else(|| app.to_owned())
}

pub fn display_name(app: &str, max_chars: usize) -> String {
    let count = app.chars().count();
    if count <= max_chars {
        format!("{app:<max_chars$}")
    } else if max_chars <= 1 {
        "…".to_owned()
    } else {
        let mut truncated: String = app.chars().take(max_chars - 1).collect();
        truncated.push('…');
        format!("{truncated:<max_chars$}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(app: &str, date: &str, start: i64, end: i64) -> Session {
        Session::new(app, date.to_owned(), start, end).unwrap()
    }

    #[test]
    fn merges_short_same_app_gaps() {
        let mut sessions = vec![
            session("Code", "2026-11-21", 1000, 1100),
            session("Code", "2026-11-21", 1140, 1200),
            session("Firefox", "2026-11-21", 1200, 1250),
        ];
        merge_adjacent(&mut sessions, 60);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].seconds, 200);
        assert_eq!(sessions[0].end_ts, 1200);
    }

    #[test]
    fn does_not_merge_long_gaps_or_different_apps() {
        let mut sessions = vec![
            session("Code", "2026-11-21", 1000, 1100),
            session("Code", "2026-11-21", 1161, 1200),
            session("Firefox", "2026-11-21", 1200, 1250),
        ];
        merge_adjacent(&mut sessions, 60);
        assert_eq!(sessions.len(), 3);
    }

    #[test]
    fn groups_bursts_and_counts_sessions() {
        let sessions = vec![
            session("Code", "2026-11-21", 1000, 1060),
            session("Code", "2026-11-21", 1100, 1120),
            session("Code", "2026-11-21", 1500, 1600),
        ];
        let bursts = group_bursts(&sessions, 120);
        assert_eq!(bursts.len(), 2);
        assert_eq!(bursts[0].count, 2);
        assert_eq!(bursts[0].seconds, 80);
        assert_eq!(bursts[1].count, 1);
    }

    #[test]
    fn median_handles_even_and_odd_counts() {
        let sessions = vec![
            session("Code", "2026-11-21", 0, 60),
            session("Code", "2026-11-21", 100, 220),
            session("Code", "2026-11-21", 300, 420),
        ];
        assert_eq!(median_seconds(&sessions), 120);
        let sessions = vec![
            session("Code", "2026-11-21", 0, 60),
            session("Code", "2026-11-21", 100, 220),
        ];
        assert_eq!(median_seconds(&sessions), 90);
    }

    #[test]
    fn app_names_are_normalized() {
        assert_eq!(normalize_app("code").as_deref(), Some("Code"));
        assert_eq!(normalize_app("code-url-handler").as_deref(), Some("Code"));
        assert_eq!(normalize_app("  MyApp  ").as_deref(), Some("MyApp"));
        assert_eq!(
            normalize_app("org.gnome.Nautilus").as_deref(),
            Some("Files")
        );
        assert_eq!(
            normalize_app("org.signal.Signal").as_deref(),
            Some("Signal")
        );
        assert_eq!(
            normalize_app("xdg-desktop-portal-gtk").as_deref(),
            Some("Portal")
        );
        assert_eq!(normalize_app("zenity").as_deref(), Some("System Dialog"));
        assert_eq!(normalize_app("cs2").as_deref(), Some("CS2"));
        assert_eq!(
            normalize_app(".blueman-manager-wrapped").as_deref(),
            Some("Blueman Manager")
        );
        assert_eq!(
            normalize_app("org.gnome.Calculator").as_deref(),
            Some("Calculator")
        );
        assert_eq!(
            normalize_app("my-helper_app").as_deref(),
            Some("My Helper App")
        );
        assert_eq!(normalize_app(""), None);
        assert_eq!(normalize_app("rofi"), None);
        assert_eq!(normalize_app("hyprlock"), None);
    }

    #[test]
    fn legacy_app_keys_get_readable_labels() {
        assert_eq!(display_label("org.signal.Signal"), "Signal");
        assert_eq!(display_label("Xdg-desktop-portal-gtk"), "Portal");
        assert_eq!(display_label("Zenity"), "System Dialog");
        assert_eq!(display_label("Cs2"), "CS2");
        assert_eq!(display_label("Foot"), "Foot");
        assert_eq!(display_label(".blueman-manager-wrapped"), "Blueman Manager");
    }

    #[test]
    fn long_names_truncate_at_ten() {
        assert_eq!(display_name("Firefox", 10), "Firefox   ");
        assert_eq!(display_name("A very long application", 10), "A very lo…");
    }

    #[test]
    fn sessions_split_at_midnight() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (start, end) = timeutil::day_bounds(date);
        let sessions = split_session("Code", start + 23 * 3600, end + 3600);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].date, "2026-11-21");
        assert_eq!(sessions[1].date, "2026-11-22");
    }
}
