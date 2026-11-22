# Screen Time evaluation

This page brings together experiments, resource measurements, and a comparison
of Linux tracking tools. For installation and configuration, see the
[user guide](USER_GUIDE.md).

## Expanded sandbox benchmark

### Setup

The revised suite uses ten distinct synthetic app identities on Xvfb at
1920×1080. Linux trackers with X11 capture read the window properties. Native
Hyprland trackers receive equivalent active-window changes through the fake
Hyprland command and event sockets. Each synthetic app has a distinct PID
and executable path for trackers that resolve the focused process.

The host was NixOS on an AMD Ryzen 7 8845H. Bubblewrap isolated the PID,
network, X11 socket, home, and runtime paths. Filesystem writes went to
temporary ext4 state directories. All inputs were synthetic, and no account,
personal browser history, or real desktop socket was used.

The main comparison uses a 10-second unchanged-focus period and 10 changes
on a six-second cadence, with 66 seconds in the switching phase. The short
workload uses one pass across ten apps at 1.5 seconds each. Each reported
result is a median across repeated trials. The input schedule is the same
within each cohort.

PSS and process/thread counts are end-of-phase snapshots for the tracker
process group. CPU is a percentage of one logical core and includes CPU time
from children reaped by that group during the phase. Reported kernel writes
exclude startup and shutdown. `Apps seen` counts distinct fixture app names
observed in each tracker's native records. It cannot establish duration
accuracy or count every transition for a sampling tracker.

ActivityWatch ran its supervisor, server, window watcher and AFK watcher,
without a browser. arbtt ran at one sample per second versus its 60-second
default so the short workload could exercise sampling. Screen Time's logind
watcher was disabled in the fixture. The experiment does not compare actual
idle, lock, sleep, audio activity, browser URLs, or mobile tracking.
atracker bound its API to loopback on an ephemeral port. The other configured
capture intervals were their upstream defaults.

### Ten-tracker comparison at six seconds per focus

This run adds HPR and Screentime to the same schedule as the eight-tracker
run below. HPR uses its true-headless mode with network access and app
termination disabled. Screentime runs its Qt desktop process with the window
hidden. This preserves each product's documented background mode.

| Tracker | PSS (MiB) | Processes / threads | CPU, steady | CPU, switching | Writes (KiB) | Apps seen (range) | Footprint score |
|---|---:|---:|---:|---:|---:|---:|---:|
| Screen Time Rust | 4.48 | 1 / 3 | 0.00% | 0.00% | 80 | 10/10 | 90.2 |
| ActivityWatch | 81.79 | 4 / 25 | 0.40% | 0.85% | 2,080 | 10/10 | 9.8 |
| arbtt | 9.28 | 1 / 2 | 0.40% | 0.24% | 12 | 10/10 | 56.5 |
| ScreenTimeLogger | 8.54 | 1 / 1 | 0.40% | 0.35% | 12 | 10/10 | 59.2 |
| Niri Screen Time | 15.60 | 1 / 9 | 10.70% | 10.06% | 224 | 10/10 | 43.7 |
| Mono | 7.04 | 1 / 6 | 1.90% | 1.68% | 384 | 10/10 | 66.5 |
| Toki | 3.89 | 1 / 8 | 0.00% | 0.02% | 396 | 10/10 | 100.0 |
| atracker | 9.33 | 1 / 22 | 0.40% | 0.36% | 480 | 10/10 | 56.3 |
| HPR | 14.50 | 1 / 6 | 54.10% | 50.92% | 976 | 10/10 | 45.1 |
| Screentime (Qt) | 45.86 | 1 / 3 | 1.20% | 1.08% | 40 | 10/10 | 31.4 |

All ten trackers recorded all ten app identities in every trial. Toki measured 3.89 MiB PSS and Screen Time
4.48 MiB in this cohort. Screen Time wrote 80 KiB
during switching, compared with Toki's 396 KiB. HPR used
50.9% of one logical core in this sandbox with its stock 50-ms
poll setting. The result includes subprocess CPU and is specific to this
Hyprland fixture and HPR mode.

### Eight-tracker cross-check at six seconds per focus

| Tracker | PSS (MiB) | Processes / threads | CPU, steady | CPU, switching | Writes (KiB) | Apps seen (range) | Footprint score |
|---|---:|---:|---:|---:|---:|---:|---:|
| Screen Time Rust | 2.50 | 1 / 3 | 0.00% | 0.00% | 80 | 10/10 | 100.0 |
| ActivityWatch | 81.86 | 4 / 25 | 0.30% | 0.77% | 2,344 | 10/10 | 8.5 |
| arbtt | 9.21 | 1 / 2 | 0.20% | 0.21% | 68 | 10/10 | 45.4 |
| ScreenTimeLogger | 8.56 | 1 / 1 | 0.20% | 0.30% | 32 | 10/10 | 46.9 |
| Niri Screen Time | 14.75 | 1 / 9 | 6.70% | 8.71% | 320 | 10/10 | 37.7 |
| Mono | 7.00 | 1 / 6 | 1.20% | 1.48% | 464 | 10/10 | 51.8 |
| Toki | 3.88 | 1 / 8 | 0.00% | 0.02% | 472 | 10/10 | 73.3 |
| atracker | 9.32 | 1 / 22 | 0.20% | 0.33% | 564 | 10/10 | 45.1 |

All eight trackers recorded each fixture app in every trial. Screen Time
used 2.50 MiB PSS, followed by Toki at 3.88 MiB. ScreenTimeLogger and arbtt
wrote fewer bytes in this workload. The `0.00%` values indicate that the
sample did not register a CPU tick at the reported precision.

### Background results at 1.5 seconds per focus

| Tracker | PSS (MiB) | Processes / threads | CPU, steady | CPU, switching | Writes (KiB) | Apps seen (range) | Footprint score |
|---|---:|---:|---:|---:|---:|---:|---:|
| Screen Time Rust | 4.49 | 1 / 3 | 0.00% | 0.00% | 0 | 10/10 | 90.2 |
| ActivityWatch | 82.61 | 4 / 25 | 0.40% | 0.93% | 544 | 9/10 | 9.8 |
| arbtt | 8.30 | 1 / 2 | 0.30% | 0.27% | 4 | 10/10 | 60.2 |
| ScreenTimeLogger | 8.66 | 1 / 1 | 0.30% | 0.40% | 4 | 3/10 | 58.8 |
| Niri Screen Time | 13.02 | 1 / 7 | 9.60% | 11.47% | 48 | 8/10 | 47.5 |
| Mono | 6.98 | 1 / 6 | 1.60% | 1.73% | 376 | 6/10 | 66.9 |
| Toki | 3.90 | 1 / 8 | 0.00% | 0.07% | 216 | 1/10 | 100.0 |
| atracker | 9.05 | 1 / 21 | 0.40% | 0.40% | 84 | 2–3/10 | 57.3 |

Screen Time and arbtt at 1 Hz each recorded all ten identities. ActivityWatch
returned nine, while five-second polling yielded only two or three for
atracker and three for ScreenTimeLogger. Toki's 1/10 result reflects its
default five-second noise floor, which absorbs shorter focuses into the
previous session. The number describes what its SQLite history retained.
[Toki session policy at the tested revision](https://github.com/Ciatel-Arlanta/Toki/blob/5ae5e3b075309da9c3fa144329d37af880c31f83/crates/toki-core/src/tracker.rs)

Screen Time's 0 KiB in this 15-second write window excludes its shutdown
flush. All scripted focus changes were verified in SQLite after termination.

### Screen Time UI timing

The packaged app read SQLite history generated by the six-second workload.
The renderer took **6.02 ms** (11 warm runs, range 3.55–9.57 ms). The popup
became visible in **87.6 ms** (10 warm runs, range 80.7–210.6 ms), with
**5.99 MiB** median PSS for the launcher and Rofi. This suite did not measure
the competing browser, Qt, Tauri, or terminal interfaces under an equivalent
foreground workload.

Raw data: [ten-tracker run](benchmarks/linux-ten-trackers.json),
[eight-tracker run](benchmarks/linux-expanded-steady.json), and
[1.5-second workload](benchmarks/linux-expanded-short.json).


### Executable sizes

These are the measured tracker files from the expanded run. A file size
excludes shared libraries, interpreter and runtime dependencies, and
separately packaged dashboard assets.

| Tracker artifact | File bytes |
|---|---:|
| Screen Time binary | 7,534,736 |
| Toki daemon | 3,583,968 |
| Niri Screen Time binary | 10,471,280 |
| Mono tracker | 12,632,888 |
| atracker backend | 14,105,168 |

The original run below also records complete Nix closure sizes for its
three package sets. Binary size and Nix closure size answer different
installation questions.

### Tested builds and reproduction

| Tracker | Version or source used |
|---|---|
| Screen Time | 0.2.1 with the redesigned app-detail UI, exact binary hash in the raw data |
| ActivityWatch | 0.13.2 packaged by Nixpkgs |
| arbtt | 0.12.0.3 packaged as `haskellPackages.arbtt` |
| ScreenTimeLogger | 1.0.0, [source 15f04aa](https://github.com/abhinavbxr/ScreenTimeLogger/tree/15f04aab4b91e4f7b77482915ea57b3dcb310534) |
| Niri Screen Time | [source 147fdc3](https://github.com/probeldev/niri-screen-time/tree/147fdc3837b79dfadb54d31a236ede6a6e4fba1a), built with `CGO_ENABLED=0 go build -trimpath` |
| Mono | Cargo 0.3.0, [source 8c0d088](https://github.com/xonoxc/mono/tree/8c0d08866cb1080fa64a2a374d9deb284606f040), `cargo build --release --locked --bin mono-tracker` |
| Toki | 0.5.1, [source 5ae5e3b](https://github.com/Ciatel-Arlanta/Toki/tree/5ae5e3b075309da9c3fa144329d37af880c31f83), `cargo build --release --locked -p toki-daemon` |
| HPR | [Linux release 0.9.13](https://github.com/plexescor/HPR/releases/tag/v0.9.13), executable and bundled Slint library |
| Screentime Qt | [source ebc0fe2](https://github.com/truelockmc/Screentime/tree/ebc0fe2ca138ce146a7aa119abadd69a7cffcf58), Python 3.13 and PyQt5 5.15.10 |
| atracker | [source 5f22da9](https://github.com/nichsedge/atracker/tree/5f22da966868a2a208cf43cd39271856301ac64a), `cargo build --release --locked` inside `atracker-rs` |

Use the [sandbox launcher](../scripts/benchmark-linux-sandbox.sh) with an
absolute input directory holding the competitor binaries and source scripts.
The launcher expects a NixOS host with Bubblewrap, Nix, Rofi 2.x, Xvfb and
`hyprctl`. Put ActivityWatch, arbtt, zsh, xdotool and Python with Xlib on
`PATH`. Build Screen Time with `nix build .#`. The input directory is
mounted read-only except for its `results/` directory.

```bash
inputs=/tmp/screen-time-competitors
bash scripts/benchmark-linux-sandbox.sh "$inputs" \
  --apps 10 --repeats 3 --switches 10 --interval 6 \
  --arbtt "$(command -v arbtt-capture)" \
  --logger "$inputs/ScreenTimeLogger/screentime_logger.py" \
  --niri "$inputs/niri-bin" \
  --mono "$inputs/mono/target/release/mono-tracker" \
  --toki "$inputs/Toki/target/release/toki-daemon" \
  --atracker "$inputs/atracker/atracker-rs/target/release/atracker-rs" \
  --output "$inputs/results/linux-expanded-steady.json"
```

For the short workload use `--switches 9 --interval 1.5 --skip-ui` and a separate output.
For the ten-tracker run, pass `--skip-ui --hpr "$inputs/hpr-wrapper" --screentime
"$inputs/screentime-wrapper"` and a separate output. Both wrappers must
launch the upstream app without installing or enabling it on the host.

- HPR's wrapper copies the bundled `config.csv` into the sandbox's
  `$HOME/.config/HPR/`, sets `true-headless-mode,true`,
  `allow-network-activity,false` and `kill-apps,false`, then launches the
  bundled executable. On the tested NixOS host it used a glibc 2.44 loader
  and explicitly supplied the bundled Slint, D-Bus, libcurl, GCC runtime and
  font libraries. `libslint_cpp.so` in this release requires a glibc symbol
  version of at least 2.43.
- Screentime's wrapper copies its Python files and `map.json` into writable
  sandbox state before launching `main.py`. The app's database is located
  beside its source files. Use Python with PyQt5, psutil, qdarkstyle and
  Pillow, and set `QT_PLUGIN_PATH` to the Qt5 plugin directory containing
  the XCB platform plugin. The runner supplies `--hidden`.

The [runner](../scripts/benchmark-linux.py) records individual samples,
package paths, artifact hashes, and Screen Time popup timings.

## Linux feature comparison

The comparison covers ten Linux competitors plus Screen Time. Selection
favors automatic desktop app tracking, local alternatives, and documented
Linux clients. The list is a comparison set, with no popularity ranking.
Platform support and features come from upstream documentation and source.
The runtime section states which builds were exercised.

| App | Linux capture and setup | History and dependencies | UI and reporting | Strengths and trade-offs |
|---|---|---|---|---|
| **Screen Time** | Hyprland IPC events, systemd user service, Rofi 2.x. Per-user release installer or Nix/Home Manager. | Local SQLite WAL and tmpfs snapshot. App class only. Idle detection uses logind. | Rofi day timeline with 30-minute slots, app details, 7-day history, Matugen and preset shortcuts. | Fast keyboard access, no window titles, low daemon footprint. Hyprland-specific, fixed categories, no built-in web or project reports or browser URL tracking. The main timeline fills a half-hour slot after at least one minute of app use in that slot. |
| [ActivityWatch](https://docs.activitywatch.net/en/latest/watchers.html) | Official window watcher on X11. Wayland capture uses community watchers. Native Linux package or release archive. | Local server and watchers. Browser/editor extensions are optional. Window watcher records app and title; AFK has its own watcher. | Browser timeline, category rules, cross-platform reporting and extensible watchers. | Mature local reporting with rules and extensions. A browser and several background processes are involved. |
| [arbtt](https://arbtt.nomeata.de/doc/users_guide/effective-use.html) | X11 sampling daemon and CLI. Package availability varies; Nix provides `haskellPackages.arbtt`. | Local binary capture log. Samples capture window metadata and inactivity. Default interval is 60 seconds. | CLI statistics and rule-based categorization. | Small writes in the sandbox. Setup and reporting require text rules; polling can miss brief activity. |
| [ManicTime for Linux](https://docs.manictime.com/linux/installation) | Linux client packages include DEB, RPM and tarball. [Wayland support depends on compositor](https://docs.manictime.com/linux/wayland), including Hyprland app tracking. | Linux tracker sends data to ManicTime Cloud or a connected Server. Screen lock/away detection; URL and screenshot support depends on extensions and desktop support. | [Cloud or Server timeline and reports](https://docs.manictime.com/server/reports/overview), tagging, shared reporting. | Useful for detailed work records and team reporting. The Linux tray client needs the connected service to view timelines. |
| [HPR](https://github.com/plexescor/HPR) | Compiled Linux release. Hyprland, niri, KDE, GNOME and Cinnamon backends with desktop-specific setup. | Local SQLite. App, title and optional extension data. Configurable network access and tray integration. | Slint desktop UI, goals, limits, projects, insights and switch history. | Broad Wayland coverage and richer analysis. Linux release dependencies and integrations vary by desktop; extensions can add collection and runtime costs. |
| [Mono](https://github.com/xonoxc/mono) | Linux Rust build with user daemon. Hyprland, Sway, GNOME, KDE and X11 backends with differing capabilities. | Local SQLite. App and window titles, configurable category and browser tracking features. | Ratatui terminal dashboard and CLI for summaries. | Keyboard-oriented without a browser; broader compositor support. Terminal interaction and title collection differ from the app-only popup. |
| [Niri Screen Time](https://github.com/probeldev/niri-screen-time) | Go binary, AUR or source. niri and Hyprland. The measured upstream revision invokes `/bin/zsh` for compositor commands. | Local SQLite. Records app identity and title. | CLI for aggregate and detailed date-range reports, with text and JSON output. | Scriptable and supports niri. The reader uses a CLI rather than a dedicated popup. |
| [atracker](https://github.com/nichsedge/atracker) | Rust watcher and React dashboard. Hyprland and niri IPC plus other desktop-specific integrations. | Local SQLite and HTTP API; app/window details. Optional connectivity and multi-device features have their own setup. | Browser dashboard, configurable categories, filters, timeline and productivity views. | Flexible dashboard and native Wayland backends. Foreground use needs a browser, and API bind address matters when deploying. |
| [Toki](https://github.com/Ciatel-Arlanta/Toki) | Linux Hyprland collector with Debian, RPM and tarball releases. Rust daemon and on-demand Tauri/WebKitGTK UI. | Private SQLite; app sessions and local browser-domain resolution. Native idle and audio features depend on Wayland/logind/audio services. | Desktop timeline and category/usage breakdowns. | Direct Hyprland peer with richer presence and browser context. Those features and the WebKit UI need separate runtime tests. |
| [ScreenTimeLogger](https://github.com/abhinavbxr/ScreenTimeLogger) | Hyprland and Python standard library. User systemd service runs a `hyprctl` poll every 5 seconds by default. | Local append-only monthly JSONL with app, title, time range and duration. | CLI text or JSON reports, hourly heatmaps and session export. | Readable files and few dependencies. Five-second polling can miss brief switches; reporting stays in the CLI. |
| [Screentime by truelockmc](https://github.com/truelockmc/Screentime) | Python/Qt desktop app, X11, optional KDE Wayland KWin bridge. Other Wayland sessions can appear as one generic entry. | Local SQLite daily totals; app identification depends on desktop integration. | Desktop and tray window, app totals, charts and date-range statistics. | Convenient GUI for Linux users with a supported backend. Native per-app capture on Hyprland is absent from its documented setup. |

For a single-user Hyprland workflow, Screen Time prioritizes opening a daily
overview from a key binding and collecting only app identifiers. ActivityWatch,
HPR and atracker cover broader reporting. Toki is the closest event-driven
Hyprland alternative in the measured set. Mono suits a terminal workflow;
arbtt, Niri Screen Time and ScreenTimeLogger suit scripted reports. ManicTime
adds a connected team/reporting workflow.

### How to read the scores

The resource score describes one workload's measured background footprint.
It weights PSS 75% and process count 25%; each component is
`100 × lowest median / observed median`. The lowest value is recomputed
inside each benchmark cohort and focus cadence. An app can score highly
for footprint while retaining fewer fixture apps. CPU, write bytes and capture coverage have
their own columns. Combining them into one score would hide differences
in polling cadence, idle detection, persistence and record detail. Features
are compared directly rather than assigning points for product preferences.

## Original three-tracker Linux benchmark

Screen Time 0.2.1, ActivityWatch 0.13.2, and arbtt 0.12.0.3 ran together in
three independent trials on an AMD Ryzen 7 8845H host with NixOS. An isolated
Xvfb display at 1920×1080 supplied three synthetic applications. Each trial
held one app in focus for 10 seconds, then switched 22 times at 3-second
intervals and held the last app for another 3 seconds. ActivityWatch and
arbtt consumed X11 metadata. The same driver sent equivalent events to
Screen Time over a simulated Hyprland socket. State was on ext4, runtime
snapshots and benchmark logs were on tmpfs.

ActivityWatch ran its packaged test stack (supervisor, server, window watcher,
and AFK watcher) with its browser dashboard closed. arbtt ran at one sample
per second so it could observe the short switches. Its default interval is
60 seconds. Screen Time's logind idle watcher was disabled because the
fixture had no logind desktop session. The steady-focus interval measures
the cost of an unchanged foreground app. It does not exercise transition
into AFK or idle. The trials used private home/config/state paths and no
personal tracking history.

### Background tracking results

Values are medians across three trials. PSS, process and thread counts are
snapshots at the end of the switching phase. CPU is relative to one logical
core. Writes are bytes attributed by the kernel to the tracker process group
during switching, excluding startup and shutdown. They are not SSD wear estimates.

| Tracker | PSS (KiB) | Processes | Threads | CPU, steady focus | CPU, switching | Writes, switching (KiB) | Footprint score |
|---|---:|---:|---:|---:|---:|---:|---:|
| Screen Time (Rust) | 2,587 | 1 | 3 | 0.00% | 0.01% | 80 | 100.0 |
| ActivityWatch | 84,847 | 4 | 25 | 0.50% | 1.23% | 2,208 | 8.5 |
| arbtt | 9,459 | 1 | 2 | 0.30% | 0.30% | 12 | 45.5 |

Footprint score = 75% PSS and 25% process count. Each component is
`100 × (lowest median / measured median)`. CPU and writes are reported
separately because they depend on capture cadence, how much each product
records, and when it flushes. A score of 100 is the lightest result in
this sandbox.

All three captured the fixture applications. Screen Time persisted the
initial focus and all 22 switches in each trial, ActivityWatch returned
22 window events per trial at the end of the sampling window, and arbtt
showed each of the three apps as the active window in its samples. Event
and session counts use each tracker's own recording rules.

### Screen Time popup and renderer

The foreground experiment used the SQLite history from the final trial.
One warm-up was discarded for each metric. Startup ends when X11 reports the
Rofi window visible, and PSS covers the launcher and Rofi after 200 ms.

| Metric | Median | Min–max | Runs |
|---|---:|---:|---:|
| Rust renderer, warm, same fixture | 5.18 ms | 4.45–8.69 ms | 11 |
| Launcher to visible Rofi window, warm | 163.3 ms | 148.3–180.5 ms | 10 |
| Launcher + Rofi PSS | 13,348 KiB | 13,282–13,393 KiB | 10 |

These are results for the package tested in that run. The earlier v1 measurements
below used different display geometry, launch logic, and timing conditions.

### Installation footprint

These are uncompressed Nix store closure sizes, including transitive
dependencies. Shared paths are counted once in the Screen Time plus Rofi
row. The numbers do not represent download size or the incremental space on
a host with dependencies already installed.

| Package set | Closure bytes | MiB |
|---|---:|---:|
| Screen Time package | 57,613,464 | 54.9 |
| Screen Time + Rofi 2.0.0 | 488,726,432 | 466.1 |
| arbtt | 93,721,400 | 89.4 |
| ActivityWatch, without a separate browser | 2,572,375,896 | 2,453.2 |

### Reproduce

Build the package, provide Xvfb and Rofi 2.x on `PATH`, then run the script
from the repo root. The benchmark uses Xvfb display `:198` and the loopback
ActivityWatch test port 5666. It aborts if either is occupied.

```bash
nix build .#
nix shell nixpkgs#activitywatch nixpkgs#haskellPackages.arbtt nixpkgs#xdotool \
  -c nix shell --impure --expr 'with import <nixpkgs> {}; python3.withPackages (ps: [ ps.xlib ])' \
  -c bash -ec 'python3 scripts/benchmark-linux.py --arbtt "$(command -v arbtt-capture)" --output /tmp/linux-trackers.json'
```

The script and machine-readable samples are available in
[benchmark-linux.py](../scripts/benchmark-linux.py) and
[linux-trackers.json](benchmarks/linux-trackers.json).

ManicTime for Linux is covered in the feature comparison. Its documented Linux
workflow uses a Cloud or Server connection to view timelines, and a matching
local server setup was outside this run. The benchmark has no ManicTime
resource result or cross-app foreground score.

## Historical Screen Time runtime measurements

These are historical v1 results from NixOS. The popup ran on Xvfb at 1280×800
with a warm cache. The daemon test used ext4, tmpfs for the live snapshot, and
12 synthetic focus switches over 66 seconds with logind idle detection disabled.

| Metric | Reported result |
|---|---:|
| Rust renderer, median of 5 warm runs | 4.72 ms |
| Launcher to visible Rofi window, median of 3 warm runs | 75 ms |
| Rofi PSS, median of 3 runs | 7,796 kB |
| Daemon PSS, start / end | 6,564 / 6,584 kB |
| Daemon disk writes over the 66-second focus-switch run | 135,168 bytes |
| Daemon disk writes in a separate run with no focus events | 4,096 bytes |

PSS is proportional set size. The separate zero-focus run includes startup and
shutdown. These measurements predate the current dashboard and theme shortcuts.
See the [individual samples and test conditions](FINAL_REPORT.md#5-raw-measurements).

## Historical prototype benchmarks

The comparison ran on Xvfb at 1600×1000 with software rendering and warm cache.
Each workload had three runs. Lower resource use scores higher, with 100
assigned to the lightest baseline within each workload group.

| Workload group | Pair | Measured results | Resource score |
|---|---|---|---:|
| Foreground UI prototypes | Rofi 2.0.0 / GTK4 4.22.5 | Visible window 75.5 / 196.1 ms, PSS 7.8 / 149.2 MB | 100 / 27.9 |
| Background tracker | Bash polling mock / ActivityWatch 0.13.2 | PSS 3.0 / 87.3 MB, 2 / 4 processes | 100 / 10.4 |

The UI score weights startup, PSS, idle CPU, threads, and Nix closure size.
The background score weights PSS, idle CPU, and process count. GTK4 is a
controlled prototype. The Bash mock omits persistence and session logic.
ActivityWatch was measured with its tracking stack running and its browser
dashboard closed. The current Rust daemon and full desktop apps do not have a
matched benchmark in this data. See
[raw results, weights, caveats, and reproduction commands](ROFI_RESOURCE_BENCHMARK.md).

The storage experiment compared full JSON rewrite, append-only JSONL, and
batched SQLite WAL over 720 samples. Physical writes measured over that
workload were 2.95 MB, 287 KB, and 647 KB respectively. Its daily figures are
extrapolations. See the [storage benchmark and method](SCREEN_TIME_STORAGE_ARCHITECTURE.md#2-measured-write-amplification).

## Historical device validation (M9)

Validated on `think14gryzen` (Hyprland 0.55.4, Rofi 2.0.0, eDP-1 3072x1920 at
scale 1.5) against the real `~/.config/theme/generated/palette.json` and
`~/.config/theme/static.env`:

- **Real palette:** base `#101410`, mantle `#1c211b`, text `#e0e4db`, muted
  `#c2c9bd`, accent `#9fd49c`, power `#9eb09a`; static font
  `JetBrainsMono Nerd Font 10`.
- **Real wallpaper change:** a generated wallpaper was applied with
  `~/.config/theme/theme-apply --wallpaper ...`. The palette changed to base
  `#1a1110`, mantle `#271d1c`, text `#f1dfdb`, muted `#d8c2be`, accent
  `#ffb4a6`, power `#caa29b`; the next popup used the new accent/power/muted
  colors with no daemon restart. The original wallpaper was restored afterwards.
  Screenshots:
  [`m9-main-original-palette.png`](screenshots/m9-main-original-palette.png)
  and
  [`m9-main-changed-palette.png`](screenshots/m9-main-changed-palette.png).
- **Font sizes 12/14/16:** ruler widths were 116/68/68 columns; 14 pt and 16 pt
  selected the compact/top-four layout, and `layout.mode = "full"` forced the
  116-column layout back.
- **No Hyprland config parsing:** only the generated palette, `static.env`, the
  config file, and the optional Rofi theme are read.
- **Packaging:** `nix build .#` produced `screen-time 0.2.1`; the installer
  updated v0.1.0 → v0.2.1, rolled back to v0.1.0, kept exactly one backup after
  a successful update, and uninstalled with no leftover files or staging
  directories.

The full before/after values, measurements, and golden diffs are in
[`docs/FINAL_REPORT.md`](FINAL_REPORT.md#m8m9-validation).

## Data path and write policy

- Active app changes come from the Hyprland event socket. The window title is
  parsed only to find the comma separator and is immediately discarded.
- Idle begin/end comes from the logind `IdleHint` D-Bus property. Suspend,
  lock, and display-off are not tracked.
- The current day is held in memory and exposed atomically in tmpfs
  `live.json`. The renderer reads that file once; it never polls.
- SQLite uses WAL, `synchronous=NORMAL`, `busy_timeout=5000`,
  `foreign_keys=ON`, and one writer.
- Events and sessions commit in one transaction at **60 seconds or 100 events**,
  whichever comes first, and on clean shutdown/day rollover.
- Retention: raw events 30 days, sessions 90 days, daily aggregates 365 days.
- Main reads tmpfs for today; a past day reads compact SQLite aggregates.
  Detail reads indexed sessions from SQLite plus the open interval from tmpfs.
  No raw-event scan is used to render.
- If `live.json` is older than two minutes, the renderer does not extend the
  open interval; it shows closed totals and a `stale` note. A missing or
  stopped tracker falls back to durable daily aggregates.

The database and snapshot are private to the user (`0600` files, `0700`
directories). There are no log files; logs go to journald.
