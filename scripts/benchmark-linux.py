#!/usr/bin/env python3
"""Compare Linux background trackers and measure the Screen Time popup."""

import argparse
from contextlib import ExitStack, contextmanager
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import socket
import sqlite3
from statistics import median
import subprocess
import tempfile
import threading
import time
from urllib.request import ProxyHandler, build_opener

from Xlib import X, Xatom, display

APP_NAMES = ("editor", "browser", "terminal", "mail", "chat", "music", "files",
             "reader", "video", "calendar")
HTTP = build_opener(ProxyHandler({}))
AW_URL = "http://127.0.0.1:5666/api/0"
CLK = os.sysconf("SC_CLK_TCK")


def run(*args, env=None):
    return subprocess.check_output(args, env=env, text=True, stderr=subprocess.DEVNULL).strip()


@contextmanager
def start(command, env, log):
    with log.open("wb") as output:
        process = subprocess.Popen(command, env=env, stdout=output, stderr=output,
                                   stdin=subprocess.DEVNULL, start_new_session=True)
        try:
            yield process
        except Exception:
            tail = log.read_text(errors="replace").splitlines()[-12:]
            if tail:
                print(f"{log.name}: " + "\n".join(tail), flush=True)
            raise
        finally:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)


def until(test, seconds=12, interval=0.02):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = test()
            if value:
                return value
        except (OSError, subprocess.CalledProcessError):
            pass
        time.sleep(interval)
    raise RuntimeError(f"condition timed out: {test.__name__}")


def private_env(base, parent):
    env = base.copy()
    for key, name in {
        "HOME": "home", "XDG_CONFIG_HOME": "config", "XDG_DATA_HOME": "data",
        "XDG_STATE_HOME": "state", "XDG_CACHE_HOME": "cache",
    }.items():
        path = parent / name
        path.mkdir(parents=True, mode=0o700, exist_ok=True)
        env[key] = str(path)
    return env


def metrics(group):
    pss = writes = ticks = threads = procs = 0
    for path in Path("/proc").glob("[0-9]*/stat"):
        try:
            fields = path.read_text().rsplit(")", 1)[1].split()
            if int(fields[2]) != group or fields[0] == "Z":
                continue
            memory = (path.parent / "smaps_rollup").read_text()
            io = (path.parent / "io").read_text()
        except (FileNotFoundError, ProcessLookupError):
            continue
        pss += int(re.search(r"^Pss:\s+(\d+)", memory, re.M)[1])
        writes += int(re.search(r"^write_bytes:\s+(\d+)", io, re.M)[1])
        ticks += sum(map(int, fields[11:15]))
        threads += int(fields[17])
        procs += 1
    assert procs, f"empty process group {group}"
    return {"pss_kib": pss, "processes": procs, "threads": threads,
            "write_bytes": writes, "cpu_ticks": ticks}


def api(path):
    with HTTP.open(AW_URL + path, timeout=1) as response:
        return json.load(response)


@contextmanager
def fake_hypr(directory, active, pids):
    directory.mkdir(parents=True)
    with socket.socket(socket.AF_UNIX) as events, socket.socket(socket.AF_UNIX) as commands:
        events.bind(str(directory / ".socket2.sock"))
        commands.bind(str(directory / ".socket.sock"))
        events.listen()
        commands.listen()
        commands.settimeout(0.1)
        stop = threading.Event()

        def command_server():
            while not stop.is_set():
                try:
                    conn, _ = commands.accept()
                except TimeoutError:
                    continue
                with conn:
                    conn.settimeout(1)
                    request = conn.recv(1024)
                    name, index = active
                    if b"activewindow" not in request:
                        reply = "[]"
                    elif b"j/" in request:
                        reply = json.dumps({"class": name, "initialClass": name,
                                            "title": name, "pid": pids[index], "address": f"0x{100 + index:x}"})
                    else:
                        reply = f"Window {100 + index:x}:\n\tclass: {name}\n\ttitle: {name}\n"
                    conn.sendall(reply.encode())

        thread = threading.Thread(target=command_server, daemon=True)
        thread.start()
        try:
            yield events
        finally:
            stop.set()
            thread.join(timeout=2)


def elapsed_metrics(before, after, seconds):
    return {"pss_kib": after["pss_kib"], "processes": after["processes"],
            "threads": after["threads"],
            "cpu_pct_one_core": 100 * (after["cpu_ticks"] - before["cpu_ticks"]) / CLK / seconds,
            "write_bytes": after["write_bytes"] - before["write_bytes"]}


def activate(connection, active_window):
    root = connection.screen().root
    root.change_property(connection.intern_atom("_NET_ACTIVE_WINDOW"), Xatom.WINDOW,
                         32, [active_window.id])
    active_window.set_input_focus(X.RevertToParent, X.CurrentTime)
    connection.sync()


def fixture_windows(connection, apps, pids):
    screen = connection.screen()
    root = screen.root
    windows = []
    for i, app in enumerate(apps):
        window = root.create_window(40 + 340 * (i % 5), 40 + 220 * (i // 5), 280, 150, 0, screen.root_depth,
                                    X.InputOutput, X.CopyFromParent,
                                    background_pixel=screen.white_pixel)
        window.set_wm_class(app, app)
        window.set_wm_name(app)
        window.change_property(connection.intern_atom("_NET_WM_PID"), Xatom.CARDINAL, 32, [pids[i]])
        window.map()
        windows.append(window)
    root.change_property(connection.intern_atom("_NET_CLIENT_LIST"), Xatom.WINDOW,
                         32, [window.id for window in windows])
    connection.sync()
    return windows


def recorded_apps(directory, query, apps):
    found = set()
    for path in sorted(directory.rglob("*")):
        if path.suffix not in (".db", ".sqlite", ".sqlite3"):
            continue
        try:
            with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as db:
                found.update(Path(str(row[0])).name.lower() for row in db.execute(query))
        except sqlite3.OperationalError as exc:
            if "no such table" not in str(exc):
                raise
    return sorted(set(apps) & found)


def trackers(args, base, folder, connection, windows, apps, pids):
    active = [apps[0], 0]
    st_env = private_env(base, folder / "st")
    aw_env = private_env(base, folder / "aw")
    st_runtime = Path(base["XDG_RUNTIME_DIR"]) / folder.name
    st_runtime.mkdir()
    st_env.update(SCREEN_TIME_STATE_DIR=st_env["XDG_STATE_HOME"],
                  SCREEN_TIME_RUNTIME_DIR=str(st_runtime),
                  SCREEN_TIME_DISABLE_IDLE="1",
                  SCREEN_TIME_HYPRLAND_DIR=str(st_runtime / "hypr" / "bench"))
    database = Path(st_env["SCREEN_TIME_STATE_DIR"]) / "screen-time.db"
    rofi_database = folder / "fixture.db"
    activate(connection, windows[0])
    with ExitStack() as stack:
        fake = stack.enter_context(fake_hypr(Path(st_env["SCREEN_TIME_HYPRLAND_DIR"]), active, pids))
        st = stack.enter_context(start([args.binary, "daemon"], st_env, st_runtime / "daemon.log"))
        aw = stack.enter_context(start(["aw-qt", "--testing", "--no-gui"], aw_env, st_runtime / "aw.log"))
        all_trackers = {"screen_time": st, "activitywatch": aw}
        if args.arbtt:
            arbtt_env = private_env(base, folder / "arbtt")
            arbtt_log = folder / "arbtt.log"
            capture = stack.enter_context(start([args.arbtt, "--sample-rate", "1", "--logfile",
                                                str(arbtt_log)], arbtt_env, st_runtime / "arbtt.log"))
            all_trackers["arbtt"] = capture

        fake.settimeout(10)
        conn, _ = fake.accept()
        stack.enter_context(conn)

        logger_state = folder / "logger" / "records"
        for name, path, options in [
            ("screen_time_logger", args.logger, ["--state-dir", str(logger_state)]),
            ("niri_screen_time", args.niri, ["-daemon"]),
            ("mono", args.mono, []),
            ("toki", args.toki, ["run"]),
            ("atracker", args.atracker, []),
            ("hpr", args.hpr, []),
            ("screentime_qt", args.screentime, ["--hidden"]),
        ]:
            if not path:
                continue
            env = private_env(base, folder / name)
            env.update(XDG_RUNTIME_DIR=str(st_runtime), XDG_CURRENT_DESKTOP="Hyprland",
                       HYPRLAND_INSTANCE_SIGNATURE="bench")
            if name != "screentime_qt":
                env["WAYLAND_DISPLAY"] = "bench"
            if name == "atracker":
                config = Path(env["HOME"]) / ".config" / "atracker-rs" / "config-rs.yaml"
                config.parent.mkdir(parents=True)
                config.write_text(
                    "dashboard: {port: 0, host: 127.0.0.1}\n"
                    f'database: {{path: "{env["XDG_STATE_HOME"]}/atracker.db", retention_days: 90}}\n'
                    "tracking: {poll_interval: 5, idle_threshold: 120}\n")
            command = [path, *options]
            if name == "screen_time_logger":
                command.insert(0, shutil.which("python3"))
            all_trackers[name] = stack.enter_context(start(command, env, st_runtime / f"{name}.log"))

        fake.setblocking(False)
        listeners = [conn]

        def broadcast(current):
            while True:
                try:
                    listener, _ = fake.accept()
                except BlockingIOError:
                    break
                listeners.append(stack.enter_context(listener))
            event = f"activewindow>>{active[0]},test title\nactivewindowv2>>{100 + current:x}\n"
            for listener in list(listeners):
                try:
                    listener.sendall(event.encode())
                except BrokenPipeError:
                    listeners.remove(listener)
        bucket = until(lambda: next((b for b in api("/buckets") if b.startswith("aw-watcher-window_")), None))
        until(lambda: any(apps[0] in e["data"]["app"].lower()
                          for e in api(f"/buckets/{bucket}/events?limit=10")))
        time.sleep(2)
        idle_before = {name: metrics(proc.pid) for name, proc in all_trackers.items()}
        idle_start = time.monotonic()
        time.sleep(args.idle_seconds)
        idle_duration = time.monotonic() - idle_start
        before = {name: metrics(proc.pid) for name, proc in all_trackers.items()}
        begin = time.monotonic()
        for i in range(1, args.switches + 1):
            time.sleep(max(0, begin + args.interval * i - time.monotonic()))
            current = i % len(apps)
            activate(connection, windows[current])
            active[:] = [apps[current], current]
            broadcast(current)
        time.sleep(args.interval)
        duration = time.monotonic() - begin
        after = {name: metrics(proc.pid) for name, proc in all_trackers.items()}
        bucket_events = api(f"/buckets/{bucket}/events?limit=200")
        aw_classes = {event["data"]["app"].lower() for event in bucket_events}
        aw_seen = sorted({app for app in apps if any(app in name for name in aw_classes)})
        assert len(aw_seen) >= 2, aw_classes
        assert after["activitywatch"]["processes"] >= 4
        st.send_signal(signal.SIGTERM)
        assert st.wait(timeout=6) == 0
        with sqlite3.connect(database) as src, sqlite3.connect(rofi_database) as snapshot:
            names = {item[0].lower() for item in src.execute("select key from apps")}
            assert set(apps) <= names, names
            focus = [row[0].lower() for row in src.execute(
                "select app from events where kind='focus' order by id")]
            expected_switches = [apps[i % len(apps)] for i in range(1, args.switches + 1)]
            assert focus[-args.switches:] == expected_switches, focus
            sessions = src.execute("select count(*) from sessions").fetchone()[0]
            src.backup(snapshot)
        result = {name: elapsed_metrics(before[name], after[name], duration) for name in before}
        for name in before:
            result[name]["steady_focus"] = elapsed_metrics(idle_before[name], before[name], idle_duration)
        result["screen_time"]["sessions_after_shutdown"] = sessions
        result["screen_time"]["focus_events_at_shutdown"] = len(focus)
        result["activitywatch"]["window_events_at_end"] = len(bucket_events)
        result["screen_time"]["unique_fixture_apps"] = len(set(apps) & names)
        result["activitywatch"]["unique_fixture_apps"] = len(aw_seen)
        if args.arbtt:
            assert arbtt_log.stat().st_size > 100
            dump = run(shutil.which("arbtt-dump") or str(Path(args.arbtt).with_name("arbtt-dump")),
                       "--logfile", str(arbtt_log), "--format", "JSON", env=arbtt_env)
            active_apps = {window["program"].lower() for entry in json.loads(dump)
                           for window in entry["windows"] if window["active"]}
            assert len(set(apps) & active_apps) >= 2, active_apps
            result["arbtt"]["unique_fixture_apps"] = len(set(apps) & active_apps)
            result["arbtt"]["capture_file_bytes_at_end"] = arbtt_log.stat().st_size
        if args.logger:
            logger = all_trackers["screen_time_logger"]
            logger.send_signal(signal.SIGTERM)
            assert logger.wait(timeout=5) == 0
            entries = [json.loads(line) for path in logger_state.glob("*.jsonl")
                       for line in path.read_text().splitlines()]
            result["screen_time_logger"]["unique_fixture_apps"] = len(
                set(apps) & {entry["app"] for entry in entries})
        for name, query in {
            "niri_screen_time": "SELECT DISTINCT app_id FROM screen_time",
            "mono": "SELECT DISTINCT app_name FROM sessions",
            "toki": "SELECT DISTINCT exe_path FROM apps JOIN sessions ON sessions.app_id = apps.id",
            "atracker": "SELECT DISTINCT wm_class FROM events",
            "hpr": "SELECT DISTINCT name FROM app_usage",
            "screentime_qt": "SELECT DISTINCT app_name FROM DailyUsage",
        }.items():
            if name in all_trackers:
                if name == "screentime_qt":
                    qt_tracker = all_trackers[name]
                    qt_tracker.send_signal(signal.SIGTERM)
                    assert qt_tracker.wait(timeout=5) == 0
                found = recorded_apps(folder / name, query, apps)
                result[name]["unique_fixture_apps"] = len(found)
        for name, trial in result.items():
            if args.interval >= 6 and trial["unique_fixture_apps"] < 2:
                raise RuntimeError(f"{name} did not record the focus fixture: {trial}")
        return {"seconds": duration, "steady_focus_seconds": idle_duration, **result}, rofi_database


def popup(args, base, fixture, apps):
    renderer = []
    measurements = []
    directory = fixture.parent
    env = base | {"SCREEN_TIME_DB": str(fixture), "SCREEN_TIME_SCREEN_WIDTH": "1920",
                  "SCREEN_TIME_SCREEN_HEIGHT": "1080",
                  "ROFI_PIDFILE": str(Path(base["XDG_RUNTIME_DIR"]) / "rofi.pid")}
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")
    env = private_env(env, directory / "popup")
    for _ in range(12):
        start_time = time.perf_counter_ns()
        output = run(args.binary, "render", "--format", "rofi", "--now", now, env=env)
        elapsed = (time.perf_counter_ns() - start_time) / 1e6
        assert "APP TIMELINE" in output and any(app in output.lower() for app in apps)
        renderer.append(elapsed)

    for i in range(11):
        begin = time.perf_counter_ns()
        with start([args.launcher], env, Path(base["XDG_RUNTIME_DIR"]) / f"rofi-{i}.log") as proc:
            window = until(lambda: run("xdotool", "search", "--onlyvisible", "--class", "rofi", env=env),
                           interval=0.005).splitlines()[0]
            visible = (time.perf_counter_ns() - begin) / 1e6
            time.sleep(0.2)
            measurements.append({"visible_ms": visible, **metrics(proc.pid)})
            run("xdotool", "windowfocus", window, "key", "Escape", env=env)
            proc.wait(timeout=5)
    return {"renderer_ms": renderer[1:], "launches": measurements[1:]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="./result/bin/screen-time")
    parser.add_argument("--launcher", default="./result/bin/rofi-screen-time")
    parser.add_argument("--display", type=int, default=198)
    parser.add_argument("--apps", type=int, choices=(3, 10), default=3)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--switches", type=int, default=22)
    parser.add_argument("--interval", type=float, default=3)
    parser.add_argument("--idle-seconds", type=float, default=10)
    parser.add_argument("--arbtt", help="Absolute path to arbtt-capture (optional)")
    parser.add_argument("--logger")
    parser.add_argument("--niri")
    parser.add_argument("--mono")
    parser.add_argument("--toki")
    parser.add_argument("--atracker")
    parser.add_argument("--hpr")
    parser.add_argument("--screentime")
    parser.add_argument("--skip-ui", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    apps = tuple(f"bench-{name}" for name in APP_NAMES[:args.apps])
    assert args.switches >= args.apps - 1
    assert args.display >= 100 and args.repeats > 0 and args.switches >= 2
    assert args.interval > 0 and args.idle_seconds > 0
    assert not Path(f"/tmp/.X{args.display}-lock").exists()
    assert not Path(f"/tmp/.X11-unix/X{args.display}").exists()
    with socket.socket() as port:
        assert port.connect_ex(("127.0.0.1", 5666)) != 0, "ActivityWatch test port is already in use"

    Path("target").mkdir(exist_ok=True)
    with (tempfile.TemporaryDirectory(prefix="screen-time-linux-") as runtime,
          tempfile.TemporaryDirectory(prefix="bench-", dir="target") as state,
          ExitStack() as stack):
        runtime, state = Path(runtime), Path(state).resolve()
        base = {key: os.environ[key] for key in ("PATH", "NIX_SSL_CERT_FILE") if key in os.environ}
        base.update(DISPLAY=f":{args.display}", XDG_RUNTIME_DIR=str(runtime), TZ="UTC",
                    GDK_BACKEND="x11", QT_QPA_PLATFORM="xcb", XDG_SESSION_TYPE="x11")
        desktop = private_env(base, state / "desktop")
        xvfb = stack.enter_context(start(["Xvfb", base["DISPLAY"], "-screen", "0",
                                          "1920x1080x24", "-nolisten", "tcp"], desktop,
                                         runtime / "xvfb.log"))
        until(lambda: xvfb.poll() is None and Path(f"/tmp/.X11-unix/X{args.display}").exists())
        connection = display.Display(base["DISPLAY"])
        stack.callback(connection.close)
        pids = []
        for app in apps:
            executable = state / app
            shutil.copy2(shutil.which("bash"), executable)
            sleeper = stack.enter_context(start([str(executable), "--noprofile", "--norc",
                                                 "-c", "sleep 900 & wait"],
                                                desktop, runtime / f"{app}.log"))
            assert sleeper.poll() is None
            pids.append(sleeper.pid)
        windows = fixture_windows(connection, apps, pids)
        samples = []
        for i in range(args.repeats):
            series, fixture = trackers(args, base, state / f"trial-{i}", connection, windows, apps, pids)
            samples.append(series)
            print(f"trial {i + 1}: {json.dumps(series)}", flush=True)
        ui = None if args.skip_ui else popup(args, base, fixture, apps)
        result = {
            "measured_at_utc": datetime.now(timezone.utc).isoformat(),
            "source_revision": run("git", "rev-parse", "HEAD"),
            "screen_time_package": str(Path(args.binary).resolve().parent.parent),
            "screen_time_version": run(args.binary, "--version"),
            "activitywatch_package": str(Path(shutil.which("aw-qt")).parent.parent),
            "arbtt_binary": args.arbtt,
            "arbtt_version": run(args.arbtt, "--version") if args.arbtt else None,
            "rofi_version": run("rofi", "-version"),
            "kernel": platform.release(),
            "arch": platform.machine(),
            "cpu": next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                        if line.startswith("model name")),
            "state_filesystem": run("stat", "-f", "-c", "%T", str(state)),
            "runtime_filesystem": run("stat", "-f", "-c", "%T", str(runtime)),
            "fixture_apps": apps,
            "screen_time_binary_sha256": run("sha256sum", args.binary).split()[0],
            "competitor_binaries": {
                name: {"path": path, "sha256": run("sha256sum", path).split()[0]}
                for name, path in [(name, getattr(args, name)) for name in
                                   ("logger", "niri", "mono", "toki", "atracker", "hpr", "screentime")]
                if path
            },
            "focus_switches_per_trial": args.switches,
            "interval_seconds": args.interval,
            "tracker_samples": samples,
        }
        if ui:
            result["ui"] = ui
            result["ui_median_ms"] = {
                "render": median(ui["renderer_ms"]),
                "visible": median(row["visible_ms"] for row in ui["launches"]),
            }
        args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
