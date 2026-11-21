use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{self, Config, ThemeMode};

pub const DEFAULT_FAMILY: &str = "JetBrainsMono Nerd Font";
pub const DEFAULT_SIZE: u8 = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub base: String,
    pub mantle: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub power: String,
    pub other: String,
    pub font_family: String,
    pub font_size: u8,
    pub rofi_theme: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Colors {
    base: String,
    mantle: String,
    text: String,
    muted: String,
    accent: String,
    power: String,
    other: String,
}

impl Theme {
    /// Resolve the theme on every render. Never cached: a palette or font
    /// change applies on the next Rofi open, with no daemon restart.
    pub fn load() -> Self {
        let config = Config::load_default().unwrap_or_default();
        Self::resolve(&config)
    }

    pub fn resolve(config: &Config) -> Self {
        let home = config::home_dir();
        let palette = config.palette_path(&home);
        let static_env = config.static_env_path(&home);
        Self::resolve_with_paths(config, &home, &palette, &static_env)
    }

    pub fn resolve_with_paths(
        config: &Config,
        home: &Path,
        palette_path: &Path,
        static_env_path: &Path,
    ) -> Self {
        let mode = config.theme_mode();
        let mut colors = Colors::builtin();

        if matches!(mode, ThemeMode::Auto | ThemeMode::Matugen) {
            if let Some(palette) = colors_from_matugen(palette_path) {
                colors = palette;
            } else if matches!(mode, ThemeMode::Auto | ThemeMode::Matugen) {
                if let Some(static_colors) = colors_from_static(static_env_path) {
                    colors = static_colors;
                }
            }
        }
        if mode == ThemeMode::Static {
            if let Some(static_colors) = colors_from_static(static_env_path) {
                colors = static_colors;
            }
        }
        if mode == ThemeMode::Auto {
            // Matugen and static were attempted above. A missing/invalid
            // Matugen palette already fell through to static; a missing static
            // file leaves the built-in colors.
        }

        apply_explicit_colors(&mut colors, config);

        let font_from_static = font_from_static(static_env_path);
        let (mut font_family, mut font_size) = (DEFAULT_FAMILY.to_owned(), DEFAULT_SIZE);
        if let Some((family, size)) = font_from_static {
            if let Some(family) = family {
                font_family = family;
            }
            if let Some(size) = size {
                font_size = size;
            }
        }
        if let Some(family) = &config.font.family {
            let family = family.trim();
            if !family.is_empty() && !family.chars().any(char::is_control) {
                font_family = family.to_owned();
            }
        }
        if let Some(size) = config.font.size {
            if (6..=72).contains(&size) {
                font_size = size;
            }
        }

        Self {
            base: colors.base,
            mantle: colors.mantle,
            text: colors.text,
            muted: colors.muted,
            accent: colors.accent,
            power: colors.power,
            other: colors.other,
            font_family,
            font_size,
            rofi_theme: chrome_theme_path(config, home),
        }
    }

    pub fn builtin() -> Self {
        let colors = Colors::builtin();
        Self {
            base: colors.base,
            mantle: colors.mantle,
            text: colors.text,
            muted: colors.muted,
            accent: colors.accent,
            power: colors.power,
            other: colors.other,
            font_family: DEFAULT_FAMILY.to_owned(),
            font_size: DEFAULT_SIZE,
            rofi_theme: None,
        }
    }

    pub fn font(&self) -> String {
        format!("{} {}", self.font_family, self.font_size)
    }

    /// A complete fallback Rofi theme generated from the resolved roles. The
    /// wrapper writes this to a runtime file and passes it with `-theme`; the
    /// standalone installed theme is used only if generation fails.
    pub fn rofi_rasi(&self) -> String {
        let mut out = String::new();
        if let Some(path) = &self.rofi_theme {
            out.push_str(&format!(
                "@theme \"{}\"\n",
                escape_rasi(&path.display().to_string())
            ));
        }
        let font = escape_rasi(&self.font());
        out.push_str(&format!(
            r#"configuration {{
    font: "{font}";
    show-icons: false;
    disable-history: true;
    hover-select: false;
    cycle: true;
    matching: "fuzzy";
}}

* {{
    background-color: {base};
    text-color: {text};
    border-color: {mantle};
    selected-normal-background: {mantle};
    selected-normal-foreground: {text};
    normal-background: {base};
    normal-foreground: {text};
    foreground: {text};
    background: {base};
    spacing: 0;
    margin: 0;
    padding: 0;
}}

window {{
    width: 94%;
    height: 55%;
    location: center;
    anchor: center;
    border: 0;
    padding: 14px;
    background-color: {base};
}}

mainbox {{
    background-color: {base};
    children: [ listview ];
    spacing: 0;
}}

inputbar {{
    enabled: false;
}}

listview {{
    background-color: {base};
    columns: 1;
    lines: 16;
    spacing: 0;
    scrollbar: false;
    cycle: true;
    dynamic: false;
}}

element {{
    padding: 2px 8px;
    background-color: {base};
    text-color: {text};
}}

element selected {{
    background-color: {mantle};
    text-color: {text};
}}

element-text {{
    background-color: transparent;
    text-color: inherit;
    font: "{font}";
    vertical-align: 0.5;
}}
"#,
            base = self.base,
            mantle = self.mantle,
            text = self.text,
        ));
        out
    }
}

impl Colors {
    fn builtin() -> Self {
        Self {
            base: "#0d1117".to_owned(),
            mantle: "#161b22".to_owned(),
            text: "#f0f6fc".to_owned(),
            muted: "#8b949e".to_owned(),
            accent: "#58a6ff".to_owned(),
            power: "#39c5cf".to_owned(),
            other: "#8b949e".to_owned(),
        }
    }
}

fn colors_from_matugen(path: &Path) -> Option<Colors> {
    let bytes = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let base = json_color(&value, &["colors", "surface", "default", "color"])?;
    let mantle = json_color(&value, &["colors", "surface_container", "default", "color"])?;
    let text = json_color(&value, &["colors", "on_surface", "default", "color"])?;
    let muted = json_color(
        &value,
        &["colors", "on_surface_variant", "default", "color"],
    )?;
    let accent = json_color(&value, &["colors", "primary", "default", "color"])?;
    let power = json_color(&value, &["palettes", "secondary", "70", "color"])?;
    Some(Colors {
        base,
        mantle,
        text,
        muted: muted.clone(),
        accent,
        power,
        other: muted,
    })
}

fn json_color(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    let color = current.as_str()?;
    valid_color(color).then(|| color.to_owned())
}

fn colors_from_static(path: &Path) -> Option<Colors> {
    let vars = parse_env_file(path)?;
    let base = required_color(&vars, &["THEME_STATIC_BASE"])?;
    let mantle = required_color(&vars, &["THEME_STATIC_MANTLE"])?;
    let text = required_color(&vars, &["THEME_STATIC_TEXT"])?;
    let muted = required_color(&vars, &["THEME_STATIC_SUBTEXT", "THEME_SIGNAL_MUTED"])?;
    let accent = required_color(&vars, &["THEME_STATIC_ACCENT"])?;
    let power = required_color(&vars, &["THEME_STATIC_CYAN"])?;
    Some(Colors {
        base,
        mantle,
        text,
        muted: muted.clone(),
        accent,
        power,
        other: muted,
    })
}

fn required_color(vars: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = vars.get(*key) {
            if valid_color(value) {
                return Some(value.clone());
            }
            return None;
        }
    }
    None
}

fn font_from_static(path: &Path) -> Option<(Option<String>, Option<u8>)> {
    let vars = parse_env_file(path)?;
    let family = ["THEME_MONO_FONT", "THEME_UI_FONT"]
        .iter()
        .find_map(|key| vars.get(*key))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control));
    let size = vars
        .get("THEME_ROFI_FONT_SIZE")
        .and_then(|value| value.trim().parse::<u8>().ok())
        .filter(|size| (6..=72).contains(size));
    Some((family, size))
}

fn apply_explicit_colors(colors: &mut Colors, config: &Config) {
    let supplied = config.explicit_colors();
    let values: Vec<&str> = supplied.iter().filter_map(|value| *value).collect();
    if values.is_empty() {
        return;
    }
    if values.iter().any(|value| !valid_color(value)) {
        // Never half-apply an explicit palette.
        return;
    }
    let mut values = values.iter();
    if supplied[0].is_some() {
        colors.base = (*values.next().unwrap()).to_owned();
    }
    if supplied[1].is_some() {
        colors.mantle = (*values.next().unwrap()).to_owned();
    }
    if supplied[2].is_some() {
        colors.text = (*values.next().unwrap()).to_owned();
    }
    if supplied[3].is_some() {
        colors.muted = (*values.next().unwrap()).to_owned();
    }
    if supplied[4].is_some() {
        colors.accent = (*values.next().unwrap()).to_owned();
    }
    if supplied[5].is_some() {
        colors.power = (*values.next().unwrap()).to_owned();
    }
    if supplied[6].is_some() {
        colors.other = (*values.next().unwrap()).to_owned();
    }
    // `other` follows `muted` unless explicitly set.
    if supplied[3].is_some() && supplied[6].is_none() {
        colors.other = colors.muted.clone();
    }
}

fn parse_env_file(path: &Path) -> Option<HashMap<String, String>> {
    let text = fs::read_to_string(path).ok()?;
    let mut vars = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('\'') && value.ends_with('\''))
                || (value.starts_with('"') && value.ends_with('"')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        vars.insert(key.to_owned(), value.to_owned());
    }
    Some(vars)
}

pub fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn chrome_theme_path(config: &Config, home: &Path) -> Option<PathBuf> {
    let env_path = std::env::var_os("ROFI_THEME")
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    if env_path.is_some() {
        return env_path;
    }
    let configured = config.rofi_theme_path(home);
    if configured.is_file() {
        return Some(configured);
    }
    let default = home.join(".config").join("rofi").join("theme.rasi");
    default.is_file().then_some(default)
}

fn escape_rasi(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, value: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, value).unwrap();
    }

    fn palette(accent: &str) -> String {
        format!(
            r##"{{
              "colors": {{
                "surface": {{"default": {{"color": "#101010"}}}},
                "surface_container": {{"default": {{"color": "#202020"}}}},
                "on_surface": {{"default": {{"color": "#f0f0f0"}}}},
                "on_surface_variant": {{"default": {{"color": "#909090"}}}},
                "primary": {{"default": {{"color": "{accent}"}}}}
              }},
              "palettes": {{"secondary": {{"70": {{"color": "#00cccc"}}}}}}
            }}"##
        )
    }

    fn static_env() -> &'static str {
        r#"
THEME_MONO_FONT='Static Mono'
THEME_ROFI_FONT_SIZE=13
THEME_STATIC_BASE='#111111'
THEME_STATIC_MANTLE='#222222'
THEME_STATIC_TEXT='#eeeeee'
THEME_STATIC_SUBTEXT='#999999'
THEME_STATIC_ACCENT='#3333ff'
THEME_STATIC_CYAN='#33cccc'
"#
    }

    #[test]
    fn matugen_wins_over_static() {
        let dir = tempdir().unwrap();
        let palette_path = dir.path().join("palette.json");
        let static_path = dir.path().join("static.env");
        write(&palette_path, &palette("#123456"));
        write(&static_path, static_env());
        let config = Config::default();
        let theme = Theme::resolve_with_paths(&config, dir.path(), &palette_path, &static_path);
        assert_eq!(theme.base, "#101010");
        assert_eq!(theme.accent, "#123456");
        assert_eq!(theme.power, "#00cccc");
        assert_eq!(theme.font_family, "Static Mono");
        assert_eq!(theme.font_size, 13);
    }

    #[test]
    fn config_colors_win_and_invalid_config_is_not_half_applied() {
        let dir = tempdir().unwrap();
        let palette_path = dir.path().join("palette.json");
        let static_path = dir.path().join("static.env");
        write(&palette_path, &palette("#123456"));
        write(&static_path, static_env());
        let mut config: Config = toml::from_str(
            r##"
            [theme]
            accent = "#abcdef"
            power = "#fedcba"
            [font]
            family = "Config Mono"
            size = 16
            "##,
        )
        .unwrap();
        let theme = Theme::resolve_with_paths(&config, dir.path(), &palette_path, &static_path);
        assert_eq!(theme.accent, "#abcdef");
        assert_eq!(theme.power, "#fedcba");
        assert_eq!(theme.base, "#101010");
        assert_eq!(theme.font_family, "Config Mono");
        assert_eq!(theme.font_size, 16);

        config.theme.accent = Some("not-a-color".to_owned());
        let theme = Theme::resolve_with_paths(&config, dir.path(), &palette_path, &static_path);
        assert_eq!(theme.accent, "#123456");
    }

    #[test]
    fn corrupt_palette_falls_back_to_static_then_builtin() {
        let dir = tempdir().unwrap();
        let palette_path = dir.path().join("palette.json");
        let static_path = dir.path().join("static.env");
        write(&palette_path, &palette("broken"));
        write(&static_path, static_env());
        let config = Config::default();
        let theme = Theme::resolve_with_paths(&config, dir.path(), &palette_path, &static_path);
        assert_eq!(theme.base, "#111111");
        assert_eq!(theme.accent, "#3333ff");

        let theme = Theme::resolve_with_paths(
            &config,
            dir.path(),
            &dir.path().join("missing.json"),
            &dir.path().join("missing.env"),
        );
        assert_eq!(theme.base, "#0d1117");
        assert_eq!(theme.accent, "#58a6ff");
    }

    #[test]
    fn stale_mode_uses_static_even_when_palette_exists() {
        let dir = tempdir().unwrap();
        let palette_path = dir.path().join("palette.json");
        let static_path = dir.path().join("static.env");
        write(&palette_path, &palette("#123456"));
        write(&static_path, static_env());
        let config: Config = toml::from_str("[theme]\nmode = \"static\"\n").unwrap();
        let theme = Theme::resolve_with_paths(&config, dir.path(), &palette_path, &static_path);
        assert_eq!(theme.accent, "#3333ff");
    }

    #[test]
    fn generated_theme_contains_font_roles_and_import() {
        let dir = tempdir().unwrap();
        let rofi = dir.path().join("user.rasi");
        write(&rofi, "* { }\n");
        let config: Config = toml::from_str(&format!(
            "[theme]\nrofi_theme = \"{}\"\n[font]\nfamily = \"Test Mono\"\nsize = 14\n",
            rofi.display()
        ))
        .unwrap();
        let theme = Theme::resolve_with_paths(
            &config,
            dir.path(),
            &dir.path().join("missing.json"),
            &dir.path().join("missing.env"),
        );
        let rasi = theme.rofi_rasi();
        assert!(rasi.contains(&format!("@theme \"{}\"", rofi.display())));
        assert!(rasi.contains("font: \"Test Mono 14\""));
        assert_eq!(theme.accent, "#58a6ff");
        assert_eq!(theme.power, "#39c5cf");
    }

    #[test]
    fn ui_font_is_secondary_fallback() {
        let dir = tempdir().unwrap();
        let palette_path = dir.path().join("missing.json");
        let static_path = dir.path().join("static.env");
        write(
            &static_path,
            &static_env().replace("THEME_MONO_FONT='Static Mono'", "THEME_UI_FONT='UI Mono'"),
        );
        let theme =
            Theme::resolve_with_paths(&Config::default(), dir.path(), &palette_path, &static_path);
        assert_eq!(theme.font_family, "UI Mono");
        assert_eq!(theme.font_size, 13);
    }

    #[test]
    fn palette_colors_must_be_hex() {
        assert!(valid_color("#abcdef"));
        assert!(!valid_color("#abcde"));
        assert!(!valid_color("abcdef"));
        assert!(!valid_color("#gggggg"));
    }
}
