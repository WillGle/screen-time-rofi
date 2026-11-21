# screen-time-rofi

Local, per-user screen-time tracker with a static, high-visibility Rofi day
timeline. It answers four questions in one screen:

- how much did I use the device today?
- which apps used the time?
- when was each app active?
- when was the device powered on?

**Status:** v1 implementation, Linux + Hyprland + Rofi + systemd user session.
App-level only; no window titles, categories, cloud, socket, or animation.

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
```

`flake.nix` builds the same binary with `rustPlatform.buildRustPackage` and
installs the wrapper, theme, and unit under the package path. The installer and
the Nix package use the same runtime layout; do not manage one installation
with both.

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

The main screen follows the 96-column, 30-minute-slot UI spec. The `POWER` row
uses `▓▓`; app activity uses `██`; the aggregated other row uses `▒▒`. The
detail view shows total, share of day, session count, median, switches/hour,
bursts, `sessions through HH:MM`, and a 7-day trend.

### The `×N` experiment

Session counts are off by default. To test the alternate row layout:

```bash
SCREEN_TIME_SESSION_COUNT=1 rofi-screen-time
```

Or build with `--features session-count`. The default remains duration only.

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
open-interval math, retention, batching, atomic snapshots, stale handling, and
Hyprland parsing; golden main/detail screens at 1280x800 and 1920x1080; and a
daemon smoke test with a fake Hyprland socket and SIGTERM shutdown.

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
