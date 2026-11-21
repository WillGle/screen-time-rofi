# Screen Time Rofi — Final v1 UI Specification

- **Status:** Locked design for v1, 2026-11-21. No implementation yet.
- **Platform:** Rofi, Linux, Hyprland, per-user, local only.
- **Purpose:** a static daily briefing: device on/off, total active time, app
  durations, and when each app was used.
- **Related:** `SCREEN_TIME_ROFI_UI.md` (research), `ROFI_RESOURCE_BENCHMARK.md`
  (measured UI cost), `SCREEN_TIME_STORAGE_ARCHITECTURE.md` (storage/read path),
  `GITHUB_INSTALLER.md` (install/update).

## 1. Locked constraints

| Area | Decision |
|---|---|
| UI | Static Rofi; no animation, no live redraw |
| Visibility | Opaque background, high contrast, 12pt body, 16–18pt total |
| Layout | Wide and short; ~94% width, ~55–60% height |
| Timeline | 30-minute slots, 2 characters per slot, 96 columns |
| Apps | Top 5 + one aggregated “other” line |
| Scope | App-level only; no window titles, no categories |
| Power | On/off only; no suspend/lock/display-off |
| Colors | Section-coded theme roles (accent/power/muted); no category palette, no moral colors |
| Navigation | Zero visible buttons; keyboard + one-line footer |
| Main data | tmpfs `live.json` |
| Detail data | SQLite closed sessions + tmpfs open interval; may lag one batch |
| Detail freshness | Accepted lag; show `as of` / `last commit` |
| Session count | `×N`/`SESS` is an experiment, not a v1 default |
| Install | One-time `install.sh`; UI assets installed with the binary |

## 2. Screen inventory

1. **Main — day tape** (default).
2. **App detail** — one level only.
3. No settings screen, no week mode, no category screen, no power-detail screen.

## 3. Main screen

### Wireframe

```text
▌ SCREEN TIME                                  Fri 21 Nov · Today
 6h 48m active   ▲ 38m vs usual at this time
 powered 12h 40m · span 07:05–17:15 · peak 09:00–10:15
 7d ▁▂▃▅▆▄█  avg 6h 12m

▌ WHEN
                   00                      06                      12                      18                    24
 POWER     12h40m                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
▌ APPS
 Code       2h14m                                      ██████          ████
 Firefox    1h32m                                ██                          ████
 Terminal     48m                                              ████
 Slack        32m                                            ██
 Music        22m                                                    ██

 +3 apps    1h20m                                                                  ▒▒▒▒▒▒

 ←/→ day   T today   R refresh   ↵ detail   esc close
```

`▌` is a colored section marker, not a data mark. Section colors come from
theme roles, not from app categories.

### Anatomy

| Zone | Content | Data source |
|---|---|---|
| Header | Date/relative day, active total, same-time delta, powered total, active span, peak, 7-day sparkline + average | `live.json` + daily aggregates |
| `SCREEN TIME` section | Summary; accent-colored marker | static + theme |
| `WHEN` section | Ruler + POWER row; power-colored marker | static + power sessions |
| `APPS` section | Ranked app rows + other; accent-colored marker | live + SQLite |
| Ruler | 00–24, aligned to 30-minute slots | static |
| POWER | On/off strip; `▓▓` on, blank off | power sessions |
| App rows | Top five by total active time: name, duration, per-app 96-column strip | `live.json` + SQLite |
| Other | `+N apps`, aggregate duration, neutral strip | aggregates |
| Footer | Key hints | static |

### Timeline encoding

- **30 minutes per slot, 2 characters wide.** 48 slots = 96 columns.
- `▓▓` = device powered on; `██` = app active; `▒▒` = aggregated other.
- Blank = no activity in that slot.
- One accent color for all app activity. App identity comes from the row label;
  no color legend.
- `POWER` is neutral, not the accent, so device-on time is visually distinct
  from app activity.
- No per-app sparkline in the main list; the row strip is the timeline.
- A slot is active only if it contains at least one minute of activity.

### Row order

- Sort by total active time, descending.
- Keep the order stable while the popup is open.
- Top five only; everything else is aggregated.
- Long names truncate at 10 characters; full name appears in detail.
- `×N`/`SESS` experiment: add a 3–4 character session count after the duration
  and test at 1280×800 and 1920×1080. Off by default. If it breaks width or is
  misread as minutes, drop it.

```text
A (default)   Code       2h14m  ██████
B (experiment) Code       2h14m ×18  ██████
```


### Header rules

- `active` and `powered` are separate; never merge them.
- `▲/▼` delta compares today’s cumulative total with the same clock time over
  the previous seven days. Hide if fewer than three days of history.
- `span` = first to last active time; label it as a span, not a session.
- `peak` = busiest 90-minute window.
- `7d` sparkline includes today, highlighted; `avg` excludes today.
- No sample count, interval, or tracker diagnostics in the header.
- If `live.json` is stale (older than two minutes), do not extend the open
  interval; show the last closed total and a small `stale` note.

## 4. App detail

### Wireframe

```text
▌ Code                                    2h 14m · 33% of today
 18 sessions · median 3m · 8 switches/h
 sessions through 15:21 · may lag one batch

▌ BURSTS
 09:12–09:18   4×   6m
 09:30–09:34   2×   4m
 10:02–10:18   3×   9m
 13:05–13:12   2×   4m
 14:02–14:20   3×  18m
 + 4 longer sessions   1h 33m

▌ 7 DAY
 ▁▂▃▅▆▄█   avg 1h 48m
 ←/→ day   esc back
```

### Rules

- One level only. `Enter` opens; `Esc`/`Backspace` returns.
- Closed sessions come from SQLite, indexed by date/app.
- The current open session comes from `live.json` and is computed on render.
- `sessions through HH:MM` is the last SQLite commit time, not the last event.
- Accept up to one batch of lag (60 seconds or 100 events). Do not add a socket,
  pending overlay, or live query in v1.
- If the selected app is currently active, include the open session and add one
  to the session count.
- No categories, tags, projects, editing, or title-level detail.
- If SQLite has no sessions yet (first minutes of use), show a calm message:
  `Sessions will appear after the first batch.`

## 5. Navigation

| Key | Action |
|---|---|
| `←` / `→` | Previous / next day; clamp at today |
| `↑` / `↓` | Select app row |
| `Enter` | Open app detail |
| `Esc` | Close, or back from detail |
| `Backspace` | Back from detail |
| `T` | Today |
| `R` | Refresh (re-read tmpfs + SQLite) |
| Mouse click | Open app detail |
| Click outside | Close |

- No visible navigation buttons.
- Footer hints are always visible.
- No `P` key; power is an on/off row, not a separate screen.
- No hover-only affordances; the footer documents the keys.

## 6. States

| State | Behavior |
|---|---|
| No activity today | `No activity recorded today`; POWER row may still show on |
| Device off | POWER row gap; app rows empty |
| Partial day | Future slots blank; open interval computed on render |
| Stale live snapshot | Do not extend open interval; show `stale` and last closed totals |
| No 7-day history | Hide delta and average; keep the day |
| Many apps | Top five + `+N apps`; no scroll |
| Long names | Truncate at 10 chars; full name in detail |
| Narrow window | 30-minute slots at one character, top four apps |
| Detail lag | Show `sessions through HH:MM · may lag one batch` |
| DB unavailable | Main still renders from `live.json`; detail shows a static error |

## 7. Visual system

- Opaque background; no transparency or alpha for primary data.
- **Section colors:** summary/apps use `accent`; timeline/POWER use `power`;
  other/secondary use `muted`. Section markers (`▌`) use the same role.
- App activity uses `accent`; `POWER` uses `power`; `other` uses `muted`.
- No category colors and no red/green judgment.
- Monospace for ruler, durations, and strips.
- Body ~12pt; total ~16–18pt.
- No borders around every section. Section markers and spacing carry the
  grouping.
- No icons required; names carry identity.
- No animation, spinners, progress bars, hover motion, or live reveal.
- Contrast checked at 1280×800 and 1920×1080.

### Section color roles

| Section | Marker/text color | Data color |
|---|---|---|
| `SCREEN TIME` | `accent` | totals use `text` |
| `WHEN` | `power` | POWER `▓▓` uses `power` |
| `APPS` | `accent` | app `██` uses `accent`; other `▒▒` uses `muted` |
| `BURSTS` | `accent` | session rows use `text` |
| `7 DAY` | `power` | sparkline uses `power` |

## 7b. Theme and font synchronization

The app must follow the existing Matugen pipeline on this machine, not invent a
second theme system.

### Existing pipeline (reference)

```text
wallpaper
  → theme-apply / Matugen
  → ~/.config/theme/generated/palette.json
  → ~/.config/theme/generated/rofi.rasi
  → ~/.config/rofi/theme.rasi (Home Manager link)
```

`~/.config/theme/static.env` holds the resolved fonts and static fallback
colors. Existing screen-time uses `@theme "~/.config/rofi/theme.rasi"` and
`modules/desktop/theme/lib/palette.sh`.

### Matugen role mapping

Read the raw Matugen palette at
`~/.config/theme/generated/palette.json`:

| App role | Matugen path | Static fallback |
|---|---|---|
| `base` | `.colors.surface.default.color` | `THEME_STATIC_BASE` |
| `mantle` | `.colors.surface_container.default.color` | `THEME_STATIC_MANTLE` |
| `text` | `.colors.on_surface.default.color` | `THEME_STATIC_TEXT` |
| `muted` | `.colors.on_surface_variant.default.color` | `THEME_STATIC_SUBTEXT` / `THEME_SIGNAL_MUTED` |
| `accent` | `.colors.primary.default.color` | `THEME_STATIC_ACCENT` |
| `power` | `.palettes.secondary["70"].color` | `THEME_STATIC_CYAN` |
| `other` | `muted` | `THEME_STATIC_SUBTEXT` |

Validate every value as `^#[0-9A-Fa-f]{6}$`; on any failure, fall back to
`static.env`, then to built-in defaults. Never half-apply a palette.

### Rofi theme and chrome

- If `theme.rofi_theme` exists, import it for window chrome/font via `@theme`.
- The app still computes Pango colors from the palette/static env because inline
  spans need concrete hex values.
- If the Rofi theme is missing, use the installed standalone app theme.
- Do not parse `hyprland.conf`; Hyprland does not own app fonts. The shared sync
  point is the Rofi theme + `static.env`.

### Font synchronization

Matugen does not set fonts. Use this priority:

1. explicit `~/.config/screen-time/config.toml` `[font]`;
2. `THEME_MONO_FONT` + `THEME_ROFI_FONT_SIZE` from `static.env`;
3. installed app default (JetBrainsMono Nerd Font 12pt).

On this machine, changing `theme.fonts.mono` / `theme.fonts.rofi.size` and
rebuilding updates `static.env`; the app reads it on the next open. No app
rebuild or daemon restart is required. General GitHub users edit config.toml.

### Config schema

```toml
[theme]
mode = "auto" # auto | matugen | static | config
rofi_theme = "~/.config/rofi/theme.rasi"
palette = "~/.config/theme/generated/palette.json"
static_env = "~/.config/theme/static.env"

[font]
family = "JetBrainsMono Nerd Font"
size = 12

[layout]
mode = "auto" # auto | full | compact
```

### Resolution priority

1. explicit `[theme]`/`[font]` values;
2. Matugen palette + `static.env` when present;
3. Rofi theme for chrome/font if resolvable;
4. built-in high-contrast defaults.

### Refresh behavior

- Read the palette/static env on every `render`; no daemon restart, no cache.
- Wallpaper change → `theme-apply` regenerates palette/rofi.rasi → next Rofi
  open shows the new section colors.
- If the palette is temporarily missing/corrupt, use the last static fallback;
  never render invisible or low-contrast text.
- If the font size increases enough that 96 columns no longer fit, `layout.mode
  = "auto"` switches to 48-column/compact and four app rows. `full`/`compact`
  can override.

## 8. Copy deck

| Use | Avoid |
|---|---|
| `6h 48m active` | `6h 48m 12s` |
| `powered 12h 40m` | `on time` without definition |
| `▲ 38m vs usual at this time` | `+38m` without context |
| `peak 09:00–10:15` | `densest window` |
| `+3 apps` | `Other` / `Mixed` |
| `18 sessions` | bare `18×` without a label |
| `sessions through 15:21` | `live` / `real-time` |
| `No activity recorded today` | `No data` |
| `Not enough history` | `—` / `null` |

UI language is English for v1, matching the repository and Rofi theme. A later
translation is a copy-only change.

## 9. Technical binding

- Rofi dmenu with custom keys; app rows in a single-column listview.
- Header, ruler, POWER, other, and footer are textboxes.
- Detail is a second Rofi invocation; no socket, no DB connection from Rofi.
- Renderer is one subcommand (`screen-time render --format rofi`), not a `jq`
  pipeline.
- Main reads `$XDG_RUNTIME_DIR/screen-time/live.json` once.
- Detail reads SQLite for closed sessions; reads live snapshot for the open
  session and `last_commit_at`.
- `src/theme.rs` resolves config → Matugen palette → `static.env` → built-in
  defaults on every render; no daemon restart/cache.
- `src/config.rs` reads `~/.config/screen-time/config.toml`; missing file is
  valid and uses auto mode.
- Theme is installed by `install.sh`; `ROFI_THEME`/`--theme` can override the
  Rofi chrome. Installer never overwrites a user config.
- Layout `auto` switches to compact when the configured font/screen width no
  longer fits 96 columns.
- No configuration is required for defaults; theme/font config is optional.
- The `×N` experiment can be a build/env flag, not a user-facing setting.
- No multi-user, network, socket, or live query.

## 10. Acceptance criteria

1. Main screen opens with zero visible navigation buttons.
2. At 1280×800 with 12pt body, all top-five durations and strips fit without
   horizontal wrapping.
3. The 30-minute/two-character strip is legible; POWER is distinguishable from
   app activity without a legend.
4. A reader can identify total active time, powered time, top app, and peak in
   under five seconds.
5. No category legend, no category colors, no title-level data.
6. Main view reads tmpfs; detail reads SQLite; no raw-log scan.
7. Detail shows `sessions through HH:MM` and may lag one batch; no socket.
8. Stale live snapshot does not extend the open interval beyond the stale
   threshold.
9. No animation; process is idle while the popup is open.
10. `×N` is either clearly legible as sessions or not shipped.
11. Sections are visually distinct via `▌` markers and role colors; no category
    legend is introduced.
12. A Matugen palette change updates section/app/power colors on the next open
    without restarting the daemon.
13. A missing/corrupt palette falls back to `static.env`, then built-in colors;
    no invisible text.
14. Changing `theme.fonts.mono`/`theme.fonts.rofi.size` on this machine (or
    `[font]` in config.toml) changes the app font on the next open.
15. A larger font that breaks 96 columns automatically selects compact layout,
    or the user can force `layout.mode = "compact"`.

## 11. Only remaining UI experiment

`×N`/`SESS` is not locked. Run a static A/B test:

- A: duration only (default).
- B: duration + `×N` sessions.

Measure at 1280×800 and 1920×1080:

- does the row still fit?
- does the reader interpret `18×` as sessions, not minutes?
- does it help distinguish fragmented use from continuous use?
- does it add enough value to justify the extra column?

Until that test wins, ship A.
