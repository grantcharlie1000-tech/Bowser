"""Local fixture server for benchmarks/scripts/e2e-bug-bash.ps1 (loopback only).

Routes:
  /site/<slug>?t=<title>&c=<rrggbb>   a plain page with that title and a coloured favicon
  /icon/<rrggbb>.png                    a 16x16 solid PNG
  /sw/page.html?label=..&k=..&runs=..&n=..
                                        the service-worker stand-in for Gmail: the first visit
                                        registers /sw/sw.js (scope /sw/) and reloads under its
                                        control; each controlled load then fetches n subresources
                                        through the worker and POSTs its timings to /report
  /sw/sw.js                             a worker that answers every request with fetch()
  /sw/res/<i>                           a small JavaScript-ish body (no-store)
  /report  (POST)                       appends one JSON line to --reports

Usage: python e2e-bug-bash-server.py --port 8411 --reports reports.jsonl
Binds 127.0.0.1 and ::1 (so both http://127.0.0.1 and http://localhost are distinct sites).
"""

import argparse
import html
import json
import socket
import struct
import threading
import time
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

REPORTS = None
LOCK = threading.Lock()


def png(rgb: str) -> bytes:
    try:
        r, g, b = int(rgb[0:2], 16), int(rgb[2:4], 16), int(rgb[4:6], 16)
    except ValueError:
        r, g, b = 90, 90, 200
    raw = b"".join(b"\x00" + bytes([r, g, b, 255]) * 16 for _ in range(16))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 16, 16, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


SITE = """<!doctype html><html><head><meta charset="utf-8"><title>{title}</title>
<link rel="icon" href="/icon/{colour}.png"></head>
<body style="margin:0;font:28px Segoe UI,sans-serif;background:#{colour}22">
<h1 style="margin:40px">{title}</h1><p style="margin:40px">e2e fixture page {slug}</p></body></html>"""

SW_PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>SW stand-in</title>
<link rel="icon" href="/icon/d93025.png"></head>
<body style="font:20px Segoe UI,sans-serif;margin:30px"><h1>Service worker stand-in</h1><pre id="log"></pre>
<script>
const q = new URLSearchParams(location.search);
const label = q.get('label') || 'solo';
const k = Number(q.get('k') || 1), runs = Number(q.get('runs') || 5), n = Number(q.get('n') || 200);
const log = (t) => { document.getElementById('log').textContent += t + '\\n'; };
const send = (body) => fetch('/report', { method: 'POST', body: JSON.stringify(body), cache: 'no-store' }).catch(() => {});
async function main() {
  if (!navigator.serviceWorker) { await send({ label, error: 'no serviceWorker' }); document.title = 'SW error'; return; }
  if (!navigator.serviceWorker.controller) {
    const t0 = performance.now();
    await navigator.serviceWorker.register('/sw/sw.js', { scope: '/sw/' });
    await navigator.serviceWorker.ready;
    if (!navigator.serviceWorker.controller) {
      await new Promise((r) => { navigator.serviceWorker.addEventListener('controllerchange', r, { once: true }); setTimeout(r, 5000); });
    }
    await send({ label, phase: 'installed', ms: performance.now() - t0, controlled: !!navigator.serviceWorker.controller });
    location.reload();
    return;
  }
  const start = performance.now();
  const jobs = [];
  for (let i = 0; i < n; i++) jobs.push(fetch('/sw/res/' + i + '?k=' + k + '&r=' + Math.random(), { cache: 'no-store' }).then((r) => r.text()));
  const bodies = await Promise.all(jobs);
  const done = performance.now();
  const nav = performance.getEntriesByType('navigation')[0] || {};
  const result = { label, phase: 'run', k, n, ok: bodies.filter((b) => b.length > 0).length,
    fetch_ms: +(done - start).toFixed(1), nav_to_done_ms: +done.toFixed(1),
    response_end_ms: +(nav.responseEnd || 0).toFixed(1), dcl_ms: +(nav.domContentLoadedEventEnd || 0).toFixed(1),
    controlled: true, visible: document.visibilityState };
  log(JSON.stringify(result));
  await send(result);
  if (k < runs) { setTimeout(() => { q.set('k', String(k + 1)); location.search = q.toString(); }, 400); }
  else { document.title = 'SW done ' + label; }
}
main();
</script></body></html>"""

SW_JS = """self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', (e) => { e.respondWith(fetch(e.request)); });
"""


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def send(self, code, body: bytes, ctype: str, extra=None):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        url = urlparse(self.path)
        q = {k: v[0] for k, v in parse_qs(url.query).items()}
        path = url.path
        if path.startswith("/site/"):
            slug = path[len("/site/"):] or "page"
            title = q.get("t", slug)
            colour = q.get("c", "4060c0")
            body = SITE.format(title=html.escape(title), colour=html.escape(colour), slug=html.escape(slug))
            return self.send(200, body.encode(), "text/html; charset=utf-8")
        if path.startswith("/icon/") and path.endswith(".png"):
            return self.send(200, png(path[6:-4]), "image/png")
        if path == "/sw/page.html":
            return self.send(200, SW_PAGE.encode(), "text/html; charset=utf-8")
        if path == "/sw/sw.js":
            return self.send(200, SW_JS.encode(), "text/javascript", {"Service-Worker-Allowed": "/sw/"})
        if path.startswith("/sw/res/"):
            body = ("/* resource %s */ var r%s = %d;\n" % (path[8:], path[8:].replace("-", "_"), int(time.time()))).encode()
            return self.send(200, body * 4, "text/javascript")
        if path == "/health":
            return self.send(200, b"ok", "text/plain")
        return self.send(404, b"not found", "text/plain")

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        data = self.rfile.read(length) if length else b""
        if urlparse(self.path).path == "/report":
            try:
                record = json.loads(data.decode("utf-8") or "{}")
            except ValueError:
                record = {"raw": data.decode("utf-8", "replace")}
            record["server_time"] = time.time()
            with LOCK, open(REPORTS, "a", encoding="utf-8") as out:
                out.write(json.dumps(record) + "\n")
            return self.send(204, b"", "text/plain")
        return self.send(404, b"not found", "text/plain")


class V6Server(ThreadingHTTPServer):
    address_family = socket.AF_INET6


def main():
    global REPORTS
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8411)
    parser.add_argument("--reports", required=True)
    args = parser.parse_args()
    REPORTS = args.reports
    servers = [ThreadingHTTPServer(("127.0.0.1", args.port), Handler)]
    try:
        servers.append(V6Server(("::1", args.port), Handler))
    except OSError as error:
        print("no ::1 listener:", error, flush=True)
    for server in servers:
        server.daemon_threads = True
        threading.Thread(target=server.serve_forever, daemon=True).start()
    print("serving on", args.port, flush=True)
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    main()
