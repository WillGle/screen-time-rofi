# screen-time-rofi

Local, per-user screen-time tracker with a static, high-visibility Rofi day
timeline. It answers four questions in one screen:

- how much did I use the device today?
- which apps used the time?
- when was each app active?
- when was the device powered on?

**Status:** v1 implementation with section-role colors and Matugen/font sync,
Linux + Hyprland + Rofi + systemd user session. App-level only; no window
titles, categories, cloud, socket, or animation.

## Install

### From a GitHub release

Once a release is published:

```bash
curl -fsSLO https://github.com/WillGle/screen-time-rofi/releases/latest/download/install.sh
less install.sh
bash install.sh
```

The installer refuses to run as root, verifies the release checksum, stages
everything in one temporary directory, and writes no files outside the user's
home. It installs:

```text
~/.local/bin/screen-time
~/.local/bin/rofi-screen-time
~/.local/share/screen-time/rofi/screen-time.rasi
~/.config/systemd/user/screen-time.service
```

State and history are created by the daemon:

```text
~/.local/state/screen-time/screen-time.db
$XDG_RUNTIME_DIR/screen-time/live.json
```

Enable the popup in Hyprland:

```text
bind = SUPER, S, exec, rofi-screen-time
```

### From a local build or tarball

```bash
cargo build --release
cargo install --path . --locked           # or: bash install.sh --tarball ...
./target/release/screen-time --version
```

The release tarball layout is `bin/screen-time`, `bin/rofi-screen-time`,
`share/rofi/screen-time.rasi`, `systemd/screen-time.service`, `LICENSE`, and
`README.md`. For an offline install:

```bash
bash install.sh --tarball dist/screen-time-x86_64-linux.tar.gz --sha256 <hex>
```

### Nix

```bash
nix build .#
./result/bin/screen-time --version

# or try without installing
nix run . -- --version
```

`flake.nix` builds the same binary with `rustPlatform.buildRustPackage` and
installs the wrapper, theme, and unit under the package path.

Home Manager module:

```nix
inputs.screen-time-rofi.url = "github:WillGle/screen-time-rofi";

imports = [ inputs.screen-time-rofi.homeManagerModules.default ];
services.screen-time.enable = true;
```

This installs the package, user service, and Rofi wrapper declaratively.
`nix profile install` alone is still not a one-time install because it does not
wire the service. Do not manage one installation with both Nix and `install.sh`.

## Use

Open the popup:

```bash
rofi-screen-time
```

Keys:

| Key | Action |
|---|---|
| `←` / `→` | Previous / next day (clamped at today) |
| `↑` / `↓` | Select an app row |
| `Enter` | Open the selected app detail |
| `Esc` | Close, or back from detail |
| `Backspace` | Back from detail |
| `T` | Today |
| `R` | Re-read the live snapshot and SQLite |

There are no visible navigation buttons. The footer is the key map.

The main screen follows the 96-column, 30-minute-slot UI spec. Sections are
marked `▌ SCREEN TIME`, `▌ WHEN`, and `▌ APPS`; detail uses `▌ BURSTS` and
`▌ 7 DAY`. The `POWER` row uses `▓▓`; app activity uses `██`; the aggregated
other row uses `▒▒`. App activity and the summary/apps markers use the accent
role; POWER, `WHEN`, and `7 DAY` use the power role; secondary/other data uses
muted. The detail view shows total, share of day, session count, median,
switches/hour, bursts, `sessions through HH:MM`, and a 7-day trend.

### The `×N` experiment

Session counts are off by default. To test the alternate row layout:

```bash
SCREEN_TIME_SESSION_COUNT=1 rofi-screen-time
```

Or build with `--features session-count`. The default remains duration only.

## Theme and font

The app follows the existing Matugen pipeline; it does not parse
`hyprland.conf` and does not create a second theme system. Optional config lives
at:

```text
~/.config/screen-time/config.toml
```

```toml
[theme]
mode = "auto" # auto | matugen | static | config
rofi_theme = "~/.config/rofi/theme.rasi"
palette = "~/.config/theme/generated/palette.json"
static_env = "~/.config/theme/static.env"
# Optional explicit overrides (validated as #RRGGBB; never half-applied)
# base = "#0d1117"
# accent = "#58a6ff"

[font]
family = "JetBrainsMono Nerd Font"
size = 12

[layout]
mode = "auto" # auto | full | compact
```

Resolution order is explicit config → Matugen palette → `static.env` →
built-in high-contrast defaults. Every color is validated as `^#[0-9A-Fa-f]{6}$`;
an invalid palette is ignored as a whole and the next fallback is used.
`theme.mode = "static"` skips the Matugen palette; `"config"` ignores both the
palette and `static.env` colors and uses explicit `[theme]` values plus the
built-ins. The Matugen mapping is:

| Role | Matugen path | Static fallback |
|---|---|---|
| base | `.colors.surface.default.color` | `THEME_STATIC_BASE` |
| mantle | `.colors.surface_container.default.color` | `THEME_STATIC_MANTLE` |
| text | `.colors.on_surface.default.color` | `THEME_STATIC_TEXT` |
| muted | `.colors.on_surface_variant.default.color` | `THEME_STATIC_SUBTEXT` / `THEME_SIGNAL_MUTED` |
| accent | `.colors.primary.default.color` | `THEME_STATIC_ACCENT` |
| power | `.palettes.secondary["70"].color` | `THEME_STATIC_CYAN` |
| other | muted | `THEME_STATIC_SUBTEXT` |

Font priority is config `[font]` → `THEME_MONO_FONT` (with
`THEME_UI_FONT` as a secondary fallback) + `THEME_ROFI_FONT_SIZE` →
JetBrainsMono Nerd Font 12 pt. `layout.mode = "auto"` switches to the
48-column/compact/four-row layout when the configured font no longer fits 96
columns; `full` and `compact` force a layout. `rofi-screen-time` queries the
focused monitor width with `hyprctl monitors -j`; set
`SCREEN_TIME_SCREEN_WIDTH` to override it.

The palette, `static.env`, and font are read on every `render`. A wallpaper or
font change applies on the next popup open; the daemon does not restart. The
wrapper generates a complete Rofi theme in
`$XDG_RUNTIME_DIR/screen-time/rofi-theme.rasi`, imports the configured/user Rofi
theme for chrome if it exists, and passes the resolved font/colors to Rofi.
`ROFI_THEME` overrides the base Rofi theme. The installed
`share/rofi/screen-time.rasi` is only a fallback. The installer never overwrites
`~/.config/screen-time/config.toml`.

### M9 device validation

Validated on `think14gryzen` (Hyprland 0.55.4, Rofi 2.0.0, eDP-1 3072x1920 at
scale 1.5) against the real `~/.config/theme/generated/palette.json` and
`~/.config/theme/static.env`:

- **Real palette:** base `#101410`, mantle `#1c211b`, text `#e0e4db`, muted
  `#c2c9bd`, accent `#9fd49c`, power `#9eb09a`; static font
  `JetBrainsMono Nerd Font 10`.
- **Real wallpaper change:** a generated wallpaper was applied with
  `~/.config/theme/theme-apply --wallpaper ...`. The palette changed to base
  `#1a1110`, mantle `#271d1c`, text `#f1dfdb`, muted `#d8c2be`, accent
  `#ffb4a6`, power `#caa29b`; the next popup used the new accent/power/muted
  colors with no daemon restart. The original wallpaper was restored afterwards.
  Screenshots:
  [`m9-main-original-palette.png`](docs/screenshots/m9-main-original-palette.png)
  and
  [`m9-main-changed-palette.png`](docs/screenshots/m9-main-changed-palette.png).
- **Font sizes 12/14/16:** ruler widths were 116/68/68 columns; 14 pt and 16 pt
  selected the compact/top-four layout, and `layout.mode = "full"` forced the
  116-column layout back.
- **No Hyprland config parsing:** only the generated palette, `static.env`, the
  config file, and the optional Rofi theme are read.
- **Packaging:** `nix build .#` produced `screen-time 0.2.1`; the installer
  updated v0.1.0 → v0.2.1, rolled back to v0.1.0, kept exactly one backup after
  a successful update, and uninstalled with no leftover files or staging
  directories.

The full before/after values, measurements, and golden diffs are in
[`docs/FINAL_REPORT.md`](docs/FINAL_REPORT.md#m8m9-validation).

## Update, rollback, uninstall

Re-run `install.sh` to update. The installer stops the service, backs up the
current binary/wrapper/theme/unit and a SQLite copy in
`~/.local/state/screen-time/backups/updates/`, installs atomically, restarts the
service, and runs `screen-time doctor`. It keeps one rollback backup and prunes
older ones after a successful update.

```bash
# check versions only; makes no changes
bash install.sh --check

# rollback to the previous installed version
bash install.sh --rollback

# remove artifacts; keep history
bash install.sh --uninstall

# remove artifacts and history
bash install.sh --uninstall --purge

# remove stale staging dirs / old backups only
bash install.sh --clean
```

`uninstall.sh` is the standalone equivalent of `install.sh --uninstall` and
supports `--prefix`, `--purge`, and `--dry-run`.

There is no auto-update daemon. Download and inspect the new `install.sh` before
running it. A user-modified theme is not overwritten without `--force`.

## Data path and write policy

- Active app changes come from the Hyprland event socket. The window title is
  parsed only to find the comma separator and is immediately discarded.
- Idle begin/end comes from the logind `IdleHint` D-Bus property. Suspend,
  lock, and display-off are not tracked.
- The current day is held in memory and exposed atomically in tmpfs
  `live.json`. The renderer reads that file once; it never polls.
- SQLite uses WAL, `synchronous=NORMAL`, `busy_timeout=5000`,
  `foreign_keys=ON`, and one writer.
- Events and sessions commit in one transaction at **60 seconds or 100 events**,
  whichever comes first, and on clean shutdown/day rollover.
- Retention: raw events 30 days, sessions 90 days, daily aggregates 365 days.
- Main reads tmpfs for today; a past day reads compact SQLite aggregates.
  Detail reads indexed sessions from SQLite plus the open interval from tmpfs.
  No raw-event scan is used to render.
- If `live.json` is older than two minutes, the renderer does not extend the
  open interval; it shows closed totals and a `stale` note. A missing or
  stopped tracker falls back to durable daily aggregates.

The database and snapshot are private to the user (`0600` files, `0700`
directories). There are no log files; logs go to journald.

## Troubleshooting

Run the built-in health check first:

```bash
screen-time doctor
journalctl --user -u screen-time -n 100 --no-pager
```

Common cases:

- **`live snapshot missing`**: the daemon is stopped or has not finished
  starting. `systemctl --user status screen-time`; start with
  `systemctl --user start screen-time`.
- **`Hyprland event socket not found`**: the service is not running inside a
  Hyprland session, or `HYPRLAND_INSTANCE_SIGNATURE` is missing. Start the
  service after the graphical session is ready.
- **`logind IdleHint unavailable`**: the system D-Bus or login1 session is not
  reachable. Active time still tracks, but idle gaps may be overcounted.
- **`stale` in the header**: the live snapshot is older than two minutes. Check
  the service; the popup intentionally does not extend a stale open interval.
- **No app rows**: use the popup for a minute after login, or check
  `journalctl --user -u screen-time`. A session shorter than the one-minute
  slot threshold still appears in the app total but not as a full strip block.
- **Theme conflict on update**: the installed theme differs from the last
  installer-managed copy. Back it up and rerun with `--force`.
- **`rofi-screen-time` still opens the old UI**: `~/.local/bin/rofi-screen-time`
  is probably a Home Manager/Nix symlink from the old module. The installer now
  refuses to overwrite it. Remove the old screen-time module and use
  `homeManagerModules.default`, or intentionally remove the old Nix-managed
  link before running `install.sh`; never let both manage the same path.
- **`rofi: failed to set binding`**: another Rofi instance owns the default
  pidfile. The wrapper uses `rofi-screen-time.pid`; remove a stale
  `$XDG_RUNTIME_DIR/rofi-screen-time.pid` only when no popup is running.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
shellcheck install.sh uninstall.sh share/rofi-screen-time scripts/package-release.sh
nix build .#
```

The test suite includes unit tests for schema migration, session merge, bursts,
open-interval math, retention, batching, atomic snapshots, stale handling,
Hyprland parsing, config/theme resolution, font priority, and compact layout;
golden main/detail screens at 1280x800 and 1920x1080 with section markers; and
a daemon smoke test with a fake Hyprland socket and SIGTERM shutdown. Theme
integration tests cover Matugen-over-static, corrupt-palette fallback, config
overrides, generated Rofi themes, and live palette changes on the next render.

## Docs

- [`docs/DECISIONS.md`](docs/DECISIONS.md) — implementation choices and
  spec-conflict resolutions.
- [`docs/FINAL_REPORT.md`](docs/FINAL_REPORT.md) — milestones, tests,
  artifacts, and raw measurements.
- [`docs/SCREEN_TIME_UI_SPEC.md`](docs/SCREEN_TIME_UI_SPEC.md)
- [`docs/SCREEN_TIME_STORAGE_ARCHITECTURE.md`](docs/SCREEN_TIME_STORAGE_ARCHITECTURE.md)
- [`docs/GITHUB_INSTALLER.md`](docs/GITHUB_INSTALLER.md)
- [`docs/SCREEN_TIME_V1_GOAL.md`](docs/SCREEN_TIME_V1_GOAL.md)

## License

MIT. See [`LICENSE`](LICENSE).
