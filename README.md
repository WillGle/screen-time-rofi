# screen-time-rofi

Local, per-user screen-time tracker with a static, high-visibility Rofi day
timeline.

**Status:** M0 scaffold. No tracking or rendering logic yet.

## What it will do

- Track active app usage on Hyprland (app-level only).
- Show device on/off, total active time, top app durations, and a 24-hour
  timeline.
- Open one app detail view with sessions/bursts.
- Run as a per-user systemd service.
- Store current-day state in tmpfs, persist history in SQLite WAL, and batch
  writes at 60 seconds or 100 events.
- Install once via `install.sh` from a GitHub release.

## Non-goals

No window titles, categories, websites, suspend/lock/display tracking, cloud,
multi-user, socket/live query, or animation.

## Build

```bash
cargo build --release
cargo run -- --version
```

Rust 1.85+ (edition 2024). The first implementation milestone will add SQLite,
JSON, and Hyprland event dependencies.

## Goal mode

See [`GOAL.md`](GOAL.md) for the one-shot autonomous build plan (M0 → M7).

## Design docs

- [`docs/SCREEN_TIME_UI_SPEC.md`](docs/SCREEN_TIME_UI_SPEC.md)
- [`docs/SCREEN_TIME_STORAGE_ARCHITECTURE.md`](docs/SCREEN_TIME_STORAGE_ARCHITECTURE.md)
- [`docs/GITHUB_INSTALLER.md`](docs/GITHUB_INSTALLER.md)
- [`docs/SCREEN_TIME_V1_GOAL.md`](docs/SCREEN_TIME_V1_GOAL.md)
- [`docs/SCREEN_TIME_ROFI_UI.md`](docs/SCREEN_TIME_ROFI_UI.md)
- [`docs/ROFI_RESOURCE_BENCHMARK.md`](docs/ROFI_RESOURCE_BENCHMARK.md)

## Install (future)

```bash
curl -fsSLO https://github.com/WillGle/screen-time-rofi/releases/latest/download/install.sh
less install.sh
bash install.sh
```

## License

MIT. See `LICENSE`.
