# Goal Mode — Screen Time v1

- **Status:** Ready to execute; no code started.
- **Goal owner:** single user, local only.
- **Primary specs:**
  - `docs/research/SCREEN_TIME_UI_SPEC.md`
  - `docs/research/SCREEN_TIME_STORAGE_ARCHITECTURE.md`
  - `docs/research/GITHUB_INSTALLER.md`
- **Research trail:** `docs/research/SCREEN_TIME_ROFI_UI.md`,
  `docs/research/ROFI_RESOURCE_BENCHMARK.md`.

## 1. Goal statement

Ship a **per-user, local-only screen-time app** that installs once via GitHub
`install.sh`, runs a lightweight event-driven tracker, and opens a
high-visibility static Rofi day timeline.

At the end, a new user on Linux + Hyprland + Rofi + systemd user session can:

1. download and inspect `install.sh`;
2. run it once;
3. get a working tracker service and Rofi entry;
4. open a day timeline showing device on/off, total active time, top five app
   durations, and when each app was used;
5. open one app detail view with exact sessions/bursts;
6. update/rollback/uninstall without leftovers.

## 2. Non-goals

- No window-title / website / project / category tracking.
- No suspend, lock, or display-off states.
- No cloud, sync, accounts, multi-user, multi-client, socket, or live query.
- No animations, live redraw, or UI polling.
- No auto-update daemon.
- No mobile/iOS/Android support.
- No dashboard/card layout.
- No manual database management.

## 3. Locked product decisions

| Area | Decision |
|---|---|
| UI | Rofi dmenu, static, high contrast, no animation |
| Timeline | 30-minute slots, 2 chars each, 96 columns |
| Apps | Top 5 + other |
| Power | On/off only |
| Scope | App-level only |
| Storage | SQLite WAL, `synchronous=NORMAL` |
| Live state | tmpfs JSON |
| Batch | 60 s or 100 events |
| Raw retention | 30 days |
| Session retention | 90 days |
| App detail | SQLite + tmpfs; no live query; accept one-batch lag |
| Install | `install.sh`; Nix module optional |
| Update | Re-run installer; one rollback backup; cleanup |
| Language | English UI labels for v1 |

## 4. Proposed implementation stack

**Recommendation: Rust.**

- One static-ish binary with subcommands:
  - `screen-time daemon`
  - `screen-time render --format rofi`
  - `screen-time doctor`
  - `screen-time --version`
- `rusqlite` with bundled SQLite (no host `sqlite3` dependency).
- Serde for JSON; small Unix-socket/event parser for Hyprland; logind idle via
  D-Bus; systemd user service.
- Single binary avoids a shell/jq runtime and makes `install.sh` simple.
- Cross-build `x86_64-linux` and `aarch64-linux` with static SQLite; publish
  checksummed tarballs.

If Rust is rejected, the fallback is Go; Python is not preferred because of
packaging and daemon footprint.

## 5. Repository layout (proposed)

Keep the new implementation parallel until parity; do not overwrite the current
dirty screen-time files.

```text
pkgs/screen-time/
  default.nix
  Cargo.toml
  Cargo.lock
  src/
    main.rs
    db.rs
    capture.rs
    sessions.rs
    snapshot.rs
    render.rs
    doctor.rs
  tests/
    fixtures/

modules/desktop/apps/screen-time-v2/   # optional Nix module during migration
  default.nix

install.sh
uninstall.sh
.github/workflows/release.yml
```

The GitHub release source may live in this repo or a dedicated app repo; the
installer only consumes the release tarball.

## 6. Milestones

### M0 — Baseline and isolation

- Record `git status`, `git diff`, `git diff --cached`.
- Confirm the current screen-time working-tree changes remain untouched.
- Create a dedicated branch/worktree for the new implementation.
- Freeze the three specs.
- Confirm Rust + Hyprland-only v1 + `WillGle/screen-time-rofi`.

**Exit:** no accidental edits to existing dirty files; build environment ready.

### M1 — Data core

- SQLite schema: `events`, `sessions`, `daily_app`, `daily_slots`,
  `power_sessions`, `meta`.
- `PRAGMA user_version` migrations.
- WAL + `synchronous=NORMAL` + busy timeout.
- Batch commit at 60 s or 100 events; flush on shutdown/day rollover.
- Retention: raw 30 days, sessions 90 days, daily aggregates 365 days.
- tmpfs `live.json` with `as_of`, `last_commit_at`, open interval, totals, slots.
- Unit tests for session merge, bursts, batch, retention, open-interval math.

**Exit:** tests pass; a simulated day writes only batched transactions.

### M2 — Capture

- Hyprland active-window events; app-level only.
- Idle begin/end; boot/shutdown records.
- No suspend/lock/display events.
- Open interval in memory; no 5-second polling.
- App identity normalization; long-name handling.
- Crash recovery: close last interval at last known event; mark unknown.
- Integration tests with fixture events.

**Exit:** session and burst counts match fixtures; no timer-based disk writes.

### M3 — Rofi renderer

- `render --format rofi` emits header, ruler, POWER row, five app rows, other,
  footer, and theme snippet.
- 96-column strip, `▓▓`/`██`/`▒▒`.
- Main data from tmpfs; detail data from SQLite + tmpfs.
- Stale guard: do not extend open interval if `as_of` is older than 2 minutes.
- Detail shows `sessions through HH:MM · may lag one batch`.
- Golden output tests at 1280×800 and 1920×1080.
- No animation, no socket, no raw-log scan.

**Exit:** UI acceptance criteria from `SCREEN_TIME_UI_SPEC.md` pass.

### M4 — Rofi integration and ×N experiment

- Install theme/wrapper through the same package.
- Custom keys: arrows, T, R, Enter, Esc, Backspace.
- App detail one level only.
- Test `×N` variant at both widths; default off until it wins.
- Measure warm open time and PSS; target warm open under 100 ms.

**Exit:** static UI opens from tmpfs; no visible buttons; width tests pass.

### M5 — Installer and lifecycle

- `install.sh` per `GITHUB_INSTALLER.md`:
  - non-root, dependency checks, checksum;
  - atomic install, systemd user unit, `doctor`;
  - backup, cleanup, one rollback copy, `--clean`, `--rollback`, `--uninstall`,
    `--purge`.
- `uninstall.sh` or `install.sh --uninstall`.
- Shellcheck + bats tests for clean install/update/failure/rollback.
- Verify no leftovers: no tarballs, staging dirs, `.bak`, `.old`, `.part`.

**Exit:** fresh install and update tests pass on a clean user account/container.

### M6 — Nix integration (optional)

- `pkgs/screen-time/default.nix` builds the same binary.
- Home Manager module installs the same layout for NixOS.
- Do not mix installer and Nix ownership on the same machine.
- `nix flake check --no-build --no-write-lock-file`.
- Full build if NixOS configuration changes.

**Exit:** NixOS path and installer path produce identical runtime layout.

### M7 — Release and validation

- GitHub Actions: x86_64 + aarch64 tarballs, `SHA256SUMS`, `install.sh`,
  `uninstall.sh`.
- Optional attestations/signature.
- Release test on a clean VM/user:
  install → service active → Rofi open → detail → update → rollback → uninstall.
- Measure write amplification (`/proc/<pid>/io`), fsyncs, PSS, open latency.
- Update docs and release notes.

**Exit:** one-command install works; no clutter; all acceptance criteria pass.

## 7. Definition of done

- [ ] `install.sh` works on a clean Linux + Hyprland + Rofi + systemd user host.
- [ ] Service starts at login and survives restart.
- [ ] Main screen matches the locked 96-column layout.
- [ ] POWER row shows on/off only.
- [ ] Top five apps + other; no categories.
- [ ] App detail shows bursts/sessions and `sessions through`.
- [ ] No socket, no live query, no raw-log scan on click.
- [ ] Batch 60 s / 100 events; raw 30 d; sessions 90 d.
- [ ] No animation; idle while open.
- [ ] Update/rollback/uninstall leave no junk; one rollback backup.
- [ ] Tests and CI release artifacts pass.

## 8. Execution protocol

1. Work milestone by milestone; do not start the next until the previous exit
   criteria pass.
2. At each milestone, report changed files, commands run, raw results, and
   remaining risk.
3. Preserve unrelated dirty files; stage only the milestone’s files.
4. No `flake.lock` changes unless dependency work is explicitly in scope.
5. No structural refactor mixed with behavior change.
6. Stop before a destructive replacement of the current screen-time module; run
   the new implementation in parallel first.
7. UI changes must be checked against `SCREEN_TIME_UI_SPEC.md`; storage changes
   against `SCREEN_TIME_STORAGE_ARCHITECTURE.md`; installer changes against
   `GITHUB_INSTALLER.md`.

## 9. Decision gates

| Gate | Decision | Default |
|---|---|---|
| G0 | Language/runtime | Rust |
| G1 | Platform v1 | Hyprland only |
| G2 | GitHub owner/repo | `WillGle/screen-time-rofi` (confirmed) |
| G3 | Release signing | SHA256 + optional attestations |
| G4 | `×N` | Experiment; default off |
| G5 | Nix module | Optional, same layout |

## 10. Risk register

| Risk | Mitigation |
|---|---|
| Hyprland event API changes | Isolate capture behind one module; fixture tests |
| Idle detection unreliable | Treat idle as the only guard; document active-vs-screen-on |
| SQLite migration failure | `user_version`, transaction, pre-migration backup |
| Rofi width at 1280 | Golden layout test; 30-min/1-char fallback |
| Installer partial install | staging + atomic replace + rollback + cleanup trap |
| Existing dirty screen-time files | New package/worktree; no overwrite until parity |
| GitHub release compromise | Checksums; optional attestations; inspect script |
| Overengineering | Stick to non-goals; no categories/socket/multi-user |

## 11. First action

Start with **M0 only**: record the dirty state, create the implementation
worktree, confirm Rust + Hyprland-only + GitHub repo, and freeze the specs. No
production code before that checkpoint.
