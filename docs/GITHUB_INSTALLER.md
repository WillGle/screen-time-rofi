# Screen Time — GitHub One-Time Installer Design

- **Status:** Design only, 2026-11-21.
- **Goal:** one command installs the app from a GitHub release, per user, with no
  build tools, no root, no runtime downloads, and no post-install setup.
- **Scope:** Linux + Hyprland + Rofi + systemd user session.
- **Non-goals:** multi-user install, system-wide service, cloud sync, package
  manager integration, auto-update daemon.

## 1. Supported install paths

| Audience | Path |
|---|---|
| General Linux user | `install.sh` from the latest GitHub release |
| NixOS user | flake/Home Manager module |
| Developer | `cargo build`/`nix develop` in the repo |

The installer and the Nix module must install the **same runtime artifacts**:
one `screen-time` binary, one Rofi wrapper, one theme, one user unit. Do not
create two different runtime layouts.

## 2. Release artifacts

Each GitHub release should publish:

```text
screen-time-x86_64-linux.tar.gz
screen-time-aarch64-linux.tar.gz
SHA256SUMS
install.sh
uninstall.sh
```

Tarball layout:

```text
bin/screen-time
bin/rofi-screen-time
share/rofi/screen-time.rasi
systemd/screen-time.service
LICENSE
README.md
```

Build the binary with SQLite statically linked (or bundle the library) so the
installer does not require `libsqlite3`/`sqlite3` on the host. The Rofi wrapper
should call `screen-time render`, not depend on `jq`; `jq` is not guaranteed on
minimal systems.

## 3. Per-user install layout

```text
~/.local/bin/screen-time
~/.local/bin/rofi-screen-time
~/.local/share/screen-time/rofi/screen-time.rasi
~/.config/systemd/user/screen-time.service
```

State and runtime are created by the daemon, not by the installer:

```text
~/.local/state/screen-time/screen-time.db
$XDG_RUNTIME_DIR/screen-time/live.json
```

No files under `/usr`, `/etc`, or `/nix/store` are modified. The installer must
refuse to run as root.

## 4. install.sh flow

```text
1. parse flags
2. verify Linux, non-root, systemd --user, rofi, Hyprland
3. resolve version (latest release or --version)
4. detect arch (x86_64 / aarch64)
5. create one `mktemp -d` staging dir; download tarball + SHA256SUMS there
6. verify checksum
7. extract into the same staging dir
8. stop the existing user service (if any)
9. back up existing binary/theme/unit into the state backup dir
10. atomically install binary, wrapper, theme, unit
11. systemctl --user daemon-reload
12. systemctl --user enable --now screen-time.service
13. run `screen-time doctor`
14. prune old backups (keep one previous), print cleanup summary
15. print usage and log command
16. always remove staging dir on EXIT/INT/TERM; restore backup on failure
```

### Required checks

- `set -euo pipefail`.
- Fail if `EUID == 0`; this is a per-user install.
- Fail if `rofi` is missing, with a distro-specific hint.
- Fail if `systemctl --user` is unavailable.
- Warn (or fail with `--require-hyprland`) if `hyprctl`/`Hyprland` is missing.
  Window tracking is compositor-specific; v1 supports Hyprland.
- Check `curl` or `wget`, `tar`, and `sha256sum`.
- Do not assume `~/.local/bin` is in `PATH`; print a warning if it is not.
- Do not overwrite a user-modified `screen-time.rasi` without `--force`; back it
  up first.

### Idempotency

Re-running `install.sh` is an upgrade:

- same version: reinstall cleanly;
- newer version: replace binary/unit, keep state;
- existing service: stop before replace, start after;
- existing theme: back up, do not silently discard;
- no duplicate service/theme entries.

### Flags

```text
--version VERSION     install a specific release tag
--check               print current and latest version; no changes
--prefix PATH         default: ~/.local
--no-service          install files only
--no-theme            do not install the Rofi theme
--require-hyprland    fail if Hyprland is not detected
--force               overwrite an existing theme/unit
--dry-run             print actions without changing files
--clean               remove stale staging/backups; no update
--keep-backups N      previous versions to retain; default 1
--rollback            restore the previous binary/unit/theme
--uninstall           remove binary/wrapper/theme/unit; keep state
--purge               with --uninstall, also delete state
```

## 5. systemd user unit

```ini
[Unit]
Description=Screen Time tracker
After=graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStart=%h/.local/bin/screen-time daemon
Restart=always
RestartSec=2s
StateDirectory=screen-time
RuntimeDirectory=screen-time

[Install]
WantedBy=graphical-session.target
```

- `StateDirectory` gives `~/.local/state/screen-time` without installer logic.
- `RuntimeDirectory` gives `$XDG_RUNTIME_DIR/screen-time` for `live.json`.
- No root, no system unit, no socket activation.

## 6. Rofi integration

- Install `rofi-screen-time` as a wrapper that invokes
  `screen-time render --format rofi` and pipes rows into Rofi.
- Install a standalone theme; do not require the user's Matugen/theme setup.
- Use `ROFI_THEME`/`--theme` override for users with an existing Rofi theme.
- Do not edit the user's `~/.config/rofi/config.rasi` automatically; print the
  keybind snippet instead.
- Example keybind (Hyprland):

```text
bind = SUPER, S, exec, rofi-screen-time
```

## 7. GitHub installation

**Primary, documented method:** download, inspect, then run.

```bash
curl -fsSLO https://github.com/WillGle/screen-time-rofi/releases/latest/download/install.sh
less install.sh
bash install.sh
```

`curl | bash` may be mentioned as a convenience shortcut, but it is not the
recommended path and should not be the README’s primary instruction.

The script must verify the release tarball checksum. If GitHub attestations or
signatures are available, verify them too; otherwise document that the checksum
protects against corruption, not a compromised release account.

The installer must be readable without downloading the tarball first: dependency
checks, paths, and service actions should be visible in `install.sh` itself.

## 8. Update, rollback, and uninstall

There is **no auto-update daemon**. The user chooses when to update; the
installer is the update mechanism.

### Check for an update

```bash
bash install.sh --check
```

It prints the installed version and the latest GitHub release tag. It makes no
filesystem changes.

### Update to latest

Download and inspect the new installer, then run it again:

```bash
curl -fsSLO https://github.com/WillGle/screen-time-rofi/releases/latest/download/install.sh
less install.sh
bash install.sh
```

The installer is idempotent and performs a safe upgrade:

1. resolve/verify the new release and checksum;
2. stop the existing user service;
3. back up the current binary, unit, theme, and a DB snapshot
   (`VACUUM INTO` or a file copy);
4. replace artifacts atomically;
5. run schema migrations on daemon start;
6. `daemon-reload`, restart the service;
7. run `screen-time doctor`;
8. on failure, restore the previous binary/unit/theme and DB snapshot.

State under `~/.local/state/screen-time/` is preserved. Retention and history
are not reset.

### Update or downgrade to a specific version

```bash
bash install.sh --version v1.2.0
```

This is also the manual downgrade path. Schema migrations may be forward-only;
if the older binary cannot read the newer schema, restore the pre-update DB
snapshot. The installer should state this before downgrading.

### Rollback

```bash
bash install.sh --rollback
```

Restores the previous binary/unit/theme and the pre-update DB snapshot, then
restarts the service. Keep at least one previous version; do not garbage-collect
it automatically.

### Cleanup / no-clutter contract

Every install and update must leave the user’s filesystem clean.

- **One staging directory only:** `mktemp -d` outside the home directory.
  Remove it with a trap on `EXIT`, `INT`, and `TERM`. Never leave downloaded
  tarballs, checksum files, or extracted trees behind.
- **One backup location:** `~/.local/state/screen-time/backups/updates/`.
  Do not scatter `screen-time.old`, `*.bak`, or `*.tmp` next to binaries,
  themes, or systemd units.
- **Retain one previous version** by default (`--keep-backups 1`). After a
  successful update and `doctor` pass, prune older backups. A failed update
  keeps the rollback copy.
- **Atomic replacement:** write the new binary/unit/theme to a temp file in the
  same directory, then rename. Never leave `.new`/`.part` files.
- **Theme safety:** if the user’s `screen-time.rasi` differs from the shipped
  one, keep a single backup in the state backup dir or skip it; do not create
  timestamped backup litter beside the theme.
- **No log files:** use journald (`journalctl --user -u screen-time`). If debug
  logs are ever written, cap them by size and rotate.
- **DB cleanup:** WAL/SHM files are normal while the daemon runs. After
  migration, checkpoint/truncate the WAL; do not run `VACUUM` on every update.
  Retention cleanup stays in the daemon’s daily job.
- **Temp snapshot:** `live.json` lives in `$XDG_RUNTIME_DIR` and disappears with
  the session; the daemon removes it on clean shutdown.
- **Uninstall:** remove binaries, unit, theme, and update backups. Keep the DB
  and history unless `--purge`. Remove the systemd unit before deleting the
  binary so no failed unit remains.
- **`--clean`:** removes stale staging dirs, old update backups, and orphaned
  temp files; never touches the active DB.

The installer should print a short cleanup summary, for example:
`removed 1 staging dir, kept 1 rollback backup, pruned 2 old backups`.

### NixOS

```bash
home-manager switch   # or: sudo nixos-rebuild switch --flake ...
```

The Nix store handles versioning/rollback; do not run `install.sh --rollback` on
a Nix-managed install.

### Uninstall

```bash
screen-time --uninstall   # or: bash install.sh --uninstall
```

- Stop and disable the user service.
- Remove binary, wrapper, theme, unit.
- Keep `~/.local/state/screen-time/` by default; `--purge` deletes it.
- Never delete user data without an explicit flag.

## 9. NixOS / Nix users

Current state:

- The flake provides a package, dev shell, checks, and a Home Manager module:
  ```bash
  nix build github:WillGle/screen-time-rofi
  nix run github:WillGle/screen-time-rofi -- --version
  ```
- Home Manager:
  ```nix
  inputs.screen-time-rofi.url = "github:WillGle/screen-time-rofi";

  imports = [ inputs.screen-time-rofi.homeManagerModules.default ];
  services.screen-time.enable = true;
  ```
  The module installs the package, user unit, and Rofi wrapper.
- `nix profile install` alone still does not wire the service; it is not a
  one-time install.
- Do not run `install.sh` and also manage the package through Nix; pick one
  owner.

On NixOS, the installer detects `/etc/NIXOS` and prints this recommendation; it
must never write into `/nix/store`.

## 10. Acceptance tests

1. Fresh user, no `~/.local/bin`: one command installs, service active, Rofi
   entry opens.
2. Re-run same version: no duplicate unit/theme, service still active.
3. Upgrade: state preserved, DB migration succeeds.
4. Missing `rofi`: clear error, no partial install.
5. Non-Hyprland: clear warning/error; no false claim of tracking.
6. Root: refuses to run.
7. Interrupted download/checksum failure: no files replaced.
8. Uninstall: service gone, binaries gone, state retained; `--purge` removes it.
9. Reboot/login: service starts automatically; `live.json` appears.
10. Read-only `$HOME`/no systemd: clear error with manual-run instructions.
11. After update: no tarball/staging dir in `/tmp`, home, or `$XDG_RUNTIME_DIR`;
    only one rollback backup remains.
12. After update: no `*.bak`, `*.old`, `*.new`, or `*.part` beside the binary,
    theme, or unit.
13. Interrupted update: staging dir removed, previous version restored, service
    running.
14. `--clean`: removes stale backups/staging; active DB untouched.
15. Uninstall: binaries, unit, theme, and update backups gone; state retained
    unless `--purge`.

## 11. Open items

1. Release signing/attestation approach (repository is `WillGle/screen-time-rofi`).
2. Whether v1 is Hyprland-only or also supports Sway/X11.
3. Whether to publish a Nix flake alongside the tarball.

Decided default: `rofi-screen-time` is a thin exec shim/symlink to
`screen-time render --format rofi`; no `jq` dependency.
