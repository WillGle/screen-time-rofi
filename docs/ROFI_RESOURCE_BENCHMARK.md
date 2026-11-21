# Rofi vs Native/Store-Style UI — Resource Benchmark

- **Status:** Measured sandbox benchmark, 2026-11-21.
- **Question:** is a Rofi-based screen-time UI actually lighter than a native
  desktop app or a store-style tracker?
- **Short answer:** for the measured workloads, yes — by a large margin. The
  Rofi UI used about **1/19th** the proportional memory of an equivalent GTK4
  window, opened about **2.6×** faster, and used **8.8×** fewer threads. A
  store-style ActivityWatch background stack used about **29×** the memory of a
  small shell tracker before any browser UI was opened.

This benchmark measures representative UI shells and tracker backends, not a
specific mobile “Screen Time” app. Linux has no universal default screen-time
dashboard; Android/iOS apps cannot run in this sandbox.

## 1. Sandbox

Location: `/tmp/opencode/screen-time-bench/` (not part of the NixOS repo).

- NixOS, Xvfb `1600x1000x24`, software rendering.
- Rofi 2.0.0.
- GTK 4.22.5 (controlled native-app baseline).
- ActivityWatch 0.13.2 (store-style automatic tracker; Rust server + Python
  watchers).
- 3 runs per workload; median reported.
- Warm cache; startup measured from process launch to first visible window.

### Workloads

1. **Rofi dashboard**
   - `rofi -dmenu`, `-normal-window`, opaque 94% × 55% window.
   - Same content as the proposed design: 4 header lines, 96-column ruler,
     7 rows (`POWER`, 5 apps, `+3 apps`).
   - JetBrainsMono Nerd Font 12pt; no animation.
2. **GTK4 dashboard**
   - One 1504×550 GTK4 window, same text, same font and geometry.
   - 12 labels and a separator; no animation, no data backend.
3. **ActivityWatch background**
   - `aw-qt --no-gui --testing`, with server, window watcher, and AFK watcher.
   - This is the always-on store-style tracking stack, without its browser UI.
4. **Shell tracker mock**
   - `bash` loop: read the active window with `xdotool` every 5 seconds.
   - A deliberately lightweight lower bound for a potential Rofi backend; not
     a production tracker.

### Metrics

- **Startup:** process launch to first visible X11 window.
- **PSS:** proportional set size, summed over the process tree; primary memory
  metric.
- **RSS:** resident set size, summed; secondary, overcounts shared pages.
- **Idle CPU:** process-tree CPU over a 10-second idle sample, in percent of one
  core.
- **Threads/processes:** summed over the process tree.
- **Closure:** `nix path-info -S`, the full Nix runtime closure.

PSS is preferred for single-process apps. For multi-process ActivityWatch,
summed PSS/RSS still overcounts shared library pages; treat those numbers as
directional, not exact.

## 2. Results

### Foreground UI shell

| Metric | Rofi 2.0.0 | GTK4 4.22.5 | Ratio |
|---|---:|---:|---:|
| Startup to window | **75.5 ms** | 196.1 ms | Rofi 2.6× faster |
| PSS | **7.8 MB** | 149.2 MB | Rofi 19.0× lower |
| RSS | **33.1 MB** | 260.4 MB | Rofi 7.9× lower |
| Threads | **5** | 44 | Rofi 8.8× fewer |
| Processes | **1** | 1 | equal |
| Idle CPU | **0.0%** | 0.0% | equal |
| Nix closure | **308.8 MB** | 983.1 MB | Rofi 3.2× smaller |

Rofi min/max:

- startup 74.7–76.9 ms;
- PSS 7.83–7.88 MB;
- RSS 33.08–33.21 MB;
- threads 5;
- idle CPU 0.0%.

GTK4 min/max:

- startup 173.3–199.9 ms;
- PSS 149.0–149.3 MB;
- RSS 260.2–260.5 MB;
- threads 44;
- idle CPU 0.0%.

### Always-on tracker backend

| Metric | Shell tracker | ActivityWatch 0.13.2 | Ratio |
|---|---:|---:|---:|
| PSS | **3.0 MB** | 87.3 MB | Shell 29.2× lower |
| RSS | **15.9 MB** | 138.0 MB | Shell 8.7× lower |
| Idle CPU | **~0.0%** | 0.30% | Shell lower |
| Processes | **2** | 4 | Shell 2× fewer |
| Threads | **2** | 26 | Shell 13× fewer |
| Nix closure | ~80–100 MB (est.) | 2453.2 MB | ActivityWatch ~25–30× larger |

The shell tracker is a mock, not a complete tracker. It shows the cost of a
minimal polling backend, not the final Rofi app. The ActivityWatch number is the
real store-style background stack: server + window watcher + AFK watcher, with
no browser UI.

### Installation footprint

| Package | Nix closure |
|---|---:|
| Rofi 2.0.0 | **308.8 MB** |
| GTK4 4.22.5 | 983.1 MB |
| ActivityWatch 0.13.2 | 2453.2 MB |

A Rofi app also needs a small tracker and data scripts; a GTK/ActivityWatch app
needs its own runtime and UI stack. The closure table is not a full application
install comparison, but it shows the order of magnitude.

## 3. Scores

Scores are normalized so 100 = lightest/best in that comparison. The weights are
explicit; raw numbers above are the source of truth.

### UI shell score

Weights: startup 30%, PSS 40%, idle CPU 10%, threads 10%, closure 10%.

| UI shell | Score |
|---|---:|
| Rofi | **100** |
| GTK4 native dashboard | **27.9** |

Component scores for GTK4: startup 38.5, PSS 5.3, idle CPU 100, threads 11.4,
closure 31.4. Rofi is 100 in every component.

### Always-on backend score

Weights: PSS 60%, idle CPU 25%, process count 15%.

| Backend | Score |
|---|---:|
| Shell tracker mock | **100** |
| ActivityWatch background | **10.4** |

ActivityWatch component scores: PSS 3.4, CPU 3.2, processes 50. The actual
ActivityWatch foreground UI is a browser page and is **not included**; adding a
browser would increase memory and startup cost further.

### How to read the scores

- The UI score answers: “how heavy is the window itself?”
- The backend score answers: “how heavy is the always-on tracking process?”
- Do not add them into a single “app score” unless the app has the same feature
  set. ActivityWatch’s background is a full tracker; the shell tracker is a
  lower-bound mock. Rofi is a UI shell, not a complete app.

## 4. Interpretation

1. **The UI-shell difference is real and large.** Rofi is a single small process
   with no web engine, no GTK widget tree, and no GPU scene graph. The GTK4
   baseline uses 19× the PSS for the same text and geometry.
2. **Startup matters for a popup.** 75 ms vs 196 ms is the difference between
   “instant” and “a noticeable pause” for a keyboard-driven dashboard. A
   browser-based store UI would be slower still.
3. **The store-style stack is heavier before its UI opens.** ActivityWatch’s
   server + watchers use 87 MB PSS and 0.3% CPU continuously in this sandbox.
   That is 29× the PSS of a minimal shell tracker.
4. **Rofi is not a complete app.** A production Rofi screen-time tool still
   needs a tracker, data storage, and rendering. The shell tracker mock shows
   that a minimal backend can stay in the low single-digit MB range, but it must
   be event-driven and avoid rewriting a full day file every sample.
5. **Idle CPU is near zero for both UI shells.** The measured difference is
   memory and startup, not idle CPU. ActivityWatch’s watcher stack is the only
   workload with a measurable idle CPU in this test.
6. **The result is not a universal law.** A very small native app could beat a
   Rofi app on some metrics; a different store app may be lighter than
   ActivityWatch. The measured comparison is representative, not exhaustive.

## 5. Caveats

- Xvfb uses software rendering. Absolute memory and startup values will differ
  on real hardware; the relative ordering is the useful result.
- GTK4 is a controlled native baseline, not a specific store application.
- ActivityWatch’s desktop app is a tray/server; its actual UI is a web page. The
  browser process was not measured. A full store-style UI comparison would add
  browser memory, startup, and process count.
- The shell tracker is a mock. It does not parse JSON, persist sessions, handle
  idle/lock, or render data. It is a lower bound, not a production benchmark.
- The warm-cache startup numbers exclude first-run font/config initialization.
- Summed PSS/RSS for multi-process ActivityWatch overcounts shared pages.
- Nix closures include shared dependencies and are not directly comparable to
  distro package sizes.
- No mobile “Screen Time” app was measured; iOS/Android cannot run in this
  sandbox.

## 6. Reproduction

```bash
cd /tmp/opencode/screen-time-bench
nix-shell --run 'python3 bench.py'          # Rofi vs GTK4 UI
nix-shell --run 'python3 bench_aw.py'       # ActivityWatch background
nix-shell --run 'python3 bench_tracker.py'  # shell tracker mock
```

Raw logs are under `/tmp/opencode/screen-time-bench/logs/`.

## 7. Conclusion

For the measured representative workloads, a Rofi-based screen-time UI is
substantially cheaper than a native GTK4 window and far cheaper than a
store-style ActivityWatch stack:

- **UI shell:** 100 vs 27.9 (Rofi wins).
- **Always-on backend:** 100 vs 10.4 (minimal shell tracker wins; not a
  like-for-like production tracker).
- **Install footprint:** 309 MB vs 983 MB vs 2453 MB.

The practical recommendation is unchanged: keep the Rofi UI static, keep the
tracker event-driven, and avoid a browser/Electron/QtWebEngine UI. The UI shell
is cheap; the backend design determines whether the whole app stays cheap.

For the storage/write path — event-driven capture, tmpfs live snapshots, batched
SQLite/append writes, and measured write amplification — see
`docs/research/SCREEN_TIME_STORAGE_ARCHITECTURE.md`.
