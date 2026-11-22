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
    pub work: String,
    pub browser: String,
    pub other: String,
    pub font_family: String,
    pub font_size: u8,
    pub rofi_theme: Option<PathBuf>,
    pub communication: String,
    pub entertainment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Colors {
    base: String,
    mantle: String,
    text: String,
    muted: String,
    accent: String,
    power: String,
    work: String,
    browser: String,
    other: String,
    communication: String,
    entertainment: String,
}

impl Theme {
    /// Resolve the theme on every render. Never cached: a palette or font
    /// change applies on the next Rofi open, with no daemon restart.
    pub fn load() -> Self {
        Self::load_with_mode(None)
    }

    pub(crate) fn load_with_mode(mode: Option<&str>) -> Self {
        let mut config = Config::load_default().unwrap_or_default();
        if let Some(mode) = mode {
            config.theme.mode = Some(mode.to_owned());
        }
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
        let mut colors = match mode {
            ThemeMode::Auto | ThemeMode::Matugen => {
                colors_from_matugen(palette_path).or_else(|| colors_from_static(static_env_path))
            }
            ThemeMode::Static => colors_from_static(static_env_path),
            ThemeMode::Nord => Some(Colors::nord()),
            ThemeMode::Solarized => Some(Colors::solarized()),
            ThemeMode::Gruvbox => Some(Colors::gruvbox()),
            ThemeMode::Config => None,
        }
        .unwrap_or_else(Colors::builtin);

        apply_explicit_colors(&mut colors, config);

        let font_from_static = font_from_static(static_env_path);
        let (mut font_family, mut font_size) = (DEFAULT_FAMILY.to_owned(), DEFAULT_SIZE);
        if let Some((family, size)) = font_from_static {
            if let Some(family) = family {
                font_family = family;
            }
            if let Some(size) = size {
                font_size = size.max(DEFAULT_SIZE);
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
            work: colors.work,
            browser: colors.browser,
            other: colors.other,
            font_family,
            font_size,
            rofi_theme: chrome_theme_path(config, home),
            communication: colors.communication,
            entertainment: colors.entertainment,
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
            work: colors.work,
            browser: colors.browser,
            other: colors.other,
            font_family: DEFAULT_FAMILY.to_owned(),
            font_size: DEFAULT_SIZE,
            rofi_theme: None,
            communication: colors.communication,
            entertainment: colors.entertainment,
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
        let columns = crate::render::grid_columns(self);
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
    font: "{font}";
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
    width: calc( ( {columns}ch + 64px ) min 94% );
    location: center;
    anchor: center;
    border: 0;
    padding: 20px;
    background-color: {base};
}}

mainbox {{
    background-color: {base};
    padding: 0;
    children: [ message, listview ];
    spacing: 0;
}}

message, textbox {{
    background-color: {base};
    text-color: {text};
    padding: 0px 5px;
    border: 0;
}}

textbox {{
    font: "{family} 26";
    markup: true;
    vertical-align: 0.5;
}}

element normal.active, element alternate.active, element selected.active {{
    background-color: {mantle};
    border: 0;
    border-radius: 0;
}}

element selected.active {{
    border: 0px 0px 0px 2px;
    border-color: {accent};
}}

inputbar {{
    enabled: false;
}}

listview {{
    background-color: {base};
    columns: 1;
    lines: 24;
    spacing: 0;
    scrollbar: false;
    cycle: true;
    dynamic: true;
    fixed-height: false;
}}

element {{
    children: [ element-text ];
    padding: 3px 10px;
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
            family = escape_rasi(&self.font_family),
            accent = self.accent,
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
            work: "#91d6a1".to_owned(),
            browser: "#89b4fa".to_owned(),
            other: "#a0a9b5".to_owned(),
            communication: "#f9e2af".to_owned(),
            entertainment: "#f38ba8".to_owned(),
        }
    }

    fn light() -> Self {
        Self {
            base: "#faf8f4".into(),
            mantle: "#ece8e0".into(),
            text: "#231f20".into(),
            muted: "#55505a".into(),
            accent: "#25458b".into(),
            power: "#3e5760".into(),
            work: "#23623a".into(),
            browser: "#1e5195".into(),
            other: "#635a63".into(),
            communication: "#834c09".into(),
            entertainment: "#992755".into(),
        }
    }

    fn nord() -> Self {
        Self {
            base: "#2e3440".into(),
            mantle: "#3b4252".into(),
            text: "#eceff4".into(),
            muted: "#d8dee9".into(),
            accent: "#88c0d0".into(),
            power: "#d8dee9".into(),
            work: "#a3be8c".into(),
            browser: "#8fbcdb".into(),
            other: "#e5e9f0".into(),
            communication: "#ebcb8b".into(),
            entertainment: "#dfacd0".into(),
        }
    }

    fn solarized() -> Self {
        Self {
            base: "#fdf6e3".into(),
            mantle: "#eee8d5".into(),
            text: "#073642".into(),
            muted: "#52676e".into(),
            accent: "#00689d".into(),
            power: "#52676e".into(),
            work: "#516100".into(),
            browser: "#00689d".into(),
            other: "#58636a".into(),
            communication: "#8d5c00".into(),
            entertainment: "#a52e6f".into(),
        }
    }

    fn gruvbox() -> Self {
        Self {
            base: "#1d2021".into(),
            mantle: "#282828".into(),
            text: "#fbf1c7".into(),
            muted: "#d5c4a1".into(),
            accent: "#fabd2f".into(),
            power: "#d5c4a1".into(),
            work: "#b8bb26".into(),
            browser: "#83a598".into(),
            other: "#bdae93".into(),
            communication: "#fe8019".into(),
            entertainment: "#d3869b".into(),
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
    let base_luminance = luminance(&base)?;
    let light = contrast(0.0, base_luminance) >= contrast(1.0, base_luminance);
    let mut colors = if light {
        Colors::light()
    } else {
        Colors::builtin()
    };
    let mut backgrounds = [base_luminance, luminance(&mantle)?];
    let endpoint = if light { 0 } else { 255 };
    if contrast(f64::from(endpoint) / 255.0, backgrounds[1]) < 4.5 {
        // A palette that mixes light and dark surfaces may have no common
        // readable foreground. Keep its base and use the matching preset panel.
        backgrounds[1] = luminance(&colors.mantle)?;
    } else {
        colors.mantle = mantle;
    }
    colors.base = base;
    colors.text = text;
    colors.muted = muted;
    colors.accent = accent;
    colors.power = power;
    for color in [
        &mut colors.text,
        &mut colors.muted,
        &mut colors.accent,
        &mut colors.power,
        &mut colors.work,
        &mut colors.browser,
        &mut colors.other,
        &mut colors.communication,
        &mut colors.entertainment,
    ] {
        let Some(lum) = luminance(color) else {
            continue;
        };
        if backgrounds.iter().any(|&bg| contrast(lum, bg) < 4.5) {
            *color = readable_color(color, backgrounds, endpoint);
        }
    }
    Some(colors)
}

fn rgb(color: &str) -> Option<[u8; 3]> {
    let bytes = color.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }
    let channel = |index: usize| {
        let high = hex_digit(bytes[index])?;
        let low = hex_digit(bytes[index + 1])?;
        Some(high << 4 | low)
    };
    Some([channel(1)?, channel(3)?, channel(5)?])
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn luminance(color: &str) -> Option<f64> {
    let [r, g, b] = rgb(color)?.map(|channel| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

fn contrast(first: f64, second: f64) -> f64 {
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

fn readable_color(color: &str, backgrounds: [f64; 2], endpoint: u8) -> String {
    let Some(rgb) = rgb(color) else {
        return color.to_owned();
    };
    // Blend toward the readable endpoint in small steps to retain the hue.
    for step in 1..=20 {
        let [r, g, b] = rgb.map(|c| (u16::from(c) * (20 - step) + u16::from(endpoint) * step) / 20);
        let candidate = format!("#{r:02x}{g:02x}{b:02x}");
        let Some(lum) = luminance(&candidate) else {
            continue;
        };
        if backgrounds.iter().all(|&bg| contrast(lum, bg) >= 4.5) {
            return candidate;
        }
    }
    color.to_owned()
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
    let browser = optional_color(&vars, &["THEME_STATIC_BLUE"], &power);
    let communication = optional_color(&vars, &["THEME_STATIC_WARNING"], "#f9e2af");
    let entertainment = optional_color(&vars, &["THEME_STATIC_ERROR"], "#f38ba8");
    let other = optional_color(&vars, &["THEME_STATIC_OUTLINE"], &muted);
    Some(Colors {
        base,
        mantle,
        text,
        muted,
        work: accent.clone(),
        accent,
        power,
        browser,
        other,
        communication,
        entertainment,
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

fn optional_color(vars: &HashMap<String, String>, keys: &[&str], fallback: &str) -> String {
    keys.iter()
        .find_map(|key| vars.get(*key))
        .filter(|value| valid_color(value))
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
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
    let mut values = values.into_iter();
    let mut next = || values.next();
    if supplied[0].is_some() {
        if let Some(value) = next() {
            colors.base = value.to_owned();
        }
    }
    if supplied[1].is_some() {
        if let Some(value) = next() {
            colors.mantle = value.to_owned();
        }
    }
    if supplied[2].is_some() {
        if let Some(value) = next() {
            colors.text = value.to_owned();
        }
    }
    if supplied[3].is_some() {
        if let Some(value) = next() {
            colors.muted = value.to_owned();
        }
    }
    if supplied[4].is_some() {
        if let Some(value) = next() {
            colors.accent = value.to_owned();
            colors.work.clone_from(&colors.accent);
        }
    }
    if supplied[5].is_some() {
        if let Some(value) = next() {
            colors.power = value.to_owned();
        }
    }
    if supplied[6].is_some() {
        if let Some(value) = next() {
            colors.other = value.to_owned();
        }
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
        assert_ne!(theme.accent, "#123456");
        assert_eq!(theme.power, "#00cccc");
        let backgrounds = [
            luminance(&theme.base).unwrap(),
            luminance(&theme.mantle).unwrap(),
        ];
        for color in [
            &theme.work,
            &theme.browser,
            &theme.communication,
            &theme.entertainment,
            &theme.muted,
            &theme.other,
            &theme.accent,
        ] {
            assert!(
                backgrounds.iter().all(|&bg| {
                    contrast(luminance(color).expect("valid theme color"), bg) >= 4.5
                }),
                "{color}"
            );
        }
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
        assert_ne!(theme.accent, "#123456");
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

    #[test]
    fn rgb_is_total_for_malformed_input() {
        assert_eq!(rgb("#0000ff"), Some([0, 0, 255]));
        assert_eq!(rgb("#ABCDEF"), Some([0xab, 0xcd, 0xef]));
        for invalid in [
            "",
            "#fff",
            "#GG0000",
            "#000000ff",
            "#00000",
            "#0000000",
            "0000ff",
            "😀abc",
            "😀#000000",
            "é0000ff",
        ] {
            assert_eq!(rgb(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn explicit_base_only_changes_base() {
        let mut config = Config::default();
        config.theme.base = Some("#101010".to_owned());
        let mut colors = Colors::builtin();
        let original = colors.clone();
        apply_explicit_colors(&mut colors, &config);
        assert_eq!(colors.base, "#101010");
        assert_eq!(colors.mantle, original.mantle);
        assert_eq!(colors.text, original.text);
        assert_eq!(colors.muted, original.muted);
        assert_eq!(colors.accent, original.accent);
        assert_eq!(colors.power, original.power);
        assert_eq!(colors.other, original.other);
        assert_eq!(colors.work, original.work);
    }

    #[test]
    fn explicit_other_does_not_follow_muted_unless_supplied() {
        let mut config = Config::default();
        config.theme.other = Some("#112233".to_owned());
        let mut colors = Colors::builtin();
        let original = colors.clone();
        apply_explicit_colors(&mut colors, &config);
        assert_eq!(colors.other, "#112233");
        assert_eq!(colors.muted, original.muted);
        assert_eq!(colors.accent, original.accent);
    }

    #[test]
    fn explicit_muted_without_other_updates_both() {
        let mut config = Config::default();
        config.theme.muted = Some("#445566".to_owned());
        let mut colors = Colors::builtin();
        apply_explicit_colors(&mut colors, &config);
        assert_eq!(colors.muted, "#445566");
        assert_eq!(colors.other, "#445566");
        assert_ne!(colors.accent, "#445566");
    }

    #[test]
    fn explicit_accent_without_power_updates_work_only() {
        let mut config = Config::default();
        config.theme.accent = Some("#778899".to_owned());
        let mut colors = Colors::builtin();
        let original = colors.clone();
        apply_explicit_colors(&mut colors, &config);
        assert_eq!(colors.accent, "#778899");
        assert_eq!(colors.work, "#778899");
        assert_eq!(colors.power, original.power);
        assert_eq!(colors.muted, original.muted);
    }

    #[test]
    fn explicit_mixed_valid_and_invalid_palette_changes_nothing() {
        let mut config = Config::default();
        config.theme.base = Some("#123456".to_owned());
        config.theme.accent = Some("not-a-color".to_owned());
        let mut colors = Colors::builtin();
        let original = colors.clone();
        apply_explicit_colors(&mut colors, &config);
        assert_eq!(colors, original);
    }
}
