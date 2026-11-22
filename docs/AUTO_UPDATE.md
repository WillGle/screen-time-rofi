# Data safety and updates

Screen Time keeps local history on your machine. This page describes the update
channels, the data-safety guarantees, release verification, and recovery.

## Channels

There is exactly one owner for each installation:

| Channel | Update mechanism | Notes |
|---|---|---|
| Nix / Home Manager | `nix flake update screen-time-rofi`, then rebuild | The flake input and lock choose the version. The standalone installer refuses Nix/Home Manager-owned paths and never adds update units for them. |
| Flatpak (future) | Package manager timer | A window tracker needs host/Hyprland access; sandbox packaging is a separate project. |
| Standalone | `install.sh` plus an opt-in user timer | This page. No root, no daemon auto-executes code by default. |

`--nixos` in `install.sh` only activates an existing checkout through the host
flake input. Do not mix channels for the same binary or unit.

## What an update owns

The updater may write only executable, unit, theme, version, and hash files,
`backups/updates/`, and `updates/*.json` plus the update lock. It never writes,
truncates, or restores over these without an explicit `--purge`:

- the live database (`screen-time.db`, `-wal`, `-shm`);
- migration backups under `backups/migrations/`;
- `config.toml`;
- the legacy `~/.local/share/rofi-screen-time/` and `~/.cache/rofi-screen-time/`
  directories;
- the theme choice file.

Every snapshot and manifest is created with `0700` directories and `0600`
files; the updater sets `umask 077` rather than relying on yours.

## Update modes

```bash
bash install.sh --check                 # report only
bash install.sh --check --json          # stable JSON schema
bash install.sh --update                # manual, latest stable release
bash install.sh --auto-update=check     # daily check timer (default opt-in)
bash install.sh --auto-update=apply     # signed automatic updates
bash install.sh --no-auto-update        # remove timers and update state
```

`--check` exit codes: `0` up to date, `10` update available, `20` network
error, `21` invalid release, `22` signature error. `--check` does not replace
files. `--write-check-state` additionally writes
`~/.local/state/screen-time/updates/last-check.json` for the check timer.

`--update` is a no-op when the installed version is current. It only follows
stable `vX.Y.Z` tags. Downgrades require an explicit `--allow-downgrade` and a
schema compatibility check; `--allow-schema-downgrade` is a separate override.

## Release verification

Releases are signed with [minisign](https://jedisct1.github.io/minisign/). The
public key is pinned twice: in `minisign.pub` and in the installer. The current
key id is `4E5BF83E07DA94ED`. Release assets include:

- `screen-time-<arch>-linux.tar.gz`
- `install.sh`, `uninstall.sh`, and `minisign.pub`
- `SHA256SUMS`, `SHA256SUMS.minisig`, `install.sh.minisig`

Release tags should be signed and CI builds from the tag. The installer
downloads the checksum file and its detached signature, verifies the signature
against the pinned key, verifies the archive checksum, and verifies the
installer itself. If the running script differs from the signed
release installer, it re-executes the verified release copy. `--allow-unsigned`
exists only for a local tarball or offline test and is rejected in automatic
mode. A first install must be verified manually:

```bash
minisign -Vm install.sh -p minisign.pub
# Trusted key id: 4E5BF83E07DA94ED
```

### Key rotation and revocation

- The trusted keys live in the `TRUSTED_KEYS` array in `install.sh` (id and
  base64 public key) and in `minisign.pub`.
- Rotation uses an overlap window: publish a release signed with the new key
  while the installer still trusts both old and new keys.
- Verify the new key id out of band before the next release. Then remove the
  old entry and signed artifacts.
- A revoked key is removed from the array in a release signed by a remaining
  trusted key. Releases signed only by a revoked key fail closed.
- Emergency compromise: publish a new installer and key through the repository
  and release notes, keep the revocation list, and tell users to verify the new
  fingerprint manually. Never fetch a trusted key from the same release
  object.
- The secret key is offline and never committed. `scripts/sign-release.sh`
  reads `MINISIGN_SECRET_KEY` from the release environment.

## Timer behavior

`--auto-update=check` installs a daily `screen-time-update-check.timer` with
`Persistent=true` and `RandomizedDelaySec=1h`. It runs `--check --json
--write-check-state` and sends a desktop notification when a newer stable
version exists. It downloads no executable code and installs nothing.

`--auto-update=apply` installs `screen-time-update-apply.timer`, which runs
`--update --auto --channel stable --keep-backups 2`. Policies:

- signed stable releases only; unsigned or unverifiable releases fail closed;
- never a downgrade and never a major-version bump without manual approval;
- no `--force`, `--uninstall`, or interactive prompt;
- skip while another update holds the lock;
- exponential backoff (1 h, 2 h, 4 h, ... capped at 24 h) after a failure, and
  no immediate retry of the same bad tag;
- skip while the session is locked or the machine is discharging on battery;
- checked outside a graphical session using `doctor --json --no-runtime`:
  runtime warnings such as missing Hyprland or Rofi do not trigger a rollback;
- notifications use `notify-send` when available and always log to the journal.

User timers run only while the systemd user manager is available. With
lingering enabled they can run after logout; `Persistent=true` catches up on
the next login. Without a user manager the timer does not run and nothing is
reported as a failed update.

The updater units use `NoNewPrivileges=true`, `PrivateTmp=true`,
`ProtectSystem=strict`, explicit `ReadWritePaths`, journal logging, and
`UMask=0077`. They are removed by `--no-auto-update`, `--uninstall`, and
`uninstall.sh`.

## Backups, retention, and rollback

Update backups live under
`~/.local/state/screen-time/backups/updates/<generation>/` and contain the
previous binary, launcher, theme, unit, version, and a `db/` snapshot with
`manifest.json`. The update manifest records versions, release tag, asset and
installer hashes, signature key id, service state, and the data fingerprint.
Backup status is `pending` until post-update checks pass, then `verified`.

- At least two verified backups are always retained.
- Pruning runs only after a successful `doctor` and data check.
- Failed attempts are kept and marked failed for diagnosis.
- `--rollback` restores only the latest **verified successful** backup, runs the
  data check and `doctor --json --no-runtime`, and keeps the backup.
- Backups created by 0.3.0 have no manifest and are not selected by
  `--rollback`; restore those manually while the service is stopped.
- If the backup schema is newer than the backup binary supports, rollback
  refuses unless `--allow-schema-downgrade` is passed.
- A rollback that fails its checks stops the service, keeps the backup, and
  writes a recovery manifest with instructions.

The live database is snapshotted with SQLite `VACUUM INTO` after the service is
stopped, so WAL/SHM state cannot produce a torn copy. The updater never runs
`VACUUM` against a corrupt live database and never deletes one before a restore
is verified.

## History retention

History is kept forever by default. `[retention]` in `config.toml` is opt-in;
missing fields and `0` mean keep forever. Migration backups are never pruned
automatically. See the [user guide](USER_GUIDE.md#history-retention) for the
configuration and a manual cleanup command.

## Recovery runbook

Failed update:

1. `install.sh --no-auto-update` if a timer is enabled.
2. Inspect `updates/last-update.json`, the backup `manifest.json`, and
   `journalctl --user -u screen-time`.
3. Run `install.sh --rollback`; it only selects a verified backup.
4. Confirm `screen-time doctor --json --no-runtime` and the data fingerprint.
5. Confirm the service state matches the backup manifest.
6. Keep the failed backup and logs and report the release tag, key id, and
   backup path.

Interrupted update (power loss or kill):

- `updates/update-in-progress.json` records the phase, backup path, and previous
  service state. The next install/update restores the backup and service state
  before continuing.
- A backup without a verified status is never used by `--rollback`.

Corrupt live database:

1. Stop the service and timer.
2. Find the latest **verified** snapshot under `backups/updates/` (or a
   `backups/migrations/` copy); never pick a failed artifact.
3. Restore with `--rollback`, or copy the snapshot only while the service is
   stopped.
4. Run `doctor --json --no-runtime` and compare counts, session seconds, app
   keys, and `quick_check`.
5. Do not `VACUUM` the corrupt database or delete it until the restored copy is
   verified.

Lost config: restore `config.toml` from your external backup. The installer
never owns or modifies it. Lost `last-check.json` / `last-update.json` are
diagnostic only; deleting them does not affect install or rollback safety.

## External backups

Update backups protect against a bad update, not disk failure or `rm`. Use
restic or borg for `~/.local/state/screen-time`, including `config.toml` and
`backups/`, and test a restore periodically. A periodic restore drill should
start from the oldest retained generation, not only the newest:

```bash
scripts/restore-drill.sh                       # newest verified backup
scripts/restore-drill.sh --oldest              # oldest verified generation
scripts/restore-drill.sh --binary ~/.local/bin/screen-time
```

The drill copies the snapshot into a temporary state directory and checks the
manifest hash, `quick_check`, doctor JSON, fingerprint, and `0700`/`0600`
permissions without touching the live installation.
