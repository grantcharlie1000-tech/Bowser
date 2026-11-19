"""Collect Windows trials. See README.md for the frozen native campaign contract."""

import argparse
from hashlib import file_digest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from itertools import permutations
import json
from math import isfinite
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import subprocess
import sys
from threading import Event, Thread
from time import perf_counter_ns, sleep, time_ns

from win_job import Job, Windows


ROOT = Path(__file__).resolve().parent
ARMS = ("main", "candidate", "helium")
FIXTURE = {"light": (24, [320, 180]), "demanding": (2400, [1200, 675])}
ORDERS = list(permutations(ARMS))
FRAMES = 900
IDLE_SECONDS = 30


def digest(path):
    with path.open("rb") as handle:
        return file_digest(handle, "sha256").hexdigest()


def file_identity(path):
    stat = path.stat()
    return [stat.st_size, stat.st_mtime_ns]


def stamp():
    before = perf_counter_ns()
    utc = time_ns()
    after = perf_counter_ns()
    return {"qpc_ns": (before + after) // 2, "utc_ns": utc, "clock_read_span_ns": after - before}


def wait_until(test, seconds, message, interval=0.01):
    deadline = perf_counter_ns() + seconds * 1e9
    while perf_counter_ns() < deadline:
        value = test()
        if value:
            return value
        sleep(interval)
    raise RuntimeError(message)


class FixtureServer(ThreadingHTTPServer):
    daemon_threads = True
    attempt = None


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def match_attempt(self):
        row = self.server.attempt
        if row is None or not self.path.startswith(f"/{row['token']}/"):
            self.send_error(404)
            return None, None
        return row, self.path.rsplit("/", 1)[-1]

    def reply(self, kind, body):
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        row, path = self.match_attempt()
        if path == "index.html":
            fixture = self.server.fixtures[row["fixture"]]
            probe = b'<script defer src="probe.js"></script>'
            title = f"BOWSER_BENCH_READY_{row['fixture'].upper()}"
            fixture = fixture.replace(title.encode(), row["title_marker"].encode())
            self.reply("text/html", fixture.replace(b"</head>", probe + b"</head>"))
        elif path == "probe.js":
            self.reply("text/javascript", self.server.probe)
        elif path == "control":
            if self.server.control.wait(140):
                self.reply("application/json", json.dumps(self.server.command).encode())
            else:
                self.send_error(408)
        elif row is not None:
            self.send_error(404)

    def do_POST(self):
        at = stamp()
        row, phase = self.match_attempt()
        if row is None:
            return
        if phase not in ("ready", "done") or phase in row["beacons"]:
            self.send_error(400)
            return
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 < length <= 100_000:
            self.send_error(400)
            return
        data = json.loads(self.rfile.read(length))
        self.reply("text/plain", b"ok")
        row["beacons"][phase] = {"received": at, **data}
        self.server.beacon_events[phase].set()


def parse_viewport(text):
    size, dpr = text.split("@")
    width, height = map(int, size.lower().split("x"))
    scale = float(dpr)
    if width < 1 or height < 1 or not isfinite(scale) or scale <= 0:
        raise argparse.ArgumentTypeError("viewport must have positive dimensions and scale")
    return [width, height, scale]


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def check_power(state, expected_ac, initial_saver):
    if state["ac"] != expected_ac or state["saver"] != initial_saver:
        raise RuntimeError(f"power source or battery saver changed: {state}")


def check_ready(row, expected_viewport):
    ready = row["beacons"]["ready"]
    count, canvas = FIXTURE[row["fixture"]]
    if (ready["title"] != row["title_marker"] or ready["count"] != count or ready["canvas"] != canvas
            or not ready["visible"] or not 0 < ready["fcp_ms"] < 90_000):
        raise RuntimeError(f"fixture readiness/semantics failed: {ready}")
    if expected_viewport and ready["viewport"] != expected_viewport:
        raise RuntimeError(f"viewport {ready['viewport']} does not match {expected_viewport}")
    if row.get("workload") == "animation" and (ready["viewport"][0] < 660 or ready["viewport"][1] < 340):
        raise RuntimeError("viewport crops the optical animation fixture")


def trial(api, server, config, row, profile, attempt_file, initial_power, foreground_events):
    arm = row["arm"]
    exe = config[arm]
    browser_class = "Chrome_WidgetWin_1" if arm == "helium" else "BowserNativeWindow"
    token = secrets.token_hex(8)
    row.update(token=token, title_marker=f"Bench-ready-{token}", valid=False, drained=False, beacons={})
    row["profile"] = str(profile)
    write_json(attempt_file, row)
    url = f"http://127.0.0.1:{server.server_port}/{token}/index.html"
    profile.mkdir(exist_ok=True, parents=True)
    env = dict(os.environ)
    # The inputs in the repository's standalone runner are opt-in. Inherited test overrides
    # would make one run differ from the next even with the same binaries.
    inherited = [key for key in env if key.upper().startswith(("BOWSER_", "WEBVIEW2_"))]
    for key in inherited:
        del env[key]
    if arm == "helium":
        args = [f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
                "--new-window", f"--window-size={config['helium_size']}", url]
    else:
        local = profile.parent / "local-appdata"
        local.mkdir(exist_ok=True)
        env["LOCALAPPDATA"] = str(local)
        (profile / "import-offer.txt").write_text("answered\n", encoding="ascii")
        args = ["--data-dir", str(profile), "--open-url", url]
        if config["diagnostic"]:
            # Separate Bowser-only lane. Primary analysis refuses this collection.
            args += ["--diagnostic-log", str(attempt_file.with_suffix(".events.csv"))]
    row.update(executable=str(exe), args=args, removed_inherited_overrides=inherited)
    write_json(attempt_file, row)
    control = Event()
    server.attempt, server.control, server.command = row, control, {"animation_frames": 0}
    server.beacon_events = {"ready": Event(), "done": Event()}
    job = Job(api)
    try:
        row["power_before"] = api.power()
        check_power(row["power_before"], config["ac"], initial_power["saver"])
        if api.display() != config["display"]:
            raise RuntimeError("display control changed")
        row["launch"] = stamp()
        row["launcher_pid"] = job.start(exe, args, env)

        def observe():
            window = job.window(api.GetForegroundWindow(), browser_class)
            if window is None:
                return False
            if "first_owned_foreground_window" not in row:
                row["first_owned_foreground_window"] = {**window, "observed": stamp()}
            return window if row["title_marker"] in window["title"] else False

        window = wait_until(observe, 90, "owned foreground fixture title absent")
        row["title_observed"] = {**window, "observed": stamp()}
        if not server.beacon_events["ready"].wait(8):
            raise RuntimeError("fixture readiness beacon absent")
        check_ready(row, config["viewport"])

        def foreground():
            if api.GetForegroundWindow() != window["hwnd"] or job.window(window["hwnd"], browser_class) is None:
                raise RuntimeError("owned fixture window lost foreground or visibility")

        if row["prime"]:
            control.set()
        else:
            sleep(5)  # steady phase starts after equally settled content
            foreground()
            row["memory_before"] = job.memory()
            row["host_before"] = api.system_cpu()
            row["cpu_before"] = job.accounting()
            row["work_start"] = stamp()
            if row["workload"] == "animation":
                server.command = {"animation_frames": FRAMES}
                control.set()
                if not server.beacon_events["done"].wait(90):
                    raise RuntimeError("animation work incomplete")
                row["work_end"] = row["beacons"]["done"]["received"]
            else:
                sleep(IDLE_SECONDS)
                row["work_end"] = stamp()
            row["cpu_after"] = job.accounting()
            row["host_after"] = api.system_cpu()
            foreground()
            row["memory_after"] = job.memory()
            control.set()
            if not server.beacon_events["done"].wait(8):
                raise RuntimeError("work completion beacon absent")
            done = row["beacons"]["done"]
            if not done["visible"] or done["visibility"] or done["viewport"] != row["beacons"]["ready"]["viewport"]:
                raise RuntimeError("document visibility or viewport changed")
            if row["workload"] == "animation":
                times = done["timestamps"]
                if len(times) != FRAMES or any(a >= b for a, b in zip(times, times[1:])):
                    raise RuntimeError("animation callback sequence is incomplete")
            for key in ("memory_before", "memory_after"):
                if not row[key] or any("error" in process or not process["image"] for process in row[key]):
                    raise RuntimeError(f"incomplete native memory or image identity in {key}")
                if arm != "helium":
                    runtime = {str(Path(process["image"])).casefold() for process in row[key]
                               if Path(process["image"]).name.lower() == "msedgewebview2.exe"}
                    if runtime != {str(config["webview_exe"]).casefold()}:
                        raise RuntimeError(f"unexpected WebView2 process images: {runtime}")
            row["foreground_events"] = [event for event in foreground_events
                                        if row["first_owned_foreground_window"]["observed"]["qpc_ns"]
                                        < event["qpc_ns"] <= row["work_end"]["qpc_ns"]]
            if any(event["hwnd"] != window["hwnd"] for event in row["foreground_events"]):
                raise RuntimeError("foreground changed during observation")
            kernel = row["host_after"]["kernel_100ns"] - row["host_before"]["kernel_100ns"]
            user = row["host_after"]["user_100ns"] - row["host_before"]["user_100ns"]
            idle = row["host_after"]["idle_100ns"] - row["host_before"]["idle_100ns"]
            own = sum(row["cpu_after"][key] - row["cpu_before"][key] for key in ("user_100ns", "kernel_100ns"))
            row["ambient_cpu_fraction"] = max(0, kernel + user - idle - own) / (kernel + user)
            if row["ambient_cpu_fraction"] > 0.05:
                raise RuntimeError("ambient CPU exceeded 5% of machine capacity")
        row["power_after"] = api.power()
        check_power(row["power_after"], config["ac"], initial_power["saver"])
        row["valid"] = True
    except Exception as error:
        row["error"] = str(error)
    finally:
        control.set()
        try:
            job.request_close()
            wait_until(lambda: not job.accounting()["active"], 35, "owned process group did not exit", 0.2)
            row["job_final"] = job.accounting()
            # A process launched through an external broker can exist outside the inherited job.
            # Check profile references only after the measured interval. Never kill other processes.
            escaped = escaped_profile_owners(profile)
            if escaped:
                row["escaped_profile_processes"] = escaped
                raise RuntimeError("profile-owned process survived job drain")
            row["drained"] = True
        except Exception as error:
            row.update(valid=False, drain_error=str(error), remaining_job_pids=job.pids())
        finally:
            job.close()
            server.attempt = None
            write_json(attempt_file, row)
    return row["valid"]


def escaped_profile_owners(profile):
    # Names and IDs only, to avoid retaining unrelated command lines. Matching is done in PowerShell.
    argument = str(profile).replace("'", "''")
    cmd = (f"$p='{argument}'; Get-CimInstance Win32_Process -ErrorAction Stop | "
           "Where-Object { $_.ProcessId -ne $PID -and $_.ProcessId -gt 4 -and $_.CommandLine "
           "-and ($_.CommandLine.IndexOf($p,[StringComparison]::OrdinalIgnoreCase) -ge 0 "
           "-or $_.CommandLine.IndexOf($p.Replace('\','/'),[StringComparison]::OrdinalIgnoreCase) -ge 0) } | "
           "Select-Object ProcessId,Name | ConvertTo-Json -Compress")
    result = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", cmd],
                            check=True, capture_output=True, text=True, timeout=25)
    return json.loads(result.stdout) if result.stdout.strip() else []


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for arm in ARMS:
        cli.add_argument(f"--{arm}", type=Path, required=True)
    cli.add_argument("--build-identities", type=Path, required=True, help="frozen JSON identity manifest")
    cli.add_argument("--setup-record", type=Path, required=True, help="hardware, OS and test-session record")
    cli.add_argument("--out", type=Path, required=True, help="new directory outside all real profiles")
    cli.add_argument("--helium-size", required=True, help="calibrated Helium outer width,height")
    cli.add_argument("--viewport", type=parse_viewport, help="expected inner WIDTHxHEIGHT@DPR")
    cli.add_argument("--power", required=True, choices=["ac", "battery"])
    cli.add_argument("--pilot", action="store_true")
    cli.add_argument("--diagnostic", action="store_true", help="separate Bowser-only pilot with event logging")
    args = cli.parse_args()
    if sys.platform != "win32":
        cli.error("native capture requires Windows")
    if not args.viewport and not args.pilot:
        cli.error("final collection requires a viewport matched in a pilot")
    if not re.fullmatch(r"[1-9][0-9]*,[1-9][0-9]*", args.helium_size):
        cli.error("Helium outer dimensions must be width,height in pixels")
    if (os.cpu_count() or 1) > 64:
        cli.error("GetSystemTimes ambient CPU accounting requires a single processor group")
    if args.diagnostic and not args.pilot:
        cli.error("diagnostic logging is only supported in a separate pilot")
    api = Windows()
    config = {arm: getattr(args, arm).resolve(strict=True) for arm in ARMS}
    baseline_arm = ("main", "candidate") if args.diagnostic else ARMS
    identities = json.loads(args.build_identities.read_text(encoding="utf-8-sig"))
    if not re.fullmatch(r"[0-9a-f]{40}", identities["main"]["source"]):
        cli.error("the Bowser reference requires a full Git commit SHA")
    hashes = {arm: digest(path) for arm, path in config.items()}
    if any(identities[arm]["sha256"].lower() != hashes[arm] for arm in ARMS):
        cli.error("exe integrity failed against the build identity manifest")
    if len(set(hashes.values())) != len(ARMS):
        cli.error("the three pinned arms require distinct binaries")
    display = api.display()
    if display["monitors"] != 1 or display["refresh_hz"] <= 1:
        cli.error(f"use one interactive physical display at fixed refresh: {display}")
    power = api.power()
    ac = int(args.power == "ac")
    check_power(power, ac, power["saver"])
    webview_exe = Path(identities["webview2"]["exe"]).resolve(strict=True)
    webview_sha256 = identities["webview2"]["sha256"].lower()
    if digest(webview_exe) != webview_sha256:
        cli.error("WebView2 integrity failed against the build identity manifest")
    frozen_images = {path: (file_identity(path), digest(path)) for path in [*config.values(), webview_exe]}
    config.update(helium_size=args.helium_size, viewport=args.viewport, ac=ac, webview_exe=webview_exe,
                  display=display, diagnostic=args.diagnostic)
    foreground_events = api.foreground_events()
    out = args.out.resolve()
    out.mkdir()
    shutil.copyfile(args.build_identities, out / "build-identities.json")
    shutil.copyfile(args.setup_record, out / "setup-record.txt")
    fixture_paths = {kind: ROOT.parent / "fixtures" / f"{kind}.html" for kind in FIXTURE}
    frozen = sorted([*ROOT.glob("*.py"), *ROOT.glob("*.md"), *ROOT.glob("*.js"), *ROOT.glob("*.ps1"),
                     *fixture_paths.values()])
    write_json(out / "campaign.json", {
        "contract_sha256": {str(p.relative_to(ROOT.parent)): digest(p) for p in frozen},
        "exe_sha256": hashes, "python": sys.version, "platform": platform.platform(),
        "config": {k: str(v) if isinstance(v, Path) else v for k, v in config.items()},
        "initial_power": power, "clock": stamp(), "pilot": args.pilot,
        "idle_seconds": IDLE_SECONDS, "animation_frames": FRAMES,
    })
    server = FixtureServer(("127.0.0.1", 0), Handler)
    server.fixtures = {kind: path.read_bytes() for kind, path in fixture_paths.items()}
    server.probe = (ROOT / "probe.js").read_bytes()
    Thread(target=server.serve_forever, daemon=True).start()
    failures = 0
    try:
        for fixture, workload in (("light", "idle"), ("demanding", "idle"), ("demanding", "animation")):
            for warmth in ("fresh", "warm"):
                for block, order in enumerate([baseline_arm] if args.pilot else ORDERS * 2, start=1):
                    for position, arm in enumerate(order, start=1):
                        label = f"{fixture}-{workload}-{warmth}-{block:02d}-{position}-{arm}"
                        profile = out / "profiles" / label / "data"
                        phases = (True, False) if warmth == "warm" else (False,)
                        for prime in phases:
                            if any(file_identity(path) != identity for path, (identity, _) in frozen_images.items()):
                                raise RuntimeError("a frozen browser executable changed during collection")
                            row = dict(arm=arm, fixture=fixture, workload=workload, warmth=warmth,
                                       block=block, position=position, prime=prime, arm_order=order)
                            attempt = out / f"{label}{'-prime' if prime else ''}.json"
                            valid = trial(api, server, config, row, profile, attempt, power, foreground_events)
                            print(f"{attempt.name} {'valid' if valid else 'INVALID'}", flush=True)
                            if not row["drained"]:
                                raise RuntimeError(f"unsafe to launch another trial; see {attempt}")
                            if not valid:
                                failures += 1
                                break
                        sleep(2)
    finally:
        server.shutdown()
        server.server_close()
    if any(digest(path) != expected for path, (_, expected) in frozen_images.items()):
        raise RuntimeError("postflight exe digest mismatch")
    (out / "complete.txt").write_text("Capture finished and postflight executable hashes matched.\n", encoding="ascii")
    return int(failures != 0)


if __name__ == "__main__":
    sys.exit(main())
