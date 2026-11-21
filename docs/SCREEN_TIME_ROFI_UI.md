# Screen Time Rofi UI — Clean-Slate, High-Visibility Product Research

> **Final v1 UI spec:** `docs/research/SCREEN_TIME_UI_SPEC.md`. This document is
> the research trail; the spec is the locked implementation target.

- **Status:** Research only. No implementation.
- **Date:** 2026-11-21
- **Basis:** the product brief and the Rofi platform only.
- **Added constraint:** high visibility, static rendering, no animation, low
  resource use.
- **Explicitly excluded:** the existing screen-time implementation, its data
  schema, its current layout, and local usage data. This is a from-scratch
  design exercise.

## 1. Product brief

The requested product must answer four questions:

1. **App timeline:** which app was used when during the day?
2. **App duration:** how long was each app used today?
3. **Device time:** how long was the device used today?
4. **Daily shape:** how was usage distributed across the day — peaks, gaps,
   morning/evening, and the active span?

The UI must be:

- compact and calm;
- readable in a few seconds;
- navigable with very few actions;
- high-visibility and high-contrast;
- static: no animation, no transitions, and no motion;
- low-resource: one render per open, no timers, no continuous redraw;
- delivered as a Rofi popup.

### Product thesis

This is **not** a dashboard with a chart added. It is a **static day tape**: a
single 24-hour timeline is the primary object, and app totals and device totals
are annotations on that timeline. The screen is a snapshot, not a live app.

That thesis follows directly from the brief. "App timeline" is the first
requirement, and "which app, how long, and when" are all properties of the same
time axis. A ranked app list alone would answer "how long" but not "when". A
total alone would answer neither.

## 2. Jobs and success criteria

| Job | When it happens | Desired answer | Interaction budget |
|---|---|---|---|
| How much have I used the device today? | Open | Total active time + active span | 0 actions |
| Which app consumed the most time? | Open | Ranked rows + duration | 0 actions |
| When did I use each app? | Open | Shared 24-hour timeline | 0 actions |
| When was I most active? | Open | Visible clusters + peak label | 0 actions |
| When was the device powered on/off? | Open | `POWER` row + powered total | 0 actions |
| What about yesterday? | Sometimes | Same layout, one day back | 1 key |
| Show me the exact sessions for this app | Occasionally | Session list for one app | 1 key + 1 Enter |
| Is today unusual? | Sometimes | 7-day trend / usual-time comparison | 0 actions, secondary |

### Success criteria

- Answer the first four questions in under five seconds.
- Zero visible navigation buttons.
- Zero actions to see today; one key for the previous day; one key for app
  detail.
- Top five apps and their durations visible without scrolling at 1280×800.
- No horizontal wrapping at the target font size.
- The total, app names, durations, and active blocks remain legible at 1280×800
  with the target font; no thin or faint marks are used for primary data.
- A reader can identify the busiest app and the busiest time period without
  reading a legend.
- The layout works without color: row labels and positions carry identity.
- No animation or live update while the popup is open; the process is idle
  between opens.
- `active` and `powered` are visually distinct; the reader does not confuse
  device-on time with input-active time.

## 3. Market scan

| Product | Primary representation | What it proves | What to avoid |
|---|---|---|---|
| **ManicTime** | Computer-usage timeline (active/away) + application timeline + summary by app | The exact model requested: which app, when, for how long; each app visible on a shared time axis | Desktop density, tags, documents, screenshots, multiple panels |
| **Timing** | Vertical day timeline + activity list ordered descending by duration | Timeline and ranked durations belong together; a timeline alone is not enough for "which app" | Project/billing workflow, calendar integration, AI summaries |
| **ActivityWatch** | Timeline swimlanes, merge by app, AFK filtering, categories | Swimlanes are the clearest way to show "when" per app; keyboard/local-first matters | Browser-dashboard density, reports, category management |
| **Apple Screen Time** | Day/Week total, Daily Average, top apps, category bars | A total needs a baseline; top apps are the default composition view | No app-level timeline; parental controls; category-heavy charts |
| **Google Digital Wellbeing** | Dashboard, app list, app timers, top-three widget | Glanceable app totals and a small widget can be very effective | Unlock/notification counts; phone-centric behavior controls |
| **RescueTime** | Hourly graph + category/app reports + productivity score | "When" can be shown as an hourly shape; app totals are a separate report | Productivity judgment; account/cloud model; report depth |
| **Toggl / Clockify** | Manual timeline / timesheet | Chronology is natural for time data | Manual start/stop; not automatic device usage |

### What the market teaches

1. **Desktop timeline tools already validate the model.** ManicTime explicitly
   combines an active/away timeline, an application timeline ("which
   applications were used, when and for how long"), and a summary by
   application. Timing combines a day timeline with an activity list ordered by
   duration.
2. **Phone screen-time products skip the timeline.** Apple and Google are strong
   on totals and app lists, but weak on "when did this app run?". That is the
   gap a Rofi day tape can fill.
3. **A total needs context.** Apple pairs the total with a Daily Average and a
   Day/Week switch. A raw total is not a product.
4. **A timeline needs a ranking beside it.** Timing's activity list is ordered
   by duration. A timeline without totals makes "which app" slow to answer.
5. **Density is the main failure mode.** Timeline guidance warns that timelines
   become noisy when too much state is shown at once; overview first, detail on
   demand.
6. **One accent color is enough.** Dashboard research favors a single accent for
   the primary signal; decorative color competes with the data.
7. **Keyboard-first is rare.** Most desktop trackers assume a mouse and a large
   window. A Rofi timeline can win on speed and keyboard flow.

### Market gap

There is no lightweight, keyboard-first, local day timeline that opens in a
fraction of a second and closes just as fast. Existing timeline tools are
full desktop applications; existing phone tools are app-list dashboards.

## 4. Design principles

1. **Time is the primary axis.** Every visual element either sits on the 24-hour
   axis or annotates it.
2. **One screen, one scale.** All days use the same 00–24 axis, so patterns are
   comparable at a glance.
3. **Rows are the legend.** Each app row is labeled; no separate color legend.
4. **Duration lives with the app.** The total is attached to the row, not hidden
   in a separate report.
5. **Total is the header.** Device time is a headline, not a card.
6. **Rank by duration, but keep time alignment.** Sort rows by total time; keep
   every row on the same time axis.
7. **Selection is not navigation.** Up/Down selects an app; it does not open a
   new screen. Only Enter opens detail.
8. **Zero visible navigation chrome.** Day navigation is a keymap plus a
   one-line hint.
9. **One accent, no moral coloring.** Screen time is descriptive; do not use
   success/warning/error colors for apps or categories.
10. **Wide and short.** A timeline needs horizontal room; do not build a tall
    card dashboard.
11. **Details on demand.** The main screen is spatial; exact sessions belong to
    one optional detail level.
12. **Trust is part of the UI.** Define what "device time" means and show when
    data is stale or incomplete.
13. **Static by design.** No spinners, transitions, animated charts, or live
    polling while open. The screen is rendered once and read.
14. **Legibility over density.** Larger type and chunky blocks beat
    fine-grained marks that require squinting. Resolution can be sacrificed for
    visibility because exact sessions live in detail.
15. **High contrast on an opaque surface.** Primary data uses full-contrast
    text and solid accent blocks; no transparency, alpha, or faint marks for
    information that matters.

## 5. Concept directions

### A. Swimlane day timeline — recommended

Rows are apps, ordered by duration. Each row is a labeled 24-hour strip. The
header shows total device time and context.

```text
 SCREEN TIME                                  Fri 21 Nov · Today
 6h 48m active   ▲ 38m vs usual at this time
 powered 12h 40m · span 07:05–17:15 · peak 09:00–10:15
 7d ▁▂▃▅▆▄█  avg 6h 12m

                   00                      06                      12                      18                    24
 POWER     12h40m                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
 Code       2h14m                                      ██████          ████
 Firefox    1h32m                                ██                          ████
 Terminal     48m                                              ████
 Slack        32m                                            ██
 Music        22m                                                    ██
 +3 apps    1h20m                                                                  ▒▒▒▒▒▒

 ←/→ day   T today   R refresh   ↵ detail   esc close
```

- One slot = 30 minutes, rendered as two characters wide; 48 slots across.
- A blank slot is empty; `▓▓` is a powered-on device slot; `██` is an app's
  active slot; `▒▒` is the aggregated "other" row.
- The `POWER` row answers device on/off independently from app activity. Gaps
  in that row are time with the device off.
- All app activity uses one accent color. App identity comes from the row label.
- The "other" row uses a neutral shade if it has activity.
- The chunky two-character blocks are deliberate: they stay visible at a glance
  and do not depend on thin marks or hover states.
- The strip is illustrative; the duration column is the authoritative total.
- No category legend, no cards, no nav buttons.

**Strengths:** answers all five questions in one view (device on/off, active
time, app duration, per-app timing, daily shape); per-app "when" is obvious; no
color mapping; compact; Rofi-friendly; high visibility.

**Weaknesses:** power row plus five app rows plus header; 30-minute granularity
hides short switches; exact sequence needs the detail view.

### B. Single ribbon + ranked legend

One 24-hour ribbon colored by the active app, followed by a ranked list of app
names and durations.

```text
 00    03    06    09    12    15    18    21    24
 ·····██████████████▓▓▓▓▓▓······████████··········
 1 Code 2h14m   2 Firefox 1h32m   3 Terminal 48m
```

**Strengths:** most compact; most purely chronological; very fast to scan.

**Weaknesses:** requires a color-to-app mapping; per-app "when" is hard; a
colorblind user loses the mapping. Better as a compact fallback than the default.

### C. Chronological session list

Vertical rows of exact sessions:

```text
09:12–09:47  Code        35m
09:47–10:03  Firefox     16m
10:03–10:30  Code        27m
```

**Strengths:** exact sequence; best for auditing; natural in a Rofi list.

**Weaknesses:** not spatial; hard to compare days; weak at showing the overall
shape; too long for a day with many switches.

### Decision

| Criterion | A Swimlanes | B Ribbon | C Session list |
|---|---|---|---|
| Device total | High | High | Medium |
| App duration | High | High | Medium |
| When per app | High | Medium | High |
| Overall daily shape | High | High | Low |
| Glanceability | High | High | Medium |
| Rofi fit | High | High | High |
| Vertical cost | Medium | Low | High |
| Color independence | High | Low | High |

**Choose A.** Use B only if the window must be extremely short. Use C only as
the app-detail view, where exact chronology is the job.

## 6. Recommended first screen

### Wireframe

```text
 SCREEN TIME                                  Fri 21 Nov · Today
 6h 48m active   ▲ 38m vs usual at this time
 powered 12h 40m · span 07:05–17:15 · peak 09:00–10:15
 7d ▁▂▃▅▆▄█  avg 6h 12m

                   00                      06                      12                      18                    24
 POWER     12h40m                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
 Code       2h14m                                      ██████          ████
 Firefox    1h32m                                ██                          ████
 Terminal     48m                                              ████
 Slack        32m                                            ██
 Music        22m                                                    ██
 +3 apps    1h20m                                                                  ▒▒▒▒▒▒

 ←/→ day   T today   R refresh   ↵ detail   esc close
```

### Anatomy

| Zone | Question | Content | Interaction |
|---|---|---|---|
| Header | How much, and is that normal? | Date, total active time, powered-on time, same-time delta, active span, peak, 7-day trend + average | None |
| Axis | What time is it? | 00–24 ruler aligned to the 30-minute grid | None |
| Power row | When was the device on/off? | 96-column powered-on strip; blank = off | None |
| App rows | Which app, how long, when? | App name, duration, 96-column strip; top five + other | Up/Down select; Enter detail |
| Footer | How do I drive this? | One line of key hints | Keyboard; mouse optional |

### Why this layout

- **The timeline is the page.** There is no separate chart card competing with
  the app list.
- **The app row is self-contained.** Name, duration, and timing are on one line;
  the eye does not jump between a chart and a legend.
- **The header answers "device time".** The total is the first number, not a
  tile among many. `active` and `powered` are separate, so a device left on but
  idle is not mistaken for active use.
- **The POWER row separates power state from app activity.** Device off/on is
  visible even when no app was active.
- **The 7-day trend answers "how much per day" across days.** It is a secondary
  line, not a mode.
- **No category column.** Category is not part of the brief. Removing it removes
  taxonomy, colors, and a legend.
- **No percentages on the main screen.** Duration is the requested unit; share
  belongs in detail.
- **No "other apps" chart unless there is activity.** The row shows count and
  total; its strip can be neutral.

### App ordering

- Sort descending by total duration.
- Keep the order stable for the day; do not reorder on selection.
- Top five plus one "other" row.
- Rationale: five rows fit a short window, preserve legibility, and cover the
  common "which app" question. The exact number is a design hypothesis to test;
  if it feels too sparse, try seven.

### Experiment: ×N session count on the main view

The main view is currently binary: a block means activity, not continuity. A
session count is one way to expose fragmentation without adding a second chart.

Test two variants:

```text
APP          TIME  SESS          APP          TIME
Code        2h14m   18×          Code        2h14m ×18
Firefox     1h32m    6×          Firefox     1h32m ×6
```

- **Preferred variant:** a dedicated `SESS` column. It is easier to scan and
  harder to confuse with the duration than an inline `×N`.
- **Definition:** `N` is the number of merged sessions, not the number of raw
  switches. Cap the display at `99+`.
- **Label it:** `SESS` in the header, or `×N = sessions` in the footer. Do not
  leave a bare `×18` unexplained.
- **Power row:** if power sessions are counted, use a separate label such as
  `PWR`; do not reuse `SESS`.
- **Test:** compare no-count vs count at 1280×800. Measure width, scan time, and
  whether readers interpret `18×` as 18 sessions rather than 18 minutes.
- **Recommendation:** default off until the test shows a clear win. If it wins,
  use the column, not a tiny superscript or a second fill pattern.

### Timeline granularity and visibility

Visibility is the priority, so the default trades resolution for larger marks.

- **Primary: 30 minutes per slot, rendered two characters wide.** 48 slots =
  96 columns. Active slots are chunky `██` blocks that remain readable at a
  glance.
- **High-detail option: 15 minutes per slot, one character.** Same 96 columns,
  but thinner marks. Use only if a layout test shows it is still legible; it is
  a precision option, not the default.
- **Narrow fallback: 30 minutes per slot, one character.** 48 columns, top four
  apps, same layout.
- **Detail: exact session times.** The main view aggregates; the detail view
  tells the truth about short sessions.
- Use a minimum threshold (for example one minute in a slot) before marking a
  slot active, so a single stray sample does not create a full block.

### Short and fragmented usage

A 30-minute block answers "was there activity?" It does **not** answer "how many
times?" A solid block must not imply that the app was used continuously.

The right split is:

- **Capture finely:** record raw focus samples or events at a short interval
  (for example 1–5 seconds). If sub-five-second toggles matter, prefer
  event-driven focus-change events over polling. This is a data-layer decision,
  not a display decision.
- **Merge into sessions:** consecutive samples of the same app form a session;
  a short gap threshold (for example 30–60 seconds) prevents micro-flicker from
  creating hundreds of rows.
- **Group into bursts for reading:** sessions separated by a short gap (for
  example under two minutes) can be displayed as one burst, with the number of
  sessions and total duration inside it.
- **Keep the main view binary:** the 30-minute strip shows presence, not
  fragmentation. The exact behavior lives in detail.
- **Show fragmentation in detail:** session count, median session length,
  switches per hour, longest session, and a burst/session list.
- **Optional main-screen cue:** a small `×N` after the duration, meaning
  `N sessions`; use it only if testing shows it helps. Do not overload the
  timeline with a second fill pattern unless fragmentation is the primary
  question.

If short on/off use is the dominant behavior, a 15-minute main grid still hides
micro-sessions and costs visibility. A session list at 1–5 second capture
resolution is the better detail, not a denser main chart.

**Can detail be added later?** The UI level can wait; the data level cannot. If
raw samples or events are recorded from day one, a detailed view can be added
later without rewriting history. If only daily totals are stored, short on/off
usage is lost permanently. Separate **capture resolution** (fine, always on)
from **display resolution** (30-minute main screen, exact detail).

### Device power on/off

Device on/off is a different axis from app usage. Do not model it as an app
session.

- **POWER row:** a thin 24-hour row above the app rows. `▓▓` means powered on;
  blank means powered off. It answers "when was the device on?" directly.
- **Two totals in the header:** `active` (input-active time) and `powered`
  (wall-clock time the device was on). A device can be powered on but idle, so
  these are not the same number.
- **Power sessions are on/off only:** boot → shutdown. Suspend, lock, and
  display-off are explicitly not tracked, so there is no suspend state and no
  separate screen-on metric.
- **Shutdown cannot be recorded while the device is off.** Reconstruct it from
  the next boot: use boot IDs and the last known sample, or systemd/journal
  boot and shutdown records. Mark abrupt power loss as `unknown`, not as a clean
  session.
- **Idle remains the only guard for inactive time.** Without lock/display
  tracking, a locked or display-off period can still count as active until the
  idle threshold closes the interval. The UI must not claim screen-on accuracy.
- **Detail view:** on/off sessions with start, end, duration, and reason
  (`shutdown`, `unclean`, `unknown`). This is where repeated power-on/off
  behavior becomes visible.
- **Do not put app session counts and power session counts in the same `×N`
  column** unless the label makes the difference explicit.

### Visual rules

- Wide, short window: roughly 92–96% width and 50–60% height. The timeline needs
  horizontal space; this should not look like a tall analytics page.
- **Opaque background and full-contrast primary text.** Do not use transparency,
  alpha, or faint colors for totals, app names, durations, or active blocks.
- One accent color for all active blocks. No per-app palette, no category
  palette, no success/warning/error colors.
- Monospace for the axis, durations, and strips.
- **Larger type:** target a body around 12pt and a total around 16–18pt; the
  exact values depend on the Rofi theme. Prefer a larger window over shrinking
  the type.
- Labels in sentence case; durations as `2h 14m`.
- Empty slots are blank; active slots are solid. Do not use a faint dot grid
  that competes with the data.
- Optional current-time marker: one solid vertical mark; no pulsing or blinking.
- No borders around every section. Use spacing and one separator at most.
- No icons required. App names are sufficient and avoid icon-theme failure
  modes.
- **No animation.** No spinners, progress bars, transitions, hover motion, or
  animated chart reveals. The popup appears fully rendered.

## 7. Navigation model

### Zero visible navigation buttons

| Key | Action | Notes |
|---|---|---|
| `←` / `→` | Previous / next day | Clamp at today; no future days |
| `↑` / `↓` | Select an app row | Selection does not change the layout |
| `Enter` | Open selected app detail | One level only |
| `Esc` | Close, or return from detail | Consistent cancel/back |
| `Backspace` | Return from detail | Secondary back key |
| `T` | Jump to today | Useful after browsing history |
| `R` | Refresh | Re-read the day |
| `P` | Open power detail | Optional; only if power sessions are enabled |
| Mouse click | Open app detail | Optional; keyboard remains primary |
| Click outside | Close | Default Rofi behavior |

Footer copy:

```text
←/→ day   T today   R refresh   ↵ detail   esc close
```

### Why remove buttons?

- Day navigation, refresh, and close are not equal-weight tasks; a row of
  buttons gives them equal visual weight.
- A button row consumes vertical space that the timeline needs.
- Rofi is keyboard-first; arrow keys and `Esc` are already part of the
  interaction model.
- The footer keeps the keymap discoverable without adding chrome.

### If discoverability fails

Keep two small header arrows `‹` and `›` only. Do not restore a full navigation
row. `T`, `R`, and `Esc` remain keyboard actions.

## 8. App detail

One optional level, opened with `Enter` on an app row.

```text
 Code                            2h 14m · 33% of today
 18 sessions · median 3m · 8 switches/h

 Bursts
 09:12–09:18   4×   6m
 09:30–09:34   2×   4m
 10:02–10:18   3×   9m
 13:05–13:12   2×   4m
 14:02–14:20   3×  18m
 + 4 longer sessions   1h 33m

 7d ▁▂▃▅▆▄█   avg 1h 48m
 ←/→ day   esc back
```

- The main screen answers "when did this app run?" spatially.
- The detail answers "what exactly happened?" chronologically, including short
  on/off bursts.
- A main-screen block is a presence signal; it must not be read as continuous
  use. The detail supplies the session count and median duration.
- No deeper levels, no project assignment, no editing.
- If session data is unavailable, the detail can show first/last and a 7-day
  trend, but the burst/session list is the preferred design.

## 8b. Device power detail (optional)

If repeated power on/off is a primary question, bind `P` to a separate power
detail. Do not mix it with app sessions.

```text
 POWER                                     12h 40m powered
 06:30–09:12   2h 42m   on
 09:12–09:18      6m    off
 09:18–19:16   9h 58m   on
 19:16–…               off / unknown

 ←/→ day   esc back
```

- Show start, end, duration, and reason: `shutdown`, `unclean`, `unknown`.
- The main POWER row is enough for most reads; this detail is for auditing
  repeated off/on behavior.
- `P` is optional and does not appear in the default footer unless the feature
  is enabled.

## 9. Content and states

### Copy

| Use | Avoid |
|---|---|
| `6h 48m active` | `6h 48m 12s` |
| `powered 12h 40m` | `on time` without a definition |
| `▲ 38m vs usual at this time` | `+38m` without context |
| `peak 09:00–10:15` | `densest window` |
| `+3 apps` | `Other` / `Mixed` |
| `18 sessions` | bare `18×` without a label |
| `No activity today` | `No data` |
| `Last sample 14:02` | `2915 samples · 5s interval` |
| `Not enough history` | `—` / `null` |

### States

- **No activity:** a calm line, not an empty grid:
  `No activity recorded today`.
- **Device off:** the POWER row shows a gap; app rows are empty. Do not render
  off time as idle app time.
- **Partial day:** future slots stay empty; the current time is marked or the
  data simply stops.
- **Stale data:** one inline note: `Last sample 14:02 · tracker may be paused`.
- **Many apps:** top five + other; other row shows count and total.
- **Long app names:** truncate at the label width; full name in detail.
- **Narrow window:** 30-minute slots at one character wide, four app rows, keep
  header and footer.
- **No history:** hide the delta and 7-day average; show only the day.

## 10. Data requirements

The UI cannot be built from daily totals alone. A timeline requires timestamped
app usage.

### Minimum data

- Timestamped raw samples or events: timestamp, app identifier, display name,
  and duration/end.
- Device state: active and idle. Suspend, lock, and display-off are not tracked.
- Power events: boot ID, boot time, clean shutdown time. Needed for the POWER
  row and device on/off sessions.
- Enough history for the 7-day trend and same-time comparison (optional).
- Fine capture resolution (for example 1–5 seconds), preferably event-driven
  focus changes, so short on/off usage can be reconstructed later. Do not store
  only daily totals.
- Retention policy: keep raw events long enough for the detail view; older days
  may be rolled up into sessions and slots.

### Derived data

- **Sessions:** merge consecutive samples of the same app; split on a gap
  threshold (for example 30–60 seconds for raw sessions, 1–2 minutes for a
  readable session list). Keep the threshold explicit.
- **Bursts:** group sessions separated by a short gap (for example under two
  minutes) into one display row with a session count and total duration.
- **Fragmentation:** session count, median session length, longest session, and
  switches per hour. Show these in app detail, not on the main timeline.
- **Power sessions:** pair boot/shutdown events into on/off intervals. Infer an
  off gap from the last sample and the next boot; mark unclean shutdowns as
  unknown.
- **30-minute slots:** per app, for the timeline strip; keep finer raw sessions
  for detail.
- **Daily totals:** device total and per-app totals. Total is the sum of active
  time, not the sum of session spans; do not double-count gaps.
- **Active span:** first to last active time.
- **Peak:** busiest 30–90 minute window.
- **7-day totals:** one value per day for the trend.
- **Same-time baseline:** cumulative total at the current time over the previous
  seven days. Hide if fewer than three days exist.

### Resource cost of session and burst thresholds

The gap thresholds themselves are cheap. They are timestamp comparisons, not
extra tracking. The real cost is **how often you capture and how you store** the
raw data.

| Capture model | Approx. events/day | Cost profile |
|---|---:|---|
| 5-second polling | ~17,000 | Moderate; misses sub-5s toggles |
| 1-second polling | ~86,000 | High I/O if rewriting a full day file each sample; catches short toggles |
| Event-driven focus changes | Hundreds to low thousands | Lowest I/O; best for short on/off; requires an event source |
| Boot/shutdown events | Tens | Negligible |

- **Do not rewrite a whole day file on every sample.** Append raw events, or use
  a small database, then aggregate incrementally.
- **Compute sessions and bursts on write or in a cached daily aggregate.** If
  every popup open re-sorts tens of thousands of raw events with `jq`/shell, the
  open becomes slow.
- **The `×N` count is cheap** once sessions exist: it is a count per app, not a
  second pass over raw samples.
- **Changing a gap threshold later is cheap** if raw events are retained: re-derive
  sessions; no new capture is needed.
- **Power events are tiny.** Device on/off tracking is not the resource problem;
  high-frequency app polling is.
- **Rendering is cheap.** Five app rows and a static strip cost far less than
  data collection; no animation means no continuous CPU.

### Measurement definition

Choose one and label it consistently:

- **Active time:** only while the user is actively using an app; excludes idle.
  Best match for "how long did I use the device?". Lock/suspend/display-off are
  not tracked, so this is not a screen-on metric.
- **Powered-on time:** wall-clock time the device is on, including idle. This is
  the POWER row; it is not the same as active time.
- **Session span:** first to last activity; overstates usage and should not be
  the headline.

**Recommendation:** show both `active` and `powered` in the header with clear
labels. Do not claim screen-on accuracy; it is not measured.

### Measurement caveats

- Idle threshold: short enough to exclude breaks, long enough not to drop
  reading or video watching.
- Media playback without input can look idle; decide whether media apps count.
- Lock, suspend, and display-off are not tracked. A locked or display-off
  period can still count as active until the idle threshold closes the interval.
  Do not claim screen-on accuracy.
- Short on/off bursts need an explicit merge/debounce rule; otherwise the
  session count becomes noise. Record raw events, but report a merged session
  and burst count.
- If the device itself is turned on/off repeatedly, rely on boot/shutdown
  records; app focus alone cannot distinguish a short glance from a long
  session.
- When the device is off, nothing can be sampled. Reconstruct off gaps from boot
  IDs and the last known sample; do not assume the gap was idle or active.
- Clean shutdown, abrupt power loss, and unknown off gaps are the only power
  reasons. Do not invent suspend/display states that are not tracked.
- Clock jumps, timezone changes, and NTP corrections can corrupt durations; use
  monotonic time for duration and wall-clock time for display.
- Day boundary is local midnight; late-night usage belongs to that calendar day.
- Historical comparisons require the same definition across days.

## 11. Rofi feasibility

### Constraints

- Rofi renders text, Pango markup, icons, and theme boxes; there is no native
  chart or canvas.
- The timeline must be constructed from monospace glyphs (`██`, `▒▒`).
- Width is character-based; 96 columns plus label and duration is roughly 120
  characters per row.
- Height is limited; five app rows plus header, ruler, and footer is the target.
- Pango supports foreground color, weight, and size; it does not provide
  proportional layout.
- Rofi is a static popup: the design should not depend on transitions, live
  redraw, or animation. See `docs/research/ROFI_RESOURCE_BENCHMARK.md` for the
  measured comparison against GTK4 and ActivityWatch.

### Feasible design

- Header, ruler, and footer as textboxes.
- App rows as a single-column listview, each row one line, markup enabled.
- Custom keybindings for day navigation and refresh.
- Up/Down for selection; Enter for detail; Esc for close/back.
- A second invocation or a script-mode state for detail; one level only.
- No dynamic highlight of the selected app across the timeline in the first
  version. Selection only affects the row; the shared timeline remains static.
  This avoids a dependency on live redraw.

### Static rendering and resource budget

The popup is a snapshot. It should behave like a printed card.

- **Render once per open.** No animation loop, no timer, no polling while the
  window is visible. Read a small tmpfs live snapshot; do not read the raw event
  store on click. See `docs/research/SCREEN_TIME_STORAGE_ARCHITECTURE.md`.
- **No live updates.** Data refreshes on reopen or on an explicit `R`; the
  screen does not tick while the user reads it.
- **No spinner or progress animation.** If the snapshot is not ready, show the
  previous cached snapshot or a static message; never animate the wait.
- **No selection-driven redraw.** Moving Up/Down highlights the row only; the
  timeline is not re-rendered.
- **No transparency or compositor effects required.** Use an opaque background
  and solid colors.
- **Idle CPU target:** effectively zero while the popup is open; no background
  work after the first paint.
- **Open-time target:** warm open under 100ms; cold open under 1s if possible.
  If cold rendering is slower, cache the rendered snapshot rather than showing
  motion.

### Technical spikes before development

1. Measure 96-column row width at 1280×800 with the target 12pt font. If it
   wraps, fall back to 48 columns.
2. Verify contrast and legibility of the total, app labels, durations, and
   active blocks on an opaque background.
3. Confirm custom-key behavior with a disabled input bar and a selectable list.
4. Confirm one-line row height and vertical fit for five rows plus header.
5. Confirm Enter/Esc detail-view behavior and return path.
6. Confirm that the popup performs no periodic work while open.
7. Measure warm and cold open time; cache the snapshot if needed.

## 12. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Timeline is too dense | High | 30-minute/two-character blocks, five rows, blank inactive cells, one accent |
| Short sessions disappear | Medium | Detail view with burst/session list; 30-minute granularity |
| Sub-sampling misses very short toggles | High | Event-driven focus changes, or 1-second sampling; never only 5-second polling |
| Fragmented use looks continuous | High | Main block is presence only; detail shows session count, median, switches/hour; optional `×N` cue |
| Power-off is inferred incorrectly | High | Boot IDs, last-sample timestamp, clean shutdown records; mark unclean gaps as unknown |
| No lock/suspend/display tracking overcounts | Medium | Idle timeout is the only guard; label the metric active, not screen-on |
| App sessions confused with power sessions | Medium | Separate `SESS` and `PWR` labels; never share a bare `×N` |
| `×N` is misread as minutes | Medium | Use a `SESS` column or explicit `N sessions` label; test before shipping |
| `×N` adds clutter and breaks width | Medium | Default off; test at 1280×800 with 2–3 digit counts; cap at `99+` |
| Five rows do not fit | Medium | 30-minute/48-column fallback; four rows |
| Color-only identification | High | Rows are labeled; color is decorative, not the only mapping |
| Color palette becomes noise | Medium | Single accent; no category colors |
| Low contrast or thin glyphs | High | 12pt body, 16–18pt total, solid blocks, opaque background; test at 1280×800 |
| Static snapshot goes stale | Medium | Refresh on reopen or `R`; show `Last sample` only when it matters |
| "Other" row is ambiguous | Low | Show count and total; neutral strip |
| Total is untrustworthy | High | Define active vs screen-on; stale cue; validate measurement |
| Hotkeys are undiscoverable | Medium | Persistent footer; optional header arrows |
| Data has totals but no timestamps | High | Treat timestamped data as a hard prerequisite |
| 7-day/compare adds clutter | Medium | Keep it to one secondary line; hide when unavailable |

## 13. Validation plan

### Task test

Use a static wireframe or throwaway Rofi screen with six tasks:

1. "How long was the device used today?"
2. "Which app used the most time?"
3. "When was Firefox used?"
4. "When was the busiest period?"
5. "Show yesterday, then come back to today."
6. "Show the exact sessions for Code."

Measure time to answer, key presses, errors, and whether the footer hints were
noticed.

### Layout test

- 1280×800, 1440×900, 1920×1080.
- Body fonts around 12–13pt; totals around 16–18pt. Test 10–11pt only as a
  fallback, not as the target.
- Five vs seven app rows.
- 30-minute slots rendered two characters wide vs 15-minute slots rendered one
  character wide.
- Contrast of active blocks, labels, and durations on an opaque background.
- Long app names and an "other" row with activity.
- Verify that the popup performs no periodic work while open.

### Data test

- Verify that timestamped raw sessions exist.
- Simulate short on/off use: for example ten 20–40 second sessions inside one
  30-minute slot, plus sub-five-second switches. Verify total, session count,
  burst grouping, and that gaps are not double-counted.
- Compare event-driven capture with 1-second and 5-second polling; verify that
  very short toggles are not lost.
- Verify idle and media behavior. Lock/suspend/display-off are out of scope.
- Simulate clean shutdown and abrupt power loss. Verify the POWER row, powered
  duration, and that off time is not counted as active.
- Compare one manually observed hour with the rendered timeline.
- Verify day-boundary behavior.

### Interaction test

- Compare main view with and without `×N`/`SESS` at 1280×800.
- Check that readers interpret `×N` as sessions, not minutes.
- Check that `POWER` is read as device on/off, not app activity.

### Comprehension test

Ask the reader to point to:

- the busiest app;
- the app used at a specific time;
- the busiest period;
- the total device time.

If any answer requires reading a legend or opening detail, the main screen is
not yet good enough.

## 14. Recommendation and decisions

### Recommendation

Build **Concept A: swimlane day timeline**.

- One wide, short, opaque screen.
- Header: date, total active time, powered-on time, active span, peak, 7-day
  trend.
- Body: a `POWER` row, five app rows + other, each with duration and a 96-column
  timeline (30-minute slots, two characters wide).
- Footer: key hints; no navigation buttons.
- Detail: one app, burst/session list, fragmentation metrics, power sessions,
  7-day trend.
- Static snapshot: no animation, no live update, no periodic work while open.

### Decisions needed

1. **Metrics:** which are headline numbers — active time, powered-on time, or
   screen-on time?
2. **Granularity:** 30-minute slots rendered two characters wide (recommended
   for visibility) or 15-minute slots rendered one character wide?
3. **Rows:** top five + other (recommended) or top seven?
4. **Color:** single accent (recommended) or one color per app?
5. **Detail:** include in v1, or ship the main screen first?
6. **Fragmentation cue:** run the `×N`/`SESS` experiment; default off until it
   wins.
7. **Session/burst thresholds:** what gap counts as the same session, and what
   gap groups sessions into a burst?
8. **Power states:** decided — on/off only; suspend/lock/display-off are not
   tracked.
9. **Comparison:** include the same-time delta and 7-day average in v1, or keep
   only the total and trend?
10. **Type size:** body 12pt vs larger; how much of the window may be sacrificed
    for legibility?
11. **Refresh:** refresh only on reopen, or also on explicit `R`?
12. **Language:** English UI labels or Vietnamese?
13. **Sequence:** is a swimlane enough, or is an exact chronological list the
    primary requirement? If exact sequence is primary, make the session list the
    main view and the swimlane the detail.

## 15. Sources

Accessed 2026-11-21 unless noted.

- ManicTime documentation, "Day view" (computer usage timeline, application
  timeline, summary by application):
  <https://docs.manictime.com/win-client/overview>
- Timing, "The Activities Screen" (day timeline + activity list ordered by
  duration):
  <https://timingapp.com/help/activities>
- ActivityWatch v0.14.0 release notes (timeline swimlanes, merge by app, AFK):
  <https://activitywatch.net/blog/activitywatch-v0-14-0>
- Apple Support, "Track app and device usage in Screen Time on Mac":
  <https://support.apple.com/guide/mac-help/track-app-and-device-usage-mchlf2c0c770/mac>
- Google Support, "Manage how you spend time on your Android phone with Digital
  Wellbeing":
  <https://support.google.com/android/answer/9346420>
- RescueTime Support, "Your Dashboard":
  <https://help.rescuetime.com/article/30-dashboard>
- UX Patterns for Developers, "Timeline" (axis, event marker, labels, density
  and accessibility guidance):
  <https://uxpatterns.dev/patterns/data-display/timeline>
- Nielsen Norman Group, "Progressive Disclosure":
  <https://www.nngroup.com/articles/progressive-disclosure>
- Nielsen Norman Group, "Dashboards: Making Charts and Graphs Easier to
  Understand":
  <https://www.nngroup.com/articles/dashboards-preattentive>
- Rofi 2.0.0 manual pages: `rofi(1)`, `rofi-dmenu(5)`, `rofi-script(5)`,
  `rofi-keys(5)`, `rofi-theme(5)` (local platform documentation).
