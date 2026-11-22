# Docs

## 1. App and installation

- [User guide](USER_GUIDE.md) — overview, distro installation, configuration, and usage.
- [Evaluation](EVALUATION.md) — visual comparison, benchmarks, and Linux app comparisons.

## 2. Reproduce development checks

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
shellcheck install.sh uninstall.sh share/rofi-screen-time scripts/package-release.sh scripts/benchmark-linux-sandbox.sh
nix build .#
python3 scripts/render-evaluation-charts.py --check
```

The test suite includes unit tests for schema migration, session merge, bursts,
open-interval math, retention, batching, atomic snapshots, stale handling,
Hyprland parsing, config/theme resolution, font priority, and compact layout;
golden main/detail screens at 1280x800 and 1920x1080 with section markers; and
a daemon smoke test with a fake Hyprland socket and SIGTERM shutdown. Theme
integration tests cover Matugen-over-static, corrupt-palette fallback, config
overrides, generated Rofi themes, and live palette changes on the next render.
Charts under `docs/charts/` are generated from `docs/benchmarks/*.json` by
`scripts/render-evaluation-charts.py`.

Internal decisions, design notes, and development reports stay in the local
`docs/internal/` directory and are gitignored.
