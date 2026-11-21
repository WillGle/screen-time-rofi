use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::cli::RenderArgs;
use crate::paths::Paths;
use crate::render;
use crate::timeutil;
use crate::view;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiState {
    pub date: Option<String>,
    pub app: Option<String>,
}

pub fn handle_rofi(paths: &Paths, args: &RenderArgs, now: DateTime<Local>) -> Result<String> {
    let retv = std::env::var("ROFI_RETV")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(0);
    let today = now.date_naive();
    let mut state = read_state(paths).unwrap_or_default();

    if retv == 0 {
        state = UiState {
            date: None,
            app: None,
        };
        if let Some(date) = &args.date {
            state.date = Some(date.clone());
        }
        if let Some(app) = &args.app {
            state.app = Some(app.clone());
        }
        write_state(paths, &state)?;
    } else {
        match retv {
            1 => {
                if let Ok(info) = std::env::var("ROFI_INFO") {
                    if !info.is_empty() && info != "__back__" {
                        state.app = Some(info);
                    }
                }
            }
            10 => {
                state.app = None;
                state.date = Some(shift_date(&state, today, -1));
            }
            11 => {
                state.app = None;
                state.date = Some(shift_date(&state, today, 1));
            }
            12 => {
                state.app = None;
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
                state.date = None;
            }
            16 => {
                // Uppercase R refresh: keep the viewed date.
            }
            _ => {}
        }
        write_state(paths, &state)?;
    }

    let date = state
        .date
        .as_deref()
        .and_then(|value| timeutil::parse_date(value).ok())
        .unwrap_or(today)
        .min(today);
    let output = match state.app.as_deref() {
        Some(app) if !app.is_empty() => {
            let detail = view::load_detail(paths, date, app, now)?;
            render::render_detail(&detail, render::OutputMode::Rofi)
        }
        _ => {
            let day = view::load_main(paths, date, now)?;
            render::render_main(&day, render::OutputMode::Rofi)
        }
    };
    Ok(output)
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
    if let Some(parent) = paths.ui_state_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec(state).context("cannot serialize UI state")?;
    let temp = paths
        .ui_state_path
        .with_file_name(format!(".ui.json.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .with_context(|| format!("cannot create {}", temp.display()))?;
        file.write_all(&bytes)?;
        file.flush()?;
        drop(file);
        fs::rename(&temp, &paths.ui_state_path)
            .with_context(|| format!("cannot replace {}", paths.ui_state_path.display()))?;
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
        };
        assert_eq!(shift_date(&state, today, 1), "2026-11-21");
        assert_eq!(shift_date(&state, today, -1), "2026-11-20");
        state.date = Some("2026-11-19".to_owned());
        assert_eq!(shift_date(&state, today, 1), "2026-11-20");
    }
}
