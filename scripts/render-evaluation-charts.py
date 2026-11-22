#!/usr/bin/env python3
"""Generate the comparison charts embedded in docs/EVALUATION.md.

The charts are plain SVG, generated from the machine-readable benchmark files
with the Python standard library. Run with --check in CI to detect stale charts.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BENCHMARKS = ROOT / "docs" / "benchmarks"
OUT = ROOT / "docs" / "charts"

FONT = (
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, "
    "Arial, sans-serif"
)
MONO = "ui-monospace, SFMono-Regular, Menlo, Consolas, 'Liberation Mono', monospace"
BG = "#0b1220"
PANEL = "#111a2b"
BORDER = "#243044"
GRID = "#1d2939"
TEXT = "#e6edf3"
MUTED = "#8b98a9"
ACCENT = "#58a6ff"
ORANGE = "#f0883e"
PURPLE = "#bc8cff"

NAMES = {
    "screen_time": "Screen Time",
    "activitywatch": "ActivityWatch",
    "arbtt": "arbtt",
    "screen_time_logger": "ScreenTimeLogger",
    "niri_screen_time": "Niri Screen Time",
    "mono": "Mono",
    "toki": "Toki",
    "atracker": "atracker",
    "hpr": "HPR",
    "screentime_qt": "Screentime (Qt)",
}

# Trackers with a foreground UI. The measured UI is closed for every row in
# the background charts; CLI-only tools are kept in a separate reference table.
UI_KEYS = (
    "screen_time",
    "activitywatch",
    "mono",
    "toki",
    "atracker",
    "hpr",
    "screentime_qt",
)
UI_SHORT_KEYS = ("screen_time", "activitywatch", "mono", "toki", "atracker")


def esc(value: object) -> str:
    return (
        str(value)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def text(
    x: float,
    y: float,
    value: object,
    size: int = 14,
    fill: str = TEXT,
    weight: str = "400",
    anchor: str = "start",
    mono: bool = False,
) -> str:
    family = MONO if mono else FONT
    return (
        f'<text x="{x:.1f}" y="{y:.1f}" fill="{fill}" font-size="{size}" '
        f'font-family="{family}" font-weight="{weight}" text-anchor="{anchor}" '
        f'font-variant-numeric="tabular-nums">{esc(value)}</text>'
    )


def label(
    x: float,
    y: float,
    value: object,
    size: int = 13,
    fill: str = TEXT,
    weight: str = "400",
    anchor: str = "start",
) -> str:
    return (
        f'<text x="{x:.1f}" y="{y:.1f}" fill="{fill}" font-size="{size}" '
        f'font-family="{FONT}" font-weight="{weight}" text-anchor="{anchor}" '
        f'paint-order="stroke" stroke="{BG}" stroke-width="4" '
        f'stroke-linejoin="round">{esc(value)}</text>'
    )


def svg(width: int, height: int, title: str, desc: str, body: str) -> str:
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="chart-title chart-desc">
<title id="chart-title">{esc(title)}</title>
<desc id="chart-desc">{esc(desc)}</desc>
<rect width="{width}" height="{height}" fill="{BG}"/>
{body}
</svg>
"""


def header(title: str, subtitle: str, width: int, note: str = "") -> str:
    parts = [
        text(40, 50, title, 27, TEXT, "700"),
        text(40, 80, subtitle, 14, MUTED),
    ]
    if note:
        parts.append(text(width - 40, 50, note, 12, MUTED, anchor="end"))
    return "".join(parts)


def footer(value: str, width: int, height: int) -> str:
    return text(40, height - 22, value, 13, MUTED)


def read_medians(name: str) -> tuple[dict[str, dict[str, float]], dict]:
    data = json.loads((BENCHMARKS / name).read_text())
    samples = data["tracker_samples"]
    out: dict[str, dict[str, float]] = {}
    for key, display in NAMES.items():
        if key not in samples[0]:
            continue
        rows = [sample[key] for sample in samples]
        out[key] = {
            "name": display,
            "pss": statistics.median(row["pss_kib"] for row in rows) / 1024,
            "processes": statistics.median(row["processes"] for row in rows),
            "cpu": statistics.median(row["cpu_pct_one_core"] for row in rows),
            "writes": statistics.median(row["write_bytes"] for row in rows) / 1024,
            "apps": statistics.median(
                row.get("unique_fixture_apps", 10) for row in rows
            ),
        }
    return out, data


def score_rows(rows: dict[str, dict[str, float]]) -> list[dict[str, float]]:
    min_pss = min(row["pss"] for row in rows.values())
    min_processes = min(row["processes"] for row in rows.values())
    for row in rows.values():
        row["score"] = 100 * (
            0.75 * min_pss / row["pss"] + 0.25 * min_processes / row["processes"]
        )
    return sorted(rows.values(), key=lambda row: row["score"], reverse=True)


def chart_footprint() -> str:
    rows = score_rows(
        {key: row for key, row in read_medians("linux-ten-trackers.json")[0].items() if key in UI_KEYS}
    )
    width, height = 1200, 540
    label_x = 190
    top = 180
    row_h = 40
    panels = [
        ("PSS (MiB)", "lower is better", 0.0, 85.0, lambda r: r["pss"], 205, 200, 2),
        ("Switching CPU", "% of one core", 0.0, 55.0, lambda r: r["cpu"], 455, 290, 2),
        ("Writes", "KiB, log scale", 0.0, 1.0, lambda r: r["writes"], 770, 370, 0),
    ]
    body = [
        header(
            "Background footprint",
            "UI-capable cohort, UI closed; median of 3 trials on the six-second workload.",
            width,
            "lower is better",
        )
    ]
    for index, row in enumerate(rows):
        y = top + index * row_h
        if row["name"] == "Screen Time":
            body.append(
                f'<rect x="18" y="{y - 8}" width="{width - 36}" height="{row_h - 4}" '
                f'rx="6" fill="{ACCENT}" opacity="0.08"/>'
            )
            body.append(
                f'<rect x="18" y="{y - 8}" width="3" height="{row_h - 4}" '
                f'fill="{ACCENT}"/>'
            )
        body.append(
            f'<line x1="24" y1="{y + row_h - 8}" x2="{width - 24}" y2="{y + row_h - 8}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
    for title, subtitle, _, _, _, x, _, _ in panels:
        body.append(text(x + 6, 122, title, 17, TEXT, "700"))
        body.append(text(x + 6, 144, subtitle, 13, MUTED))
        body.append(
            f'<line x1="{x + 4}" y1="{top - 16}" x2="{x + 4}" y2="{top + row_h * len(rows) - 6}" '
            f'stroke="{BORDER}" stroke-width="1"/>'
        )
    for index, row in enumerate(rows):
        y = top + index * row_h
        body.append(label(label_x, y + 17, row["name"], 15, TEXT, "600", "end"))
        for panel_index, (_, _, vmin, vmax, getter, bar_x, bar_w, decimals) in enumerate(panels):
            value = getter(row)
            if panel_index == 2:
                ratio = (math.log10(max(value, 10.0)) - 1.0) / (
                    math.log10(2200.0) - 1.0
                )
            else:
                ratio = (value - vmin) / (vmax - vmin)
            ratio = max(0.0, min(1.0, ratio))
            actual_w = max(2.0, bar_w * ratio)
            if panel_index == 1:
                color = ORANGE if value >= 5 else (ACCENT if row["name"] == "Screen Time" else "#64748b")
            elif panel_index == 2:
                color = ORANGE if value >= 1000 else (ACCENT if row["name"] == "Screen Time" else "#64748b")
            else:
                color = ACCENT if row["name"] == "Screen Time" else "#64748b"
            shown = f"{value:.{decimals}f}"
            body.append(
                f'<rect x="{bar_x}" y="{y + 1}" width="{actual_w:.1f}" height="20" '
                f'rx="4" fill="{color}"/>'
            )
            body.append(label(bar_x + actual_w + 8, y + 17, shown, 14, TEXT, "500"))
    for value in (0, 20, 40, 60, 80):
        x = 205 + 200 * value / 85
        body.append(text(x, 500, value, 11, MUTED, anchor="middle"))
    for value in (0, 10, 20, 30, 40, 50):
        x = 455 + 290 * value / 55
        body.append(text(x, 500, value, 11, MUTED, anchor="middle"))
    for value in (10, 100, 1000):
        x = 770 + 370 * ((math.log10(value) - 1) / (math.log10(2200) - 1))
        body.append(text(x, 500, value, 11, MUTED, anchor="middle"))
    body.append(
        footer(
            "Blue = Screen Time; orange = a high-cost value. Tracker process only; the UI is closed "
            "for every row. Writes exclude startup and shutdown.",
            width,
            height,
        )
    )
    return svg(
        width,
        height,
        "Background footprint of UI-capable Linux trackers",
        "Horizontal bar chart comparing median PSS, switching CPU, and disk writes. "
        "Screen Time uses 4.48 MiB PSS, 0.00 percent CPU, and 80 KiB writes.",
        "".join(body),
    )


def chart_fast_switch() -> str:
    rows = {
        key: row
        for key, row in read_medians("linux-expanded-short.json")[0].items()
        if key in UI_SHORT_KEYS
    }
    ordered = sorted(
        rows.values(), key=lambda row: row["apps"], reverse=True
    )
    width, height = 1080, 400
    top = 145
    row_h = 40
    bar_x = 270
    bar_w = 720
    body = [
        header(
            "Fast-switch coverage",
            "UI-capable cohort; distinct apps retained in the 1.5-second workload.",
            width,
            "higher is better",
        )
    ]
    for index, row in enumerate(ordered):
        y = top + index * row_h
        if row["name"] == "Screen Time":
            body.append(
                f'<rect x="24" y="{y - 8}" width="{width - 48}" height="{row_h - 4}" '
                f'rx="6" fill="{ACCENT}" opacity="0.08"/>'
            )
        body.append(
            f'<line x1="24" y1="{y + row_h - 8}" x2="{width - 24}" y2="{y + row_h - 8}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
        body.append(label(bar_x - 14, y + 18, row["name"], 15, TEXT, "600", "end"))
        apps = row["apps"]
        actual_w = max(2.0, bar_w * apps / 10)
        color = ACCENT if row["name"] == "Screen Time" else "#64748b"
        body.append(
            f'<rect x="{bar_x}" y="{y + 1}" width="{actual_w:.1f}" height="20" '
            f'rx="4" fill="{color}"/>'
        )
        shown = "2–3" if row["name"] == "atracker" else f"{apps:.0f}"
        body.append(label(bar_x + actual_w + 8, y + 18, f"{shown} / 10", 14, TEXT, "600"))
    body.append(
        footer(
            "UI-capable cohort, UI closed. 10 fixture apps, 1.5 s each, median of 3 trials. "
            "A low result means short focuses were merged or missed.",
            width,
            height,
        )
    )
    return svg(
        width,
        height,
        "Fast-switch coverage for UI-capable Linux trackers",
        "Bar chart of distinct apps retained. Screen Time and arbtt retained 10 of 10; "
        "Toki retained 1 of 10.",
        "".join(body),
    )


def chart_memory_coverage() -> str:
    rows = {
        key: row
        for key, row in read_medians("linux-expanded-short.json")[0].items()
        if key in UI_SHORT_KEYS
    }
    width, height = 1100, 690
    left, right = 104, 1040
    top, bottom = 170, 560
    xmin, xmax = 3.5, 100.0
    ymin, ymax = 0.0, 10.5

    def xpos(value: float) -> float:
        return left + (math.log10(value) - math.log10(xmin)) / (
            math.log10(xmax) - math.log10(xmin)
        ) * (right - left)

    def ypos(value: float) -> float:
        return bottom - (value - ymin) / (ymax - ymin) * (bottom - top)

    body = [
        header(
            "Memory vs. capture completeness",
            "UI-capable cohort, UI closed. Blue = Screen Time; gray = other trackers. The useful corner is top-left.",
            width,
            "PSS: lower is better",
        ),
        f'<rect x="{left}" y="{top}" width="{right - left}" height="{bottom - top}" '
        f'fill="{PANEL}" stroke="{BORDER}" rx="10"/>',
    ]
    for value in (4, 6, 8, 12, 16, 24, 32, 48, 64, 96):
        x = xpos(value)
        body.append(
            f'<line x1="{x:.1f}" y1="{top}" x2="{x:.1f}" y2="{bottom}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
        body.append(text(x, bottom + 24, value, 12, MUTED, anchor="middle"))
    for value in (0, 2, 4, 6, 8, 10):
        y = ypos(value)
        body.append(
            f'<line x1="{left}" y1="{y:.1f}" x2="{right}" y2="{y:.1f}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
        body.append(text(left - 16, y + 4, value, 12, MUTED, anchor="end"))
    body.append(text((left + right) / 2, bottom + 54, "PSS (MiB, log scale)", 14, MUTED, anchor="middle"))
    body.append(
        f'<text x="{left - 58}" y="{(top + bottom) / 2}" fill="{MUTED}" font-size="14" '
        f'font-family="{FONT}" text-anchor="middle" transform="rotate(-90 {left - 58} {(top + bottom) / 2})">'
        "Apps retained / 10</text>"
    )

    offsets = {
        "screen_time": (14, 22, "start"),
        "toki": (14, 5, "start"),
        "screen_time_logger": (14, -10, "start"),
        "atracker": (14, 18, "start"),
        "mono": (14, 5, "start"),
        "activitywatch": (-14, 5, "end"),
    }
    for key, row in rows.items():
        x = xpos(row["pss"])
        y = ypos(row["apps"])
        is_main = key == "screen_time"
        body.append(
            f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{10 if is_main else 6}" '
            f'fill="{ACCENT if is_main else "#94a3b8"}" '
            f'stroke="{BG}" stroke-width="2"/>'
        )
        dx, dy, anchor = offsets.get(key, (14, 5, "start"))
        shown_apps = "2–3" if key == "atracker" else f"{row['apps']:.0f}"
        value = f"{row['name']}  {row['pss']:.2f} MiB · {shown_apps}/10"
        body.append(label(x + dx, y + dy, value, 14, TEXT if is_main else MUTED, "600" if is_main else "400", anchor))

    body.append(
        footer(
            "PSS is proportional set size; coverage is distinct fixture app names retained. "
            "Median of 3 trials. Screen Time kept all 10 apps at 4.49 MiB.",
            width,
            height,
        )
    )
    return svg(
        width,
        height,
        "PSS versus fast-switch coverage",
        "Scatter chart with memory on a logarithmic x-axis and apps retained on the y-axis. "
        "Screen Time sits at 4.49 MiB and 10 of 10; Toki is smaller but retains 1 of 10.",
        "".join(body),
    )


def chart_install_size() -> str:
    values = [
        ("Screen Time package", 54.9, ACCENT),
        ("Screen Time + Rofi 2.0.0", 466.1, PURPLE),
        ("ActivityWatch", 2453.2, ORANGE),
    ]
    width, height = 1040, 380
    left, bar_x, bar_w = 40, 300, 620
    top, row_h = 140, 46
    xmin, xmax = 50.0, 3000.0
    body = [
        header(
            "Install footprint",
            "Nix closure size, including transitive dependencies. Log scale.",
            width,
            "lower is better",
        )
    ]
    for index, (name, value, color) in enumerate(values):
        y = top + index * row_h
        body.append(
            f'<line x1="24" y1="{y + row_h - 10}" x2="{width - 24}" y2="{y + row_h - 10}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
        body.append(label(bar_x - 14, y + 18, name, 14, TEXT, "600", "end"))
        ratio = (math.log10(value) - math.log10(xmin)) / (
            math.log10(xmax) - math.log10(xmin)
        )
        actual_w = max(3.0, bar_w * ratio)
        body.append(
            f'<rect x="{bar_x}" y="{y + 2}" width="{actual_w:.1f}" height="20" '
            f'rx="4" fill="{color}"/>'
        )
        body.append(label(bar_x + actual_w + 8, y + 18, f"{value:,.1f} MiB", 13, TEXT, "600"))
    for value in (50, 100, 500, 1000, 3000):
        x = bar_x + bar_w * ((math.log10(value) - math.log10(xmin)) / (math.log10(xmax) - math.log10(xmin)))
        body.append(text(x, height - 78, value, 12, MUTED, anchor="middle"))
    body.append(
        footer(
            "UI-capable install comparison. Screen Time + Rofi is the realistic desktop install; "
            "numbers are uncompressed store closures, not download sizes.",
            width,
            height,
        )
    )
    return svg(
        width,
        height,
        "Installation footprint",
        "Log-scale bar chart of Nix closure sizes: Screen Time package 54.9 MiB, "
        "Screen Time plus Rofi 466.1 MiB, ActivityWatch 2453.2 MiB.",
        "".join(body),
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the committed charts differ from generated output",
    )
    parser.add_argument("--png", action="store_true", help="also write PNG previews")
    args = parser.parse_args()

    charts = {
        "memory-coverage.svg": chart_memory_coverage(),
        "footprint.svg": chart_footprint(),
        "fast-switch.svg": chart_fast_switch(),
        "install-size.svg": chart_install_size(),
    }
    if args.check:
        stale = []
        for name, content in charts.items():
            path = OUT / name
            if not path.is_file() or path.read_text() != content:
                stale.append(name)
        if stale:
            print("stale charts: " + ", ".join(stale), file=sys.stderr)
            return 1
        print(f"{len(charts)} charts are current")
        return 0

    OUT.mkdir(parents=True, exist_ok=True)
    for name, content in charts.items():
        (OUT / name).write_text(content)
        print(f"wrote docs/charts/{name}")

    if args.png:
        import shutil
        import subprocess

        converter = shutil.which("rsvg-convert")
        for name in charts:
            png = OUT / (Path(name).stem + ".png")
            if converter:
                subprocess.run(
                    [converter, "-w", "1600", "-o", str(png), str(OUT / name)],
                    check=True,
                )
            else:
                magick = shutil.which("magick") or shutil.which("convert")
                if magick:
                    subprocess.run(
                        [magick, str(OUT / name), "-resize", "1600x", str(png)],
                        check=True,
                    )
            if png.is_file():
                print(f"wrote {png.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
