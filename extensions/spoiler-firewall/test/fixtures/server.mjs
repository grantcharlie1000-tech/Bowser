// Dependency-free static fixture server with chunked / delayed streaming.
//
//   GET /pages/<name>.html                  whole file
//   GET /chunked/<name>.html?delay=120&size=40
//        streams the file; splits on <!--SPLIT--> markers and additionally every `size` bytes (default 0 = off),
//        waiting `delay` ms between writes, so a spoiler can be cut in the middle of a word across network chunks.
//   GET /media/<file>, /static/<file>
// Placeholders {{ORIGIN}} and {{ORIGIN_B}} (a second, cross-origin listener) are substituted in .html files.
// Listens on two ports; hostnames are mapped to loopback by the browser via --host-resolver-rules.
import http from "node:http";
import { readFileSync, existsSync, statSync } from "node:fs";
import { dirname, join, normalize, extname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const TYPES = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".webm": "video/webm", ".svg": "image/svg+xml", ".png": "image/png", ".json": "application/json" };
// Pretty URLs so adapters (which read location.pathname / search) see realistic paths.
export const REWRITES = {
  "/home": "/pages/x-timeline.html",
  "/launch/home": "/pages/x-launch-home.html",
  "/counters/home": "/pages/x-counters.html",
  "/someone/status/1234567890": "/pages/x-conversation.html",
  "/r/pics/comments/abc/some_post/": "/pages/reddit-new.html",
  "/r/pics/": "/pages/reddit-old.html",
  "/watch": "/pages/youtube-watch.html",
  "/results": "/pages/youtube-search.html",
  "/search": "/pages/google.html",
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function resolveFile(urlPath) {
  const rel = normalize(decodeURIComponent(urlPath)).replace(/^[/\\]+/, "");
  const full = join(HERE, rel);
  if (!full.startsWith(HERE)) return null;
  return existsSync(full) && statSync(full).isFile() ? full : null;
}

export async function startServer({ port = 0, portB = 0, hostname = "127.0.0.1" } = {}) {
  const origins = { a: "", b: "" };
  const handler = async (req, res) => {
    try {
      const u = new URL(req.url, "http://x");
      let path = u.pathname;
      let chunked = false;
      if (REWRITES[path]) path = REWRITES[path];
      if (path.startsWith("/chunked/")) { chunked = true; path = "/pages/" + path.slice(9); }
      if (u.searchParams.has("__chunk")) chunked = true;
      const file = resolveFile(path);
      if (!file) { res.writeHead(404); res.end("not found"); return; }
      const ext = extname(file);
      res.setHeader("Content-Type", TYPES[ext] || "application/octet-stream");
      res.setHeader("Cache-Control", "no-store");
      if (ext !== ".html") { res.end(readFileSync(file)); return; }
      let html = readFileSync(file, "utf8").replace(/\{\{ORIGIN_B\}\}/g, origins.b).replace(/\{\{ORIGIN\}\}/g, origins.a);
      if (!chunked) { res.end(html.replace(/<!--SPLIT-->/g, "")); return; }
      const delay = Number(u.searchParams.get("__chunk") || u.searchParams.get("delay") || 120);
      const size = Number(u.searchParams.get("__size") || u.searchParams.get("size") || 0);
      // Chunk boundaries: <!--SPLIT--> markers plus every `size` characters, EXCEPT inside the first `guard`
      // characters after a sentinel's opening tag: a spoiler that is not yet recognisable (e.g. just "GTA") is not
      // a spoiler yet, so those boundaries are moved later. Everything after the recognisable prefix may be cut anywhere.
      const guard = Number(u.searchParams.get("__guard") ?? 30);
      const segs = html.split("<!--SPLIT-->");
      const clean = segs.join("");
      const cuts = new Set();
      let pos = 0;
      for (let i = 0; i < segs.length - 1; i++) { pos += segs[i].length; cuts.add(pos); }
      if (size > 0) for (let i = size; i < clean.length; i += size) cuts.add(i);
      const guarded = [];
      const re = /class="sentinel[^"]*"[^>]*>/g;
      let m;
      while ((m = re.exec(clean))) guarded.push([m.index + m[0].length, m.index + m[0].length + guard]);
      const ok = (i) => !guarded.some(([x, y]) => i >= x && i < y);
      const sorted = Array.from(cuts).filter(ok).sort((x, y) => x - y);
      const parts = [];
      let prev = 0;
      for (const c of sorted) { parts.push(Buffer.from(clean.slice(prev, c), "utf8")); prev = c; }
      parts.push(Buffer.from(clean.slice(prev), "utf8"));
      res.writeHead(200);
      for (const p of parts) {
        res.write(p);
        await sleep(delay);
      }
      res.end();
    } catch (e) {
      try { res.writeHead(500); res.end(String(e && e.message)); } catch (x) { /* ignore */ }
    }
  };
  const a = http.createServer(handler);
  const b = http.createServer(handler);
  await new Promise((r) => a.listen(port, hostname, r));
  await new Promise((r) => b.listen(portB, hostname, r));
  origins.a = `http://${hostname}:${a.address().port}`;
  origins.b = `http://${hostname}:${b.address().port}`;
  return {
    portA: a.address().port,
    portB: b.address().port,
    close: () => Promise.all([new Promise((r) => a.close(r)), new Promise((r) => b.close(r))]),
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const s = await startServer({ port: Number(process.env.PORT || 8750), portB: Number(process.env.PORT_B || 8751) });
  console.log(`fixtures: http://127.0.0.1:${s.portA}/pages/ (cross-origin listener on ${s.portB})`);
}
