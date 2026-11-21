# GOAL — Screen Time v1, one-shot autonomous build

You are executing this goal in one continuous session. Work autonomously until
the Definition of Done is satisfied. Do not stop for design approval; make the
smallest choice consistent with the specs, record it in `docs/DECISIONS.md`, and
continue.

## 0. Repository and boundaries

- **Repo:** `https://github.com/WillGle/screen-time-rofi`
- **Workdir:** the clone containing this `GOAL.md`.
- **Branch:** `main` (empty repo; create the initial commit).
- **Language:** Rust 2024.
- **Do not modify `/etc/nixos` or its existing screen-time module.** This repo is
  the new implementation.
- **Do not add** categories, window titles, websites, suspend/lock/display
  states, cloud, multi-user, multi-client, Unix socket, live query, animation,
  or an auto-update daemon.
- **Runtime scope:** Linux + Hyprland + Rofi + systemd user session, per-user,
  local only.

Read these before coding:

- `docs/SCREEN_TIME_UI_SPEC.md`
- `docs/SCREEN_TIME_STORAGE_ARCHITECTURE.md`
- `docs/GITHUB_INSTALLER.md`
- `docs/SCREEN_TIME_V1_GOAL.md`

If two documents conflict, the UI spec wins for UI, the storage doc wins for
storage, and the installer doc wins for installation. Record the resolution.

## 1. Deliverables

A working repository containing:

```text
Cargo.toml / Cargo.lock
src/
  main.rs
  cli.rs
  paths.rs
  db.rs
  migrations.rs
  capture.rs
  idle.rs
  sessions.rs
  snapshot.rs
  render.rs
  doctor.rs
tests/
install.sh
uninstall.sh
flake.nix
README.md
LICENSE
.github/workflows/release.yml
docs/
```

`screen-time` is one binary with:

```text
screen-time daemon
screen-time render --format rofi [--date YYYY-MM-DD] [--app KEY]
screen-time doctor
screen-time --version
```

No `jq`, Python, Node, or host `sqlite3` dependency. SQLite is bundled.

## 2. Non-negotiable behavior

- Event-driven; no 5-second polling for state.
- App-level only. No title storage.
- Hyprland active-window events; idle begin/end via logind D-Bus.
- Boot/shutdown records for power on/off; no suspend/lock/display tracking.
- Current day in memory + tmpfs `live.json`.
- SQLite WAL, `synchronous=NORMAL`, one writer.
- Batch commit at **60 seconds or 100 events**, whichever first.
- Flush on clean shutdown and day rollover.
- Raw events 30 days; sessions 90 days; daily aggregates 365 days.
- Main view reads tmpfs; detail reads SQLite + tmpfs; no raw-log scan.
- Detail may lag one batch; show `sessions through HH:MM`.
- Static Rofi; no animation, live redraw, socket, or auto-update.
- `live.json` is written atomically (temp + rename) and has `as_of`,
  `last_commit_at`, open interval, totals, slots.
- Stale guard: if `as_of` is older than 2 minutes, do not extend the open
  interval; show last closed totals and a stale marker.

## 3. Milestones and exit criteria

### M0 — Initial commit

- Keep the existing M0 scaffold.
- Add `GOAL.md`, `docs/DECISIONS.md`.
- `cargo fmt --check`, `cargo check`, `cargo run -- --version`.
- Commit: `chore: initialize screen-time v1 scaffold`.

### M1 — Data core

- Add dependencies: `anyhow`, `clap` (derive), `serde`, `serde_json`,
  `rusqlite` (bundled), `time` or `chrono`, `zbus` (blocking API), `libc` or
  `signal-hook` if needed.
- SQLite schema/migrations with `PRAGMA user_version`.
- Tables: `apps`, `events`, `sessions`, `daily_app`, `daily_slots`,
  `power_sessions`, `meta`.
- WAL, `synchronous=NORMAL`, `busy_timeout`, `foreign_keys=ON`.
- Batch writer: 60 s / 100 events.
- Retention job: raw 30 d, sessions 90 d, daily 365 d.
- tmpfs snapshot schema and atomic writer.
- Unit tests: session merge, burst, open interval, batch, retention,
  migration idempotency.

Exit: `cargo test` passes; no timer-based disk writes; write path uses batches.

### M2 — Capture

- Hyprland IPC socket discovery (`$XDG_RUNTIME_DIR/hypr/...` and `/tmp/hypr/...`).
- Parse `activewindow>>class,title`; app-level only.
- Ignore empty/desktop/lock/rofi classes.
- Idle begin/end via logind `IdleHint` D-Bus signal; no lock/display events.
- Boot ID from `/proc/sys/kernel/random/boot_id`; clean shutdown record.
- On startup, close previous open interval at last known event; mark gaps
  `unknown`, never `active`.
- Session/burst builder and app-name normalization.
- Fixture tests for switches, empty windows, idle, boot, crash recovery.

Exit: fixtures match; `live.json` updates on events only.

### M3 — Rofi renderer

- `render --format rofi` emits a complete Rofi payload without `jq`.
- Main screen exactly follows `SCREEN_TIME_UI_SPEC.md`:
  - 4 header lines;
  - 96-column ruler;
  - POWER row (`▓▓`);
  - top 5 app rows (`██`);
  - `+N apps` line (`▒▒`);
  - footer key hints.
- 30-minute slots, 2 chars each; no categories, no legend.
- App detail: total, sessions, median, switches/hour, bursts, `sessions
  through`, 7-day trend; no live query.
- `×N` experiment behind a build/env flag; default off.
- Golden tests for main/detail at 1280×800 and 1920×1080.
- Stale/as-of behavior tested.

Exit: golden output passes; no wrapping at 1280×800 with 12pt body.

### M4 — Rofi integration

- Install a standalone theme and a `rofi-screen-time` exec shim/symlink.
- Keys: arrows, T, R, Enter, Esc, Backspace.
- No visible navigation buttons.
- `render` reads tmpfs; detail reads SQLite + tmpfs.
- App detail one level only.
- Warm open target under 100 ms; measure PSS.

Exit: static UI opens; no animation; width/contrast checks pass.

### M5 — Installer and lifecycle

- `install.sh` as specified in `GITHUB_INSTALLER.md`:
  - non-root, dependency checks, checksum;
  - one staging temp dir with `EXIT/INT/TERM` cleanup;
  - atomic replace;
  - user systemd unit;
  - backup, `doctor`, rollback copy, `--clean`, `--rollback`, `--uninstall`,
    `--purge`.
- Default one rollback backup; prune old backups after a successful update.
- No `.bak`, `.old`, `.new`, `.part` beside binaries/theme/unit.
- No log files; journald only.
- `shellcheck install.sh uninstall.sh`.
- Test install/update/rollback/uninstall in a clean temp HOME.

Exit: no leftovers; service active; repeated install is idempotent.

### M6 — Nix and CI

- `flake.nix` builds the same binary with `rustPlatform.buildRustPackage`.
- `nix build` local x86_64; `cargo fmt --check`, `cargo clippy`, `cargo test`.
- GitHub Actions release workflow:
  - x86_64-linux and aarch64-linux release tarballs;
  - `SHA256SUMS`;
  - include `install.sh`, `uninstall.sh`, theme, unit;
  - no auto-publish without tag.
- If cross-compilation is blocked, publish x86_64 and document the aarch64
  follow-up; do not fake an artifact.

Exit: local artifact installs; CI workflow is syntactically valid.

### M7 — Final validation and report

- Fresh HOME install → service active → Rofi main/detail → update → rollback →
  uninstall.
- Verify no staging/tarball/backups left; only one rollback backup.
- Measure `/proc/<pid>/io` write bytes, fsync count, PSS, warm open.
- Update README and docs with actual commands/results.
- Final report: commits, tests, artifacts, measurements, remaining risks.

## 4. Git protocol

- Commit at each milestone with conventional messages.
- Do not rewrite history; do not force-push.
- If credentials allow, push `main`; otherwise keep local commits and report.
- Do not commit `target/`, `dist/`, databases, or generated release tarballs.
- If the repo is empty, the first commit is M0.

## 5. Decision policy

- Prefer the smallest implementation that satisfies the specs.
- No new abstractions unless a second consumer exists.
- Keep modules small; avoid async runtime unless required by a dependency.
- If a crate is needed, prefer well-maintained, minimal dependencies.
- If a spec is ambiguous, choose the simplest behavior, document it in
  `docs/DECISIONS.md`, and continue.
- If a required external action is impossible (GitHub credentials, release
  upload), finish local work, produce artifacts, and report the exact blocker.
- Do not stop for optional UI experiments; ship `×N` off by default.

## 6. Definition of Done

- [ ] `cargo fmt --check`, `cargo clippy`, `cargo test` pass.
- [ ] `cargo run -- --version` works.
- [ ] Daemon tracks Hyprland app-level events and idle.
- [ ] SQLite WAL batches 60 s/100 events; raw 30 d; sessions 90 d.
- [ ] `live.json` is atomic and stale-safe.
- [ ] Rofi main screen matches the 96-column spec.
- [ ] App detail shows bursts and `sessions through`.
- [ ] No categories/titles/suspend/lock/display/socket/live query.
- [ ] `install.sh` clean install/update/rollback/uninstall with no leftovers.
- [ ] Nix build and release workflow exist.
- [ ] README has install, update, rollback, uninstall, and troubleshooting.
- [ ] Final report includes raw measurements and artifacts.

Execute M0 → M7 in order. Do not stop until the Definition of Done is satisfied
or a hard blocker is documented.
