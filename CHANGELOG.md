# Changelog

## 0.4.0

Data safety and updates:

- History retention is opt-in and defaults to keep all. `[retention]` supports
  independent events/sessions/aggregates/power cutoffs; invalid config falls
  back to keep-all.
- `doctor` reports schema, per-table counts, quick_check, retention, database,
  WAL and backup sizes, service state, snapshot freshness, and a data
  fingerprint. Add `--json` and `--no-runtime` for updater checks.
- Add `screen-time snapshot --to DIR --json` (consistent `VACUUM INTO`, hash,
  manifest, fingerprint) and `screen-time check-data` for fingerprint guards.
- Release assets are signed with minisign. The installer verifies
  `SHA256SUMS`, `install.sh`, and a pinned key before replacing files, and
  refuses unsigned or tampered releases.
- Add `install.sh --check --json` and `--update`, with documented exit codes,
  no-op-on-current behavior, no automatic downgrade, and update manifests.
- Add opt-in `--auto-update=check` and `--auto-update=apply` user timers with
  locking, backoff, locked/battery gating, signed stable releases only, and
  major-version approval.
- Update backups use consistent database snapshots with manifests, keep at
  least two verified generations, prune only after data checks pass, and
  restore the previous files, database, and service state on failure.
- `--rollback` restores only a verified backup, runs data/health checks, and
  keeps the backup for audit.
- `--uninstall` keeps database, config, and backups by default. `--purge`
  requires confirmation or `--yes`.
- Protect user-modified themes and units; never pass `--force` automatically.
- Add installer integration and fault-injection tests, a restore drill, release
  signing, and CI validation.

Migration from 0.3.0:

- The fixed 30/90/365-day pruning is now an opt-in example. Data already pruned
  by 0.3.0 cannot be recovered.
- The default update backup count is 2. Existing backups remain.
- There was no auto-update or release signature verification in 0.3.0.
