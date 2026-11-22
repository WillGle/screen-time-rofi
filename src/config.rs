use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Optional user configuration. A missing file is valid and equivalent to
/// `Config::default()` (auto theme mode, automatic layout, default font,
/// retention disabled).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeConfig,
    pub font: FontConfig,
    pub layout: LayoutConfig,
    pub retention: RetentionConfig,
    pub storage: StorageConfig,
}

/// Optional history pruning. Every field is opt-in: a missing field or `0`
/// keeps that category forever.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RetentionConfig {
    /// Raw focus/idle/boot events.
    pub events_days: Option<u32>,
    /// Closed session rows.
    pub sessions_days: Option<u32>,
    /// Daily per-app and per-slot aggregates.
    pub aggregates_days: Option<u32>,
    /// Closed power sessions.
    pub power_days: Option<u32>,
}

impl RetentionConfig {
    /// Categories with a positive day count. `None` and `0` mean keep all.
    pub fn enabled_days(&self) -> [(&'static str, Option<u32>); 4] {
        [
            ("events", normalize_days(self.events_days)),
            ("sessions", normalize_days(self.sessions_days)),
            ("aggregates", normalize_days(self.aggregates_days)),
            ("power", normalize_days(self.power_days)),
        ]
    }

    pub fn is_disabled(&self) -> bool {
        self.enabled_days().iter().all(|(_, days)| days.is_none())
    }

    /// A short stable summary used by `doctor` and log lines.
    pub fn summary(&self) -> String {
        if self.is_disabled() {
            return "disabled".to_owned();
        }
        self.enabled_days()
            .iter()
            .filter_map(|(name, days)| days.map(|days| format!("{name}={days}d")))
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn normalize_days(days: Option<u32>) -> Option<u32> {
    days.filter(|days| *days > 0)
}

/// Size thresholds used by `doctor` and the installer's disk check. These do
/// not delete anything; they only surface growth and block an update that
/// cannot fit two backups.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageConfig {
    pub warn_db_mib: u64,
    pub hard_db_mib: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            warn_db_mib: 250,
            hard_db_mib: 1024,
        }
    }
}

impl StorageConfig {
    pub fn warn_db_mib(&self) -> u64 {
        self.warn_db_mib.max(1)
    }

    pub fn hard_db_mib(&self) -> u64 {
        self.hard_db_mib.max(self.warn_db_mib.max(1))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub mode: Option<String>,
    pub rofi_theme: Option<String>,
    pub palette: Option<String>,
    pub static_env: Option<String>,

    // Optional explicit colors. They are an extension of the documented
    // schema so a user can fully pin a palette without touching Matugen.
    pub base: Option<String>,
    pub mantle: Option<String>,
    pub text: Option<String>,
    pub muted: Option<String>,
    pub accent: Option<String>,
    pub power: Option<String>,
    pub other: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: Option<String>,
    pub size: Option<u8>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    /// auto | full | compact
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Auto,
    Matugen,
    Static,
    Config,
    Nord,
    Solarized,
    Gruvbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Auto,
    Full,
    Compact,
}

impl Config {
    pub fn load_default() -> Result<Self> {
        Self::load(&default_config_path())
    }

    /// Load the default config, falling back to keep-all retention when the
    /// file is unreadable or malformed. The error is returned for logging.
    pub fn load_default_lenient() -> (Self, Option<anyhow::Error>) {
        match Self::load_default() {
            Ok(config) => (config, None),
            Err(error) => (Self::default(), Some(error)),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("cannot parse config {}", path.display()))
    }

    pub fn theme_mode(&self) -> ThemeMode {
        ThemeMode::parse(self.theme.mode.as_deref())
    }

    pub fn layout_mode(&self) -> LayoutMode {
        LayoutMode::parse(self.layout.mode.as_deref())
    }

    pub fn rofi_theme_path(&self, home: &Path) -> PathBuf {
        self.theme
            .rofi_theme
            .as_deref()
            .map(|value| expand_path(value, home))
            .unwrap_or_else(|| home.join(".config").join("rofi").join("theme.rasi"))
    }

    pub fn palette_path(&self, home: &Path) -> PathBuf {
        self.theme
            .palette
            .as_deref()
            .map(|value| expand_path(value, home))
            .unwrap_or_else(|| {
                home.join(".config")
                    .join("theme")
                    .join("generated")
                    .join("palette.json")
            })
    }

    pub fn static_env_path(&self, home: &Path) -> PathBuf {
        self.theme
            .static_env
            .as_deref()
            .map(|value| expand_path(value, home))
            .unwrap_or_else(|| home.join(".config").join("theme").join("static.env"))
    }

    pub fn explicit_colors(&self) -> [Option<&str>; 7] {
        [
            self.theme.base.as_deref(),
            self.theme.mantle.as_deref(),
            self.theme.text.as_deref(),
            self.theme.muted.as_deref(),
            self.theme.accent.as_deref(),
            self.theme.power.as_deref(),
            self.theme.other.as_deref(),
        ]
    }
}

impl ThemeMode {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
            "matugen" => Self::Matugen,
            "static" => Self::Static,
            "config" => Self::Config,
            "nord" | "dark" => Self::Nord,
            "solarized" | "light" => Self::Solarized,
            "gruvbox" | "high-contrast" => Self::Gruvbox,
            _ => Self::Auto,
        }
    }
}

impl LayoutMode {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
            "full" => Self::Full,
            "compact" => Self::Compact,
            _ => Self::Auto,
        }
    }
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_config_path() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home_dir().join(".config"));
    base.join("screen-time").join("config.toml")
}

pub fn expand_path(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home.join(rest);
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        home.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_config_is_valid_default() {
        let dir = tempdir().unwrap();
        let config = Config::load(&dir.path().join("missing.toml")).unwrap();
        assert_eq!(config.theme_mode(), ThemeMode::Auto);
        assert_eq!(config.layout_mode(), LayoutMode::Auto);
        assert!(config.font.family.is_none());
        assert!(config.font.size.is_none());
    }

    #[test]
    fn parses_schema_and_explicit_colors() {
        let config: Config = toml::from_str(
            r##"
            [theme]
            mode = "matugen"
            rofi_theme = "~/.config/rofi/theme.rasi"
            palette = "~/.config/theme/generated/palette.json"
            static_env = "~/.config/theme/static.env"
            accent = "#112233"

            [font]
            family = "JetBrainsMono Nerd Font"
            size = 14

            [layout]
            mode = "compact"
            "##,
        )
        .unwrap();
        assert_eq!(config.theme_mode(), ThemeMode::Matugen);
        assert_eq!(config.layout_mode(), LayoutMode::Compact);
        assert_eq!(
            config.font.family.as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(config.font.size, Some(14));
        assert_eq!(config.theme.accent.as_deref(), Some("#112233"));
    }

    #[test]
    fn expands_tilde_and_relative_paths() {
        let home = Path::new("/home/user");
        assert_eq!(
            expand_path("~/.config/x", home),
            PathBuf::from("/home/user/.config/x")
        );
        assert_eq!(
            expand_path("relative/x", home),
            PathBuf::from("/home/user/relative/x")
        );
        assert_eq!(expand_path("/abs/x", home), PathBuf::from("/abs/x"));
    }

    #[test]
    fn unknown_modes_fall_back_to_auto() {
        let config: Config =
            toml::from_str("[theme]\nmode = \"bogus\"\n[layout]\nmode = \"wide\"\n").unwrap();
        assert_eq!(config.theme_mode(), ThemeMode::Auto);
        assert_eq!(config.layout_mode(), LayoutMode::Auto);
    }

    #[test]
    fn retention_defaults_to_disabled() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.retention.is_disabled());
        assert_eq!(config.retention.summary(), "disabled");
    }

    #[test]
    fn retention_zero_means_keep_forever() {
        let config: Config = toml::from_str(
            "[retention]\nevents_days = 0\nsessions_days = 0\naggregates_days = 0\npower_days = 0\n",
        )
        .unwrap();
        assert!(config.retention.is_disabled());
        assert_eq!(config.retention.summary(), "disabled");
    }

    #[test]
    fn retention_parses_each_category() {
        let config: Config = toml::from_str(
            "[retention]\nevents_days = 7\nsessions_days = 30\naggregates_days = 365\npower_days = 90\n",
        )
        .unwrap();
        assert_eq!(
            config.retention.summary(),
            "events=7d,sessions=30d,aggregates=365d,power=90d"
        );
    }

    #[test]
    fn storage_thresholds_have_safe_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.storage.warn_db_mib(), 250);
        assert_eq!(config.storage.hard_db_mib(), 1024);
        let config: Config =
            toml::from_str("[storage]\nwarn_db_mib = 10\nhard_db_mib = 20\n").unwrap();
        assert_eq!(config.storage.warn_db_mib(), 10);
        assert_eq!(config.storage.hard_db_mib(), 20);
    }

    #[test]
    fn invalid_config_is_a_clear_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[retention\nevents_days = 1\n").unwrap();
        assert!(Config::load(&path).is_err());
    }
}
