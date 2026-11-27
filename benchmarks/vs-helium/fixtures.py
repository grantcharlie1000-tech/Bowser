"""Loopback workloads and pinned Speedometer distribution."""

import hashlib
import html
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import re
import shutil
import subprocess
import tarfile
import threading
import time
from urllib.parse import urlsplit
from urllib.request import urlopen


REVISION = "1386415be8fef2f6b6bbdbe1828872471c5d802a"
ARCHIVE_HASH = "cfefa818d7789ed2f2b9f3f1bf608cc35e4e8241489eef47049296ce92791313"
ARCHIVE_URL = f"https://codeload.github.com/WebKit/Speedometer/tar.gz/{REVISION}"
MAGENTA = 0xFF00FF


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def prepare_speedometer(cache):
    archive = cache / f"speedometer-{REVISION}.tar.gz"
    if not archive.exists():
        temporary = archive.with_suffix(".download")
        with urlopen(ARCHIVE_URL, timeout=45) as remote, temporary.open("wb") as output:
            while chunk := remote.read(1024 * 1024):
                output.write(chunk)
        if sha256(temporary) != ARCHIVE_HASH:
            raise RuntimeError("Speedometer download SHA-256 mismatch")
        temporary.replace(archive)
    if sha256(archive) != ARCHIVE_HASH:
        raise RuntimeError("Cached Speedometer SHA-256 mismatch")
    # Verify and extract the archive each invocation. Execution never trusts stale extracted files.
    return archive


def prepare_video(cache):
    # Fixed workload, with meaningful motion and enough headroom for software decoding.
    video = cache / "testsrc2-1080p30-h264-crf21-12s-v1.mp4"
    if not video.exists():
        temporary = video.with_suffix(".tmp.mp4")
        subprocess.run([
            "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi",
            "-i", "testsrc2=size=1920x1080:rate=30", "-t", "12", "-an", "-c:v", "libx264",
            "-preset", "medium", "-crf", "21", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
            str(temporary),
        ], check=True, timeout=180)
        temporary.replace(video)
    return video


REPORT = """
const benchReport = (kind, data={}) => fetch('/report/'+location.pathname.split('/')[1],
    {method:'POST', body:JSON.stringify({kind, ...data})}).catch(()=>{});
addEventListener('load', () => requestAnimationFrame(() => requestAnimationFrame(() =>
    benchReport('ready',{width:innerWidth,height:innerHeight,dpr:devicePixelRatio,
                         visible:!document.hidden,title:document.title}))));
"""

SPEEDOMETER_REPORTER = """
const benchClient = globalThis.benchmarkClient;
const benchToken = "__BENCH_TOKEN__";
const benchPost = body => fetch('/report/' + benchToken,
    {method:'POST', body:JSON.stringify(body)});
addEventListener('DOMContentLoaded', async () => {
    const gate = await fetch('/gate/' + benchToken);
    if (gate.ok) benchClient.start();
});
const originalStart = benchClient.willStartFirstIteration;
benchClient.willStartFirstIteration = function(iterations) {
    originalStart.call(this);
    benchPost({kind:'score-start',iterations});
};
if (__BENCH_DIAGNOSTICS__) {
    let lastSuite;
    const originalTest = benchClient.willRunTest;
    benchClient.willRunTest = function(suite, test) {
        if (suite.name !== lastSuite) {
            lastSuite = suite.name;
            benchPost({kind:'score-suite', suite:lastSuite});
        }
        return originalTest.call(this, suite, test);
    };
}
const originalPopulate = benchClient._populateValidScore;
benchClient._populateValidScore = function(results) {
    originalPopulate.call(this, results);
    benchPost({
        kind:'score', score:results.mean, iterations:this._measuredValuesList.length,
        visible:!document.hidden, width:innerWidth, height:innerHeight, dpr:devicePixelRatio
    }).then(async () => {
        const gate = await fetch('/finish/' + benchToken);
        if (gate.ok) location.replace('/' + benchToken + '/site/1');
    });
};
const invalidPopulate = benchClient._populateInvalidScore;
benchClient._populateInvalidScore = function() {
    invalidPopulate.call(this);
    benchPost({kind:'score-error', reason:'Speedometer marked its result invalid'});
};
"""

SPEEDOMETER_DIAGNOSTICS = """
<script>
const reportSpeedometerError = reason => fetch('/report/__BENCH_TOKEN__',
    {method:'POST', body:JSON.stringify({kind:'score-error',reason:String(reason).slice(0,500)})});
addEventListener('error', e => {
    if (e instanceof ErrorEvent) reportSpeedometerError(e.message);
    if (e.target instanceof HTMLScriptElement) reportSpeedometerError('Could not load '+e.target.src);
},true);
addEventListener('unhandledrejection', e => reportSpeedometerError(e.reason));
</script>
"""


def page(token, kind, index):
    title = f"Bench-{token}-{kind}-{index}"
    style = ""
    scripts = REPORT
    content = ""
    if kind == "color":
        style = "html,body{background:#ff00ff;margin:0;height:100%}"
    if kind in {"site", "article"}:
        count = 2200 if kind == "article" else 18 + 11 * index
        style = "body{font:17px system-ui;margin:30px auto;max-width:780px;background:#f5f3ed}"
        content = f"<h1>Article {index}</h1>" + "".join(
            f"<section><h2>Section {n + 1}</h2><p>"
            + "The benchmark fixture presents a stable document with text, headings and panels. "
            * (2 + n % 4) + "</p></section>" for n in range(count))
        if kind == "article":
            scripts += "setInterval(()=>benchReport('scroll',{y:scrollY}),2000);"
        if kind == "site":
            content += '<canvas id="asset"></canvas>'
            scripts += f"""
const canvas = document.getElementById('asset');
canvas.width = {128 + index * 28};
canvas.height = {96 + index * 22};
const ctx = canvas.getContext('2d');
for (let i=0;i<{index * 18};i++) {{
    ctx.fillStyle = 'hsl('+(i*67%360)+' 58% 50%)';
    ctx.fillRect(i*113%canvas.width,i*19%canvas.height,17,14);
}}
"""
    if kind == "video":
        content = ('<video id="clip" src="/video.mp4" autoplay loop muted playsinline '
                   'style="width:100%; max-height:calc(100vh - 20px)"></video>')
        scripts += """
const video = document.getElementById('clip');
setInterval(()=>{
  const quality = video.getVideoPlaybackQuality();
  benchReport('video',{frames:quality.totalVideoFrames,dropped:quality.droppedVideoFrames,
      width:video.videoWidth,height:video.videoHeight,playing:!video.paused&&!video.ended});
},2000);
"""
    return (f'<!doctype html><meta charset="utf-8"><title>{html.escape(title)}</title>'
            f"<style>{style}</style>{content}<script>{scripts}</script>").encode()


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def copyfile(self, source, outputfile):
        try:
            super().copyfile(source, outputfile)
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            # Speedometer unloads iframe documents as it moves between suites.
            pass

    def log_request(self, code="-", size="-"):
        if hasattr(self, "speedometer_trace"):
            self.speedometer_trace["requests"] += 1
            recent = self.speedometer_trace["recent"]
            recent.append(self.path)
            del recent[:-12]
            errors = self.speedometer_trace["errors"]
            if isinstance(code, int) and code >= 400 and len(errors) < 20:
                errors.append({"path": self.path, "code": code})

    def do_POST(self):
        parts = urlsplit(self.path).path.split("/")
        if len(parts) != 3 or parts[1] != "report" or parts[2] not in self.server.reports:
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length < 4096:
                raise ValueError("invalid report length")
            report = json.loads(self.rfile.read(length))
            if not isinstance(report, dict) or not isinstance(report.get("kind"), str):
                raise ValueError("invalid report")
        except ValueError:
            self.send_error(400)
            return
        self.server.reports[parts[2]].append({**report, "timestamp": time.perf_counter()})
        self.send_response(204)
        self.end_headers()

    def respond(self, content, mime):
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(content)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        try:
            self.wfile.write(content)
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass

    def do_GET(self):
        path = urlsplit(self.path).path
        parts = path.strip("/").split("/")
        if len(parts) == 2 and parts[0] in {"gate", "finish"}:
            gates = (self.server.speedometer_gates if parts[0] == "gate"
                     else self.server.speedometer_finishes)
            event = gates.get(parts[1])
            if event and event.wait(30):
                self.respond(b"ready", "text/plain")
            else:
                try:
                    self.send_error(503)
                except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                    pass
            return
        if parts == ["video.mp4"] and self.server.video:
            return self.video()
        if len(parts) >= 2 and parts[0] in self.server.reports:
            if parts[1] in {"color", "site", "article", "video"} and len(parts) == 3:
                try:
                    index = int(parts[2])
                except ValueError:
                    self.send_error(404)
                    return
                return self.respond(page(*parts[:2], index), "text/html; charset=utf-8")
        token = self.server.active_speedometer
        if token:
            return self.serve_speedometer(path, token)
        self.send_error(404)

    def serve_speedometer(self, path, token):
        self.speedometer_trace = self.server.speedometer_traces[token]
        self.directory = str(self.server.speedometer)
        if path in ("/", "/index.html"):
            self.speedometer_trace["document"] = self.requestline
            content = (self.server.speedometer / "index.html").read_text(encoding="utf-8")
            diagnostics = SPEEDOMETER_DIAGNOSTICS.replace("__BENCH_TOKEN__", token)
            content = content.replace("<head>", "<head>" + diagnostics, 1)
            return self.respond(content.encode(), "text/html; charset=utf-8")
        if path == "/resources/main.mjs":
            content = (self.server.speedometer / "resources/main.mjs").read_text(encoding="utf-8")
            reporter = (SPEEDOMETER_REPORTER.replace("__BENCH_TOKEN__", token)
                        .replace("__BENCH_DIAGNOSTICS__", "true" if self.server.quick else "false"))
            return self.respond((content + "\n" + reporter).encode(), "text/javascript")
        return super().do_GET()

    def video(self):
        path = self.server.work / "video.mp4"
        size = path.stat().st_size
        first, last = 0, size - 1
        requested = self.headers.get("Range")
        if requested:
            match = re.fullmatch(r"bytes=(\d+)-(\d*)", requested)
            if not match:
                self.send_error(416)
                return
            first = int(match[1])
            last = min(last, int(match[2])) if match[2] else last
            if first > last:
                self.send_error(416)
                return
        self.send_response(206 if requested else 200)
        self.send_header("Content-Type", "video/mp4")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(last + 1 - first))
        if requested:
            self.send_header("Content-Range", f"bytes {first}-{last}/{size}")
        self.end_headers()
        try:
            with path.open("rb") as stream:
                stream.seek(first)
                remaining = last + 1 - first
                while remaining:
                    block = stream.read(min(remaining, 65536))
                    self.wfile.write(block)
                    remaining -= len(block)
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


class Fixtures(ThreadingHTTPServer):
    def __init__(self, work, video, speedometer, archive, quick=False):
        super().__init__(("127.0.0.1", 0), Handler)
        self.reports = {}
        self.speedometer_traces = {}
        self.speedometer_gates = {}
        self.speedometer_finishes = {}
        self.active_speedometer = None
        self.quick = quick
        self.work = work
        self.video = bool(video)
        self.speedometer = speedometer
        if video:
            shutil.copyfile(video, work / "video.mp4")
        if archive:
            with tarfile.open(archive) as source:
                source.extractall(speedometer.parent, filter="data")
            assert (speedometer / "resources/main.mjs").is_file()
        self.thread = threading.Thread(target=self.serve_forever, daemon=True)
        self.thread.start()

    def url(self, token, route):
        self.reports.setdefault(token, [])
        if route == "speedometer/":
            self.speedometer_gates[token] = threading.Event()
            self.speedometer_finishes[token] = threading.Event()
            self.speedometer_traces[token] = {"requests": 0, "errors": [], "recent": []}
            self.active_speedometer = token
            return f"http://127.0.0.1:{self.server_port}/"
        return f"http://127.0.0.1:{self.server_port}/{token}/{route}"

    def close(self):
        self.shutdown()
        self.server_close()
        self.thread.join()
