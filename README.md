# screen-time-rofi

Local, per-user screen-time tracker with a static, high-visibility Rofi day
timeline. It answers four questions in one screen:

- how much did I use the device today?
- which apps used the time?
- when was each app active?
- when was the device powered on?

**Status:** v1 implementation with section-role colors and Matugen/font sync,
Linux + Hyprland + Rofi + systemd user session. App-level only; no window
titles or cloud sync. The dashboard groups apps into categories.

## Install

Screen Time tracks the active app in a **Hyprland** session and opens its
dashboard in **Rofi**. Automatic tracking uses a **systemd user session** with
logind. Use Rofi 2.x for native Wayland and the current dashboard theme. Package
availability varies by distro release. Check `hyprctl version`, `rofi -version`,
and `systemctl --user status` from a terminal inside Hyprland.

### Debian, Ubuntu, Fedora, Arch, and openSUSE

Install the dependencies through your distro, then run the Screen Time
installer as your desktop user.

| Distro family | Dependency command | Notes |
|---|---|---|
| Debian, Ubuntu, Mint, Pop!_OS | `sudo apt install curl rofi` | Ubuntu provides Rofi in Universe. Debian stable and Ubuntu LTS may ship Rofi 1.x. Use a Rofi 2.x package from a trusted source for native Wayland. |
| Fedora | `sudo dnf install curl rofi` | On Fedora Atomic desktops, provide Rofi and Hyprland in the host session. Run the per-user installer from the host. |
| Arch, EndeavourOS, Manjaro | `sudo pacman -Syu --needed curl rofi` | Arch provides Rofi through Extra. |
| openSUSE | `sudo zypper install curl rofi` | Check availability for your release with `zypper search -s rofi`. |

Install Hyprland using your distro's packages or its
[upstream installation guide](https://wiki.hypr.land/Getting-Started/Installation/),
then log in to it. Installing Screen Time on a supported Linux base does
not add tracking to GNOME, KDE Plasma, Sway, or other compositors.

From a terminal in Hyprland:

```bash
curl -fsSLO https://github.com/WillGle/screen-time-rofi/releases/latest/download/install.sh
less install.sh
bash install.sh
export PATH="$HOME/.local/bin:$PATH"
screen-time doctor
rofi-screen-time
```

Add `~/.local/bin` to your login session's PATH if Hyprland cannot resolve the
launcher. You can also use an absolute path in your Hyprland shortcut. The
release installer verifies the archive checksum and installs the binary,
launcher, fallback theme, and user service at:

```text
~/.local/bin/screen-time
~/.local/bin/rofi-screen-time
~/.local/share/screen-time/rofi/screen-time.rasi
~/.config/systemd/user/screen-time.service
```

Release archives target Linux x86_64 and, when the ARM build is published,
AArch64. These are glibc builds. If an older distro reports `GLIBC_* not found`,
or an archive is missing for your CPU, build locally on that machine or use Nix
where the flake supports the CPU. Distro package commands above describe
dependency setup. This repo's release CI builds on Ubuntu and does not run
desktop acceptance tests on each listed distro.

### From source or a local tarball

Use a current stable Rust toolchain, a C compiler and linker, and the runtime
dependencies above. For example, install the C toolchain via `build-essential`
on Debian/Ubuntu, `gcc` on Fedora, `base-devel` on Arch, or `gcc` on openSUSE.

From the checkout:

```bash
archive="$(bash scripts/package-release.sh | tail -n 1)"
bash install.sh --tarball "$archive"
```

The packaging script writes the matching `SHA256SUMS` beside the tarball. For
a separately obtained archive, put that checksum file beside it or pass its
trusted digest with `--sha256`. The tarball installs the launcher, theme, and
service together with the Rust binary.

### Nix

To preview a checkout without changing the installed package or service:

```bash
nix build .# --no-write-lock-file
./result/bin/screen-time --version
./result/bin/rofi-screen-time
```

The preview reads data from the existing tracker, when available. For an
installation, add the app to the flake that already manages your Home Manager
configuration:

```nix
inputs.screen-time-rofi = {
  url = "github:WillGle/screen-time-rofi";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

#### NixOS with Home Manager

In the NixOS modules list, where `inputs` is in scope and the Home Manager
NixOS module is already imported, add:

```nix
{
  home-manager.sharedModules = [ inputs.screen-time-rofi.homeManagerModules.default ];
  home-manager.users.will.services.screen-time.enable = true;
}
```

Replace `will` with your Home Manager user. Install Rofi in your system or
home packages if it is missing. Rebuild from your host flake directory:

```bash
sudo nixos-rebuild switch --flake ".#$(hostname)"
```

Replace the hostname with your flake's configuration name if they differ.
To try the local app checkout through an already enabled module:

```bash
bash install.sh --nixos "/etc/nixos#$(hostname)" --dry-run
bash install.sh --nixos "/etc/nixos#$(hostname)"
```

This activates the entire selected NixOS configuration, including changes
already present in it, and overrides the app input for that rebuild only. A
later ordinary rebuild uses the locked revision again.

#### Standalone Home Manager

In your existing `homeManagerConfiguration`, add these entries to `modules`:

```nix
inputs.screen-time-rofi.homeManagerModules.default
{ services.screen-time.enable = true; }
```

Then run `home-manager switch --flake .#will` from your Home Manager flake,
using your configuration name. Standalone Home Manager on a non-NixOS distro
uses that distro's Hyprland session and systemd user manager. Install Rofi via
Home Manager or through the host distro.

Use the Home Manager module for the daemon and launcher together. A
`nix profile install` installs the package but does not register its user
service. Keep the release installer and Home Manager on separate installations
to avoid competing service or launcher paths.

### First launch

Check the user service and open the dashboard from the desktop session:

```bash
systemctl --user status screen-time.service
screen-time doctor
rofi-screen-time
```

Add a key binding in the Hyprland configuration you manage:

```text
bind = SUPER, S, exec, rofi-screen-time
```

The first launch can show an empty day until the daemon records activity.
History lives at `${XDG_STATE_HOME:-~/.local/state}/screen-time/screen-time.db`.
The current-day snapshot is normally at
`$XDG_RUNTIME_DIR/screen-time/live.json`.

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
| `Alt+1` / `Alt+2` / `Alt+3` | Switch to Nord / Solarized Light / Gruvbox Dark |
| `Alt+4` | Return to the theme from your config |

The footer lists the shortcuts. Switching themes reopens the popup on the
same day and view. The shortcut selection lasts until you close the popup.
Set `theme.mode` in the config to make a theme your default.

The dashboard shows a daily total, a `DAY ACTIVITY` overview with a `POWER`
row, and an `APP TIMELINE` for the top 10 apps. Each slot represents 30
minutes and the ruler is labeled every two hours. The app rows show duration,
share of tracked app time, and a category label. Apps outside the top 10 are
grouped in a `Mixed` row. The detail view shows session count, median duration,
switches per hour, bursts, the last committed session time, and a 7-day trend.

### The `×N` experiment

Session counts are off by default. To test the alternate row layout:

```bash
SCREEN_TIME_SESSION_COUNT=1 rofi-screen-time
```

Or build with `--features session-count`. The default remains duration only.

## Theme and font

On any distro, create or edit
`${XDG_CONFIG_HOME:-~/.config}/screen-time/config.toml`. The configuration is
optional. Start with only the settings you want to change, for example
`[theme]` and `mode = "nord"`.

```toml
[theme]
mode = "auto" # auto | matugen | static | config | nord | solarized | gruvbox
rofi_theme = "~/.config/rofi/theme.rasi"
palette = "~/.config/theme/generated/palette.json"
static_env = "~/.config/theme/static.env"
# Optional explicit overrides (validated as #RRGGBB; never half-applied)
# base = "#0d1117"
# mantle = "#161b22"
# text = "#f0f6fc"
# muted = "#8b949e"
# accent = "#58a6ff"
# power = "#39c5cf"
# other = "#a0a9b5"

[font]
family = "JetBrainsMono Nerd Font"
size = 12

[layout]
mode = "auto" # auto | full | compact
```

Select a preset in `theme.mode` to use a palette independent of the wallpaper.
Changes take effect when you reopen Rofi.

| Mode | Palette |
|---|---|
| `nord` | Nord, with cool slate surfaces |
| `solarized` | Solarized Light, with darker foregrounds for readability |
| `gruvbox` | Gruvbox Dark, with warm charcoal surfaces |

The earlier names `dark`, `light`, and `high-contrast` are aliases for these
presets in the same order.

`auto` and `matugen` try the Matugen palette, then `static.env`, then the built-in fallback.
`static` starts at `static.env` and `config` starts at the built-in fallback. Explicit
`[theme]` color overrides take precedence in all modes.

Matugen provides the window and panel surfaces, foreground text, primary accent,
and secondary power color. App categories use a fixed color palette chosen for
the lightness of the window. Foreground colors from either source are adjusted
as needed to reach 4.5:1 against the window and panel before explicit overrides
are applied.

Font priority is config `[font]` → `THEME_MONO_FONT` (with
`THEME_UI_FONT` as a secondary fallback) + `THEME_ROFI_FONT_SIZE` →
JetBrainsMono Nerd Font 12 pt. `layout.mode = "auto"` switches to the
48-column timeline when the 96-column timeline plus its labels and stats
does not fit. `full` and `compact` force a timeline width. `rofi-screen-time` queries the
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

### Manage the config with Home Manager

Apply the same settings declaratively in a Home Manager module:

```nix
{ pkgs, ... }:
{
  xdg.configFile."screen-time/config.toml".source = (pkgs.formats.toml {}).generate
    "screen-time.toml" {
      theme.mode = "nord";
      font.size = 12;
      layout.mode = "auto";
    };
}
```

If the destination already holds a hand-written config, move it to a backup
first and carry its values into the module. Once Home Manager owns the file,
edit the Nix module and switch your configuration to apply changes.

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

### Nix and Home Manager

From the flake directory that declares the app input, run
`nix flake update screen-time-rofi`, then use the same rebuild or Home Manager
switch command as at install time. To roll back only Screen Time, restore its
previous lock entry and switch the configuration again. To remove it, set
`services.screen-time.enable = false` in Home Manager and switch. Your state
and history stay in the user's state directory.

### GitHub release or tarball

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
  service after the graphical session is ready. From a terminal inside Hyprland,
  run `systemctl --user import-environment HYPRLAND_INSTANCE_SIGNATURE` and
  `systemctl --user restart screen-time.service`. If this repeats at login,
  configure your Hyprland session to import its environment into the user
  manager and start `graphical-session.target`.
- **`logind IdleHint unavailable`**: the system D-Bus or login1 session is not
  reachable. Active time still tracks, but idle gaps may be overcounted.
- **`stale` in the header**: the live snapshot is older than two minutes. Check
  the service; the popup intentionally does not extend a stale open interval.
- **No app rows**: let the daemon track some activity after login, or check
  `journalctl --user -u screen-time`. A session shorter than the one-minute
  slot threshold still appears in the app total but not as a full strip block.
- **Theme conflict on update**: the installed theme differs from the last
  installer-managed copy. Back it up and rerun with `--force`.
- **Installer reports a Nix-managed unit or launcher**: follow the [Nix
  installation instructions](#nix) to update the package and service together.
- **Rofi reports an existing instance**: close the existing popup before
  reopening. The wrapper uses `$XDG_RUNTIME_DIR/rofi-screen-time.pid`.
- **Rofi reports a key binding or theme error**: check `rofi -version` and
  use Rofi 2.x on Hyprland. Check for binding conflicts in your Rofi config.

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
