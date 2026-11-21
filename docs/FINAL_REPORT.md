# Screen Time v1 — Final Report

- **Date:** 2026-11-21
- **Repository:** `/home/will/src/screen-time-rofi`
- **Branch:** `main`
- **Version:** 0.2.1
- **Platform:** Linux, Hyprland 0.55.4, Rofi 2.0.0, systemd user session

## 1. Status

M0–M9 are complete locally. Tracking, the live snapshot, SQLite persistence,
Rofi rendering, section-role colors, Matugen/font sync, the installer
lifecycle, Nix packaging, and the release workflow exist and are covered by
tests. `main` is pushed to `https://github.com/WillGle/screen-time-rofi`. Tag `v0.2.0`
is published with the M8/M9 code. GitHub Actions run `44920439259` passed the
format/clippy/test/shellcheck job and both native build jobs (`ubuntu-latest`
x86_64 and `ubuntu-24.04-arm` aarch64), then published the release. The earlier
`v0.1.0` release remains available. No AArch64 artifact was produced locally;
CI produced it successfully.

## 2. Milestone commits

```text
2c0ca38 docs: record M8/M9 commit hashes and revalidation
d342a53 docs: record M8/M9 validation and screenshots (M9)
1a2e99a feat: add section role colors and Matugen/font sync (M8)
be6b882 docs: add M8-M9 section and Matugen theme sync goal
6484a6e docs: record successful CI builds on main
9f4e15a docs: finalize v1 report, measurements, and validation (M7)
55fe810 build: add Nix package and release workflow (M6)
b745692 feat: add installer lifecycle and release packaging (M5)
63438b2 feat: add Rofi theme, launcher shim, and key navigation (M4)
c9e0df6 feat: add event-driven tracker, storage, and static Rofi renderer (M1-M3)
4a9ff4a chore: initialize screen-time v1 scaffold
2e146ff chore: initialize screen-time v1 scaffold and goal
```

`flake.nix` was staged during the Nix build and landed in `b745692`; it is
tracked and later committed with `flake.lock` and the workflow in `55fe810`.
History was not rewritten.

## 3. What was built

```text
src/
  main.rs       thin binary entry point
  lib.rs        module wiring and run()
  cli.rs        clap subcommands: daemon, render, doctor, --version
  paths.rs      XDG state/runtime paths, boot id, PID checks
  db.rs         SQLite WAL, batches, aggregates, retention, power sessions
  migrations.rs user_version schema v1
  capture.rs    Hyprland socket discovery, event parsing, active-window query
  idle.rs       logind IdleHint via zbus blocking API
  sessions.rs   app normalization, merge, bursts, median, switches/hour
  snapshot.rs   atomic tmpfs live.json, stale guard, open overlays
  daemon.rs     event loop, batching, rollover, recovery, shutdown
  render.rs     plain and Rofi script-mode payloads, 96-column layout
  view.rs       live/DB main view and detail view assembly
  ui.rs         Rofi script-mode state and keys
  doctor.rs     runtime, DB, snapshot, Hyprland, logind, rofi, service checks
tests/
  golden.rs            8 golden/protocol/stale tests
  daemon_smoke.rs      fake Hyprland socket, capture, SIGTERM, clean shutdown
  common/mod.rs        deterministic 408-minute fixture
  golden/*.txt         1280x800 and 1920x1080 main/detail goldens
install.sh / uninstall.sh
flake.nix / flake.lock
.github/workflows/release.yml
share/rofi-screen-time
share/rofi/screen-time.rasi
systemd/screen-time.service
```

The binary has the required interface:

```text
screen-time daemon
screen-time render --format rofi [--date YYYY-MM-DD] [--app KEY]
screen-time doctor
screen-time --version
```

`render` prints plain text when run outside Rofi. Under Rofi it emits script-mode
metadata (`use-hot-keys`, non-selectable rows, `info` app keys), so the same
command is both the golden-test payload producer and the UI renderer.

## 4. Validation

Commands and results on the final tree:

```text
$ cargo fmt --check
FMT_OK

$ cargo clippy --all-targets -- -D warnings
Finished `dev` profile ... (no warnings)
CLIPPY_OK

$ cargo test --locked
49 passed (unit tests, including config/theme/layout)
 1 passed (daemon_smoke)
12 passed (golden, protocol, stale, theme integration, layout)
0 failed

$ cargo run -- --version
screen-time 0.2.0

$ shellcheck install.sh uninstall.sh share/rofi-screen-time scripts/package-release.sh
SHELLCHECK_OK

$ nix build .#
.../result/bin/screen-time --version -> screen-time 0.2.0

$ nix flake check --no-build
all checks passed!

$ yq '.jobs | keys' .github/workflows/release.yml
["build", "check", "release"]
```

The Nix package builds the same binary with bundled SQLite, installs the
wrapper, theme, and unit under the package path, and rewrites the unit
`ExecStart` to the store binary.

### Golden UI checks

- Main rows are exactly 116 display columns: 20-column label/duration prefix +
  96 timeline columns.
- Ruler labels are at 00/06/12/18/24; `24` is right-aligned at column 94.
- POWER `▓▓`, app `██`, other `▒▒`; app rows are top five by duration, then
  `+N apps`.
- Detail shows 18 sessions, median, switches/hour, five bursts, the remaining
  longer sessions, 7-day trend, `sessions through HH:MM · may lag one batch`,
  and `←/→ day   esc back`.
- Stale test: an open interval is not extended when `as_of` is older than 120
  seconds; the header shows `stale` and closed totals.
- `×N` is off by default and only appears with
  `SCREEN_TIME_SESSION_COUNT=1` / the `session-count` feature.

### Capture/daemon check

`tests/daemon_smoke.rs` binds fake `.socket.sock` and `.socket2.sock` sockets,
returns an active window, sends Code -> Firefox -> Code events, waits for a live
snapshot containing a closed app plus an open app, then sends SIGTERM. It
asserts:

- sessions committed after shutdown (at least 2),
- the latest power session reason is `shutdown`,
- `clean_shutdown=1`,
- `live.json` is removed on clean shutdown.

The test was run repeatedly; the final suite is stable at 3–6 seconds.

### Rofi integration check

Under Xvfb 1280x800 with the installed wrapper and theme:

- Window size: 1203x440 (94% width, 55% height).
- Screenshot: `SCREEN TIME`, date, active/powered/span/peak, 7d line, ruler,
  POWER, top-five apps, `+3 apps`, and footer.
- One Return opened Code detail; Backspace returned to the main list; Left moved
  to the previous day and wrote `ui.json`; `T` reset to today.
- No visible navigation buttons; no animation.

## 5. Raw measurements

Environment: NixOS, ext4 on `/dev/nvme0n1p1` for the disk test, Xvfb
1280x800x24 for Rofi, warm cache. Values are direct outputs, not estimates.

### Renderer and Rofi

```text
render --format rofi warm (5 runs): 4.54, 4.72, 3.93, 5.33, 4.79 ms

rofi-screen-time launch to visible window (3 runs):
  75 ms, 68 ms, 75 ms
Rofi PSS: 7,737 / 7,799 / 7,796 kB
Rofi idle CPU while open: 0 ticks over 5 s (0.0%)
```

The launch measurement uses 5 ms `xwininfo` polling, so the true value is at or
below these numbers. All are under the 100 ms warm-open target.

### Daemon

Synthetic run: 12 focus switches over 66 seconds, one batch boundary, SQLite on
ext4, tmpfs `live.json`, idle via logind disabled to isolate capture.

```text
daemon PSS:              6,564 kB (start) / 6,584 kB (end)
daemon VmHWM:            6,668 kB
daemon idle CPU:         0 ticks over 10 s
write_bytes delta:       135,168
wchar delta:             69,726
fsync calls:             8 (strace -f -c, fsync/fdatasync)
database file:           73,728 bytes
events committed:        15
sessions committed:      12
power sessions:          1, reason shutdown
```

Batch proof from the same run:

```text
after ~10 s:  events=1, sessions=0
after ~66 s:  events=15, sessions=12
```

The first value is the immediate boot record; focus events stayed in the
in-memory batch until the 60-second boundary. A second run with no focus events:

```text
write_bytes delta: 4,096
wchar delta:       564
fsync calls:       8 (startup + shutdown)
daemon PSS:        6,556 kB
CPU over 64 s:     0 ticks
```

The synthetic 12-switch window is a stress measurement, not a typical day.
Real event-driven days have hundreds of focus changes, not one every five
seconds. The important result is the behavior: no per-event disk write and no
timer-based commit; batches commit at 60 seconds or 100 events.

## 6. Installer lifecycle

Tested in a fresh temporary HOME with `--no-service` (the sandbox's real
systemd user manager always uses `/home/will`, so a fake HOME cannot own an
installed unit safely):

```text
fresh install:        rc=0, no backup created on first install
reinstall:            rc=0, exactly one rollback backup
modified binary:      backed up, overwritten by update
--rollback:           restored the modified binary and removed the used backup
--check:              printed installed 0.1.0 and "latest: none published"
--clean:              removed 0 stale staging dirs, kept 1 backup
--uninstall:          removed binary, wrapper, theme, unit, update backups;
                      kept state/history
--uninstall --purge:  removed state/history
uninstall.sh:         standalone uninstall keeps the DB by default
```

Checks after the lifecycle test:

- no `.bak`, `.old`, `.new`, `.part`, or `.screen-time-install.*` beside
  installed artifacts,
- no staging directories left after install/update,
- one rollback backup after a successful update,
- no log files; the unit uses journald.

The generated unit passes:

```text
$ systemd-analyze --user verify <temp>/systemd/user/screen-time.service
verify_exit=0
```

Service activation was verified with a transient systemd user unit using the
same ExecStart and environment, because the fake HOME cannot be used to enable a
persistent unit in the real user manager:

```text
$ systemctl --user is-active screen-time-smoke-1834674.service
active
$ cat $XDG_RUNTIME_DIR/screen-time/live.json
{... "powered_open_start":"2026-11-21T18:03:51+07:00" ...}
$ systemctl --user stop screen-time-smoke-1834674.service
inactive
live.json removed
```

The installer itself was exercised end-to-end with `--no-service`; the
remaining service step is identical (`daemon-reload`, `enable --now`,
`doctor`).

The published release was also installed directly from GitHub in a fresh
temporary HOME:

```text
$ curl -fsSL .../releases/latest/download/install.sh
$ bash install.sh --no-service
downloading screen-time-x86_64-linux.tar.gz (v0.1.0)
installed 0.1.0 from v0.1.0
$ screen-time --version
screen-time 0.1.0
$ install.sh --check
installed: screen-time 0.1.0
latest:    v0.1.0
$ install.sh --no-service --uninstall --purge
state removed
```

The release tarball checksum matched the published `SHA256SUMS`.

## 7. Artifacts

GitHub release `v0.2.0` assets (`https://github.com/WillGle/screen-time-rofi/releases/tag/v0.2.0`):

```text
screen-time-x86_64-linux.tar.gz   2,826,776 bytes
screen-time-aarch64-linux.tar.gz  2,673,533 bytes
SHA256SUMS
install.sh                        19,537 bytes
uninstall.sh                       3,141 bytes
```

Local x86_64 release tarball:

```text
dist/screen-time-x86_64-linux.tar.gz
  bin/screen-time
  bin/rofi-screen-time
  share/rofi/screen-time.rasi
  systemd/screen-time.service
  LICENSE
  README.md
dist/SHA256SUMS
```

The checksum is regenerated from the packaged file; the release workflow
produces the same `SHA256SUMS` asset for both architectures.

### M8/M9 validation

- **Section layout:** golden main/detail now contain `▌ SCREEN TIME`, `▌ WHEN`,
  `▌ APPS`, `▌ BURSTS`, and `▌ 7 DAY`; app rows use accent strips, POWER uses
  power, and other uses muted. See
  [`screenshots/m8-main.png`](screenshots/m8-main.png) and
  [`screenshots/m8-detail.png`](screenshots/m8-detail.png).
- **Real Matugen palette on this machine:** base `#101410`, accent `#9fd49c`,
  power `#9eb09a`, muted `#c2c9bd`; static font `JetBrainsMono Nerd Font 10`.
  `render --format rofi-theme` imported `~/.config/rofi/theme.rasi`, used the
  palette background, and emitted the static font.
- **Palette change without restart:** a copied palette changed to accent
  `#B0E0A0` and power `#40C0D0`; the next render emitted those spans. The
  daemon was not running and no cache was involved. The live wallpaper was not
  changed during validation (that would alter the user's desktop); the change
  was exercised through the same palette file contract.
- **Corrupt palette:** invalid JSON fell back to the valid `static.env`; an
  invalid color rejects the whole Matugen palette rather than half-applying it.
- **Font/layout:** at 1280 px, sizes 12/14/16 produced ruler widths 116/68/68
  columns; 14 and 16 selected the compact/top-four layout, and `layout.mode =
  "full"` forced 116 columns back. Font changes need no daemon restart.
- **Generated Rofi theme:** wrapper wrote
  `$XDG_RUNTIME_DIR/screen-time/rofi-theme.rasi` (1287 bytes in the current
  palette run) before launch; the installed standalone theme is fallback only.
- **Installer update/rollback:** installed published v0.1.0, updated to the
  local v0.2.0 tarball (`screen-time 0.2.0`), then rolled back to
  `screen-time 0.1.0`; no artifacts beside the binary/theme/unit.
- **No category palette/legend:** all inline colors are role colors from the
  resolved theme; app identity remains the row label.
- **Published release:** downloaded `v0.2.0` `SHA256SUMS`, verified both
  tarballs plus `install.sh`/`uninstall.sh` with `sha256sum -c`, installed in a
  fresh temporary HOME, and ran `screen-time --version` → `0.2.0` and
  `install.sh --check` → latest `v0.2.0`.
- **Warm-open remeasurement:** the M8 wrapper generates a theme (9 ms) and
  queries the monitor width with `hyprctl` (26–32 ms). Direct Rofi startup with
  the real user theme/font under Xvfb measured ~185–260 ms end-to-end, above
  the old 100 ms target. The renderer is still 9 ms; the additional cost is
  Rofi theme/font startup and the shell/theme setup. This is recorded as a
  remaining risk, not hidden by the earlier static-theme number.
- **Wallpaper validation:** the actual desktop wallpaper was not changed (that
  would alter the user's running session). A copied real palette was changed
  to `#B0E0A0`/`#40C0D0` and the next render picked it up, which exercises the
  same palette contract used by `theme-apply`. A visual wallpaper change
  remains a manual check.

Nix result: `result -> /nix/store/...-screen-time-0.2.0` with
`bin/screen-time`, `bin/rofi-screen-time`,
`share/rofi/screen-time.rasi`, and `systemd/screen-time.service`.

`dist/` and `result` are gitignored. Generated databases, tarballs, and target
artifacts are not committed.

## 8. Remaining risks and limitations

1. **Hyprland-only.** Window identity is the Hyprland class. Sway/X11 are not
   supported in v1. Title changes with the same class are ignored, as required.
2. **Idle is the only guard for inactive time.** Lock, suspend, and display-off
   are intentionally not tracked; a locked or display-off period can count as
   active until logind says idle. The UI calls the metric `active`, not
   screen-on.
3. **Crash recovery loses the current open interval.** The daemon closes the
   previous power session at the last known event and marks the gap unknown; it
   never fabricates active time. The current app interval is not persisted
   before a crash.
4. **Sub-second intervals disappear.** Durable timestamps are whole seconds;
   a focus interval that starts and ends in the same second is not stored.
   Normal use is unaffected; rapid synthetic toggles can lose such an interval.
5. **One-batch detail lag.** Detail reads SQLite plus tmpfs and can lag by up to
   60 seconds or 100 events. It says `may lag one batch`; there is no socket or
   live query.
6. **Synthetic measurement scope.** Rofi launch was measured under Xvfb with
   software rendering. Real hardware, fonts, and effects may change absolute
   latency and PSS. The relative design target (<100 ms) was met.
7. **AArch64 CI.** The workflow uses `ubuntu-24.04-arm`; the job is marked
   experimental so the x86_64 release is not blocked if that runner is
   unavailable. No AArch64 artifact was produced locally.
8. **Release.** `v0.2.0` is published and installable from the release URL.
   Future releases are tag-triggered; there is no auto-update daemon.
9. **Fixed Rofi selection row.** The launcher selects absolute row 8, which is
   the first app row with the section layout. Changing the section/header count
   requires updating `-selected-row` in the shim.
10. **Theme ownership.** A theme modified outside the installer is protected:
    updates require `--force`. The installer records the managed checksum in
    state.

## 9. Definition of Done

- [x] `cargo fmt --check`, `cargo clippy`, `cargo test` pass.
- [x] `cargo run -- --version` works.
- [x] Daemon tracks Hyprland app-level events and idle (idle via logind; fake
      socket capture smoke test).
- [x] SQLite WAL batches 60 s/100 events; raw 30 d; sessions 90 d; daily 365 d.
- [x] `live.json` is atomic, stale-safe, and removed on clean shutdown.
- [x] Rofi main screen matches the 96-column spec with section markers and role
      colors (golden and screenshots).
- [x] App detail shows bursts and `sessions through`.
- [x] No categories, titles, suspend/lock/display, socket, or live query.
- [x] `install.sh` clean install/update/rollback/uninstall with no artifacts
      beside targets; state retained unless `--purge`.
- [x] Nix build and release workflow exist; local Nix build and workflow YAML
      validation pass.
- [x] README has install, update, rollback, uninstall, and troubleshooting.
- [x] Matugen palette + `static.env` + config font sync work without daemon
      restart.
- [x] Final report includes raw measurements and artifacts.
