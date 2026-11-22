use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::cli::RenderArgs;
use crate::paths::Paths;
use crate::render;
use crate::theme::Theme;
use crate::timeutil;
use crate::view;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiState {
    pub date: Option<String>,
    pub app: Option<String>,
    #[serde(default)]
    pub page: usize,
}

pub fn handle_rofi(paths: &Paths, args: &RenderArgs, now: DateTime<Local>) -> Result<String> {
    let retv = std::env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let today = now.date_naive();
    let mut state = read_state(paths).unwrap_or_default();

    if retv == 0 && !reopening() {
        state = UiState {
            date: None,
            app: None,
            page: 0,
        };
        if let Some(date) = &args.date {
            state.date = Some(date.clone());
        }
        if let Some(app) = &args.app {
            state.app = Some(app.clone());
        }
        write_state(paths, &state)?;
    } else if retv != 0 {
        let was_detail = state.app.is_some();
        match retv {
            1 => {
                if let Ok(info) = std::env::var("ROFI_INFO") {
                    match info.as_str() {
                        "" | "__none__" => {}
                        "__back__" => state.app = None,
                        _ => state.app = Some(info),
                    }
                }
            }
            10 => {
                state.app = None;
                state.page = 0;
                state.date = Some(shift_date(&state, today, -1));
            }
            11 => {
                state.app = None;
                state.page = 0;
                state.date = Some(shift_date(&state, today, 1));
            }
            12 => {
                state.app = None;
                state.page = 0;
                state.date = None;
            }
            13 => {
                // Refresh: keep the viewed date and reopen state.
            }
            14 => {
                state.app = None;
            }
            15 => {
                state.app = None;
                state.page = 0;
                state.date = None;
            }
            16 => {
                // Uppercase R refresh: keep the viewed date.
            }
            17 => write_theme_choice(paths, Some("nord"))?,
            18 => write_theme_choice(paths, Some("solarized"))?,
            19 => write_theme_choice(paths, Some("gruvbox"))?,
            20 => write_theme_choice(paths, None)?,
            21 => state.page = state.page.saturating_add(1),
            22 => state.page = state.page.saturating_sub(1),
            _ => {}
        }
        write_state(paths, &state)?;
        // The popup height is determined on open. Reload it when changing
        // between the app list and the taller detail view.
        if was_detail != state.app.is_some() || (17..=20).contains(&retv) {
            if let Some(request) = std::env::var_os("SCREEN_TIME_ROFI_THEME_REQUEST") {
                fs::write(request, b"reopen")?;
                return Ok(String::new());
            }
        }
    }

    let date = state
        .date
        .as_deref()
        .and_then(|value| timeutil::parse_date(value).ok())
        .unwrap_or(today)
        .min(today);
    let theme = Theme::load_with_mode(read_theme_choice(paths).as_deref());
    let output = match state.app.as_deref() {
        Some(app) if !app.is_empty() => {
            let detail = view::load_detail(paths, date, app, now)?;
            render::render_detail_with_theme(&detail, render::OutputMode::Rofi, &theme)
        }
        _ => {
            let mut day = view::load_main(paths, date, now)?;
            let page_count = day.rows.len().div_ceil(view::APP_PAGE_SIZE).max(1);
            if state.page >= page_count {
                state.page = page_count - 1;
                write_state(paths, &state)?;
            }
            day.page = state.page;
            render::render_main_with_theme(&day, render::OutputMode::Rofi, &theme)
        }
    };
    Ok(output)
}

pub(crate) fn launcher_theme() -> Result<Theme> {
    let paths = Paths::resolve()?;
    Ok(Theme::load_with_mode(read_theme_choice(&paths).as_deref()))
}

const THEME_CHOICES: [&str; 3] = ["nord", "solarized", "gruvbox"];

fn theme_choice_path(paths: &Paths) -> PathBuf {
    paths.state_dir.join("theme-choice")
}

fn read_theme_choice(paths: &Paths) -> Option<String> {
    let choice = fs::read_to_string(theme_choice_path(paths)).ok()?;
    let choice = choice.trim();
    THEME_CHOICES.contains(&choice).then(|| choice.to_owned())
}

fn write_theme_choice(paths: &Paths, choice: Option<&str>) -> Result<()> {
    let path = theme_choice_path(paths);
    if let Some(choice) = choice {
        write_atomic(&path, choice.as_bytes())
    } else {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("cannot remove {}", path.display())),
        }
    }
}

fn reopening() -> bool {
    std::env::var_os("SCREEN_TIME_ROFI_REOPEN").is_some()
}

fn shift_date(state: &UiState, today: NaiveDate, delta: i64) -> String {
    let current = state
        .date
        .as_deref()
        .and_then(|value| timeutil::parse_date(value).ok())
        .unwrap_or(today);
    let next = if delta < 0 {
        current
            .checked_sub_days(chrono::Days::new(delta.unsigned_abs()))
            .unwrap_or(current)
    } else {
        current
            .checked_add_days(chrono::Days::new(delta as u64))
            .unwrap_or(current)
            .min(today)
    };
    next.format("%Y-%m-%d").to_string()
}

fn read_state(paths: &Paths) -> Result<UiState> {
    let bytes = match fs::read(&paths.ui_state_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UiState::default());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("cannot read {}", paths.ui_state_path.display()));
        }
    };
    serde_json::from_slice(&bytes)
        .with_context(|| format!("cannot parse {}", paths.ui_state_path.display()))
}

fn write_state(paths: &Paths, state: &UiState) -> Result<()> {
    let bytes = serde_json::to_vec(state).context("cannot serialize UI state")?;
    write_atomic(&paths.ui_state_path, &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .with_context(|| format!("cannot create {}", temp.display()))?;
        file.write_all(bytes)?;
        file.flush()?;
        drop(file);
        fs::rename(&temp, path).with_context(|| format!("cannot replace {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Remove transient UI state on a clean daemon shutdown or uninstall.
pub fn remove_state(paths: &Paths) {
    let _ = fs::remove_file(&paths.ui_state_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_clamps_at_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let mut state = UiState {
            date: Some("2026-11-21".to_owned()),
            app: None,
            page: 0,
        };
        assert_eq!(shift_date(&state, today, 1), "2026-11-21");
        assert_eq!(shift_date(&state, today, -1), "2026-11-20");
        state.date = Some("2026-11-19".to_owned());
        assert_eq!(shift_date(&state, today, 1), "2026-11-20");
    }
}
