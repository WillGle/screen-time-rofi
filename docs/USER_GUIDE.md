# Screen Time user guide

Screen Time is a local tracker for the Hyprland desktop. It records active
applications and shows daily totals, time-of-day timelines, and app sessions
in a keyboard-driven Rofi popup. It keeps history on the device and discards
window titles. See the [evaluation](EVALUATION.md) for measurements and app
comparisons.

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
| Debian, Ubuntu, Mint, Pop!_OS | `sudo apt install curl minisign rofi` | Ubuntu provides Rofi in Universe. Debian stable and Ubuntu LTS may ship Rofi 1.x. Use a Rofi 2.x package from a trusted source for native Wayland. |
| Fedora | `sudo dnf install curl minisign rofi` | On Fedora Atomic desktops, provide Rofi and Hyprland in the host session. Run the per-user installer from the host. |
| Arch, EndeavourOS, Manjaro | `sudo pacman -Syu --needed curl minisign rofi` | Arch provides Rofi through Extra. |
| openSUSE | `sudo zypper install curl minisign rofi` | Check availability for your release with `zypper search -s rofi`. |

Install Hyprland using your distro's packages or its
[upstream installation guide](https://wiki.hypr.land/Getting-Started/Installation/),
then log in to it. Installing Screen Time on a supported Linux base does
not add tracking to GNOME, KDE Plasma, Sway, or other compositors.

From a terminal in Hyprland:

```bash
base=https://github.com/WillGle/screen-time-rofi/releases/latest/download
# Fetch the key from the repository, not from the release object being verified.
curl -fsSLO https://raw.githubusercontent.com/WillGle/screen-time-rofi/main/minisign.pub
curl -fsSLO "$base/install.sh"
curl -fsSLO "$base/install.sh.minisig"
# The pinned key id must be 4E5BF83E07DA94ED.
grep -q 4E5BF83E07DA94ED minisign.pub
minisign -Vm install.sh -p minisign.pub
less install.sh
bash install.sh
export PATH="$HOME/.local/bin:$PATH"
screen-time doctor
rofi-screen-time
```

The installer itself also verifies every release asset with the same pinned
minisign key before it stops the service. A missing, invalid, or unknown
signature is a hard failure. The key id is printed by `minisign -Vm` and the
fingerprint is recorded in the backup manifest.

Add `~/.local/bin` to your login session's PATH if Hyprland cannot resolve the
launcher. You can also use an absolute path in your Hyprland shortcut. The
release installer verifies signatures and archive checksums, then installs the
binary, launcher, fallback theme, and user service at:

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
bash install.sh --tarball "$archive" --allow-unsigned
```

`--allow-unsigned` is for a local build, never for an automatic update. To
exercise the signed path, sign `dist/SHA256SUMS` with
`scripts/sign-release.sh` first. For a separately obtained archive, put its
`SHA256SUMS` beside it or pass its trusted digest with `--sha256`. The tarball
installs the launcher, theme, and service together with the Rust binary.

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
| `Esc` | Close the popup |
| `Backspace` | Back from detail |
| `T` | Today |
| `R` | Re-read the live snapshot and SQLite |
| `Alt+1` / `Alt+2` / `Alt+3` | Switch to Nord / Solarized Light / Gruvbox Dark |
| `Alt+4` | Return to the theme from your config |

The footer lists the shortcuts. Switching themes reopens the popup on the
same day and view. The shortcut selection is saved as `theme-choice` in
the Screen Time state directory and applies on future launches. Press
`Alt+4` to clear it and return to `theme.mode` in the config. The shortcuts
leave `config.toml` intact, including when Home Manager owns it.

The dashboard shows a daily total, a `DAY ACTIVITY` overview with a `POWER`
row, and an `APP TIMELINE` for the top 10 apps. Each slot represents 30
minutes and the ruler is labeled every two hours. The app rows show duration,
share of tracked app time, and a category label. Apps outside the top 10 are
grouped in a `Mixed` row. App detail shows a category-colored activity strip,
bars for the relative duration of visible bursts, and labeled daily totals
for the last seven days. A dash indicates a day with no recorded usage.
Use Backspace to return to the app list, or Esc to close the popup.

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

### History retention

History is kept forever by default. Nothing is pruned unless you opt in with a
`[retention]` section. Each category is independent; a missing value or `0`
keeps that category forever.

```toml
[retention]
events_days = 30       # raw focus/idle/boot events
sessions_days = 90     # per-app session rows
aggregates_days = 365  # daily app and 30-minute slot aggregates
power_days = 365       # closed power sessions
```

The daemon applies pruning once at startup and again at date rollover. Invalid
TOML is treated as keep-all: a warning is logged and nothing is deleted. Set
`[storage] warn_db_mib` and `hard_db_mib` to change the default 250 MiB / 1 GiB
size warnings reported by `screen-time doctor`. Size warnings never delete
data.

Migration backups under
`${XDG_STATE_HOME:-~/.local/state}/screen-time/backups/migrations/` are never
pruned automatically. After verifying a migration, remove old copies manually:

```bash
ls -lh ~/.local/state/screen-time/backups/migrations/
# Review the list, then remove only the files you no longer need.
rm ~/.local/state/screen-time/backups/migrations/pre-v1-YYYYMMDD-HHMMSS.db
```

Installs before 0.4.0 pruned events/sessions/aggregates on a fixed 30/90/365
day schedule. Data already deleted cannot be recovered; keep-all only affects
future pruning.

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
the lightness of the window. Matugen foreground colors are adjusted as needed to
reach 4.5:1 against the window and panel before explicit overrides are applied.
`static.env` and explicit `[theme]` values are used as written, so check contrast
when you customize them.

Font priority is config `[font]` → `THEME_MONO_FONT` (with
`THEME_UI_FONT` as a secondary fallback) + `THEME_ROFI_FONT_SIZE` (sizes below 12
are raised to 12) → JetBrainsMono Nerd Font 12 pt. `layout.mode = "auto"` switches to the
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

## Update, rollback, uninstall

### Nix and Home Manager

From the flake directory that declares the app input, run
`nix flake update screen-time-rofi`, then use the same rebuild or Home Manager
switch command as at install time. To roll back only Screen Time, restore its
previous lock entry and switch the configuration again. To remove it, set
`services.screen-time.enable = false` in Home Manager and switch. Your state
and history stay in the user's state directory.

### GitHub release or tarball

Updates are opt-in. Nothing downloads or runs in the background unless you
install a timer. The updater:

1. takes a single update lock (a concurrent run exits before stopping the
   service);
2. verifies the release signature, checksums, and that it is a stable tag;
3. checks free space for the database, WAL, two backups, and staging;
4. records whether the service was active, stops it, and creates a consistent
   `VACUUM INTO` snapshot with a manifest and SHA-256 hash;
5. replaces the binary, launcher, theme, unit, version, and hash files
   atomically;
6. runs a data-fingerprint comparison and `screen-time doctor --json
   --no-runtime`;
7. marks the backup verified and prunes only old **verified** backups, keeping
   at least two. A failed attempt and its backup are kept for diagnosis;
8. restores the previous files, database snapshot, and service state on any
   failure after the snapshot.

```bash
# check only; no installation changes
bash install.sh --check
bash install.sh --check --json
# exit: 0 up to date, 10 update available, 20 network, 21 invalid release,
#       22 signature error

# install the latest stable release if newer
bash install.sh --update
bash install.sh --update --dry-run

# opt in to a daily check notification (no code is downloaded or installed)
bash install.sh --auto-update=check

# opt in to signed automatic updates (stable tags only, no major bumps)
bash install.sh --auto-update=apply

# remove update timers and update state
bash install.sh --no-auto-update

# restore the latest verified successful backup; keep it for audit
bash install.sh --rollback

# remove artifacts; keep database, config, and all backups
bash install.sh --uninstall

# remove artifacts, database, config, and backups after confirmation
bash install.sh --uninstall --purge

# remove stale staging dirs and old verified backups only
bash install.sh --clean
```

`install.sh --update` is a no-op when up to date. It never downgrades unless
`--allow-downgrade` is passed, and it refuses a schema downgrade unless
`--allow-schema-downgrade` is also passed. Automatic mode never accepts
`--force`, `--allow-unsigned`, `--uninstall`, or a major-version bump.

Update state lives under `~/.local/state/screen-time/updates/`:
`last-check.json`, `last-update.json`, `update-in-progress.json`,
`failure.json`, and `update.lock`. The lock and state files are diagnostics;
deleting them does not remove database history or verified backups. A failed
automatic attempt backs off exponentially (up to 24 hours) and does not retry
the same tag immediately.

`uninstall.sh` is the standalone equivalent of `install.sh --uninstall` and
supports `--prefix`, `--purge`, `--yes`, and `--dry-run`. `--purge` requires an
interactive `purge` confirmation or `--yes` in a script.

The update timer is a systemd **user** timer. It runs only while the user
manager is available; with lingering enabled it can run after logout. If no
graphical session is available the update still verifies data but skips
runtime-only checks. Auto-apply also skips while the session is locked or the
machine is on battery. A user-modified theme is not overwritten without
`--force`. A unit whose `ExecStart` was changed is backed up and never silently
replaced.

See [AUTO_UPDATE.md](AUTO_UPDATE.md) for the channel model, signing, key
rotation, and recovery runbook.

## Troubleshooting

Run the built-in health check first:

```bash
screen-time doctor
screen-time doctor --json | jq .
screen-time doctor --json --no-runtime   # skip Hyprland/Rofi/logind
journalctl --user -u screen-time -n 100 --no-pager
```

`doctor --json` reports the schema version, per-table counts, `quick_check`,
retention policy, database/WAL/backup sizes, service state, snapshot freshness,
and a data fingerprint (`counts`, total session seconds, app keys, and app-key
hash). The updater compares that fingerprint before and after installing. A
consistent snapshot can be produced without `sqlite3`:

```bash
screen-time snapshot --to /tmp/screen-time-snapshot --json
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
