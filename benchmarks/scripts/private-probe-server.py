"""Tiny local site for checking Bowser private tabs. Run with: python -I private-probe-server.py PORT LOGFILE

GET /        sets a persistent cookie (bowser_probe=<n>) and shows the Cookie header the browser sent.
GET /whoami  shows the Cookie header only (no Set-Cookie).
Every request is appended to LOGFILE as one line: "<n> <path> cookie=<header or ->".
"""
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
LOG = sys.argv[2]
LOCK = threading.Lock()
COUNT = [0]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path.startswith("/favicon"):
            self.send_response(404)
            self.end_headers()
            return
        with LOCK:
            COUNT[0] += 1
            n = COUNT[0]
            cookie = self.headers.get("Cookie") or "-"
            with open(LOG, "a", encoding="utf-8") as handle:
                handle.write(f"{n} {self.path} cookie={cookie}\n")
        body = (
            "<!doctype html><meta charset=utf-8><title>Probe page %d</title>"
            "<body style='font:20px Segoe UI;padding:2em'><h1>Probe page %d</h1>"
            "<p>Cookie header received: <code>%s</code></p>" % (n, n, cookie)
        ).encode("utf-8")
        self.send_response(200)
        if self.path.split("?")[0] == "/":
            self.send_header("Set-Cookie", f"bowser_probe=visit{n}; Max-Age=3600; Path=/")
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
