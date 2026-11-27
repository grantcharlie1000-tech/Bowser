"""Inspect renderer ownership in one instrumented Bowser restore."""

import argparse
import csv
import ctypes
from datetime import datetime, timezone
import os
from pathlib import Path
import shutil
import tempfile
import threading
import time
from types import SimpleNamespace
from urllib.parse import urlsplit

import bench
from fixtures import Fixtures, Handler, page, sha256
from native import Backdrop, OwnedJob, Windows, command_line, dpi, inventory, role


PULSE = b"""<script>
addEventListener('load', async () => {
    const token = location.pathname.split('/')[1];
    if (!(await fetch('/probe/' + token)).ok) return;
    const report = kind => fetch('/report/' + token, {
        method: 'POST', body: JSON.stringify({kind, visible: !document.hidden})
    });
    await report('probe-start');
    const end = performance.now() + 250;
    while (performance.now() < end) {}
    await report('probe-end');
});
</script>"""


class ProbeHandler(Handler):
    def do_GET(self):
        parts = urlsplit(self.path).path.strip('/').split('/')
        if len(parts) == 2 and parts[0] == 'probe' and parts[1] in self.server.reports:
            if self.server.pulse.wait(15):
                return self.respond(b'ready', 'text/plain')
            return self.send_error(503)
        if len(parts) == 3 and parts[0] in self.server.reports and parts[1:] == ['color', '1']:
            body = page(parts[0], 'color', 1)
            return self.respond(body + (PULSE if self.server.probing else b''), 'text/html; charset=utf-8')
        return super().do_GET()


def qpc():
    counter = ctypes.c_int64()
    ctypes.windll.kernel32.QueryPerformanceCounter(ctypes.byref(counter))
    return counter.value


class Probe(bench.Trial):
    def launch(self, url=None, cold=False, focus=True):
        if not cold:
            return super().launch(url, cold, focus)
        bench.observe_power(self.config, self.api, self.row)
        self.server.probing = True
        env = {key: value for key, value in os.environ.items()
               if not key.upper().startswith(('BOWSER_', 'WEBVIEW2_'))}
        env.update(LOCALAPPDATA=str(self.folder / 'local-appdata'), BOWSER_LOG_RENDERER_PIDS='1')
        args = ['--data-dir', str(self.profile), '--benchmark-log', str(self.config.events)]
        entry = dict(args=args, exe=str(self.exe))
        self.row['launches'].append(entry)
        self.job = OwnedJob(self.api)
        start, pid = self.job.launch(self.exe, args, env, self.rect)
        entry.update(start=start, pid=pid)
        self.sampler = threading.Thread(target=self.sample, args=(self.job,))
        self.sampler.start()
        return start

    def sample(self, job):
        metadata = {}
        try:
            while not self.stop.is_set():
                tick = qpc()
                stamp, processes = inventory()
                members = []
                for pid in job.pids():
                    proc = processes.get(pid)
                    if proc is None or not job.contains(pid):
                        continue
                    identity = pid, proc['created']
                    if identity not in metadata:
                        cmd = command_line(self.api, pid)
                        metadata[identity] = dict(role=role(proc['image'], cmd), command=cmd)
                    members.append(dict(pid=pid, created=proc['created'], image=proc['image'],
                                        cycles=job.process_cycles(pid, proc['created']), **metadata[identity]))
                self.row['samples'].append(dict(qpc_start=tick, qpc_end=qpc(), stamp=stamp, members=members))
                self.stop.wait(.02)
        except Exception as error:
            self.row['sampling_error'] = str(error)


def measure(config, exe, out):
    api = Windows()
    config.display = api.display()
    reference = Backdrop((50, 50, 16, 16))
    try:
        config.dpi = dpi(reference.hwnd)
    finally:
        reference.close()
    cache = (Path(os.environ['LOCALAPPDATA']) / 'bowser-bench-cache').resolve()
    cache.mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='restore-probe-', dir=cache)).resolve()
    row = dict(file='renderer-probe', values={}, samples=[], sha256=sha256(exe), valid=False)
    server = Fixtures(work, None, None, None)
    server.RequestHandlerClass = ProbeHandler
    server.pulse, server.probing = threading.Event(), False
    trial = Probe(config, api, server, (exe.parent.name, 'bowser', exe), 'launch_restore', work, row)
    trial.stop, trial.sampler = threading.Event(), None
    drained = False
    try:
        bench.wait_for_power(config, api, row)
        trial.prepare()
        bench.ambient(config, api, row, phase='before probe priming')
        trial.cold(restore=True)
        server.pulse.set()
        token = row['fixture_tokens'][0]
        end = bench.wait_until(lambda: trial.report(token, 'probe-end'), 7, 'content CPU pulse did not finish')
        if not end[-1]['visible']:
            raise RuntimeError('restored tab was hidden during the content CPU pulse')
        # Chrome can reveal after the page paints, and WebView2 frame snapshots
        # arrive asynchronously. Keep the browser alive for those log entries.
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            trial.check_power()
            time.sleep(min(.25, max(0, deadline - time.monotonic())))
        bench.require_benchmark_controls(config, api, row)
        if row.get('sampling_error'):
            raise RuntimeError(row['sampling_error'])
        row['valid'] = True
    except BaseException as error:
        row['error'] = f'{type(error).__name__}: {error}'
        raise
    finally:
        trial.stop.set()
        if trial.sampler:
            trial.sampler.join()
        try:
            trial.close()
            drained = True
            row['fixture_reports'] = server.reports
            if config.events.exists():
                with config.events.open(encoding='utf-8', newline='') as stream:
                    row['pid_events'] = [event for event in csv.DictReader(stream)
                                         if event['event'].startswith('renderer_pid_')]
            if row['valid'] and not any(e['event'] == 'renderer_pid_content_loading'
                                        for e in row.get('pid_events', [])):
                raise RuntimeError('benchmark log has no renderer snapshot at content loading')
        except Exception as error:
            row.update(valid=False, cleanup_error=str(error))
            raise
        finally:
            server.close()
            bench.write_json(out / 'renderer-probe.json', row)
            if drained and work.parent == cache:
                shutil.rmtree(work)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--measurement-marker', type=Path, required=True)
    args = parser.parse_args()
    exe = args.exe.resolve(strict=True)
    out = args.out.resolve(strict=True)
    subdir = out / 'renderer-probe'
    with args.measurement_marker.open('x', encoding='utf-8') as marker:
        try:
            marker.write(f'{out.name} renderer probe\n{datetime.now(timezone.utc).isoformat()}\n')
            marker.close()
            subdir.mkdir()
            config = SimpleNamespace(diagnostic=True, quick=False, require_ac=True, power_wait_seconds=7200,
                                     quiet_wait=1800, ambient_sample=10, max_ambient_cpu=15,
                                     arm_env={}, arm_settings={}, events=subdir / 'events.csv')
            measure(config, exe, subdir)
        finally:
            args.measurement_marker.unlink()


if __name__ == '__main__':
    main()
