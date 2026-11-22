# Docs

## 1. App and installation

- [User guide](USER_GUIDE.md) — overview, distro installation, configuration, and usage.
- [Data safety and updates](AUTO_UPDATE.md) — channel model, signed releases, timers, backups, and recovery.
- [Evaluation](EVALUATION.md) — visual comparison, benchmarks, and Linux app comparisons.

## 2. Reproduce development checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
shellcheck install.sh uninstall.sh share/rofi-screen-time scripts/package-release.sh scripts/sign-release.sh scripts/test-installer.sh scripts/restore-drill.sh scripts/benchmark-linux-sandbox.sh
nix build .#
python3 scripts/render-evaluation-charts.py --check
scripts/test-installer.sh
```

The test suite includes unit tests for schema migration, session merge, bursts,
open-interval math, disable-by-default and opt-in retention, batching, atomic
snapshots, database snapshots, data fingerprints, stale handling, Hyprland
parsing, config/theme resolution, font priority, and compact layout; golden
main/detail screens at 1280x800 and 1920x1080 with section markers; and a
daemon smoke test with a fake Hyprland socket and SIGTERM shutdown. Theme
integration tests cover Matugen-over-static, corrupt-palette fallback, config
overrides, generated Rofi themes, and live palette changes on the next render.
The installer integration script covers install/upgrade/no-op, inactive
service state, rollback, uninstall/purge, config and database invariants,
permissions, signature refusal, and concurrent-lock refusal in a temp HOME.
Charts under `docs/charts/` are generated from `docs/benchmarks/*.json` by
`scripts/render-evaluation-charts.py`.
