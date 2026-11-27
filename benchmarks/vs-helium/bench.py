"""Run Bowser and Chromium workload arms on an interactive Windows desktop."""

import argparse
from collections import defaultdict
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import traceback

from fixtures import (ARCHIVE_HASH, MAGENTA, REVISION, Fixtures, prepare_speedometer,
                      prepare_video, sha256)
from native import (Backdrop, Desktop, FocusUnavailable, Gpu, OwnedJob, Pixels, Unsafe, Windows,
                    command_line, cycle_calibration, dpi, file_version, image_path, interactive_desktop, inventory,
                    keep_display_awake, move_window, owned_window, role, show_window)


HERE = Path(__file__).resolve().parent
SCENARIOS = ("memory_tabs", "newtab", "launch_window", "launch_restore", "speedometer", "power")
PRIMED_SCENARIOS = {"memory_tabs", "power", "speedometer", "newtab"}
IDLE_STATES = ("1 tab", "20 tabs")
MEMORY_STATES = (*IDLE_STATES[:1], "10 tabs", *IDLE_STATES[1:], "back to 1 tab after 20")
POWER_CASES = ("idle hidden", "idle visible", "scrolling", "video", "switching tabs")
TEST_RECT = (50, 50, 1280, 900)
COLD_RECT = (50, 50, 1000, 720)
CORES = os.cpu_count()
MIB = 1024 * 1024
FILETIME_EPOCH = 116444736000000000


class AmbientUnavailable(RuntimeError):
    """The current machine load prevents the suite from starting a trial."""


class PowerUnavailable(RuntimeError):
    """A recorded trial requires AC with Battery Saver off."""


class PowerWaitExpired(RuntimeError):
    """The AC wait ended before the host recovered."""


def write_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def wait_until(test, timeout, reason, interval=0.05):
    end = time.perf_counter() + timeout
    while time.perf_counter() < end:
        if result := test():
            return result
        time.sleep(interval)
    raise RuntimeError(reason)


def wait_at(target):
    time.sleep(max(0, target - time.perf_counter()))


def bounds(rect):
    x, y, width, height = rect
    return [x, y, x + width, y + height]


def rgb_is(frame, rgb):
    return sum(pixel == rgb for pixel in frame) >= len(frame) * 0.95


def page_marker(page):
    return f"Bench-{page[1]}-" if page[1] else page[2]


def power_label(power):
    source = {0: "battery", 1: "AC"}.get(power["ac"], "AC status unknown")
    saver = {0: "off", 1: "on"}.get(power["saver"], "unknown")
    return f"{source}, Saver {saver}"


def observe_power(config, api, row):
    power = api.power()
    observations = row.setdefault("power_observations", [])
    if not observations or any(power[key] != observations[-1][key] for key in power):
        observations.append(dict(**power, stamp=datetime.now(timezone.utc).isoformat()))
    row["power_after"] = power
    row.setdefault("power_before", power)
    require_ac = config.require_ac or not (config.diagnostic or config.quick)
    if require_ac and (power["ac"], power["saver"]) != (1, 0):
        raise PowerUnavailable(f"need AC with Battery Saver off; observed {power_label(power)} "
                               f"at {power['charge']}% charge")


def wait_for_power(config, api, row):
    deadline = time.monotonic() + config.power_wait_seconds
    while True:
        try:
            observe_power(config, api, row)
            return
        except PowerUnavailable as error:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise PowerWaitExpired(f"{error}; power wait expired") from error
            print(f"{row['file']} waiting for power: {error}", flush=True)
            time.sleep(min(60, remaining))


def require_benchmark_controls(config, api, row):
    observe_power(config, api, row)
    if api.display() != config.display:
        raise Unsafe("primary display configuration changed")


def capture(api, job, gpu=None, threads=False, cycles=False):
    pids = set(job.pids())
    start_system = api.system_cpu()
    start_job = job.accounting()
    wall = time.time_ns() // 100 + FILETIME_EPOCH
    stamp, processes = inventory(threads)
    members = {pid: proc for pid, proc in processes.items() if pid in pids and job.contains(pid)}
    if cycles:
        for pid, proc in members.items():
            proc["cycles"] = job.process_cycles(pid, proc["created"])
    host = processes[os.getpid()]
    session = host["session"]
    dwm = next((proc for proc in processes.values()
                if proc["image"].lower() == "dwm.exe" and proc["session"] == session), None)
    gpu_stamp, engines = gpu.read(set(members) | ({dwm["pid"]} if dwm else set())) if gpu else (None, {})
    return dict(stamp=stamp, wall_100ns=wall, system=start_system, job=start_job, members=members,
                missing_pids=sorted(pids - members.keys()), dwm=dwm, gpu_stamp=gpu_stamp,
                engines=engines, harness_cpu_100ns=host["cpu_100ns"],
                job_membership_stable=pids == set(job.pids()))


def cpu_delta(before, after):
    return sum(after[key] - before[key] for key in ("user_100ns", "kernel_100ns"))


def ambient_load(before, after, excluded_100ns):
    total = cpu_delta(before, after)
    if total <= 0:
        raise RuntimeError("system CPU sample interval too short")
    busy = total - (after["idle_100ns"] - before["idle_100ns"])
    return dict(system_cpu_pct=100 * busy / total,
                excluded_cpu_pct=100 * excluded_100ns / total,
                unrelated_cpu_pct=100 * max(0, busy - excluded_100ns) / total)


def differences(before, after, roles, use_gpu, use_threads, calibration):
    seconds = after["stamp"] - before["stamp"]
    cpu = cpu_delta(before["job"], after["job"])
    harness_cpu = after["harness_cpu_100ns"] - before["harness_cpu_100ns"]
    load = ambient_load(before["system"], after["system"], cpu + harness_cpu)
    rows = []
    lost_threads = 0
    # Descheduling can lower a pulse's rate even if the thread resumes before
    # its QPC deadline. Use the median of the three fastest usable pulses.
    rates = sorted((pulse["cycles"] * 1e9 / pulse["qpc_ns"] for pulse in calibration
                    if pulse["cycles"] > 0 and pulse["qpc_ns"] <= 3_500_000), reverse=True)
    if len(rates) < 4:
        raise RuntimeError(f"could not calibrate process cycles against QPC: {calibration}")
    cycle_rate = statistics.median(rates[:3])
    unavailable_cycles = []
    for pid, start in before["members"].items():
        end = after["members"].get(pid)
        if use_threads and (not end or end["created"] != start["created"]):
            lost_threads += len(start["threads"])
    for pid, end in after["members"].items():
        start = before["members"].get(pid)
        if start and start["created"] != end["created"]:
            start = None
        born = end["created"] >= before["wall_100ns"]
        first_cycle = start.get("cycles") if start else (0 if born else None)
        last_cycle = end["cycles"]
        cycles = (last_cycle - first_cycle
                  if first_cycle is not None and last_cycle is not None and last_cycle >= first_cycle
                  else None)
        if cycles is None:
            unavailable_cycles.append(pid)
        row = dict(pid=pid, created=end["created"], image=end["image"],
                   role=roles.get((pid, end["created"]), "unknown"),
                   cpu_tick_ms_s=((end["cpu_100ns"] - (start["cpu_100ns"] if start else 0))
                                  / seconds / 10000 if start or born else None),
                   cpu_cycles_s=cycles / seconds if cycles is not None else None,
                   cpu_ms_s=cycles / seconds / cycle_rate * 1000 if cycles is not None else None)
        if use_threads:
            initial = start["threads"] if start else {}
            ending = end["threads"]
            lost_threads += len(initial.keys() - ending.keys())
            row["switches_s"] = sum(
                (value - initial[identity]) % (2 ** 32) if identity in initial else value
                for identity, value in ending.items()
                if identity in initial or int(identity.partition(":")[2]) >= before["wall_100ns"]
            ) / seconds
        rows.append(row)
    summed_tick_cpu = sum(row["cpu_tick_ms_s"] or 0 for row in rows)
    result = dict(seconds=seconds, cpu_ms_s=sum(row["cpu_ms_s"] or 0 for row in rows),
                  cpu_cycles_s=sum(row["cpu_cycles_s"] or 0 for row in rows),
                  cpu_tick_ms_s=cpu / seconds / 10000,
                  cpu_tick_unattributed_ms_s=cpu / seconds / 10000 - summed_tick_cpu,
                  cycle_calibration=calibration, calibration_cycles_per_second=cycle_rate,
                  unavailable_cycle_pids=unavailable_cycles,
                  exited_cycle_pids=sorted(pid for pid, proc in before["members"].items()
                                           if proc["created"] != after["members"].get(pid, {}).get("created")),
                  processes=rows,
                  observed_exited_threads=lost_threads, **load)
    if use_threads:
        result["switches_s"] = sum(row["switches_s"] for row in rows)
    if use_gpu:
        gpu_seconds = after["gpu_stamp"] - before["gpu_stamp"]
        engine_deltas = defaultdict(int)
        partial_gpu = []
        for name in before["engines"].keys() | after["engines"].keys():
            pid = int(name.split("_")[1])
            first, last = before["engines"].get(name), after["engines"].get(name)
            start = before["members"].get(pid)
            end = after["members"].get(pid)
            same_pid = not (start and end and start["created"] != end["created"])
            if first is not None and last is not None and last >= first and same_pid:
                engine_deltas[pid] += last - first
            elif last is not None and first is None and same_pid:
                # WDDM creates a PID/engine instance with a cumulative busy-time counter.
                engine_deltas[pid] += last
            else:
                partial_gpu.append(name)
        for row in rows:
            row["gpu_ms_s"] = engine_deltas[row["pid"]] / gpu_seconds / 10000
        gpu_ms_s = sum(row["gpu_ms_s"] for row in rows)
        if not before["dwm"] or not after["dwm"] or (
                before["dwm"]["created"] != after["dwm"]["created"]):
            raise RuntimeError("DWM identity changed or was unavailable")
        dwm_pid = after["dwm"]["pid"]
        if not any(name.startswith(f"pid_{dwm_pid}_") for name in after["engines"]):
            raise RuntimeError("DWM GPU-engine counter unavailable")
        if not any(int(name.split("_")[1]) in after["members"] for name in after["engines"]):
            raise RuntimeError("browser GPU-engine counter unavailable")
        dwm_cpu = after["dwm"]["cpu_100ns"] - before["dwm"]["cpu_100ns"]
        other_cpu = ambient_load(before["system"], after["system"], cpu + harness_cpu + dwm_cpu)
        result.update(gpu_ms_s=gpu_ms_s, activity_ms_s=result["cpu_ms_s"] + gpu_ms_s,
                      dwm_cpu_ms_s=dwm_cpu / seconds / 10000,
                      dwm_gpu_ms_s=engine_deltas[dwm_pid] / gpu_seconds / 10000,
                      non_job_nondwm_cpu_pct=other_cpu["unrelated_cpu_pct"],
                      gpu_partial_instances=sorted(partial_gpu))
    return result


class Trial:
    def __init__(self, config, api, server, arm, scenario, folder, row):
        self.config, self.api, self.server = config, api, server
        self.arm, self.scenario, self.row = arm, scenario, row
        self.name, self.kind, self.exe = arm
        self.folder = folder
        self.profile = folder / "profile"
        self.desktop = Desktop(api, row.setdefault("focus_recoveries", []))
        self.job = None
        self.hwnd = None
        self.roles = {}
        self.gpu = None
        self.pages = []
        self.rect = COLD_RECT if scenario.startswith("launch_") else TEST_RECT
        self.next_power_check = 0
        self.view_points = []

    def check_power(self):
        now = time.monotonic()
        if now >= self.next_power_check:
            observe_power(self.config, self.api, self.row)
            self.next_power_check = now + 1

    def fixture(self, route):
        token = secrets.token_hex(6)
        url = self.server.url(token, route)
        self.row.setdefault("fixture_tokens", []).append(token)
        return url, token, None

    def prepare(self):
        if self.scenario in PRIMED_SCENARIOS:
            template = self.config.templates[self.name]
            shutil.copytree(long_path(template / "profile"), long_path(self.profile))
            if (template / "local-appdata").is_dir():
                shutil.copytree(long_path(template / "local-appdata"),
                                long_path(self.folder / "local-appdata"))
            self.row["profile_template_sha256"] = self.config.template_records[self.name]["sha256"]
            return
        self.profile.mkdir()
        if self.kind == "bowser":
            (self.profile / "import-offer.txt").write_text("answered\n", encoding="ascii")
            settings = self.config.arm_settings.get(self.name, {})
            if settings:
                (self.profile / "settings.txt").write_text(
                    "".join(f"{key}={value}\n" for key, value in sorted(settings.items())),
                    encoding="utf-8")
        else:
            default = self.profile / "Default"
            default.mkdir()
            (self.profile / "First Run").touch()
            prefs = {"helium": {"completed_onboarding": True}}
            if self.scenario == "launch_restore":
                prefs["session"] = {"restore_on_startup": 1}
            write_json(default / "Preferences", prefs)

    def launch(self, url=None, cold=False, focus=True):
        observe_power(self.config, self.api, self.row)
        env = {k: v for k, v in os.environ.items()
               if not k.upper().startswith(("BOWSER_", "WEBVIEW2_"))}
        env.update(self.config.arm_env.get(self.name, {}))
        if self.kind == "bowser":
            local = self.folder / "local-appdata"
            local.mkdir(exist_ok=True)
            env["LOCALAPPDATA"] = str(local)
            args = ["--data-dir", str(self.profile)]
            if url:
                args.extend(("--open-url", url))
        else:
            x, y, width, height = self.rect
            args = [f"--user-data-dir={self.profile}", "--no-default-browser-check"]
            if not (cold and self.scenario == "launch_restore"):
                args.extend(("--no-first-run", f"--window-size={width},{height}",
                             f"--window-position={x},{y}"))
                if self.scenario == "launch_restore":
                    args.append("--force-renderer-accessibility")
            if url:
                args.append(url)
        entry = dict(args=args, exe=str(self.exe))
        self.row.setdefault("launches", []).append(entry)
        self.job = OwnedJob(self.api)
        start, pid = self.job.launch(self.exe, args, env, self.rect)
        entry.update(start=start, pid=pid)
        if not cold:
            window = self.find_window()
            self.hwnd = window["hwnd"]
            if not self.desktop.owns(self.job, self.hwnd):
                raise Unsafe("lost ownership of the discovered browser window")
            if focus:
                self.desktop.ensure_foreground(self.job, self.hwnd)
            if not move_window(self.hwnd, None, *self.rect, 0x0014):
                raise RuntimeError("could not size owned browser window")
            self.geometry()
        return start

    def find_window(self, timeout=35):
        expected = "BowserNativeWindow" if self.kind == "bowser" else "Chrome_WidgetWin_1"
        return wait_until(lambda: owned_window(self.api, self.job, expected), timeout,
                          f"owned {expected} window did not appear", interval=0.02)

    def geometry(self):
        expected = "BowserNativeWindow" if self.kind == "bowser" else "Chrome_WidgetWin_1"
        window = self.job.window(self.hwnd, expected)
        if not window or window["rect"] != bounds(self.rect) or dpi(self.hwnd) != self.config.dpi:
            raise RuntimeError(f"unmatched window rectangle or DPI: {window}, DPI {dpi(self.hwnd)}, "
                               f"expected {bounds(self.rect)} at {self.config.dpi} DPI")
        self.row["geometry"] = {"rect": window["rect"], "dpi": self.config.dpi}

    def require_foreground(self, recover=False):
        self.check_power()
        if recover:
            self.desktop.ensure_foreground(self.job, self.hwnd)
        else:
            self.desktop.foreground(self.job, self.hwnd)

    def place_in_view(self):
        if not self.desktop.owns(self.job, self.hwnd):
            raise Unsafe("window lost job ownership before visible placement")
        if not move_window(self.hwnd, -1, 0, 0, 0, 0, 0x0013):
            raise RuntimeError("could not place the owned window above occluding windows")
        x, y, width, height = self.rect
        self.view_points = [(x + width // 2, y + height // 2),
                            (x + 120, y + 200), (x + width - 120, y + height - 120)]
        wait_until(self.in_view, 8, "owned window did not become unobstructed")
        self.row["visible_window"] = dict(placement="topmost without activation",
                                          probe_points=self.view_points,
                                          required_points=2 if self.scenario == "memory_tabs" else 3)

    def in_view(self):
        visible = [self.desktop.hit(self.hwnd, x, y) for x, y in self.view_points]
        if self.scenario == "memory_tabs":
            # A peripheral popup can cover a corner while the document
            # remains in view. Require the centre and another content point.
            return visible[0] and any(visible[1:])
        return all(visible)

    def require_in_view(self, recover=False):
        self.check_power()
        if self.in_view():
            return
        blockers = [self.desktop.root_at(x, y) for x, y in self.view_points]
        if recover and self.desktop.owns(self.job, self.hwnd):
            if self.api.IsIconic(self.hwnd):
                show_window(self.hwnd, 4)  # SW_SHOWNOACTIVATE
            moved = move_window(self.hwnd, -1, 0, 0, 0, 0, 0x0053)
            if moved:
                deadline = time.perf_counter() + 2
                while time.perf_counter() < deadline:
                    if self.in_view():
                        self.row.setdefault("visibility_recoveries", []).append(
                            dict(stamp=time.perf_counter(), covered_by=blockers))
                        return
                    time.sleep(0.05)
        raise FocusUnavailable(f"the browser content probe is occluded by HWNDs {blockers}")

    def wait_in_view(self, deadline, recover=True):
        while time.perf_counter() < deadline:
            self.require_in_view(recover=recover)
            time.sleep(max(0, min(1, deadline - time.perf_counter())))

    def close(self):
        if self.job:
            job, self.job = self.job, None
            self.row["launches"][-1]["drain"] = job.drain()
        self.hwnd = None

    def title(self):
        buffer = ctypes.create_unicode_buffer(2048)
        self.api.GetWindowTextW(self.hwnd, buffer, len(buffer))
        return buffer.value

    def checked_title(self):
        self.require_foreground(recover=self.scenario in {"memory_tabs", "power"})
        return self.title()

    def report(self, token, kind):
        return [row for row in self.server.reports[token] if row["kind"] == kind]

    def ready(self, page, previous=None, focus=True, timeout=None):
        url, token, marker = page
        deadline = time.perf_counter() + (timeout or (45 if token else 75))
        stable_since = None
        last = None
        while time.perf_counter() < deadline:
            if focus:
                title = self.checked_title()
            else:
                if not self.desktop.owns(self.job, self.hwnd):
                    raise Unsafe("lost ownership while waiting for fixture readiness")
                title = self.title()
            if token:
                records = self.report(token, "ready")
                if f"Bench-{token}-" in title and records and records[-1]["visible"]:
                    break
            elif marker.casefold() in title.casefold() and title != previous:
                if title != last:
                    stable_since = time.perf_counter()
                if time.perf_counter() - stable_since >= 2:
                    break
            last = title
            time.sleep(0.1)
        else:
            raise RuntimeError(f"page readiness failed for {url}; last title {last!r}")
        stamp = time.perf_counter()
        self.row.setdefault("pages", []).append(dict(url=url, title=title, ready=stamp,
                                                     evidence="loopback beacon" if token else "stable title"))
        return stamp

    def key(self, key, ctrl=True):
        self.check_power()
        return self.desktop.chord(self.job, self.hwnd, key, ctrl)

    def navigate(self, page, new=False):
        previous = self.title()
        trace = None
        direct = new and getattr(self.config, "direct_newtab_entry", False)
        if new:
            sent = self.key(0x54)
            if direct:
                trace = dict(url=page[0], opened=sent, attempts=[])
                self.row.setdefault("tab_entry", []).append(trace)
            try:
                wait_until(lambda: self.checked_title() != previous,
                           20, "Ctrl+T did not change the active tab title")
            except RuntimeError as error:
                if trace:
                    trace["error"] = str(error)
                raise
        for attempt in range(5):
            focus_count = len(self.desktop.recoveries)
            direct_attempt = direct and attempt == 0
            if not direct_attempt:
                self.key(0x4C)
                time.sleep(0.35)
            self.desktop.ensure_foreground(self.job, self.hwnd)
            entry = dict(direct=direct_attempt, started=time.perf_counter())
            if trace:
                trace["attempts"].append(entry)
            if len(self.desktop.recoveries) != focus_count:
                entry["error"] = "focus changed before address entry"
                self.row.setdefault("navigation_retries", []).append(
                    dict(url=page[0], reason="focus changed while opening the address bar"))
                continue
            self.desktop.type(self.job, self.hwnd, page[0])
            self.desktop.ensure_foreground(self.job, self.hwnd)
            if len(self.desktop.recoveries) != focus_count:
                entry["error"] = "focus changed during address entry"
                self.row.setdefault("navigation_retries", []).append(
                    dict(url=page[0], reason="focus changed during address entry"))
                continue
            entry["submitted"] = self.key(0x0D, ctrl=False)
            try:
                ready = self.ready(page, previous, timeout=10 if page[1] and attempt == 0 else None)
                entry["ready"] = ready
                return ready
            except RuntimeError as error:
                entry["error"] = str(error)
                if isinstance(error, (FocusUnavailable, PowerUnavailable, Unsafe)) or attempt == 4:
                    raise
                self.row.setdefault("navigation_retries", []).append(
                    dict(url=page[0], reason=str(error)))
        raise RuntimeError(f"focus kept interrupting address entry for {page[0]}")

    def fill(self, count):
        stamp = None
        while len(self.pages) < count:
            index = len(self.pages)
            if self.config.sites == "local":
                page = self.fixture(f"site/{index + 1}")
            else:
                url, marker = self.config.site_list[index]
                page = (url, None, marker)
            stamp = self.navigate(page, new=bool(index))
            self.pages.append(page)
        return stamp

    def check_screen(self, pixels, focus=True):
        if focus:
            self.require_foreground()
        corner = pixels.size - 1
        for x, y in ((0, 0), (corner, 0), (0, corner), (corner, corner)):
            if not self.desktop.hit(self.hwnd, pixels.x + x, pixels.y + y):
                raise Unsafe("screen probe is covered by a window outside the target")

    def backdrop_frame(self, backdrop, pixels):
        backdrop.pump()
        hit = self.desktop.hit(backdrop.hwnd, pixels.x + 8, pixels.y + 8)
        frame = pixels.read()
        self.row["backdrop_observation"] = dict(owns_probe=hit, distinct_rgb=len(set(frame)),
                                                  sample_rgb=f"{frame[0]:06x}")
        if hit and len(set(frame)) == 1 and frame[0] != MAGENTA:
            return frame
        return None

    def set_helium_restore(self):
        self.key(0x4C)
        time.sleep(0.35)
        # Helium can leave a character in the omnibox as startup settles.
        self.key(0x41)
        self.desktop.type(self.job, self.hwnd, "helium://settings/onStartup")
        self.key(0x0D, ctrl=False)
        window = self.find_window()
        command = ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                   "-File", str(HERE / "helium-startup.ps1"), "-Window", str(window["hwnd"]),
                   "-Owner", str(window["pid"])]

        def radio(choice, action):
            self.require_foreground(recover=True)
            result = subprocess.run([*command, "-Choice", choice, "-Action", action],
                                    text=True, capture_output=True, timeout=30)
            if result.returncode:
                raise RuntimeError(f"Helium startup settings UI: {result.stderr.strip()}")
            return json.loads(result.stdout)

        choices = ("Open the New Tab page", "Continue where you left off")
        for choice in choices:
            radio(choice, "Focus")
            self.key(0x20, ctrl=False)
            radio(choice, "Verify")
        self.row["startup_ui"] = dict(selected=choices[-1])

    def cold(self, restore=False):
        page = self.fixture("color/1") if restore else None
        if restore:
            if self.kind == "helium":
                self.launch()
                self.set_helium_restore()
                self.navigate(page)
            else:
                self.launch(page[0], focus=False)
                self.ready(page, focus=False)
            # Let normal asynchronous session persistence observe the committed navigation.
            time.sleep(5)
            if self.kind == "helium":
                # Exiting the browser preserves the open-window set. Closing the
                # final window alone can save it as recently closed instead.
                self.key(0x4C)
                time.sleep(0.35)
                self.desktop.type(self.job, self.hwnd, "helium://quit/")
                self.desktop.chord(self.job, self.hwnd, 0x0D, ctrl=False, closing=True)
                wait_until(lambda: self.job.accounting()["active"] == 0, 15,
                           "Helium's browser quit command did not exit its job")
                self.row["priming_close"] = "helium://quit/"
            self.close()
            if self.kind == "helium":
                default = self.profile / "Default"
                prefs = json.loads((default / "Preferences").read_text(encoding="utf-8"))
                protected_file = default / "Secure Preferences"
                protected_prefs = (json.loads(protected_file.read_text(encoding="utf-8"))
                                   if protected_file.is_file() else {})
                startup = protected_prefs.get("session", {}).get("restore_on_startup")
                if startup is None:
                    startup = prefs.get("session", {}).get("restore_on_startup")
                self.row["persisted_restore_on_startup"] = startup
                if startup != 1:
                    raise RuntimeError("Helium did not persist its session restore setting")
                local_state = json.loads((self.profile / "Local State").read_text(encoding="utf-8"))
                self.row["primed_startup"] = dict(
                    exit_type=prefs.get("profile", {}).get("exit_type"),
                    completed_onboarding=prefs.get("helium", {}).get("completed_onboarding"),
                    first_run=(self.profile / "First Run").exists(),
                    last_used=local_state.get("profile", {}).get("last_used"),
                    last_active_profiles=local_state.get("profile", {}).get("last_active_profiles"))
                self.row["primed_session_files"] = {
                    path.name: dict(size=path.stat().st_size,
                                    has_fixture=page[0].encode() in path.read_bytes())
                    for path in (default / "Sessions").glob("*")
                    if path.is_file()
                }
                if not any(record["has_fixture"]
                           for record in self.row["primed_session_files"].values()):
                    raise RuntimeError("Helium did not persist the fixture URL in session storage")
            # Discard the priming beacon so only the restored page can satisfy readiness.
            self.server.reports[page[1]].clear()
            ambient(self.config, self.api, self.row, phase="restore relaunch")
            require_benchmark_controls(self.config, self.api, self.row)
        self.row["presentation_desktop"] = interactive_desktop()
        if not self.row["presentation_desktop"]["usable"]:
            raise Unsafe("the harness desktop is not the active input desktop")
        backdrop = None if restore else Backdrop(self.rect)
        pixels = None
        try:
            pixels = Pixels(self.api, self.rect)
            if backdrop:
                wait_until(lambda: self.backdrop_frame(backdrop, pixels), 5,
                           "the harness backdrop did not paint")
                backdrop.allow_window_above()
                time.sleep(0.15)
                background = wait_until(lambda: self.backdrop_frame(backdrop, pixels), 5,
                                        "the harness backdrop does not occupy the screen probe")
                self.row["backdrop_rgb"] = f"{background[0]:06x}"
            start = self.launch(cold=True)
            expected = "BowserNativeWindow" if self.kind == "bowser" else "Chrome_WidgetWin_1"
            end = time.perf_counter() + 40
            first_visible = None
            ceiling = []
            previous = start
            while time.perf_counter() < end:
                self.check_power()
                frame = pixels.read()
                now = time.perf_counter()
                ceiling.append(now - previous)
                previous = now
                target = rgb_is(frame, MAGENTA) if restore else frame != background
                if target:
                    candidate = self.desktop.root_at(pixels.x + 8, pixels.y + 8)
                    window = self.job.window(candidate, expected)
                    if window:
                        self.hwnd = candidate
                        self.geometry()
                        self.check_screen(pixels, focus=False)
                        first_visible = now
                        break
                time.sleep(0.001)
            if first_visible is None:
                observed = owned_window(self.api, self.job, expected)
                self.row["cold_timeout_window"] = observed
                raise RuntimeError("cold-launch presentation signal was not observed within 40 seconds")
            self.row["largest_poll_gap_ms"] = max(ceiling) * 1000
            self.row["values"]["restored paint ms" if restore else "window presented ms"] = (
                first_visible - start) * 1000
            if restore:
                self.ready(page, focus=False)
        finally:
            if pixels:
                pixels.close()
            if backdrop:
                backdrop.close()

    def newtab(self):
        original = self.fixture("color/1")
        self.launch(original[0])
        self.place_in_view()
        self.ready(original, focus=False)
        pixels = Pixels(self.api, self.rect)
        samples = self.row.setdefault("newtab_samples", [])
        try:
            for _ in range(3 if self.config.quick else 15):
                self.require_foreground(recover=True)
                self.check_screen(pixels)
                wait_until(lambda: rgb_is(pixels.read(), MAGENTA), 10, "fixture not painted")
                start = self.key(0x54)
                first = None
                previous_frame = None
                previous_stamp = start
                gap = 0
                changed_at = None
                while time.perf_counter() - start < 12:
                    if self.api.GetForegroundWindow() != self.hwnd:
                        raise FocusUnavailable("new-tab foreground changed during paint measurement")
                    self.check_screen(pixels, focus=False)
                    frame = pixels.read()
                    now = time.perf_counter()
                    gap = max(gap, now - previous_stamp)
                    previous_stamp = now
                    if not rgb_is(frame, MAGENTA) and first is None:
                        first = now
                    if frame != previous_frame:
                        previous_frame, changed_at = frame, now
                    if first is not None and now - changed_at >= 0.3:
                        break
                    time.sleep(0.001)
                else:
                    raise RuntimeError("new-tab central probe did not change and settle")
                original_marker = f"Bench-{original[1]}-"
                new_title = self.title()
                if original_marker in new_title:
                    raise RuntimeError("centre colour changed without a new active tab")
                samples.append(dict(first_ms=(first - start) * 1000,
                                    settled_ms=(now - start) * 1000, max_poll_gap_ms=gap * 1000))
                self.require_foreground(recover=True)
                # Escape exits any in-progress omnibox interaction before the
                # untimed Ctrl+W. Verify the target again before a close retry.
                self.key(0x1B, ctrl=False)
                deadline = time.perf_counter() + 12
                tries = []
                samples[-1]["close_attempts"] = tries
                while time.perf_counter() < deadline:
                    self.require_foreground(recover=True)
                    title = self.title()
                    self.check_screen(pixels, focus=False)
                    magenta = rgb_is(pixels.read(), MAGENTA)
                    if original_marker in title and magenta:
                        break
                    if title == new_title and not magenta and len(tries) < 3:
                        if not tries or time.perf_counter() - tries[-1] >= 1:
                            tries.append(self.key(0x57))
                    time.sleep(0.05)
                else:
                    raise RuntimeError(f"original tab did not return after {len(tries)} closes; last title {self.title()!r}")
                time.sleep(0.5)
        finally:
            pixels.close()
        self.row["values"].update({
            "new tab first paint ms": statistics.median(row["first_ms"] for row in samples),
            "new tab visually settled ms": statistics.median(row["settled_ms"] for row in samples),
        })

    def register_roles(self, snapshot):
        for pid, proc in snapshot["members"].items():
            key = (pid, proc["created"])
            if key not in self.roles:
                cmd = command_line(self.api, pid)
                name = role(proc["image"], cmd)
                if self.kind == "bowser" and proc["image"].casefold() == self.exe.name.casefold():
                    name = "Bowser host"
                self.roles[key] = name
                self.row.setdefault("process_roles", []).append(
                    dict(pid=pid, created=proc["created"], image=proc["image"], role=name,
                         path=image_path(self.api, pid)))
            proc["role"] = self.roles[key]

    def memory(self, name, snapshot=None):
        if snapshot is None:
            self.require_in_view()
            snapshot = self.memory_snapshot()
        self.register_roles(snapshot)
        self.row.setdefault("memory", {})[name] = snapshot
        for label, field in (("private working set", "private_ws"), ("private commit", "commit")):
            self.row["values"][f"{name} {label} MiB"] = (
                sum(row[field] for row in snapshot["members"].values()) / MIB)

    def memory_snapshot(self):
        attempts = 0

        def stable():
            nonlocal attempts
            attempts += 1
            snapshot = capture(self.api, self.job)
            if (snapshot["members"] and not snapshot["missing_pids"]
                    and snapshot["job_membership_stable"]):
                snapshot["inventory_attempts"] = attempts
                return snapshot
            return None

        return wait_until(stable, 2, "job process inventory did not stabilize for the memory sample",
                          interval=0.1)

    def finish_window(self, name, before, after, calibration, gpu=False, threads=False):
        self.register_roles(after)
        self.register_roles(before)
        result = differences(before, after, self.roles, gpu, threads, calibration)
        self.row.setdefault("windows", {})[name] = dict(before=before, after=after, delta=result)
        gate_cpu = result.get("non_job_nondwm_cpu_pct", result["unrelated_cpu_pct"])
        result["ambient_gate_cpu_pct"] = gate_cpu
        if not self.config.quick and gate_cpu > self.config.max_workload_ambient_cpu:
            raise RuntimeError(f"{name}: unrelated CPU {gate_cpu:.1f}% exceeds limit")
        return result

    def window_sample(self, name, duration, action=None, hidden=False, gpu=False):
        recover = self.scenario in {"memory_tabs", "power"}
        if not hidden:
            self.require_foreground(recover=recover)
        focus_count = len(self.desktop.recoveries)
        before = capture(self.api, self.job, self.gpu if gpu else None, cycles=True)
        self.row.setdefault("windows", {})[name] = {"before": before}
        calibration = cycle_calibration()
        end = before["stamp"] + duration
        if action:
            action(end)
        while time.perf_counter() < end:
            self.check_power()
            if hidden:
                if not self.api.IsIconic(self.hwnd):
                    raise RuntimeError("browser left minimized state")
            else:
                self.require_foreground(recover=recover)
            time.sleep(min(0.25, max(0, end - time.perf_counter())))
        if hidden and not self.api.IsIconic(self.hwnd):
            raise RuntimeError("browser left minimized state")
        if not hidden:
            self.require_foreground(recover=recover)
        calibration += cycle_calibration()
        after = capture(self.api, self.job, self.gpu if gpu else None, cycles=True)
        result = self.finish_window(name, before, after, calibration, gpu=gpu)
        result["focus_recoveries"] = len(self.desktop.recoveries) - focus_count
        return result

    def tabs(self):
        self.launch()
        self.place_in_view()
        time.sleep(1.2)
        duration = 1.5 if self.config.quick else 60
        initial_delay = 1 if self.config.quick else 10
        memory_delay = 1.5 if self.config.quick else 30
        for count in (1, 10, 20):
            reached = self.fill(count)
            label = f"{count} tab" + ("s" if count != 1 else "")
            if count == 10:
                self.wait_in_view(reached + memory_delay)
                self.memory(label)
                continue
            starts = {label: initial_delay, f"{label} settled": self.config.settle_seconds}
            events = [(memory_delay, "memory", label)]
            for state, delay in starts.items():
                events.extend([(delay, "start", state), (delay + duration, "end", state)])
            windows = {}
            for offset, event, state in sorted(events):
                self.wait_in_view(reached + offset, recover=not windows)
                self.require_in_view(recover=not windows)
                if event == "memory":
                    self.memory(label)
                elif event == "start":
                    before = capture(self.api, self.job, threads=True, cycles=True)
                    self.row.setdefault("windows", {})[state] = {"before": before}
                    windows[state] = before, cycle_calibration()
                else:
                    before, calibration = windows.pop(state)
                    calibration += cycle_calibration()
                    after = capture(self.api, self.job, threads=True, cycles=True)
                    self.finish_window(state, before, after, calibration, threads=True)
            for state in starts:
                data = self.row["windows"][state]["delta"]
                self.row["values"][f"{state} context switches/s"] = data["switches_s"]
                self.row["values"][f"{state} CPU % one core"] = data["cpu_ms_s"] / 10
                self.row["values"][f"{state} CPU ticks % one core"] = data["cpu_tick_ms_s"] / 10
        for page, next_page in zip(reversed(self.pages[1:]), reversed(self.pages[:-1])):
            if page_marker(page).casefold() not in self.title().casefold():
                raise RuntimeError(f"active tab did not match {page[0]} before close")
            self.key(0x57)
            wait_until(lambda: page_marker(next_page).casefold() in self.checked_title().casefold(), 15,
                       f"Ctrl+W did not activate {next_page[0]}")
        self.wait_in_view(time.perf_counter() + memory_delay)
        self.memory("back to 1 tab after 20")

    def speedometer(self):
        if not (self.server.speedometer / "index.html").is_file():
            raise Unsafe("the extracted Speedometer fixture disappeared before launch")
        url, token, _ = self.fixture("speedometer/")
        self.launch(url + ("?iterationCount=1" if self.config.quick else ""), focus=False)
        self.place_in_view()
        before = capture(self.api, self.job, cycles=True)
        self.row.setdefault("windows", {})["Speedometer 3.1"] = {"before": before}
        calibration = cycle_calibration()
        start = time.perf_counter()
        self.server.speedometer_gates[token].set()
        while time.perf_counter() - start < (240 if self.config.quick else 1200):
            self.require_in_view()
            errors = self.report(token, "score-error")
            if errors:
                raise RuntimeError(errors[-1]["reason"])
            reports = self.report(token, "score")
            if reports:
                result = reports[-1]
                if (not result["visible"] or not math.isfinite(result["score"]) or result["score"] <= 0
                        or result["iterations"] != (1 if self.config.quick else 10)):
                    raise RuntimeError(f"invalid Speedometer score beacon: {result}")
                self.geometry()
                self.row["score"] = result
                calibration += cycle_calibration()
                after = capture(self.api, self.job, cycles=True)
                self.server.speedometer_finishes[token].set()
                load = self.finish_window("Speedometer 3.1", before, after, calibration)
                self.row["score_unrelated_cpu_pct"] = load["unrelated_cpu_pct"]
                self.row["values"]["Speedometer 3.1 score"] = result["score"]
                self.row["values"]["Speedometer CPU ms/s"] = load["cpu_ms_s"]
                self.row["values"]["Speedometer CPU ticks ms/s"] = load["cpu_tick_ms_s"]
                # Let the fixture leave completed iframes before normal WM_CLOSE.
                wait_until(lambda: self.report(token, "ready"), 15,
                           "Speedometer's post-score fixture did not load")
                return
            if time.perf_counter() - start > 35 and not self.report(token, "score-start"):
                raise RuntimeError("Speedometer did not report that its first iteration started")
            time.sleep(0.25)
        raise RuntimeError("Speedometer did not report a valid final score")

    def power(self):
        self.gpu = Gpu()
        try:
            static = self.fixture("site/11")
            pages = {"scrolling": "article/1", "video": "video/1"}
            self.launch()
            time.sleep(1.2)
            self.fill(10)
            self.navigate(static, new=True)
            current = static
            for case in self.config.power_cases:
                page = self.fixture(pages[case]) if case in pages else static
                if page != current or case == "switching tabs":
                    self.navigate(page)
                current = page
                self.geometry()
                self.power_case(case, page, case == "idle hidden")
        finally:
            self.gpu.close()
            self.gpu = None

    def renderer_counters(self):
        started = time.perf_counter()
        snapshot = capture(self.api, self.job, cycles=True)
        self.register_roles(snapshot)
        members = snapshot["members"].values()
        counters = {f"{proc['pid']}:{proc['created']}": proc["cycles"]
                    for proc in members if proc["role"] == "renderer" and proc["cycles"] is not None}
        return dict(stamp=snapshot["stamp"], elapsed_ms=(time.perf_counter() - started) * 1000,
                    cycles=counters)

    def power_case(self, case, page, hidden):
        duration = 6 if self.config.quick else 60
        action = None
        if case == "scrolling":
            x, y, width, height = self.rect
            self.desktop.wheel_position(self.job, self.hwnd, x + width // 2, y + height // 2)

            def action(deadline):
                self.repeat(deadline, 0.10, lambda _: self.desktop.wheel(self.job, self.hwnd, -120))
        if case == "switching tabs":
            markers = [page_marker(target).casefold() for target in self.pages + [page]]
            observed = []
            self.row["switch_observations"] = observed
            trace = self.config.trace_switch_renderers

            def switch(_):
                previous = self.title()
                before = self.renderer_counters() if trace else None
                sent = self.key(0x09)

                def changed():
                    title = self.checked_title()
                    matches = [i + 1 for i, marker in enumerate(markers) if marker in title.casefold()]
                    return (title, matches[0]) if title != previous and len(matches) == 1 else None

                title, tab = wait_until(changed, 0.8,
                                        "Ctrl+Tab failed to activate a different workload tab",
                                        interval=0.025)
                record = dict(timestamp=time.perf_counter(), sent=sent, title=title, tab=tab)
                observed.append(record)
                if trace:
                    wait_at(sent + 0.4)
                    record.update(before=before, early=self.renderer_counters())

            def action(deadline):
                self.repeat(deadline, 1, switch)
        time.sleep(2 if self.config.quick else 15)
        if hidden:
            self.require_foreground(recover=True)
            show_window(self.hwnd, 6)
            wait_until(lambda: self.api.IsIconic(self.hwnd), 5, "owned window did not minimize")
            time.sleep(1)
        if case == "video":
            wait_until(lambda: any(row["playing"] and row["frames"] > 0
                                   for row in self.report(page[1], "video")), 25,
                       "1080p video did not start")
        try:
            result = self.window_sample(case, duration, action, hidden, gpu=True)
        finally:
            if hidden:
                if not self.desktop.owns(self.job, self.hwnd):
                    raise Unsafe("owned window disappeared while minimized")
                show_window(self.hwnd, 9)
                self.desktop.ensure_foreground(self.job, self.hwnd)
        interval = self.row["windows"][case]
        if case == "switching tabs" and self.config.verify_switch_order:
            expected = [i % len(markers) + 1 for i in range(6 if self.config.quick else 60)]
            if [sample["tab"] for sample in observed] != expected:
                raise RuntimeError("switching did not verify the configured strip-order itinerary")
        if case == "switching tabs" and self.config.trace_switch_renderers:
            if not self.config.quick and (len(observed) != 60 or
                                          len({sample["tab"] for sample in observed}) != len(markers)):
                raise RuntimeError("renderer tracing did not verify 60 switches across all 11 tabs")
            self.row["switch_renderers"] = summarize_switch_renderers(observed, interval)
        if case in {"scrolling", "video"}:
            kind = "scroll" if case == "scrolling" else "video"
            records = [row for row in self.report(page[1], kind)
                       if interval["before"]["stamp"] <= row["timestamp"] <= interval["after"]["stamp"]]
            if len(records) < 2:
                raise RuntimeError(f"{case} produced insufficient workload evidence")
            if kind == "scroll" and records[-1]["y"] - records[0]["y"] < 300:
                raise RuntimeError("wheel input did not scroll the article")
            if kind == "video" and (not all(row["playing"] and row["width"] == 1920 and row["height"] == 1080
                                           for row in records)
                                    or records[-1]["frames"] - records[0]["frames"] < 12):
                raise RuntimeError("1080p H.264 video did not advance through the sample")
            self.row.setdefault("power_work", {})[case] = records
        for field in ("cpu", "gpu", "activity", "dwm_cpu", "dwm_gpu"):
            self.row["values"][f"{case} {field} ms/s"] = result[f"{field}_ms_s"]
        self.row["values"][f"{case} CPU ticks ms/s"] = result["cpu_tick_ms_s"]

    def repeat(self, end, step, operation):
        due = time.perf_counter()
        index = 0
        while due < end - min(step / 4, 0.05):
            wait_at(due)
            self.check_power()
            if time.perf_counter() >= end:
                raise RuntimeError("synthetic input cadence fell behind the sample window")
            operation(index)
            due += step
            index += 1
        return index

    def run(self):
        if self.scenario == "launch_window":
            self.cold()
        elif self.scenario == "launch_restore":
            self.cold(restore=True)
        elif self.scenario == "memory_tabs":
            with keep_display_awake():
                self.tabs()
        else:
            method = {"newtab": self.newtab, "speedometer": self.speedometer,
                      "power": self.power}[self.scenario]
            method()


def summarize_switch_renderers(observations, interval):
    delta = interval["delta"]
    rate = delta["calibration_cycles_per_second"]
    targets = {event["tab"] for event in observations}
    by_renderer = defaultdict(lambda: defaultdict(list))
    for event in observations:
        first = event["before"]["cycles"]
        last = event["early"]["cycles"]
        for identity in first.keys() & last.keys():
            if last[identity] >= first[identity]:
                by_renderer[identity][event["tab"]].append((last[identity] - first[identity]) / rate * 1000)
    rows = []
    for process in delta["processes"]:
        if process["role"] != "renderer" or process["cpu_ms_s"] is None:
            continue
        identity = f"{process['pid']}:{process['created']}"
        samples = by_renderer[identity]
        tab_medians = {tab: statistics.median(values) for tab, values in samples.items()}
        ranked = sorted(tab_medians, key=tab_medians.get, reverse=True)
        total = sum(tab_medians.values())
        peak = tab_medians[ranked[0]] if ranked else 0
        active = sum(value >= max(0.5, 0.2 * peak) for value in tab_medians.values())
        hottest = ranked[:2]
        hottest_share = sum(tab_medians[tab] for tab in hottest) / total if total else 0
        count = sum(map(len, samples.values()))
        pattern = "mixed"
        if len(targets) < 3 or samples.keys() != targets or count < 0.8 * len(observations):
            pattern = "limited coverage"
        elif peak < 0.5:
            pattern = "low signal"
        elif active >= 0.75 * len(targets):
            pattern = "broad across switches"
        elif hottest_share >= 0.55 and peak >= 3 * statistics.median(tab_medians.values()):
            pattern = "activation-concentrated"
        rows.append(dict(pid=process["pid"], created=process["created"], cpu_ms_s=process["cpu_ms_s"],
                         switch_samples=count, active_tabs=active, tab_coverage=len(samples),
                         early_median_ms_by_tab=tab_medians, hottest_tabs=hottest,
                         hottest_two_share=hottest_share, pattern=pattern))
    return sorted(rows, key=lambda row: row["cpu_ms_s"], reverse=True)


def metrics(scenario, power_cases=POWER_CASES):
    if scenario == "memory_tabs":
        return ([f"{state} {field} MiB" for state in MEMORY_STATES
                 for field in ("private working set", "private commit")]
                + [f"{state}{suffix} {field}" for state in IDLE_STATES
                   for suffix in ("", " settled") for field in
                   ("context switches/s", "CPU % one core", "CPU ticks % one core")])
    if scenario == "newtab":
        return ["new tab first paint ms", "new tab visually settled ms"]
    if scenario == "launch_window":
        return ["window presented ms"]
    if scenario == "launch_restore":
        return ["restored paint ms"]
    if scenario == "speedometer":
        return ["Speedometer 3.1 score", "Speedometer CPU ms/s", "Speedometer CPU ticks ms/s"]
    return [f"{case} {field} ms/s" for case in power_cases
            for field in ("cpu", "CPU ticks", "gpu", "activity", "dwm_cpu", "dwm_gpu")]


def describe(values):
    if not values:
        return "—"
    return f"{statistics.median(values):.3f} [{min(values):.3f}–{max(values):.3f}], n={len(values)}"


def summary(config, trials):
    names = [arm[0] for arm in config.arm]
    mode = "Diagnostic" if config.diagnostic else "Smoke only" if config.quick else "Measurement"
    power_column = ["Observed power by arm"] if config.diagnostic else []

    def observations(records):
        if not power_column:
            return []
        states = defaultdict(set)
        for trial in records:
            states[trial["arm"]].update(power_label(power)
                                         for power in trial.get("power_observations", []))
        return ["; ".join(f"{name}: {' / '.join(sorted(states[name]))}" for name in names
                          if states[name]) or "—"]

    lines = ["# Bowser and Helium benchmark", "",
             f"**{mode}** using **{config.sites}** sites.",
             "Cells show median [min–max] and valid trial count. Delta is first arm minus second arm.",
             "A comparison requires valid results for every scheduled repetition on all listed arms.",
             "CPU cycle time is a calibrated estimate. CPU ticks are reported alongside it.", "",
             "| " + " | ".join(["Metric", *(f"{name} median" for name in names),
                               "Delta", "Winner", *power_column]) + " |",
             "| " + " | ".join(["---"] * (len(names) + 3 + len(power_column))) + " |"]
    for scenario in config.scenarios:
        expected = repetitions(config, scenario)
        for metric in metrics(scenario, getattr(config, "power_cases", POWER_CASES)):
            records = [row for row in trials if row["valid"] and metric in row["values"]]
            values = [[row["values"][metric] for row in records if row["arm"] == name]
                      for name in names]
            delta, winner = "—", "incomplete"
            if all(len(series) == expected for series in values):
                medians = [statistics.median(series) for series in values]
                delta = f"{medians[0] - medians[1]:+.3f}" if len(medians) >= 2 else "—"
                if config.quick:
                    winner = "smoke only"
                elif metric in {"Speedometer CPU ms/s", "Speedometer CPU ticks ms/s"}:
                    winner = "—"
                elif len(medians) >= 2:
                    ranked = sorted(zip(medians, names), reverse=metric.startswith("Speedometer"))
                    first, second = ranked[:2]
                    scale = max(abs(first[0]), abs(second[0]))
                    winner = ("photo finish" if scale == 0 or abs(first[0] - second[0]) / scale < 0.02
                              else first[1])
            lines.append("| " + " | ".join([metric, *map(describe, values), delta, winner,
                                             *observations(records)]) + " |")
    lines.extend(["", "## Per-role medians", "",
                  "| " + " | ".join(["Workload", "Category", "Role", *names, *power_column]) + " |",
                  "| " + " | ".join(["---"] * (len(names) + 3 + len(power_column))) + " |"])
    grouped = defaultdict(lambda: defaultdict(lambda: defaultdict(list)))
    group_trials = defaultdict(list)
    for trial in trials:
        if not trial["valid"]:
            continue
        workload_roles = {}
        for state, data in trial.get("memory", {}).items():
            for field, label in (("private_ws", "private working set MiB"), ("commit", "commit MiB")):
                workload_roles[(state, label)] = defaultdict(float)
                for proc in data["members"].values():
                    workload_roles[(state, label)][proc["role"]] += proc[field] / MIB
        for state, data in trial.get("windows", {}).items():
            for field in ("cpu_ms_s", "cpu_tick_ms_s", "cpu_cycles_s", "switches_s", "gpu_ms_s"):
                if field not in data["delta"]:
                    continue
                workload_roles[(state, field)] = defaultdict(float)
                for proc in data["delta"]["processes"]:
                    if proc[field] is not None:
                        workload_roles[(state, field)][proc["role"]] += proc[field]
                if field == "cpu_tick_ms_s":
                    workload_roles[(state, field)]["unattributed"] = data["delta"]["cpu_tick_unattributed_ms_s"]
        for workload, rows in workload_roles.items():
            grouped[workload][trial["arm"]][trial["ordinal"]] = rows
            group_trials[workload].append(trial)
    for (state, field), by_arm in sorted(grouped.items()):
        roles = sorted({role_name for trials_by_arm in by_arm.values()
                        for record in trials_by_arm.values() for role_name in record})
        for role_name in roles:
            values = [[record.get(role_name, 0) for record in by_arm[name].values()] for name in names]
            lines.append("| " + " | ".join([state, field, role_name, *map(describe, values),
                                             *observations(group_trials[(state, field)])]) + " |")
    traces = [row for row in trials if row["valid"] and row.get("switch_renderers")]
    if traces:
        lines.extend(["", "## Renderer activity around tab switches", "",
                      "The first 400 ms of each verified activation span two process-counter reads.",
                      "CPU ms/s uses the full switching window. Tab numbers follow open order.",
                      "A broad or concentrated pattern is temporal evidence. A PID can host multiple frames or tabs.", "",
                      "| Arm | Run | PID | Switching CPU ms/s | Visible idle CPU ms/s | Switch samples | "
                      "Active tabs | Two hottest tabs and share | Pattern | Power state |",
                      "| --- | --- | --- | ---: | ---: | ---: | ---: | --- | --- | --- |"])
        for trial in sorted(traces, key=lambda row: (names.index(row["arm"]), row["ordinal"])):
            idle = trial["windows"].get("idle visible", {}).get("delta", {}).get("processes", [])
            idle_rates = {(proc["pid"], proc["created"]): proc["cpu_ms_s"] for proc in idle
                          if proc["cpu_ms_s"] is not None}
            for proc in trial["switch_renderers"][:6]:
                idle_cpu = idle_rates.get((proc["pid"], proc["created"]))
                idle_value = f"{idle_cpu:.3f}" if idle_cpu is not None else "—"
                hottest = ", ".join(map(str, proc["hottest_tabs"]))
                power = observations([trial])
                lines.append(
                    f"| {trial['arm']} | {trial['ordinal']} | {proc['pid']} | {proc['cpu_ms_s']:.3f} | "
                    f"{idle_value} | {proc['switch_samples']} | {proc['active_tabs']}/{proc['tab_coverage']} | "
                    f"{hottest} ({proc['hottest_two_share']:.0%}) | {proc['pattern']} | "
                    f"{power[0] if power else 'AC, Saver off'} |")
    lines.extend(["", "## Invalid trials", ""])
    for row in trials:
        if not row["valid"] and not row.get("superseded"):
            power = observations([row])
            detail = f" ({power[0]})" if power else ""
            lines.append(f"- {row['file']}{detail} — {row.get('error', 'unfinished')}")
    if all(row["valid"] or row.get("superseded") for row in trials):
        lines.append("None recorded.")
    historical = [row for row in trials if row.get("superseded")]
    if historical:
        lines.extend(["", "## Superseded trials", ""])
        lines.extend(f"- {row['file']} — {row['superseded']}" for row in historical)
    (config.out / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def ambient(config, api, row, job=None, phase="before trial"):
    deadline = time.perf_counter() + config.quiet_wait
    next_notice = time.perf_counter() + 60
    while True:
        observe_power(config, api, row)
        before = api.system_cpu()
        process_before = time.process_time_ns()
        job_before = job.accounting() if job else None
        started = time.perf_counter()
        time.sleep(config.ambient_sample)
        excluded = (time.process_time_ns() - process_before) // 100
        if job:
            excluded += cpu_delta(job_before, job.accounting())
        observation = ambient_load(before, api.system_cpu(), excluded)
        observation.update(phase=phase, seconds=time.perf_counter() - started,
                           stamp=datetime.now(timezone.utc).isoformat())
        row.setdefault("ambient_samples", []).append(observation)
        value = observation["unrelated_cpu_pct"]
        row.setdefault("ambient_cpu_pct", []).append(value)
        observe_power(config, api, row)
        if value <= config.max_ambient_cpu:
            return
        now = time.perf_counter()
        if now >= deadline:
            raise AmbientUnavailable(f"ambient CPU stayed above {config.max_ambient_cpu:.1f}% "
                                     f"for the {config.quiet_wait:g} s quiet wait")
        if now >= next_notice:
            print(f"{row['file']} waiting for quiet ({phase}): {value:.1f}% over "
                  f"{observation['seconds']:.1f} s", flush=True)
            next_notice = now + 60


def long_path(path):
    # The artifact directory plus Chromium's IndexedDB filenames can exceed MAX_PATH.
    return Path("\\\\?\\" + str(path.absolute()))


def template_hash(folder):
    folder = long_path(folder)
    digest = hashlib.sha256()
    for path in sorted(folder.rglob("*")):
        if path.is_file() and path.name != "priming.json":
            digest.update(path.relative_to(folder).as_posix().encode("utf-8") + b"\0")
            digest.update(bytes.fromhex(sha256(path)))
    return digest.hexdigest()


def settle_first_run(trial, row):
    ends = time.perf_counter() + 180
    quiet_seconds = 0
    while time.perf_counter() < ends:
        trial.check_power()
        before = trial.job.accounting()
        start = time.perf_counter()
        time.sleep(5)
        after = trial.job.accounting()
        seconds = time.perf_counter() - start
        one_core = cpu_delta(before, after) / seconds / 1e5
        row.setdefault("settle_samples", []).append(dict(seconds=seconds, cpu_pct_one_core=one_core))
        quiet_seconds = quiet_seconds + seconds if one_core < 2 else 0
        if quiet_seconds >= 20:
            row["quiet_seconds"] = quiet_seconds
            return
    raise RuntimeError(f"{trial.name} profile did not settle below 2% of one core for 20 s")


def ensure_templates(config, api, server, profiles, bin_hashes):
    config.templates = {}
    config.template_records = {}
    if not PRIMED_SCENARIOS.intersection(config.scenarios):
        return
    root = config.out / "profile-templates"
    root.mkdir(exist_ok=True)
    for arm in config.arm:
        name, kind, exe = arm
        template = root / name
        evidence = template / "priming.json"
        setup = dict(env=config.arm_env.get(name, {}), settings=config.arm_settings.get(name, {}))
        if evidence.is_file():
            row = json.loads(evidence.read_text(encoding="utf-8"))
            if (not row["valid"] or row["exe_sha256"] != bin_hashes[name] or row["setup"] != setup
                    or row["sha256"] != template_hash(template)):
                raise RuntimeError(f"profile template failed verification for {name}")
        else:
            attempt = profiles / f"prime-{name}"
            attempt.mkdir()
            row = dict(file=f"profile template {name}", arm=name, valid=False, setup=setup,
                       exe_sha256=bin_hashes[name], started=datetime.now(timezone.utc).isoformat())
            trial = Trial(config, api, server, arm, "profile_template", attempt, row)
            print(f"{name} profile template starting", flush=True)
            try:
                if sha256(exe) != bin_hashes[name]:
                    raise Unsafe(f"{name} executable changed before priming")
                trial.prepare()
                ambient(config, api, row, phase="profile priming")
                require_benchmark_controls(config, api, row)
                page = trial.fixture("color/1")
                trial.launch(page[0], focus=False)
                trial.ready(page, focus=False)
                settle_first_run(trial, row)
                trial.close()
                require_benchmark_controls(config, api, row)
                if kind == "bowser":
                    # Keep the warmed profile and preferences while removing the
                    # priming page's saved tab so the measured launch begins with one tab.
                    for file in ("session.txt", "session.prev", "session.crashed"):
                        (trial.profile / file).unlink(missing_ok=True)
                if sha256(exe) != bin_hashes[name]:
                    raise Unsafe(f"{name} executable changed during priming")
                shutil.move(attempt, template)
                row["sha256"] = template_hash(template)
                row["valid"] = True
                print(f"{name} profile template settled and hashed", flush=True)
            except (Exception, KeyboardInterrupt) as error:
                row["error"] = f"{type(error).__name__}: {error}"
                raise
            finally:
                try:
                    trial.close()
                except Exception as error:
                    row["drain_error"] = str(error)
                    raise
                finally:
                    row["finished"] = datetime.now(timezone.utc).isoformat()
                    row["reports"] = {token: server.reports.pop(token)
                                      for token in row.get("fixture_tokens", [])}
                    if row["valid"]:
                        write_json(evidence, row)
                    else:
                        failure = root / f"{name}-invalid-{datetime.now():%Y%m%d-%H%M%S}.json"
                        write_json(failure, row)
                        if "drain_error" not in row:
                            shutil.rmtree(attempt)
        config.templates[name] = template
        config.template_records[name] = {key: row[key] for key in ("sha256", "started", "finished",
                                                                   "quiet_seconds")}


def repetitions(config, scenario):
    if config.quick:
        return 1
    if scenario == "launch_restore":
        return config.restore_runs
    if scenario == "launch_window":
        return config.launch_window_runs
    return config.runs


def run_trials(config, api, server, profiles, bin_hashes, trials):
    completed = {(row["scenario"], row["ordinal"], row["arm"]) for row in trials if row["valid"]}
    summary(config, trials)
    for scenario in config.scenarios:
        for index in range(repetitions(config, scenario)):
            # Consecutive blocks reverse order. Two arms produce A,B,B,A,A,B,...
            arms = config.arm if index % 2 == 0 else config.arm[::-1]
            for arm in arms:
                name, _kind, exe = arm
                if (scenario, index + 1, name) in completed:
                    continue
                stem = f"{scenario}-{index + 1:02d}-{name}"
                while True:
                    path = config.out / f"{stem}.json"
                    retry = 1
                    while path.exists():
                        path = config.out / f"{stem}-retry-{retry:02d}.json"
                        retry += 1
                    row = dict(scenario=scenario, ordinal=index + 1, arm=name, values={}, valid=False,
                               file=path.name, started=datetime.now(timezone.utc).isoformat())
                    folder = profiles / stem
                    folder.mkdir()
                    trial = Trial(config, api, server, arm, scenario, folder, row)
                    write_json(path, row)
                    print(f"{stem} starting", flush=True)
                    fatal = None
                    power_interrupted = False
                    try:
                        if sha256(exe) != bin_hashes[name]:
                            raise Unsafe(f"{name} executable changed during the run")
                        wait_for_power(config, api, row)
                        trial.prepare()
                        ambient(config, api, row)
                        require_benchmark_controls(config, api, row)
                        row["desktop"] = interactive_desktop()
                        if not row["desktop"]["usable"]:
                            raise Unsafe("the harness desktop is not the active input desktop")
                        trial.run()
                        require_benchmark_controls(config, api, row)
                        if sha256(exe) != bin_hashes[name]:
                            raise Unsafe(f"{name} executable changed during the trial")
                        row["valid"] = True
                    except (Exception, KeyboardInterrupt) as error:
                        row["error"] = f"{type(error).__name__}: {error}"
                        row["traceback"] = traceback.format_exc()
                        power_interrupted = isinstance(error, PowerUnavailable)
                        if isinstance(error, (AmbientUnavailable, PowerWaitExpired, Unsafe, KeyboardInterrupt)):
                            fatal = error
                    finally:
                        try:
                            trial.close()
                        except Exception as error:
                            row["valid"] = False
                            row["drain_error"] = str(error)
                            row["error"] = " | ".join(filter(None, (row.get("error"), str(error))))
                            fatal = error
                        else:
                            try:
                                shutil.rmtree(folder)
                            except OSError as error:
                                row["profile_cleanup_error"] = str(error)
                        tokens = row.get("fixture_tokens", [])
                        row["reports"] = {token: server.reports.pop(token) for token in tokens}
                        row["speedometer_http"] = {token: server.speedometer_traces.pop(token)
                                                   for token in tokens if token in server.speedometer_traces}
                        for token in tokens:
                            server.speedometer_gates.pop(token, None)
                            server.speedometer_finishes.pop(token, None)
                        server.active_speedometer = None
                        row["finished"] = datetime.now(timezone.utc).isoformat()
                        write_json(path, row)
                        trials.append(row)
                        summary(config, trials)
                        print(f"{stem} {'valid' if row['valid'] else row['error']}", flush=True)
                    if fatal:
                        return 2
                    if not power_interrupted:
                        break
    valid = {(row["scenario"], row["ordinal"], row["arm"]) for row in trials if row["valid"]}
    expected = sum(repetitions(config, scenario) for scenario in config.scenarios) * len(config.arm)
    return 0 if len(valid) == expected else 1


def arm_spec(value):
    label, sep, binary = value.partition("=")
    name, delimiter, kind = label.partition(":")
    if not delimiter:
        kind = "helium" if name == "helium" else "bowser"
    path = Path(binary).expanduser().resolve()
    if (not sep or not re.fullmatch(r"[A-Za-z0-9_-]+", name) or kind not in {"bowser", "helium"}
            or not path.is_file()):
        raise argparse.ArgumentTypeError("expected name[:bowser|helium]=existing-exe")
    return name, kind, path


def arm_values(parser, arms, declarations, settings=False):
    values = {}
    reserved = {"LOCALAPPDATA", "APPDATA", "USERPROFILE", "HOME", "BOWSER_DATA_DIR",
                "WEBVIEW2_USER_DATA_FOLDER", "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"}
    for declaration in declarations:
        arm, _, entry = declaration.partition(":")
        key, separator, value = entry.partition("=")
        if (arm not in arms or not separator or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key)
                or any(char in value for char in "\0\r\n")):
            parser.error(f"expected arm:KEY=VALUE, got {declaration!r}")
        if settings and arms[arm] != "bowser":
            parser.error(f"settings.txt applies only to Bowser arms ({arm})")
        if not settings:
            key = key.upper()
            if key in reserved:
                parser.error(f"{key} is reserved for disposable profile isolation")
        arm_options = values.setdefault(arm, {})
        if key in arm_options:
            parser.error(f"duplicate setting for {arm}: {key}")
        arm_options[key] = value
    return values


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arm", action="append", required=True, type=arm_spec)
    parser.add_argument("--arm-env", action="append", default=[], metavar="ARM:NAME=VALUE")
    parser.add_argument("--arm-setting", action="append", default=[], metavar="ARM:KEY=VALUE")
    parser.add_argument("--scenarios", default=",".join(SCENARIOS))
    parser.add_argument("--power-cases", metavar="CASE,CASE",
                        help="select or order power cases: " + ", ".join(POWER_CASES))
    parser.add_argument("--trace-switch-renderers", action="store_true",
                        help="sample renderer CPU counters around each verified power tab switch")
    parser.add_argument("--verify-switch-order", action="store_true",
                        help="require the full 11-tab strip-order itinerary in the switching case")
    parser.add_argument("--direct-newtab-entry", action="store_true",
                        help="type directly after Ctrl+T on the first attempt and record timing and retries")
    parser.add_argument("--require-ac", action="store_true",
                        help="enforce AC and Saver off in diagnostic or smoke mode too")
    parser.add_argument("--sites", choices=("real", "local"), default="real")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--restore-runs", type=int, default=10)
    parser.add_argument("--launch-window-runs", type=int, default=10)
    parser.add_argument("--settle-seconds", type=float, default=300)
    parser.add_argument("--max-ambient-cpu", type=float, default=15)
    parser.add_argument("--max-workload-ambient-cpu", type=float,
                        help="optional separate job-adjusted ceiling for timed workload windows")
    parser.add_argument("--quiet-wait", type=float, default=1800)
    parser.add_argument("--power-wait-seconds", type=float, default=7200,
                        help="wait this long for AC with Battery Saver off in recorded runs")
    parser.add_argument("--ambient-sample", type=float, default=10)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--quick", action="store_true")
    modes.add_argument("--diagnostic", action="store_true",
                       help="use full workloads and label observed AC and Saver state on every result row")
    parser.add_argument("--build-log", type=Path)
    parser.add_argument("--measurement-marker", type=Path,
                        help="create an exclusive coordination file for the duration of a recorded run")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--resume", action="store_true",
                        help="reuse matching completed trials in an existing --out directory")
    parser.add_argument("--resume-site-update", action="store_true",
                        help="reconcile a changed real-site list if no accepted trial used the changed pages")
    parser.add_argument("--resume-after-fix", metavar="REASON",
                        help="record a harness repair that leaves accepted measurement methods unchanged")
    config = parser.parse_args()
    config.scenarios = tuple(config.scenarios.split(","))
    if (not set(config.scenarios) <= set(SCENARIOS) or
            len(set(config.scenarios)) != len(config.scenarios)):
        parser.error("scenarios must be a unique comma-separated subset of " + ",".join(SCENARIOS))
    if config.power_cases:
        config.power_cases = tuple(case.strip() for case in config.power_cases.split(","))
        if ("power" not in config.scenarios or not set(config.power_cases) <= set(POWER_CASES)
                or len(set(config.power_cases)) != len(config.power_cases)):
            parser.error("--power-cases requires power and a unique subset of " + ", ".join(POWER_CASES))
    else:
        config.power_cases = POWER_CASES
    if (config.trace_switch_renderers or config.verify_switch_order) and (
            "power" not in config.scenarios or "switching tabs" not in config.power_cases):
        parser.error("switch order verification and tracing require the switching tabs power case")
    if len({arm[0] for arm in config.arm}) != len(config.arm):
        parser.error("arm labels must be unique")
    arms = {name: kind for name, kind, _path in config.arm}
    config.arm_env = arm_values(parser, arms, config.arm_env)
    config.arm_settings = arm_values(parser, arms, config.arm_setting, settings=True)
    if config.max_workload_ambient_cpu is None:
        config.max_workload_ambient_cpu = config.max_ambient_cpu
    if (min(config.runs, config.restore_runs, config.launch_window_runs) < 1
            or config.settle_seconds < 40 and not config.quick
            or not 0 < config.max_ambient_cpu <= 100 or config.quiet_wait < 0
            or not config.max_ambient_cpu <= config.max_workload_ambient_cpu <= 100
            or config.ambient_sample <= 0 or not 0 <= config.power_wait_seconds <= 7200):
        parser.error("invalid repetitions, settle time or ambient bounds")
    if config.quick:
        config.settle_seconds = 3
    if config.resume and (not config.out or config.quick):
        parser.error("--resume requires --out and a recorded run")
    if config.resume_site_update and (not config.resume or config.sites != "real"):
        parser.error("--resume-site-update requires --resume and --sites real")
    if config.resume_after_fix and not config.resume:
        parser.error("--resume-after-fix requires --resume")
    if config.build_log and not config.quick:
        lines = config.build_log.read_text(encoding="utf-8-sig", errors="replace").splitlines()
        if lines[-1:] != ["EXIT=0"]:
            parser.error("build log must end in EXIT=0 before measurement")
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    label = ("diagnostic" if config.diagnostic else "smoke" if config.quick else "baseline")
    label += "-" + config.sites
    config.out = (config.out or HERE.parent / "results" / "vs-helium" / f"{label}-{stamp}").resolve()
    config.site_list = json.loads((HERE / "sites.json").read_text(encoding="utf-8"))
    return config


def run(config):
    api = Windows()
    config.display = api.display()
    if (config.display["width"] < TEST_RECT[0] + TEST_RECT[2] or
            config.display["height"] < TEST_RECT[1] + TEST_RECT[3]):
        raise RuntimeError("primary screen is too small for matched window placement")
    # A probe owned by this process identifies the selected monitor DPI.
    reference = Backdrop((50, 50, 16, 16))
    try:
        config.dpi = dpi(reference.hwnd)
    finally:
        reference.close()
    config.out.mkdir(parents=True, exist_ok=config.resume)
    cache = Path(os.environ["LOCALAPPDATA"]) / "bowser-bench-cache"
    cache.mkdir(exist_ok=True)
    # Windows storage maintenance can remove files from Temp during a long
    # quiet or AC wait. Keep invocation-owned fixtures and profiles in our cache.
    work = Path(tempfile.mkdtemp(prefix="run-", dir=cache)).resolve()
    print(f"Output {config.out}\nTemporary root {work}", flush=True)
    bin_hashes = {name: sha256(exe) for name, _kind, exe in config.arm}
    server = None
    try:
        archive = prepare_speedometer(cache) if "speedometer" in config.scenarios else None
        video = prepare_video(cache) if "power" in config.scenarios and "video" in config.power_cases else None
        git = subprocess.run(["git", "rev-parse", "HEAD"], cwd=HERE, capture_output=True,
                             text=True, check=False)
        config_data = dict(argv=sys.argv, mode=("diagnostic" if config.diagnostic else
                                              "smoke" if config.quick else "measurement"),
                           sites=config.sites, site_list=config.site_list, scenarios=config.scenarios,
                           power_cases=config.power_cases, trace_switch_renderers=config.trace_switch_renderers,
                           verify_switch_order=config.verify_switch_order,
                           direct_newtab_entry=config.direct_newtab_entry,
                           require_ac=config.require_ac,
                           display=config.display, window_dpi=config.dpi, test_rect=TEST_RECT,
                           cold_rect=COLD_RECT, logical_cores=CORES, host=os.environ.get("COMPUTERNAME"),
                           processor=os.environ.get("PROCESSOR_IDENTIFIER"),
                           windows_version=list(sys.getwindowsversion()),
                           power=api.power(), settle_seconds=config.settle_seconds,
                           max_ambient_cpu=config.max_ambient_cpu,
                           max_workload_ambient_cpu=config.max_workload_ambient_cpu,
                           ambient_sample=config.ambient_sample, quiet_wait=config.quiet_wait,
                           power_wait_seconds=config.power_wait_seconds,
                           idle_window_seconds=1.5 if config.quick else 60,
                           arm_env=config.arm_env, arm_settings=config.arm_settings,
                           repetitions={scenario: repetitions(config, scenario) for scenario in config.scenarios},
                           git_head=git.stdout.strip() if not git.returncode else None,
                           harness_sha256={name: sha256(HERE / name)
                                           for name in ("bench.py", "fixtures.py", "native.py",
                                                        "helium-startup.ps1", "sites.json")},
                           arms={name: dict(kind=kind, executable=str(exe), sha256=bin_hashes[name],
                                            file_version=file_version(exe))
                                 for name, kind, exe in config.arm},
                           build_log=str(config.build_log) if config.build_log else None,
                           speedometer=dict(revision=REVISION, sha256=ARCHIVE_HASH) if archive else None,
                           video_sha256=sha256(video) if video else None,
                           temp_root=str(work))
        manifest = config.out / "manifest.json"
        trials = []
        if config.resume:
            previous = json.loads(manifest.read_text(encoding="utf-8"))
            previous.setdefault("power_cases", list(POWER_CASES))
            previous.setdefault("trace_switch_renderers", False)
            previous.setdefault("verify_switch_order", False)
            previous.setdefault("direct_newtab_entry", False)
            previous.setdefault("require_ac", False)
            # Compare JSON-normalized controls. Record the new invocation and keep the
            # original command and artifact identity alongside the resumed trials.
            current = json.loads(json.dumps(config_data))
            for file in sorted(config.out.glob("*.json")):
                if file != manifest:
                    trials.append(json.loads(file.read_text(encoding="utf-8")))
            variable = {"argv", "temp_root", "repetitions", "invocations", "quiet_wait",
                        "previous_controls", "profile_templates", "previous_site_lists",
                        "harness_repairs", "power", "power_wait_seconds", "session_power_gate",
                        "idle_window_seconds"}
            if set(current["scenarios"]) == set(previous["scenarios"]):
                variable.add("scenarios")
            for row in trials:
                if (row["valid"] and row["scenario"] == "memory_tabs"
                        and not any("CPU ticks % one core" in name for name in row["values"])):
                    row.update(valid=False, superseded="60-second cycle-calibrated idle windows")
            if "harness_repairs" in previous:
                config_data["harness_repairs"] = previous["harness_repairs"]
            if (config.resume_after_fix and
                    current["harness_sha256"] != previous["harness_sha256"]):
                config_data["harness_repairs"] = [
                    *previous.get("harness_repairs", []),
                    dict(reason=config.resume_after_fix, prior_sha256=previous["harness_sha256"],
                         kept_trials=[row["file"] for row in trials if row["valid"]],
                         superseded_trials=[row["file"] for row in trials if row.get("superseded")]),
                ]
                variable.add("harness_sha256")
            workload_accepted = any(row["valid"] and row["scenario"] in
                                    {"memory_tabs", "power", "speedometer"} for row in trials)
            prior_workload_limit = previous.get("max_workload_ambient_cpu", previous["max_ambient_cpu"])
            workload_limit = current["max_workload_ambient_cpu"]
            if (not workload_accepted or workload_limit == prior_workload_limit or
                    (config.diagnostic and config.resume_after_fix and workload_limit > prior_workload_limit)):
                variable.add("max_workload_ambient_cpu")
                if workload_limit != prior_workload_limit:
                    # Apply the changed gate only to fresh measurements.
                    config_data["previous_controls"] = [
                        *previous.get("previous_controls", []),
                        dict(max_workload_ambient_cpu=prior_workload_limit,
                             reason=config.resume_after_fix),
                    ]
            if config.resume_site_update and current["site_list"] != previous["site_list"]:
                used = {"memory_tabs": 20, "power": 10}
                for row in trials:
                    count = used.get(row["scenario"], 0)
                    if row["valid"] and previous["site_list"][:count] != current["site_list"][:count]:
                        raise RuntimeError(f"site change overlaps an accepted {row['scenario']} trial")
                config_data["previous_site_lists"] = [
                    *previous.get("previous_site_lists", []),
                    dict(site_list=previous["site_list"], harness_sha256=previous["harness_sha256"],
                         max_workload_ambient_cpu=prior_workload_limit),
                ]
                variable |= {"site_list", "harness_sha256"}
            if not any(row["valid"] for row in trials):
                mutable = {"harness_sha256", "max_ambient_cpu", "ambient_sample", "git_head",
                           "arm_env", "arm_settings"}
                if any(current[key] != previous.get(key) for key in mutable):
                    config_data["previous_controls"] = [
                        *previous.get("previous_controls", []),
                        {key: previous.get(key) for key in sorted(mutable)},
                    ]
                variable |= mutable
            for key in current.keys() - variable:
                if current[key] != previous.get(key):
                    raise RuntimeError(f"cannot resume: manifest setting {key} changed")
            if any(count < previous["repetitions"][scenario]
                   for scenario, count in current["repetitions"].items()):
                raise RuntimeError("cannot resume with fewer repetitions")
            invocations = previous.get("invocations", [previous["argv"]])
            config_data["invocations"] = [*invocations, config_data["argv"]]
            if "previous_controls" in previous:
                config_data.setdefault("previous_controls", previous["previous_controls"])
            if "previous_site_lists" in previous:
                config_data.setdefault("previous_site_lists", previous["previous_site_lists"])
            if "profile_templates" in previous:
                config_data["profile_templates"] = previous["profile_templates"]
        write_json(manifest, config_data)
        summary(config, trials)
        server = Fixtures(work, video, work / f"Speedometer-{REVISION}" if archive else None,
                          archive, quick=config.quick)
        profiles = work / "profiles"
        profiles.mkdir()
        power_gate = dict(file=config.out.name)
        config_data["session_power_gate"] = power_gate
        try:
            wait_for_power(config, api, power_gate)
            while True:
                try:
                    ensure_templates(config, api, server, profiles, bin_hashes)
                    break
                except PowerUnavailable:
                    wait_for_power(config, api, power_gate)
        finally:
            write_json(manifest, config_data)
        config_data["profile_templates"] = config.template_records
        write_json(manifest, config_data)
        result = run_trials(config, api, server, profiles, bin_hashes, trials)
    finally:
        if server:
            server.close()
        profiles = work / "profiles"
        if not profiles.exists() or not any(profiles.iterdir()):
            shutil.rmtree(work)
    return result


def main():
    config = arguments()
    marker = config.measurement_marker
    if not marker:
        return run(config)
    stream = marker.open("x", encoding="utf-8")
    try:
        with stream:
            stream.write(f"{config.out.name}\n{datetime.now(timezone.utc).isoformat()}\n")
        return run(config)
    finally:
        marker.unlink()


if __name__ == "__main__":
    sys.exit(main())
