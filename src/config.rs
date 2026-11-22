use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Optional user configuration. A missing file is valid and equivalent to
/// `Config::default()` (auto theme mode, automatic layout, default font).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeConfig,
    pub font: FontConfig,
    pub layout: LayoutConfig,
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
        let home = Path::new("/home/will");
        assert_eq!(
            expand_path("~/.config/x", home),
            PathBuf::from("/home/will/.config/x")
        );
        assert_eq!(
            expand_path("relative/x", home),
            PathBuf::from("/home/will/relative/x")
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
}
