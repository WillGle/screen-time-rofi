# Screen Time — Storage and Write-Path Research

- **Status:** Research and measured sandbox benchmark, 2026-11-21.
- **Goal:** avoid continuous read/write and SSD wear; compute and render only
  when the user opens the dashboard.
- **Constraint:** the UI is static Rofi; the tracker must run continuously, but
  it does not need to poll or rewrite a day file.
- **Scope:** single-user, local-only. No cloud, network service, multi-user,
  multi-client, authentication, or sync. File permissions are basic hygiene, not
  a product feature.

## 1. Short answer

Do not rewrite a full day file on every sample. Use **event-driven capture**,
keep the current day in memory, expose a **tmpfs live snapshot** for the UI, and
persist to disk in **batches**.

One nuance: accurate tracking still requires a background listener. You cannot
reconstruct app usage that was never observed. The goal is therefore not “no
background process”; it is **no polling, no continuous disk I/O, and no work
while the UI is open**.

Recommended stack:

1. **Capture events, not ticks.** Hyprland active-window changes, idle
   begin/end, boot/shutdown. Suspend, lock, and display-off are explicitly out of
   scope; a 5-second poll is not needed.
2. **Keep the open interval open.** Store `session_start` and accumulated closed
   totals. Do not update totals every sample.
3. **Serve the UI from tmpfs.** The tracker writes a small `live.json` to
   `/run/user/$UID/screen-time/` only when state changes. Rofi reads that file
   on click; reads are RAM, not disk.
4. **Persist in batches.** Use SQLite WAL, flushed when **60 seconds** pass or
   **100 uncommitted events** accumulate, whichever comes first. No per-sample
   transaction, no per-event `fsync`.
5. **Query compact aggregates.** Main view reads the live snapshot; app detail
   queries indexed sessions; history queries daily aggregates. Never scan raw
   events to render the popup.

## 2. Measured write amplification

Sandbox: ext4 (`/var/tmp`), Python 3.13, SQLite 3.53.1. Workload: **720
samples = one hour at 5-second intervals**, extrapolated linearly to one day
(17,280 samples). `write_bytes` comes from `/proc/self/io`; `fsync` and
`fdatasync` counts come from `strace -c`.

| Strategy | Logical bytes/hour | Actual write bytes/hour | Fsync/hour | Est. write/day | Est. fsync/day | Elapsed/hour |
|---|---:|---:|---:|---:|---:|---:|
| Full JSON rewrite per sample | 166 KB | 2.95 MB | 1,440 | **70.8 MB** | **34,560** | 1.15 s |
| Append-only JSONL, flush every 60 s | 39.6 KB | 287 KB | 62 | **6.9 MB** | **1,488** | 0.05 s |
| SQLite WAL, batch commit every 60 s, `synchronous=NORMAL` | 34.6 KB | 647 KB | 8 | **15.5 MB** | **~192** | 0.03 s |

### Interpretation

- Full rewrite is the worst on every axis: 10× the bytes of append-only, 180×
  the fsyncs of SQLite, and it scales with **file size**, not just event count.
- A 4 KB block per rewrite already produces 70.8 MB/day for a tiny snapshot. A
  larger day file (sessions, per-app metadata, history) makes it worse:
  roughly **350 MB/day at 20 KB** and **1.7 GB/day at 100 KB**.
- Append-only has the lowest bytes, but more fsyncs than SQLite WAL in this
  configuration.
- SQLite WAL with batching has the fewest fsyncs and keeps a queryable schema.
  It writes more bytes than append-only, but the difference (about 8.6 MB/day)
  is small compared with full rewrite.
- Event-driven capture reduces the record count from 17,280/day to hundreds or
  a few thousand. All three strategies then become much smaller; the ranking
  stays the same.

### Is this actually an SSD-wear problem?

Reads do not consume SSD write endurance. The concern is writes and fsyncs.

At the measured full-rewrite rate of 70.8 MB/day, that is about **25.8 GB/year**
— small compared with a typical SSD’s hundreds of TBW endurance. The bigger
problems are:

- **34,560 fsyncs/day** causing latency spikes and unnecessary I/O;
- write amplification at the filesystem/SSD layer;
- CPU spent serializing a whole file on every sample;
- a design that scales badly as the day file grows.

So the recommendation is not panic; it is to remove a clearly unnecessary write
pattern.

### How comparable apps batch

| App / stack | Batching behavior | Lesson |
|---|---|---|
| **ActivityWatch** | Watchers send heartbeats; the server merges adjacent identical events within a `pulsetime` window. `aw-server-rust` commits the SQLite transaction when **15 seconds** pass or **more than 100 events** are uncommitted. v0.14 uses SQLite WAL. | Closest analogue. 15 s / 100 events is a proven policy; it bounds loss without per-event commits. |
| **RescueTime** | Desktop client syncs with the server on a **1-minute interval**; data may take a few minutes to appear. | One minute is acceptable for screen-time data; this is a server-sync batch, not necessarily a local SQLite commit. |
| **ManicTime** | Local SQLite (`ManicTimeCore.db`, `ManicTimeReports.db`); stores application/usage intervals. Public docs do not state a commit interval. | Local SQLite is normal for this domain; interval model, not per-second rows. |
| **Android/Room** | WAL + `synchronous=NORMAL`; batch inserts in one transaction; one writer at a time. Autocommit writes pay the commit cost individually. | Batch writes; WAL/NORMAL is the standard mobile pattern. |
| **SQLite itself** | WAL with `synchronous=NORMAL` does not fsync every commit; checkpoint is the sync point. Default auto-checkpoint at 1000 pages (~4 MB). Long readers can block a checkpoint. | Keep the WAL bounded; avoid long read transactions; checkpoint in idle time. |
| **Apple/Google Screen Time** | OS-level aggregation; no public batching contract. | Not directly comparable; do not use as a write-pattern reference. |

**Takeaway:** the industry norm is not “write every sample.” It is “keep state
in memory, commit in bounded batches, and checkpoint separately.” ActivityWatch
at 15 s / 100 events and RescueTime at 1 minute both sit in the same range.

## 3. Recommended architecture

```text
 Hyprland active-window events ─┐
 logind idle begin / end ───────┤
 boot/shutdown records ─────────┘
                │
                ▼
        tracker daemon
        (single process, event-driven)
                │
      ┌─────────┼──────────────────────────┐
      │         │                          │
      ▼         ▼                          ▼
 live.json   SQLite WAL                daily rollup
 (tmpfs)     (append + batch commit)   (aggregate tables)
      │         │
      │         └── history / app detail queries
      │
      ▼
 rofi on click:
   read live.json
   compute open interval
   render
```

### Hot / warm / cold tiers

| Tier | Location | Contents | Write policy |
|---|---|---|---|
| **Hot** | `$XDG_RUNTIME_DIR/screen-time/live.json` (tmpfs) | current app, open interval start, closed totals, 30-minute slots, power state, `as_of` | update on state changes; no disk |
| **Warm** | SQLite WAL | events and sessions | commit at 60 s or 100 events, whichever first |
| **Cold** | SQLite aggregate tables / daily JSON | daily app/slot totals, power sessions | once per rollup/day |

The live tier is not durable. On reboot, the daemon reconstructs from the warm
and cold tiers and starts a new open interval.

`live.json` is still a write, just to RAM. Keep it small and update it on state
changes, not on a timer. Write it with a temp file + atomic rename so Rofi never
reads a partial snapshot. A 5–20 KB tmpfs file is cheap; a 5-second rewrite
would be unnecessary CPU and memory churn even though it does not wear the SSD.

### Capture: intervals, not samples

A five-second poll records 17,280 samples/day even when the app never changes.
An event-driven tracker records one event when state changes:

- active window changes;
- idle begins/ends;
- boot/shutdown.

Suspend, lock, and display-off are **not tracked**. That keeps the event model
small, but it has a cost: if the device is locked/suspended/display-off while an
app still has focus, active time may be overcounted unless the idle signal
closes the interval. Treat the idle timeout as the guard, and do not claim
screen-on accuracy.

The current session remains open in memory. Its duration is computed from
`now - start` on read, not by writing a counter every five seconds.

Scope is **app-level only**: a window title change with the same app id does
not create a new interval and no title column/table is stored. This keeps event
volume and raw storage small, and matches the product requirement “app nào dùng
bao lâu”.

### Read path: compute on click

Rofi opens and reads one small file:

```text
/run/user/1000/screen-time/live.json
```

The file contains:

```json
{
  "date": "2026-11-21",
  "as_of": "2026-11-21T15:21:22+07:00",
  "last_commit_at": "2026-11-21T15:21:00+07:00",
  "powered_closed_seconds": 41940,
  "powered_open_start": "2026-11-21T14:20:00+07:00",
  "closed_active_seconds": 24480,
  "open": {
    "app": "Code",
    "start": "2026-11-21T15:18:40+07:00"
  },
  "apps": {
    "Code": {"closed_seconds": 8040, "closed_sessions": 17},
    "Firefox": {"closed_seconds": 5520, "closed_sessions": 6}
  },
  "slots": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}
```

The renderer computes:

```text
total = closed_active_seconds + max(0, now - open.start)
app_total = app.closed_seconds + (open.app == app ? max(0, now - open.start) : 0)
app_sessions = app.closed_sessions + (open.app == app ? 1 : 0)
powered = powered_closed_seconds
  + (powered_open_start ? max(0, now - powered_open_start) : 0)
```

`last_commit_at` is what app detail shows as “sessions through HH:MM”; `as_of`
is the live snapshot age used for the stale guard. They are different values on
purpose: main can be fresher than the last SQLite batch.

The current open interval is overlaid on the 30-minute strip at render time. No
write is needed while the user reads the screen.

If the live file is missing (daemon stopped), read the last durable daily
snapshot and mark the view as stale. Do not scan the raw journal.

**Stale-open guard:** if `as_of` is older than a small threshold (for example
two minutes), do not extend the open interval to `now`; show the last closed
total and mark the view stale. Otherwise a crashed daemon would make the current
app look like it was used forever. Include `boot_id` and the tracker PID in the
snapshot so the renderer can detect a reboot/restart.

### tmpfs JSON vs Unix socket

Both avoid disk I/O on click. The choice is about freshness, failure behavior,
and code complexity, not raw latency: both are far below Rofi’s 75 ms open time.

| Criterion | tmpfs JSON snapshot | Unix socket query |
|---|---|---|
| **Freshness** | As fresh as the last state-change write; open interval is computed by the reader | Always current at query time; daemon computes |
| **Simplicity** | Very high: write/read one file; Bash + `jq` works | Medium: protocol, framing, timeouts, client, error handling |
| **Failure mode** | File can be stale after a daemon crash; needs TTL/stale guard | Query fails clearly if daemon is down; no silent stale read |
| **Source of truth** | Duplicated state in memory + file; schema in two places | One source of truth in the daemon |
| **Detail queries** | Must put detail in the file or fall back to SQLite | Can return exactly the requested app/day/detail |
| **Schema evolution** | Version the JSON; old readers may break | Version the protocol; daemon can adapt |
| **Security** | `0600` file in `$XDG_RUNTIME_DIR`; no socket permissions | `0600` socket in `$XDG_RUNTIME_DIR`; same-user only |
| **Debugging** | Easy: `cat`/`jq` the file | Needs `socat`/`nc` or a client; harder to inspect |
| **Resource use** | Tiny; but serialization/write on every state change | Tiny; serialization on demand; request handling |
| **Dependencies** | Rofi + shell + `jq` | Rofi + shell + socket client (`socat`, `nc`, Python, or binary) |
| **Multiple clients** | All read the same snapshot | Daemon can serve multiple clients |
| **Long-lived readers** | No DB read; no checkpoint blocking | Query must be short; avoid long read transactions |

**Pros of tmpfs JSON**

- Simplest possible bridge from a daemon to a Bash/Rofi renderer.
- No protocol, no listener, no client dependency beyond `jq`.
- Atomic temp + rename gives readers a consistent snapshot.
- Easy to debug and to read manually.
- Fits the “compute open interval on click” model: the file only needs closed
  totals plus the open start.
- Excellent for one user, one popup, one main view.

**Cons of tmpfs JSON**

- Staleness after a daemon crash; requires `as_of`/PID/boot-ID guard.
- Duplicates state and schema outside the database.
- A large detail payload makes the snapshot big and serialization expensive.
- Every state change rewrites the file in RAM; keep it event-driven, not timed.
- No request-specific filtering; the reader gets the whole snapshot.
- Two sources of truth can drift if the daemon dies between writes.

**Pros of Unix socket**

- One source of truth: the daemon’s in-memory state.
- Fresh detail: query exactly the day/app/session needed.
- Clear failure: no daemon, no data; no silent stale extension.
- Better for multiple views/clients and future features.
- Can return a compact response per request.

**Cons of Unix socket**

- More code: framing, timeouts, retries, versioning, permissions, shutdown.
- A hung/busy daemon can block the UI unless requests are async or cached.
- Harder to debug from a shell.
- Requires a client dependency or a tiny helper binary.
- Overkill for a single Rofi popup that only needs a daily snapshot.

**Recommendation:** use **tmpfs JSON for v1** because the main view only needs a
small precomputed snapshot and the renderer already computes the open interval.
Add a Unix socket only if you need live per-app detail, multiple clients, or a
single-source-of-truth API. A hybrid (socket primary, snapshot fallback) is
robust but is more machinery than this product needs now.

### App detail: no live query in v1

“Live query” here means: when the user opens app detail, ask the daemon over a
Unix socket for the current sessions, instead of reading a file/SQLite. It does
**not** mean continuously updating the detail while it is open; Rofi is static,
so the screen would still be a snapshot.

For v1: **no live query needed.**

- Main view reads tmpfs `live.json`.
- App detail reads:
  1. closed sessions for that app/day from SQLite (indexed);
  2. the current open interval from `live.json`, if the app is active.
- The renderer merges these two and computes the current session/burst. No
  socket.

The only gap is a session that closed less than one batch ago (up to 60 seconds
or 100 events): it may not be in SQLite yet.

**Decision for v1: accept that lag.** Do not add a socket or a pending-session
overlay. Show `as_of` in the detail footer; the next open or explicit refresh
sees the just-closed session.

The upgrade path, only if the lag becomes user-visible, is:

1. add `pending_sessions` to `live.json` (no socket);
2. add a Unix socket query (last resort).

A live query is worth it only if one of these is true:

- the detail must include a session that closed in the last batch and the
  difference is user-visible;
- the detail needs data that is too large to put in the tmpfs snapshot;
- multiple clients/views need different queries;
- you want a single-source-of-truth API.

None of these is true for v1. SQLite + tmpfs is enough for a static popup.

### Durable write path

#### Option A — SQLite WAL (recommended)

Use one long-lived connection in the tracker daemon. Never invoke the
`sqlite3` CLI per event.

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA temp_store = MEMORY;
```

- Insert events/sessions in one transaction per batch: **60 s or 100 events**,
  whichever comes first; also flush on clean shutdown and day rollover.
- Update aggregate tables in the same transaction.
- `synchronous=NORMAL` avoids an fsync on every commit; the database remains
  consistent after a crash, though the last batch may be lost.
- Checkpoint periodically; keep the WAL bounded.
- One writer only; the daemon owns the database.

Suggested schema:

```sql
events(id, ts, kind, app, state, boot_id)
sessions(id, date, app, start_ts, end_ts, seconds, state, confidence)
daily_app(date, app, seconds, sessions, first_ts, last_ts)
daily_slots(date, app, slot, seconds)
power_sessions(id, start_ts, end_ts, seconds, reason, confidence)
meta(key, value)
```

Indexes: `sessions(date, app, start_ts)`, `daily_app(date)`,
`daily_slots(date, app)`, `power_sessions(start_ts)`.

#### Option B — append-only journal + snapshots

- Append JSONL/CBOR events to `journal/YYYY-MM-DD.jsonl` with `O_APPEND`.
- Buffer in memory; flush every 60 s or 64 KB; `fdatasync` at the batch boundary.
- Write a compact `days/YYYY-MM-DD.json` snapshot every 15 min and on clean
  shutdown, using atomic temp + rename.
- UI reads only the snapshot; detail replays/query a compact session index, not
  the raw journal.

This has the lowest byte volume, but more custom recovery code and no indexed
queries. Use it if the app must avoid a SQLite dependency; otherwise SQLite is
the better default.

### Retention

| Data | Retention | Reason |
|---|---|---|
| Raw events/journal | **30 days** | detail, crash recovery; chosen retention |
| Sessions | **90 days** (chosen) | app detail |
| Daily aggregates | 180–365 days | history/trends |
| Live snapshot | tmpfs, ephemeral | current view only |

Raw retention is **30 days**. With app-level-only events, this is a small
volume: roughly hundreds to a few thousand events/day, at tens of bytes each.
A daily maintenance job deletes events older than 30 days; aggregates and
sessions remain. Do not run `VACUUM` on every cleanup; free pages are reused.


Roll up and prune in a daily job, not in the UI path.

### Failure and recovery

- **Crash/power loss:** SQLite WAL recovers to the last commit. Append journal
  replays from the last snapshot; a truncated final line is ignored.
- **Daemon stopped:** UI reads the last durable snapshot and labels it stale.
- **Clock jump:** use monotonic time for durations, wall clock for display;
  detect negative/absurd intervals and clamp.
- **Unclean shutdown:** close the open interval at the last known event; mark
  the gap `unknown`, not `off` or `active`.
- **Timezone/day boundary:** roll over at local midnight; keep events in UTC or
  monotonic + offset, display in local time.

## 4. Read/write rules

1. **No polling for state.** Events drive state changes.
2. **No per-sample disk write.** Memory + tmpfs only.
3. **No full-file rewrite per sample.** Batch append or SQLite transaction.
4. **No raw-log scan to render.** Main view reads live snapshot; detail uses
   indexed sessions; history uses aggregates.
5. **No `updated_at` write on every tick.** The live snapshot has `as_of`; the UI
   computes elapsed time.
6. **No fsync per event.** Batch at 60 s or 100 events; accept a bounded loss
   window.
7. **No animation or live refresh.** Static Rofi snapshot; refresh on reopen or
   explicit key.
8. **No disk read on click if the daemon is alive.** Use tmpfs; fall back to
   durable snapshot only when needed.

## 5. What the UI needs from storage

- **Main screen (today):** one small live snapshot: total active/powered,
  app totals, 30-minute slots, open interval, power state.
- **Main screen (past day):** one compact daily aggregate.
- **App detail:** indexed sessions for one date/app (or an in-memory session
  list); no full event scan.
- **Power detail:** indexed power sessions.
- **7-day trend:** seven aggregate rows.

Each is O(1) or indexed; none requires replaying raw events.

## 6. Estimated impact

For a 5-second sampling model, the measured difference is:

| Metric | Full rewrite | Append | SQLite WAL |
|---|---:|---:|---:|
| Write/day | 70.8 MB | 6.9 MB | 15.5 MB |
| Fsync/day | 34,560 | 1,488 | ~192 |
| Relative write (append = 1) | 10.3× | 1.0× | 2.2× |
| Relative fsync (SQLite = 1) | 180× | 7.8× | 1.0× |

With event-driven capture (fewer than 17,280 records/day), all numbers drop
further. The exact value depends on how often the user switches apps, but it is
bounded by transitions, not by a timer.

## 7. Rollout plan

1. **Define events and state.** Active window, idle begin/end, boot/shutdown;
   app identity; open interval. No suspend/lock/display events.
2. **Build the tracker daemon.** Event sources, in-memory state, live snapshot
   writer, batch writer.
3. **Add the live snapshot.** `$XDG_RUNTIME_DIR/screen-time/live.json`; atomic
   write via temp + rename; no disk.
4. **Add durable storage.** SQLite WAL with batched commits; or append journal +
   snapshots.
5. **Change the Rofi read path.** Read live snapshot; compute open interval;
   fall back to durable snapshot.
6. **Move detail/history to indexed queries.** No raw log scans.
7. **Verify.** Measure `/proc/<pid>/io` write bytes, fsync count, open latency,
   and a one-day disk-write total.

## 7b. One-time install model

The target is: **install once with the system configuration, then never set it
up again.** Non-NixOS users get the same one-time install through
`install.sh`; see `docs/research/GITHUB_INSTALLER.md`.

### Install surface

- One Nix package: `screen-time`.
- One Home Manager module: `services.screen-time.enable = true;`.
- One user systemd service, started at login.
- One Rofi entry/script, installed by the same module.
- No pip/npm/cargo install, no runtime download, no browser extension, no
  external database server.

A single executable with subcommands is the cleanest packaging:

```text
screen-time daemon     # tracker, SQLite, tmpfs snapshot
screen-time snapshot   # optional manual flush/export
screen-time doctor     # check Hyprland/logind/DB health
```

The Rofi wrapper can be a small script in the same package; it only reads the
tmpfs snapshot and renders.

### First start

On first start, the daemon must create everything itself:

- `$XDG_STATE_HOME/screen-time/` (or `~/.local/state/screen-time/`);
- SQLite DB, schema, and `user_version`;
- `$XDG_RUNTIME_DIR/screen-time/` for `live.json`;
- default retention settings;
- no prompts, no config file required.

### Runtime behavior

- `WantedBy=graphical-session.target`; starts with the session.
- `Restart=always`; a crash does not require user action.
- `StateDirectory=screen-time`, `RuntimeDirectory=screen-time`.
- Daily retention cleanup is internal (on day rollover/start), not a separate
  cron/timer to install.
- No network access.

### Upgrade behavior

- A `home-manager switch`/`nixos-rebuild` replaces the package.
- On startup, the daemon reads `PRAGMA user_version` and runs idempotent
  migrations in one transaction.
- Keep a small pre-migration backup (for example `VACUUM INTO`) before a schema
  change; do not require a manual migration command.
- Never write to the Nix store; all state stays under the user’s state/runtime
  directories.

### Uninstall behavior

- Remove the module and rebuild; the service and Rofi entry disappear.
- User data remains under `~/.local/state/screen-time/` until explicitly
  deleted. Provide an optional `screen-time purge` command rather than deleting
  data silently.

### What “one-time” does not mean

It does not mean “no background process.” Accurate tracking needs the per-user
daemon. It means **no per-session setup, no runtime installation, no manual
database management, and no recurring configuration**.

## 8. Decisions

Decided:

1. **Durable store:** SQLite WAL.
2. **Raw retention:** 30 days.
3. **Scope:** app-level only; no window-title storage.
4. **Live transport:** tmpfs JSON for v1.
5. **Batch policy:** commit when **60 seconds** pass or **100 uncommitted
   events** accumulate, whichever comes first; flush on clean shutdown and day
   rollover. ActivityWatch uses 15 s / 100 events; this is the lower-write
   variant with a bounded one-minute loss window.
6. **Session retention:** **90 days**.
7. **Power states:** simple **on/off only**. Suspend, lock, and display-off are
   not tracked. Power session reasons are limited to `shutdown`, `unclean`, and
   `unknown`.
8. **App detail:** no live query, no socket, no pending overlay. Read SQLite for
   closed sessions plus tmpfs for the open interval; accept up to one batch of
   lag and show `as_of`.
9. **Deployment:** one per-user tracker, one SQLite file, one tmpfs JSON, one
   Rofi reader. No socket/server/multi-client layer.

Recommended defaults:

9. **Failure policy:** `synchronous=NORMAL`; accept losing at most the last
   batch on power loss. Do not use `synchronous=FULL` per event.
10. **Checkpoint:** keep the default auto-checkpoint at 1000 pages (~4 MB), and
    checkpoint on idle/shutdown. Do not checkpoint every commit.

Storage decisions for v1 are complete. The remaining product decisions are the
UI-level ones in `SCREEN_TIME_ROFI_UI.md` (row count, `×N` experiment, type size,
language, sequence).

## 9. Reproduction

Write benchmark:

```bash
cd /var/tmp/opencode-screen-time-bench
for s in full append sqlite; do
  strace -f -c -e trace=fsync,fdatasync \
    -o "trace-$s.txt" python3 writebench.py "$s" --json
done
```

The benchmark uses 720 samples (one hour at 5 seconds) and extrapolates to a
day. It writes small files under `/var/tmp`; remove them when done.

## 10. Sources

Accessed 2026-11-21.

- ActivityWatch `aw-datastore` commit, “Guarantee DB commit every 15 seconds”
  (15 s or >100 uncommitted events):
  <https://github.com/ActivityWatch/aw-server-rust/commit/17dff67>
- ActivityWatch data model, Heartbeats and `pulsetime`:
  <https://docs.activitywatch.net/en/latest/buckets-and-events.html>
- ActivityWatch v0.14 release notes, SQLite WAL:
  <https://activitywatch.net/blog/activitywatch-v0-14-0>
- RescueTime API/help, desktop sync interval:
  <https://rescuetime.com/rtx/developers>
- ManicTime database location (SQLite):
  <https://docs.manictime.com/win-client/faq/database-location>
- SQLite WAL documentation:
  <https://www.sqlite.org/wal.html>
- Android SQLite performance best practices (WAL, `synchronous=NORMAL`, batch
  inserts):
  <https://developer.android.com/topic/performance/sqlite-performance-best-practices>
