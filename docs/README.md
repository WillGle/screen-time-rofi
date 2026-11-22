# Docs

## 1. App and installation

- [User guide](USER_GUIDE.md) — overview, distro installation, configuration, and usage.
- [Installer design](GITHUB_INSTALLER.md) — release packaging and install lifecycle.

## 2. Experiments and comparisons

- [Evaluation](EVALUATION.md) — runtime measurements, benchmarks, and Linux app comparisons.
- [Resource benchmark](ROFI_RESOURCE_BENCHMARK.md) — raw comparison and reproduction.
- [Final report](FINAL_REPORT.md) — milestone evidence and measurements.
- [Storage research](SCREEN_TIME_STORAGE_ARCHITECTURE.md) — storage and write path.
- [UI research](SCREEN_TIME_ROFI_UI.md) — market scan and design experiments.
- [UI specification](SCREEN_TIME_UI_SPEC.md) — original v1 design.
- [Decisions](DECISIONS.md) — implementation choices and spec resolutions.
- [V1 goal](SCREEN_TIME_V1_GOAL.md) — execution plan and milestones.

### Reproduce development checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
shellcheck install.sh uninstall.sh share/rofi-screen-time scripts/package-release.sh
nix build .#
```

The test suite includes unit tests for schema migration, session merge, bursts,
open-interval math, retention, batching, atomic snapshots, stale handling,
Hyprland parsing, config/theme resolution, font priority, and compact layout;
golden main/detail screens at 1280x800 and 1920x1080 with section markers; and
a daemon smoke test with a fake Hyprland socket and SIGTERM shutdown. Theme
integration tests cover Matugen-over-static, corrupt-palette fallback, config
overrides, generated Rofi themes, and live palette changes on the next render.
