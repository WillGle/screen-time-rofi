# Implementation decisions

This file records the smallest choices made where the specifications leave room
or overlap. New entries are append-only unless a decision is superseded.

## 0. Document precedence

When documents disagree:

- `docs/SCREEN_TIME_UI_SPEC.md` wins for UI.
- `docs/SCREEN_TIME_STORAGE_ARCHITECTURE.md` wins for storage and the write
  path.
- `docs/GITHUB_INSTALLER.md` wins for installation, updates, rollback, and
  uninstall.

Recorded conflicts and resolutions:

1. The storage document's example `slots` array is a flat list, while the UI
   needs a per-app strip. Resolution: the live snapshot keeps a flat total
   `slots` array for compatibility and adds per-app `apps[].slots` plus
   `power_slots`. This is a storage-document extension, not a UI change.
2. The storage document suggests updating the live snapshot on state changes,
   while the UI stale guard requires `as_of` to stay fresh during a long
   uninterrupted session. Resolution: write `live.json` on every state change,
   on batch commit (to update `last_commit_at`), on day rollover, and on
   shutdown. A once-per-minute tmpfs-only heartbeat refreshes `as_of` without
   polling application state and without any disk write. This is not a
   5-second sampler and does not change the batch policy.
3. The installer document names `rofi-screen-time` both as the launcher and as
   a thin shim to `screen-time render`. Resolution: one shell shim detects
   whether Rofi is calling it (`ROFI_RETV` is set). Under Rofi it execs the
   renderer (script-mode protocol); when run directly it launches Rofi with
   itself as the mode. There is still one runtime artifact.
4. The storage document's `power_sessions` example has no boot id. The goal
   requires boot IDs. Resolution: add a `boot_id` column to `power_sessions`
   and store boot ids in `events` and the live snapshot.
5. `SCREEN_TIME_ROFI_UI.md` is research; its recommendations are overridden by
   the locked UI spec wherever they differ.

## 1. Scope

- Rust 2024, one binary, SQLite bundled through `rusqlite`.
- Linux, Hyprland, Rofi, systemd user session only.
- App-level data only. Window titles are parsed only to find the separator in
  the Hyprland event and are immediately discarded, never persisted or rendered.
- No socket, live query, categories, suspend/lock/display states, cloud,
  multi-user, or auto-update daemon.

## 2. Time and identity

- Durable timestamps are Unix seconds (UTC). Display dates use local time.
- The calendar day is local midnight. Sessions crossing midnight are closed at
  midnight and reopened for the same app, so every stored session belongs to one
  local date.
- Timestamps are wall-clock. Durations are clamped to zero on negative or
  absurd intervals (clock jumps) and are never negative.
- Boot identity is `/proc/sys/kernel/random/boot_id`.

## 3. Sessions, gaps, and bursts

- A session starts when an app becomes the active window and ends when the
  active window changes, idle begins, the day rolls over, or the daemon shuts
  down.
- Adjacent unflushed sessions for the same app separated by at most **60
  seconds** are merged (micro-flicker guard). Once a session has been committed
  to SQLite it is not rewritten; this bounds the merge to the current batch.
- Bursts for the detail view group sessions separated by less than **2
  minutes**. The detail shows the first five bursts chronologically and a
  summary of the remaining sessions. This is a display-only grouping.
- App names are normalized from the Hyprland class: trim, collapse whitespace,
  strip a `-url-handler` suffix, and apply a small canonical-name table for
  common applications. Unknown all-lowercase names get an initial capital.
- Ignored classes: empty/`desktop`, the known lock surfaces (`hyprlock`,
  `swaylock`, `gtklock`, `i3lock`, `xscreensaver`, `org.gnome.ScreenSaver`,
  `lock`), and Rofi/Wofi launchers. Substring matching is not used so ordinary
  app names such as "Clock" are not discarded.
- A slot is active when it contains at least **60 seconds** of activity, as
  required by the UI spec.

## 4. Idle and power

- Idle comes only from the logind `IdleHint` property on the caller's session
  (`/org/freedesktop/login1/session/auto`). No lock, suspend, or display-event
  tracking is added.
- On idle begin, the open app session closes. On idle end, the last known app
  resumes with a new session at the idle-end time.
- Power on/off comes from daemon start/stop and boot IDs. A previous open power
  session found at startup is closed at the last known event and marked
  `unclean`. A clean daemon stop records `shutdown`; a missing/unknown boot
  identity records `unknown`.
- An open app interval is never persisted as active across a crash. On restart
  the gap after the last event is unknown and is not counted as active.

## 5. Live snapshot and stale guard

- `live.json` is written atomically (same-directory temp file + rename) with
  mode `0600` under `$XDG_RUNTIME_DIR/screen-time/`.
- It contains `version`, `date`, `as_of`, `last_commit_at`, `boot_id`, `pid`,
  closed active/powered totals, open intervals, per-app closed totals/sessions/
  slots, total slots, and powered slots.
- The renderer extends an open interval only when `as_of` is at most **120
  seconds** old, the boot id matches the current boot, and the tracker PID is
  alive. Otherwise it renders closed totals and a `stale` note. When there is
  no open interval at all, the closed totals are authoritative and the stale
  marker is still shown in detail because the snapshot age is real.
- For a past day or a missing snapshot, the main view reads compact SQLite
  aggregates, never raw events.

## 6. UI choices

- 96 columns, 48 slots, two characters per slot.
- Timeline starts after a 20-column prefix: 10-character app label, 7-character
  right-aligned duration, and 3 separating spaces. The ruler uses the same
  20-column prefix.
- Ruler labels are placed at columns 0, 24, 48, 72, and 94 of the 96-column
  strip (the `24` label is right-aligned at the strip end).
- App labels truncate at 10 characters. Full names are used in detail and are
  carried in Rofi row `info` metadata.
- `POWER` uses `▓▓`; app rows use `██`; the aggregated other row uses `▒▒`.
  Interactive Rofi rows use Pango markup: one accent for app activity, a
  neutral color for `POWER` and the other row. Plain output (used by golden
  tests) has no escapes.
- Peak is the busiest 90-minute window. The label is `peak HH:MM–HH:MM`.
- The same-time delta is shown only for today, when at least three prior days
  have data. A delta under one minute is rendered as
  `on par with usual at this time`.
- The 7-day sparkline ends on the viewed date. Its average excludes the viewed
  date and is hidden when fewer than three earlier days have data.
- The `×N` session-count experiment is off by default and enabled only by
  `SCREEN_TIME_SESSION_COUNT=1` (or the `session-count` build feature). It adds
  `×N` after the duration and is covered by width tests.
- Narrow fallback (one character per slot, top four apps) is implemented by
  setting `SCREEN_TIME_NARROW=1`, but is not the default. At 1280x800 and 12pt
  the 96-column layout is the golden target and is expected to fit.
- Detail title shows total, share of the viewed day, session count, median
  session length, switches per hour, bursts, `sessions through HH:MM · may lag
  one batch`, a 7-day trend, and `←/→ day   esc back`.
- The detail shows `Sessions will appear after the first batch.` when the
  database has no closed sessions and there is no open session.

## 7. Interactive Rofi contract

- `screen-time render --format rofi` prints a plain, complete payload when run
  outside Rofi. Golden tests use this form.
- Under Rofi (`ROFI_RETV` set) the same command emits Rofi script-mode metadata:
  hot keys, non-selectable header/ruler/power/footer rows, and `info` metadata
  carrying the full app key on selectable app rows.
- Custom keys: Left/Right previous/next day, `T`/`t` today, `R`/`r` refresh,
  `BackSpace` back from detail. `Esc` closes; Rofi owns that key. The default
  Left/Right/Backspace bindings are moved to Alt-modified keys so the required
  custom bindings do not conflict.
- Rofi skips empty rows. With the M8 section layout the first selectable app
  row is absolute row 8 (`SCREEN TIME`, summary lines, `WHEN`, ruler, POWER,
  `APPS`); the launcher passes `-selected-row 8`. Before M8 it was row 6.
  Script mode keeps the selection across re-renders.
- UI state (viewed date and open detail) lives in
  `$XDG_RUNTIME_DIR/screen-time/ui.json`, is reset on the first script call, and
  is written atomically. It is ephemeral UI state, not tracked data.
- Detail is one level. Selecting an app on the main view switches the same
  Rofi invocation to detail; Backspace returns. No second Rofi process and no
  socket.

## 8. Storage and retention

- SQLite WAL, `synchronous=NORMAL`, `busy_timeout=5000`,
  `foreign_keys=ON`, `temp_store=MEMORY`.
- Exactly one writer (the daemon). Renderers open SQLite read-only and use a
  short busy timeout.
- Commit when 60 seconds have passed or 100 events are pending, whichever
  comes first; also on clean shutdown and day rollover.
- Retention: raw events 30 days, sessions 90 days, daily app/slot aggregates
  365 days. Power sessions use the 365-day aggregate window. Retention runs at
  startup and day rollover, never in the render path.
- No `VACUUM` in the retention job. A pre-migration `VACUUM INTO` backup is
  made in the state directory when `user_version` must change; the installer
  also copies the database before an update.

## 9. Installer

- `install.sh` has one staging directory created with `mktemp -d` and removed
  by an `EXIT`/`INT`/`TERM` trap. Downloads and extraction happen inside it.
- One rollback directory under
  `~/.local/state/screen-time/backups/updates/` (or the state directory when
  `--purge` is not used). Only one previous version is kept by default.
- Files are replaced by writing a temp file in the destination directory and
  renaming. No `.bak`, `.old`, `.new`, or `.part` files remain beside installed
  artifacts.
- A user-modified theme is detected by comparing it with the checksum recorded
  at the last install; `--force` is required to replace it.
- `--tarball PATH` and `--sha256 HEX` are test/local-install additions. Remote
  installs always download `SHA256SUMS` from the same release and verify it.
- `uninstall.sh` is standalone (it does not assume `install.sh` is present) and
  supports `--prefix`, `--purge`, and `--dry-run`.
- The installer refuses root, requires a systemd user manager, warns about
  missing Hyprland, and prints distro-neutral hints.

## 10. M8 — section colors and theme sync

- Added `src/config.rs` and `src/theme.rs`; rendering resolves the theme on
  every invocation, never in the daemon. A palette/font change is visible on the
  next Rofi open with no service restart.
- Config path is `$XDG_CONFIG_HOME/screen-time/config.toml` (default
  `~/.config/screen-time/config.toml`). A missing file is valid. In addition to
  the documented schema, optional `[theme]` color fields
  (`base`, `mantle`, `text`, `muted`, `accent`, `power`, `other`) are accepted
  so a user can pin a palette without Matugen. All explicit values are
  validated; if any explicit color is invalid, the entire explicit color set is
  ignored rather than half-applied.
- Color source priority is: explicit config colors → Matugen palette →
  `static.env` → built-in high-contrast palette. Matugen values are read from
  the documented paths and all seven roles must validate, otherwise the whole
  palette is discarded:
  `surface.default`, `surface_container.default`, `on_surface.default`,
  `on_surface_variant.default`, `primary.default`, and
  `palettes.secondary["70"]`; `other` follows muted.
- `static.env` values are the documented `THEME_STATIC_*`,
  `THEME_MONO_FONT`, and `THEME_ROFI_FONT_SIZE` keys. Single/double quotes and
  an optional `export ` prefix are accepted. `THEME_SIGNAL_MUTED` is the
  secondary muted fallback.
- Font priority is config `[font]` → `static.env` → JetBrainsMono Nerd Font 12.
  Invalid/empty families and sizes outside 6–72 are ignored.
- Section markers are `▌ SCREEN TIME`, `▌ WHEN`, `▌ APPS`, `▌ BURSTS`, and
  `▌ 7 DAY`. Summary/apps/bursts markers use accent; WHEN/7 DAY use power;
  other data uses muted; app strips use accent; POWER strips use power.
- The Rofi wrapper generates a complete theme file at
  `$XDG_RUNTIME_DIR/screen-time/rofi-theme.rasi` on every open. The generated
  theme imports the configured/user `rofi_theme` (or `ROFI_THEME`) for chrome
  when it exists, then overrides the app window/list rows/font with resolved
  values. The installed standalone theme is only a fallback. Rofi itself still
  receives concrete inline Pango colors for markers and strips.
- Installed `share/rofi/screen-time.rasi` now uses the built-in high-contrast
  colors and JetBrainsMono 12 instead of Catppuccin hardcoding.
- `layout.mode = auto` estimates the monospace fit from the resolved font size,
  a 94%-width window, the 20-column label prefix, and the 96/48-column strip.
  It switches to 48 columns/top four when 96 columns do not fit; `full` and
  `compact` override. `SCREEN_TIME_SCREEN_WIDTH` overrides the monitor width
  (the wrapper obtains it from `hyprctl monitors -j`); the default for a plain
  `render` is 1280. `SCREEN_TIME_NARROW=1` remains a test/debug force.
- Rofi skips empty rows, so the taller section layout moves the first app row
  from absolute row 6 to row 8. The launcher uses `-selected-row 8`; detail
  still uses Backspace for back navigation.
