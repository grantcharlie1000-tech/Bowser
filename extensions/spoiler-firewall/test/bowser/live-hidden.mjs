// LIVE verification of Bowser's built-in Spoiler Firewall against the REAL release executable, on a hidden Win32 desktop.
//
//   node extensions/spoiler-firewall/test/bowser/live-hidden.mjs [--runs default,control,lockdown,balanced,garbage,guest,private]
//        [--exe <bowser.exe>] [--only id,id] [--desktop SfwLive] [--list]
//
// Invisibility: Bowser is only ever started through hidden-desktop.ps1 (a never-shown desktop: no window, no focus, no input,
// no audio). The only input is window messages posted by win-helper.ps1, which itself runs on that desktop and only
// addresses the PID this script started. Always `--dev --data-dir <fresh dir under %TEMP%>`; never the user's profile.
//
// One Bowser launch runs a whole suite: a Node server (two origins on 127.0.0.1) serves the e2e fixtures (test/fixtures), the red-team
// fixtures (test/redteam, rendered from fixtures.mjs) and Bowser-specific ones (test/bowser/fixtures), injecting a probe as the first
// script of every HTML document. The probe (live-probe-src.mjs) scans for unshielded rendered sentinels at every mutation batch,
// rAF and task boundary, drives the fixture's scenario, beacons results to /__r, then navigates to /__next. Evidence is DOM-level:
// PrintWindow captures only black for WebView2 on a hidden desktop. Hostnames such as x.com cannot be mapped in Bowser, so adapters
// see a generic 127.0.0.1 site (site-specific adapters are NOT exercised here).
import http from "node:http";
import { readFileSync, writeFileSync, existsSync, statSync, mkdirSync, readdirSync, rmSync, openSync, closeSync, renameSync } from "node:fs";
import { spawnSync, spawn } from "node:child_process";
import { dirname, join, normalize, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";
import { probeScript } from "./live-probe-src.mjs";
import { FIXTURES as STD } from "../fixtures/manifest.mjs";
import { FIXTURES as RED, BASE_CSS } from "../redteam/fixtures.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..", "..");
const FIXDIR = join(HERE, "..", "fixtures");
const BWDIR = join(HERE, "fixtures");
const OUT = join(HERE, "out");
mkdirSync(OUT, { recursive: true });
const argv = process.argv.slice(2);
const opt = (n, d) => { const i = argv.indexOf("--" + n); return i >= 0 ? argv[i + 1] : d; };
const flag = (n) => argv.includes("--" + n);
const EXE = opt("exe", join(REPO, "target", "release", "bowser.exe"));
const DESKTOP = opt("desktop", "SfwLive");
const ONLY = opt("only", "") ? opt("only", "").split(",") : null;
const RUNS = opt("runs", "default,control,lockdown,balanced,garbage,guest,private").split(",");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const S = "GTA 6 ending spoiler: the hero dies in the final mission";

// ------------------------------------------------------------------------------------------------ catalog
// entry: { id, path, kind, meta:{settle,scenario,actions,stage,...}, scope: must|limit|gap|info, expectShield, pageShield, hot, evasive, core, cache, why }
const CAT = [];
const add = (e) => CAT.push(Object.assign({ scope: "must", meta: {}, kind: "std" }, e));
const CORE_STD = new Set(["x-timeline", "news", "blog", "adv-split@chunked", "adv-shadow", "adv-iframes", "adv-churn", "adv-scroll", "adv-virtual", "adv-spa", "adv-evasive", "rt-sandbox-srcdoc", "rt-sandbox-src", "adv-title", "youtube-search-hot"]);
const CORE_RT = new Set(["canvas-with-context-text", "css-important-visibility", "body-innerhtml-swap", "svg-foreignobject", "iframe-blob-url", "shadow-closed-late-text", "details-open-late", "document-write-after-load", "attr-only-spoiler-title", "alt-text-late-img"]);
const NOPOPUP_RUNS = new Set(["guest", "private"]);

function buildCatalog() {
  CAT.length = 0;
  add({ id: "bw-clean", kind: "bw", path: "/bw/bw-clean.html", core: true, scope: "control", why: "clean control page: engine injected, nothing shielded", meta: { settle: 500 } });
  add({ id: "bw-mention", kind: "bw", path: "/bw/mention.html", core: true, scope: "mode", why: "mode matrix: plain mention / clear spoiler / code-word spoiler", meta: { settle: 700 } });
  for (const f of STD) {
    const meta = { settle: f.settle || 400, scenario: f.scenario };
    if (f.id === "adv-hover") meta.scenario = "hover";
    const base = { kind: "std", path: f.url, scope: f.knownGap ? "gap" : "must", meta, hot: !!f.hotUrl || !!f.pageShield, evasive: f.detect === "evasive", why: f.knownGap || "" };
    add({ ...base, id: f.id, core: CORE_STD.has(f.id) });
    if (f.chunk) add({ ...base, id: f.id + "@chunked", path: f.url + (f.url.includes("?") ? "&" : "?") + `__chunk=${f.chunk.delay}&__size=${f.chunk.size}`, core: CORE_STD.has(f.id + "@chunked") });
  }
  for (const f of RED) {
    const popup = !!f.popup;
    add({ id: "rt/" + f.id, kind: "rt", path: `/rt/${f.id}.html`, scope: f.scope === "limit" ? "limit" : "must", expectShield: !!f.expectShield, popup, why: f.why,
      meta: { settle: f.settle || 700, actions: f.actions || [], popupWait: popup, popupMs: popup ? 2500 : undefined, maxMs: f.stream ? 40000 : 30000 }, core: CORE_RT.has(f.id) });
  }
  // Bowser-specific
  const B = (id, path, o = {}) => add({ id, kind: "bw", path, ...o, meta: { settle: 700, ...(o.meta || {}) } });
  B("bw-matrix", "/bw/bw-matrix.html", { core: true, meta: { settle: 1200 }, why: "every frame kind at once" });
  B("bw-frame-cross", "/bw/bw-frame-cross.html", { meta: { settle: 900 } });
  B("bw-frame-srcdoc", "/bw/bw-frame-srcdoc.html", { meta: { settle: 900 } });
  B("bw-frame-blank", "/bw/bw-frame-blank.html", { meta: { settle: 900 } });
  B("bw-frame-blob", "/bw/bw-frame-blob.html", { meta: { settle: 1000 } });
  B("bw-frame-data", "/bw/bw-frame-data.html", { meta: { settle: 1200 } });
  B("bw-sandbox-noscript", "/bw/bw-sandbox-noscript.html", { meta: { settle: 1200 } });
  B("bw-frame-nested", "/bw/bw-frame-nested.html", { meta: { settle: 1200 } });
  B("bw-frame-nested-xo", "/bw/bw-frame-nested-xo.html", { meta: { settle: 1800 } });
  B("bw-frame-late", "/bw/bw-frame-late.html", { meta: { settle: 3600 } });
  B("bw-title-static", "/bw/bw-title-static.html", { core: true, title: true, pageShieldOk: true });
  B("bw-title-script", "/bw/bw-title-script.html", { title: true, meta: { settle: 1600 } });
  B("bw-title-final", "/bw/bw-title-final.html", { title: true, last: true, meta: { settle: 7000, stay: true }, why: "kept open at the end so session.txt records this tab" });
  B("bw-title-og", "/bw/bw-title-og.html", { title: true, scope: "info", meta: { settle: 1200 }, why: "clean body, spoiler title set late: only the title channel" });
  B("bw-docopen-late", "/bw/bw-docopen-late.html", { meta: { settle: 2000 } });
  B("bw-docopen-chunks-late", "/bw/bw-docopen-chunks-late.html", { meta: { settle: 2000 } });
  B("bw-reload", "/bw/bw-reload.html?cache=1", { meta: { stage: "reload", settle: 700 }, why: "location.reload()" });
  B("bw-bfcache", "/bw/bw-bfcache.html?nocache=1", { meta: { stage: "bfcache", settle: 1500 }, why: "navigate away and history.back()" });
  B("bw-httpcache", "/bw/bw-cached.html?cache=1", { meta: { stage: "cache", settle: 700 }, why: "second visit served from the HTTP cache" });
  B("bw-sw-cached", "/bw/bw-sw-register.html?target=cached", { meta: { settle: 900, sw: true, passivePaths: ["bw-sw-register.html"], maxMs: 40000 }, why: "service-worker-controlled page served from SW cache" });
  B("bw-sw-synth", "/bw/bw-sw-register.html?target=synth", { meta: { settle: 900, sw: true, passivePaths: ["bw-sw-register.html"], maxMs: 40000 }, why: "service-worker-synthesised HTML" });
  B("bw-popup-open-url", "/bw/bw-popup-open-url.html", { popup: true, meta: { settle: 3800, actions: [{ click: "#go" }], popupWait: true, popupHandoff: true, popupMs: 3000 } });
  B("bw-popup-open-url-xo", "/bw/bw-popup-open-url-xo.html", { popup: true, meta: { settle: 3800, actions: [{ click: "#go" }], popupWait: true, popupHandoff: true, popupMs: 3000 } });
  B("bw-popup-link", "/bw/bw-popup-link.html", { popup: true, meta: { settle: 3800, actions: [{ click: "#go" }], popupWait: true, popupHandoff: true, popupMs: 3000 } });
  B("bw-popup-blank-then-nav", "/bw/bw-popup-blank-then-nav.html", { popup: true, meta: { settle: 3800, actions: [{ click: "#go" }], popupWait: true, popupHandoff: true, popupMs: 3000 } });
  B("bw-popup-blank-write-late", "/bw/bw-popup-blank-write-late.html", { popup: true, meta: { settle: 3800, actions: [{ click: "#go" }], popupWait: true, popupHandoff: true, popupMs: 3000 } });
  B("bw-popup-blank-innerhtml", "/bw/bw-popup-blank-innerhtml.html", { popup: true, meta: { settle: 3800, actions: [{ click: "#go" }], popupWait: true, popupHandoff: true, popupMs: 3000 } });
  B("bw-ocr", "/bw/bw-ocr.html", { scope: "ocr", ocrOnly: true, meta: { settle: 9000 }, why: "native OCR of image text (debug exe + BOWSER_SPOILER_OCR_TEST_HOST)" });
  B("bw-big", "/bw/big.html", { meta: { settle: 1500, maxMs: 90000, scenario: "sentinels" }, why: "5 MB HTML, spoilers at 5/50/95 %, scrolled into view one by one" });
  B("bw-big-stream", "/bw/big.html?stream=1", { meta: { settle: 1500, maxMs: 90000, scenario: "sentinels" }, why: "5 MB HTML in 64 KB network chunks" });
  B("bw-stream10", "/bw/stream10.html", { meta: { settle: 800, maxMs: 60000, streamMs: 0 }, why: "page streaming for 10 s, spoiler split mid-word across chunks" });
  const order = [...CAT];
  // stable order: control, std, rt (non popup), bw frames, title, nav modes, popups last (popups open extra tabs)
  const rank = (e) => (e.id === "bw-clean" ? 0 : e.last ? 9 : e.kind === "std" ? 1 : e.kind === "rt" ? (e.popup ? 6 : 2) : e.popup ? 7 : e.meta && e.meta.stage ? 4 : e.id.startsWith("bw-big") || e.id === "bw-stream10" ? 5 : 3);
  order.sort((a, b) => rank(a) - rank(b));
  return order;
}

// ------------------------------------------------------------------------------------------------ server
const TYPES = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".webm": "video/webm", ".svg": "image/svg+xml", ".png": "image/png", ".json": "application/json" };
const REWRITES = { "/home": "/pages/x-timeline.html", "/r/pics/comments/abc/some_post/": "/pages/reddit-new.html", "/r/pics/": "/pages/reddit-old.html", "/watch": "/pages/youtube-watch.html", "/results": "/pages/youtube-search.html", "/search": "/pages/google.html" };
const state = { catalog: [], byId: new Map(), runs: {}, run: null, currentFx: null, inst: new Map(), marks: [], hits: new Map(), origins: { a: "", b: "" }, lastBeaconAt: 0, requests: 0, popupClick: new Map(), popupTries: {}, beaconByFx: new Map() };

function redHtml(fx) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>redteam ${fx.id}</title><style>${BASE_CSS}</style>${fx.head || ""}</head><body>${fx.body || ""}${fx.script ? `<script>${fx.script}</script>` : ""}</body></html>`;
}
function bigHtml() {
  const parts = [`<!doctype html><html><head><meta charset="utf-8"><title>big page</title><style>${BASE_CSS}</style></head><body>`];
  const N = 52000; // ~100 bytes each => ~5 MB
  const at = new Set([Math.floor(N * 0.05), Math.floor(N * 0.5), Math.floor(N * 0.95)]);
  for (let i = 0; i < N; i++) {
    parts.push(at.has(i) ? `<p class="sentinel" data-expect="shield">${S} (position ${i})</p>` : `<p class="item" data-expect="clear">clean filler paragraph number ${i} about hiking, pasta and weather, padding padding</p>`);
  }
  parts.push("</body></html>");
  return parts.join("\n");
}
function injectProbe(html, tag) {
  const m = /<head[^>]*>/i.exec(html);
  if (m) return html.slice(0, m.index + m[0].length) + tag + html.slice(m.index + m[0].length);
  const h = /<html[^>]*>/i.exec(html);
  if (h) return html.slice(0, h.index + h[0].length) + tag + html.slice(h.index + h[0].length);
  const d = /^\s*<!doctype[^>]*>/i.exec(html);
  if (d) return html.slice(0, d[0].length) + tag + html.slice(d[0].length);
  return tag + html;
}
function resolveIn(root, urlPath) {
  const rel = normalize(decodeURIComponent(urlPath)).replace(/^[/\\]+/, "");
  const full = join(root, rel);
  if (!full.startsWith(root)) return null;
  return existsSync(full) && statSync(full).isFile() ? full : null;
}
function recordBeacon(d) {
  const key = d.fx + "|" + d.id;
  const prev = state.inst.get(key);
  const rec = prev || { first: Date.now(), maxPopUnsafe: 0, snaps: 0 };
  rec.snap = d; rec.last = Date.now(); rec.snaps++;
  for (const p of d.popups || []) if (p.unshielded > rec.maxPopUnsafe) rec.maxPopUnsafe = p.unshielded;
  rec.run = d.run;
  state.inst.set(key, rec);
  state.lastBeaconAt = Date.now();
  state.beaconByFx.set(d.fx, Date.now());
}
function nextOf(run, after) {
  const q = state.runs[run].queue;
  const i = q.indexOf(after);
  return q[i + 1] || null;
}
function fixtureUrl(origin, e) { return origin + e.path; }

async function startServer() {
  const handler = async (req, res) => {
    state.requests++;
    try {
      const u = new URL(req.url, "http://x");
      const host = req.headers.host || "";
      const origin = "http://" + host;
      let path = u.pathname;
      if (path === "/__r" && req.method === "POST") {
        const chunks = []; for await (const c of req) chunks.push(c);
        try { recordBeacon(JSON.parse(Buffer.concat(chunks).toString("utf8"))); } catch (e) { /* ignore */ }
        res.writeHead(204); res.end(); return;
      }
      if (path === "/__popupclick") { state.popupClick.set(u.searchParams.get("fx"), Date.now()); res.writeHead(204); res.end(); return; }
      if (path === "/__popupseen") {
        const fx = u.searchParams.get("fx");
        const seen = [...state.inst.entries()].some(([k, v]) => k.startsWith(fx + "|") && v.snap.label && v.snap.label.startsWith("popup-doc"));
        res.writeHead(200, { "Content-Type": "application/json", "Cache-Control": "no-store", "Access-Control-Allow-Origin": "*" }); res.end(JSON.stringify({ seen })); return;
      }
      if (path === "/__probe.js") {
        const fx = u.searchParams.get("fx") || state.currentFx || "unknown";
        const e = state.byId.get(fx);
        const cfg = { fx, label: u.searchParams.get("label") || (req.headers["sec-fetch-dest"] === "document" ? "doc" : "frame"), origin, run: state.run, role: u.searchParams.get("role") || null, meta: e ? e.meta : {} };
        res.writeHead(200, { "Content-Type": "text/javascript", "Cache-Control": "no-store" });
        res.end(probeScript(cfg)); return;
      }
      if (path === "/__start" || path === "/__next") {
        const run = u.searchParams.get("run") || state.run;
        const after = path === "/__start" ? null : u.searchParams.get("after");
        const r = state.runs[run];
        let nx;
        if (path === "/__start") { const from = u.searchParams.get("from"); nx = from ? nextOf(run, from) : r.queue[0]; state.run = run; }
        else { nx = nextOf(run, after); state.marks.push({ t: Date.now(), event: "done", fx: after }); }
        if (!nx) { r.done = true; state.marks.push({ t: Date.now(), event: "suite-done" }); res.writeHead(200, { "Content-Type": "text/html", "Cache-Control": "no-store" }); res.end("<!doctype html><title>suite done</title><p>suite done</p>"); return; }
        state.currentFx = nx; r.current = nx; r.startedCur = Date.now(); r.started = r.started || Date.now();
        state.marks.push({ t: Date.now(), event: "start", fx: nx });
        const e = state.byId.get(nx);
        res.writeHead(302, { Location: fixtureUrl(state.origins.a, e), "Cache-Control": "no-store" }); res.end(); return;
      }
      if (REWRITES[path]) path = REWRITES[path];
      let chunked = false, root = FIXDIR;
      if (path.startsWith("/chunked/")) { chunked = true; path = "/pages/" + path.slice(9); }
      if (u.searchParams.has("__chunk")) chunked = true;
      // dynamic documents
      let html = null; let dynamic = false;
      let m;
      if ((m = /^\/rt\/(.+)\.html$/.exec(path))) { const fx = RED.find((f) => f.id === m[1]); if (!fx) { res.writeHead(404); res.end("nofx"); return; } html = redHtml(fx); dynamic = true; if (fx.stream) { u.searchParams.set("__rtstream", fx.stream.size + "," + fx.stream.delay); } }
      else if (path === "/bw/big.html") { html = bigHtml(); dynamic = true; }
      else if (path === "/bw/stream10.html") { html = "STREAM10"; dynamic = true; }
      else if (path === "/bw/mention.html") {
        html = `<!doctype html><html><head><meta charset="utf-8"><title>mode matrix</title><style>${BASE_CSS}</style></head><body>
<article class="item" data-unit="mention" data-expect="mode">I really cannot wait for GTA 6 to come out next year, so hyped!</article>
<article class="item" data-unit="spoiler" data-expect="shield"><span class="sentinel">${S}</span></article>
<article class="item" data-unit="clean" data-expect="clear">Pasta recipe: boil water, add salt, cook eight minutes.</article>
<article class="item" data-unit="codeword" data-expect="mode">Rockstar's big new open-world game from Vice City has a huge twist in the last mission, the protagonist does not survive.</article></body></html>`; dynamic = true;
      }
      else if (path === "/bw/sw.js") {
        res.writeHead(200, { "Content-Type": "text/javascript", "Cache-Control": "no-store", "Service-Worker-Allowed": "/bw/" });
        res.end(`self.addEventListener('install',function(e){self.skipWaiting();e.waitUntil(caches.open('sfw').then(function(c){return c.add('/bw/bw-sw-cached-doc.html')}))});
self.addEventListener('activate',function(e){e.waitUntil(self.clients.claim())});
var SYNTH=${JSON.stringify(`<!doctype html><html><head><meta charset="utf-8"><script src="/__probe.js?fx=bw-sw-synth&label=doc"></script><title>sw synth</title><style>${BASE_CSS}</style></head><body><p class="item" data-expect="clear">Parent page clean text.</p><p class="sentinel" data-expect="shield">${S} (synthesised by the service worker)</p></body></html>`)};
self.addEventListener('fetch',function(e){var u=new URL(e.request.url);
 if(u.pathname==='/bw/bw-sw-cached-doc.html'){e.respondWith(caches.match('/bw/bw-sw-cached-doc.html').then(function(r){return r||fetch(e.request)}));}
 else if(u.pathname==='/bw/bw-sw-synth-doc.html'){e.respondWith(new Response(SYNTH,{headers:{'content-type':'text/html; charset=utf-8'}}));}});`);
        return;
      }
      let file = null;
      if (!dynamic) {
        if ((m = /^\/bw\/(.+)$/.exec(path))) { file = resolveIn(BWDIR, m[1]); }
        else file = resolveIn(FIXDIR, path);
        if (!file) { res.writeHead(404); res.end("not found"); return; }
      }
      state.hits.set(path, (state.hits.get(path) || 0) + 1);
      const ext = dynamic ? ".html" : extname(file);
      const cacheable = u.searchParams.has("cache");
      const noc = u.searchParams.has("nocache");
      res.setHeader("Content-Type", TYPES[ext] || "application/octet-stream");
      res.setHeader("Cache-Control", cacheable ? "public, max-age=600" : noc ? "no-cache" : "no-store");
      if (ext !== ".html") { res.end(readFileSync(file)); return; }
      const fx = u.searchParams.get("__fx") || state.currentFx || "unknown";
      const role = u.searchParams.get("__role");
      const label = req.headers["sec-fetch-dest"] === "document" ? (role === "popup" ? "popup-doc" : "doc") : "frame";
      const tag = `<script src="/__probe.js?fx=${encodeURIComponent(fx)}&label=${label}${role ? "&role=" + role : ""}"></script>`;
      if (html === "STREAM10") {
        res.writeHead(200);
        const head = injectProbe(`<!doctype html><html><head><meta charset="utf-8"><title>stream 10 s</title><style>${BASE_CSS}</style></head><body>`, tag);
        res.write(head);
        const steps = 40; // 40 x 250 ms
        for (let i = 0; i < steps; i++) {
          if (i === 16) res.write(`<p class="sentinel" data-expect="shield">GTA 6 end`);
          else if (i === 17) res.write(`ing spoiler: the hero dies in the final mission (split across chunks)</p>`);
          else if (i === 30) res.write(`<p class="sentinel" data-expect="shield">GTA 6 ending spoiler: `);
          else if (i === 31) res.write(`the hero dies in the final mission (late)</p>`);
          else res.write(`<p class="item" data-expect="clear">clean streamed paragraph ${i}</p>`);
          await sleep(250);
        }
        res.end("</body></html>"); return;
      }
      if (html == null) html = readFileSync(file, "utf8");
      html = html.replace(/\{\{ORIGIN_B\}\}/g, state.origins.b).replace(/\{\{ORIGIN\}\}/g, state.origins.a).replace(/\{\{FX\}\}/g, encodeURIComponent(fx));
      html = injectProbe(html, tag);
      const rts = u.searchParams.get("__rtstream");
      if (rts) {
        const [size, delay] = rts.split(",").map(Number);
        res.writeHead(200);
        for (let i = 0; i < html.length; i += size) { res.write(html.slice(i, i + size)); await sleep(delay); }
        res.end(); return;
      }
      if (u.searchParams.has("stream")) {
        res.writeHead(200);
        const buf = Buffer.from(html, "utf8");
        for (let i = 0; i < buf.length; i += 65536) { res.write(buf.subarray(i, i + 65536)); await sleep(5); }
        res.end(); return;
      }
      if (!chunked) { res.end(html.replace(/<!--SPLIT-->/g, "")); return; }
      const delay = Number(u.searchParams.get("__chunk") || 120);
      const size = Number(u.searchParams.get("__size") || 0);
      const guard = Number(u.searchParams.get("__guard") ?? 30);
      const segs = html.split("<!--SPLIT-->");
      const clean = segs.join("");
      const cuts = new Set(); let pos = 0;
      for (let i = 0; i < segs.length - 1; i++) { pos += segs[i].length; cuts.add(pos); }
      const bodyStart = Math.max(0, clean.indexOf("<body"));
      if (size > 0) for (let i = bodyStart + size; i < clean.length; i += size) cuts.add(i);
      const guarded = []; const re = /class="sentinel[^"]*"[^>]*>/g; let mm;
      while ((mm = re.exec(clean))) guarded.push([mm.index + mm[0].length, mm.index + mm[0].length + guard]);
      const ok = (i) => !guarded.some(([x, y]) => i >= x && i < y);
      const sorted = Array.from(cuts).filter(ok).sort((x, y) => x - y);
      res.writeHead(200);
      let prev = 0;
      for (const c of sorted) { res.write(Buffer.from(clean.slice(prev, c), "utf8")); prev = c; await sleep(delay); }
      res.write(Buffer.from(clean.slice(prev), "utf8")); res.end();
    } catch (e) { try { res.writeHead(500); res.end(String(e && e.message)); } catch (x) { /* ignore */ } }
  };
  const a = http.createServer(handler), b = http.createServer(handler);
  await new Promise((r) => a.listen(0, "127.0.0.1", r));
  await new Promise((r) => b.listen(0, "127.0.0.1", r));
  state.origins.a = `http://127.0.0.1:${a.address().port}`;
  state.origins.b = `http://127.0.0.1:${b.address().port}`;
  return { close: () => Promise.all([new Promise((r) => a.close(r)), new Promise((r) => b.close(r))]) };
}

// ------------------------------------------------------------------------------------------------ hidden-desktop plumbing
const PS = "powershell.exe";
let CHILD_ENV = { ...process.env };
// Output goes through a temp file, not a pipe: a process started on the hidden desktop by a launcher must not keep this call waiting
// for pipe EOF (the helper processes outlive the launcher).
function ps(args, opts = {}) {
  const f = join(os.tmpdir(), "sfw-ps-" + process.pid + "-" + Math.random().toString(36).slice(2) + ".out");
  const fd = openSync(f, "w");
  let r;
  try { r = spawnSync(PS, ["-NoProfile", "-ExecutionPolicy", "Bypass", ...args], { stdio: ["ignore", fd, fd], timeout: opts.timeout || 150000, env: opts.env || process.env, windowsHide: true }); } finally { closeSync(fd); }
  let out = ""; try { out = readFileSync(f, "utf8"); } catch (e) { /* ignore */ } try { rmSync(f, { force: true }); } catch (e) { /* ignore */ }
  return { status: r.status, stdout: out, stderr: "" };
}
function runHidden(exe, args, wait = false, env = undefined) {
  const r = ps(["-File", join(HERE, "hidden-desktop.ps1"), "-Action", "run", "-Desktop", DESKTOP, "-Exe", exe, "-Args", args, ...(wait ? ["-Wait"] : [])], { env, timeout: 900000 });
  DBG("runHidden done");
  const pid = parseInt((r.stdout || "").trim().split(/\s+/).pop(), 10);
  if (!pid) throw new Error("hidden-desktop failed: " + (r.stdout || "") + (r.stderr || ""));
  return pid;
}
const DBG = (m) => { if (process.env.SFW_DEBUG) console.log(new Date().toISOString().slice(11, 23), m); };
// Commands for win-helper.ps1, which runs as a server on the hidden desktop (one PowerShell start and one Add-Type compile for the whole
// session): a JSON file per command in helperDir, answered by <id>.out / <id>.done. Async, so the fixture server keeps serving.
let helperDir = null, helperSeq = 0;
async function startHelper() {
  helperDir = join(os.tmpdir(), "sfw-helper-" + Date.now());
  mkdirSync(helperDir, { recursive: true });
  runHidden("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe", `-NoProfile -ExecutionPolicy Bypass -File "${join(HERE, "win-helper.ps1")}" -Action server -Dir "${helperDir}"`);
  for (let i = 0; i < 1800 && !existsSync(join(helperDir, "ready")); i++) await sleep(500);
  if (!existsSync(join(helperDir, "ready"))) throw new Error("helper server did not start");
}
function stopHelper() { if (helperDir) { try { writeFileSync(join(helperDir, "stop"), "stop"); } catch (e) { /* ignore */ } helperDir = null; } }
process.on("exit", stopHelper);
// Bowser is started by the helper server (already on the hidden desktop), not by another hidden-desktop.ps1 run.
async function launchBowser(exe, args, env) {
  const extra = {};
  for (const k of Object.keys(env || {})) if (!process.env[k] || process.env[k] !== env[k]) extra[k] = env[k];
  const out = await H("launch", { exe, args, env: extra });
  const pid = parseInt(out, 10);
  if (!pid) throw new Error("launch failed: " + out);
  return pid;
}
async function H(action, f = {}, timeoutMs = 180000) {
  const id = String(++helperSeq).padStart(6, "0");
  const out = f.out || join(helperDir, id + ".out");
  const tmp = join(helperDir, id + ".tmp"), json = join(helperDir, id + ".json"), done = join(helperDir, id + ".done");
  writeFileSync(tmp, JSON.stringify({ id, action, pid: f.pid, keys: f.keys || "", text: f.text || "", out, exe: f.exe, args: f.args, env: f.env }));
  renameSync(tmp, json);
  const t0 = Date.now();
  while (!existsSync(done)) { if (Date.now() - t0 > timeoutMs) throw new Error("helper timeout: " + action); await sleep(100); }
  const o = join(helperDir, id + ".out");
  return existsSync(o) ? readFileSync(o, "utf8").replace(/^\uFEFF/, "").trim() : "";
}
// A desktop is destroyed when its last handle closes; hidden-desktop.ps1 closes its handle right after CreateProcess, so a process
// started without -Wait can lose its desktop before it finishes initialising (it then dies silently). A keeper process that
// lives on the desktop for the whole session prevents that. It is stopped by PID at the end.
let keeperMarker = null;
async function startKeeper() {
  keeperMarker = "SFWKEEPER_" + Date.now();
  const child = spawn(PS, ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(HERE, "hidden-desktop.ps1"), "-Action", "run", "-Desktop", DESKTOP, "-Exe", process.env.ComSpec || "C:/Windows/System32/cmd.exe", "-Args", `/c title ${keeperMarker} & ping -n 4000 127.0.0.1 > nul`, "-Wait"], { stdio: "ignore", windowsHide: true });
  child.unref();
  // wait until the keeper cmd.exe really exists (on a loaded machine PowerShell + its compile can take a minute or more)
  for (let i = 0; i < 600; i++) {
    await sleep(1000);
    const r = spawnSync("wmic", ["process", "where", `name='cmd.exe' and commandline like '%${keeperMarker}%'`, "get", "processid", "/value"], { encoding: "utf8" });
    if (/ProcessId=\d+/.test(r.stdout || "")) { await sleep(1500); return; }
  }
  throw new Error("keeper did not start");
}
// Stops only the keeper this script started (found by its marker); also runs on exit so an interrupted run does not leak it.
function stopKeeper() {
  if (!keeperMarker) return;
  const m = keeperMarker; keeperMarker = null;
  spawnSync(PS, ["-NoProfile", "-Command", `Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" | Where-Object { $_.CommandLine -like '*${m}*' } | ForEach-Object { taskkill /PID $_.ProcessId /T /F | Out-Null }`], { stdio: "ignore", windowsHide: true, timeout: 60000 });
}
process.on("exit", stopKeeper);
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (e) { return false; } };

// ------------------------------------------------------------------------------------------------ runs
const SETTINGS = {
  default: null,
  control: "spoiler_protection=off\r\nspoiler_ocr=off\r\n",
  lockdown: "spoiler_mode = lockdown\r\n",
  balanced: "spoiler_mode = balanced\r\n",
  garbage: "spoiler_protection=banana\r\nspoiler_mode=!!!lockdown\r\nspoiler_reduced_effects=maybe\r\n",
};
const RUN_DEFS = {
  default: { suite: "full", perms: true, caption: true },
  control: { suite: "full", perms: true, controlOff: true },
  lockdown: { suite: "core", perms: true },
  balanced: { suite: "core", perms: true },
  garbage: { suite: "core", perms: true },
  guest: { suite: "core", args: ["--guest"], perms: false, noPopups: true, caption: true },
  ocr: { suite: "ocr", exe: "debug", perms: false, env: { BOWSER_SPOILER_OCR_TEST_HOST: "127.0.0.1" } },
  "ocr-off": { suite: "ocr", exe: "debug", perms: false, env: { BOWSER_SPOILER_OCR_TEST_HOST: "127.0.0.1" }, settingsText: "spoiler_ocr=off\r\n" },
  private: { suite: "core", private: true, perms: false, noPopups: true, caption: true },
};

function findFiles(dir, names, depth = 0, acc = []) {
  try {
    for (const n of readdirSync(dir)) {
      const p = join(dir, n);
      let st; try { st = statSync(p); } catch (e) { continue; }
      if (st.isDirectory() && depth < 3 && !/WebView|EBWebView|Cache|Code Cache|GPUCache/i.test(n)) findFiles(p, names, depth + 1, acc);
      else if (st.isFile() && names.some((x) => n.toLowerCase() === x)) acc.push(p);
    }
  } catch (e) { /* ignore */ }
  return acc;
}

async function runSuite(name, catalog, srv) {
  const def = RUN_DEFS[name];
  const list = def.suite === "ocr" ? catalog.filter((e) => e.ocrOnly) : catalog.filter((e) => (def.suite === "full" || e.core) && !e.ocrOnly && (!ONLY || ONLY.includes(e.id)) && !(def.noPopups && e.popup) && !e.skip);
  state.runs[name] = { queue: list.map((e) => e.id), done: false, current: null, started: null };
  state.run = name; state.currentFx = null; state.marks = []; state.lastBeaconAt = Date.now(); state.hits.clear(); state.popupClick.clear(); state.popupTries = {};
  const dataDir = join(os.tmpdir(), `sfw-live-${name}-${Date.now()}`);
  mkdirSync(dataDir, { recursive: true });
  if (SETTINGS[name]) writeFileSync(join(dataDir, "settings.txt"), SETTINGS[name]);
  if (def.settingsText) writeFileSync(join(dataDir, "settings.txt"), def.settingsText);
  if (def.perms) writeFileSync(join(dataDir, "permissions.txt"), `${state.origins.a} allow popups\n${state.origins.b} allow popups\n`);
  const t0 = Date.now();
  const result = { run: name, dataDir, startedAt: new Date().toISOString(), suite: list.length, restarts: 0, stalled: [], notes: [], privateTab: null };
  const launchArgs = (from) => ["--dev", "--data-dir", dataDir, ...(def.args || []), ...(def.private && !from ? [] : ["--open-url", `${state.origins.a}/__start?run=${name}${from ? "&from=" + encodeURIComponent(from) : ""}`])].join(" ");
  const exe = def.exe === "debug" ? join(REPO, "target", "debug", "bowser.exe") : EXE;
  const launchEnv = def.private ? { ...process.env, BOWSER_UI: "native" } : def.env ? { ...process.env, ...def.env } : undefined; // native chrome: the omnibox is a real Edit control that posted messages can drive
  let pid = await launchBowser(exe, launchArgs(null), launchEnv);
  result.pid = pid;
  console.log(`[${name}] bowser pid ${pid}, ${list.length} fixtures, data dir ${dataDir}`);
  let capPid = null, capFile = join(dataDir, "..", `sfw-caption-${name}-${t0}.log`), stopFile = capFile + ".stop";
  if (def.caption) {
    await sleep(2500);
    try { await H("caption", { pid, out: capFile }); capPid = true; } catch (e) { result.notes.push("caption helper failed: " + e.message); }
  }
  if (def.private) await tryPrivate(pid, result, name);
  let lastCurrent = null, lastProgress = Date.now();
  const hardEnd = Date.now() + 40 * 60 * 1000;
  while (!state.runs[name].done && Date.now() < hardEnd) {
    await sleep(1000);
    const cur = state.runs[name].current;
    if (cur !== lastCurrent) { lastCurrent = cur; lastProgress = Date.now(); }
    if (cur && (state.beaconByFx.get(cur) || 0) > lastProgress) lastProgress = state.beaconByFx.get(cur);
    const curMeta = cur && state.byId.get(cur) ? state.byId.get(cur).meta : null;
    const overdue = cur && state.runs[name].startedCur && Date.now() - state.runs[name].startedCur > ((curMeta && curMeta.maxMs) || 30000) + 90000;
    if (!alive(pid)) { result.notes.push("bowser exited early at " + cur); if (state.runs[name].done) break; }
    // Script popups open as separate (background) tabs. Activate the next tab so it loads, then its probe takes over the suite.
    if (cur && state.byId.get(cur) && state.byId.get(cur).meta.popupHandoff && state.popupClick.get(cur)) {
      const seen = [...state.inst.keys()].some((k) => k.startsWith(cur + "|") && state.inst.get(k).snap.label.startsWith("popup-doc"));
      const tries = (state.popupTries[cur] = state.popupTries[cur] || { n: 0, at: 0 });
      if (!seen && tries.n < 2 && Date.now() - state.popupClick.get(cur) > 1800 + tries.n * 7000 && Date.now() - tries.at > 4000) {
        const key = ["ctrl+tab", "ctrl+shift+tab"][tries.n]; tries.n++; tries.at = Date.now(); DBG("popup key " + key + " for " + cur);
        const kf = join(os.tmpdir(), `sfw-popkey-${Date.now()}.txt`);
        try { const o = await H("keys", { pid, keys: key }); (result.popupKeys = result.popupKeys || []).push({ fx: cur, key, out: o }); } catch (e) { /* ignore */ }
      }
    }
    if (def.private && !state.runs[name].current && Date.now() - lastProgress > 30000) { result.notes.push("private tab navigation could not be driven by posted messages; run aborted"); break; }
    if (Date.now() - lastProgress > 120000 || overdue || !alive(pid)) {
      result.stalled.push(cur); console.log(`[${name}] stalled at ${cur}; restarting after it`);
      try { process.kill(pid, "SIGKILL"); } catch (e) { /* gone */ }
      await sleep(2500);
      try { rmSync(join(dataDir, "running.lock"), { force: true }); } catch (e) { /* ignore */ }
      if (++result.restarts > 6) { result.notes.push("too many restarts"); break; }
      pid = await launchBowser(exe, launchArgs(cur), launchEnv); result.pid = pid; lastProgress = Date.now(); state.runs[name].startedCur = Date.now();
    }
  }
  result.suiteDone = !!state.runs[name].done;
  result.durationMs = Date.now() - t0;
  await sleep(def.caption ? 5000 : 1500); // lets the debounced session save run
  // evidence from the live window before closing
  try { result.windows = (await H("windows", { pid })).split(/\r?\n/).slice(0, 40); } catch (e) { /* ignore */ }
  if (capPid) { try { await H("caption-stop", { pid }); } catch (e) { /* ignore */ } await sleep(300); }
  // graceful close (flushes history/session), then stop only our PID if it is still there
  try { await H("close", { pid }); } catch (e) { result.notes.push("close helper: " + e.message); }
  for (let i = 0; i < 30 && alive(pid); i++) await sleep(500);
  if (alive(pid)) { result.notes.push("did not exit after WM_CLOSE; stopped by PID"); try { process.kill(pid, "SIGKILL"); } catch (e) { /* gone */ } await sleep(1000); }
  result.files = findFiles(dataDir, ["history.txt", "sessions.txt", "session.txt", "settings.txt", "permissions.txt"]);
  result.fileText = {};
  for (const f of result.files) { try { result.fileText[f.slice(dataDir.length + 1)] = readFileSync(f, "utf8").slice(0, 200000); } catch (e) { /* ignore */ } }
  if (def.caption && existsSync(capFile)) result.captions = readFileSync(capFile, "utf8").split(/\r?\n/).filter(Boolean).map((l) => { const [t, ...c] = l.split("\t"); return { t: +t, caption: c.join("\t") }; });
  result.marks = state.marks.slice();
  return result;
}

// Private tab without OS input: post Ctrl+Shift+P (Bowser's "New private tab" accelerator) to the hidden window from a helper
// that runs on the same hidden desktop, then post the omnibox text. Best effort; the outcome is recorded either way.
async function tryPrivate(pid, result, name) {
  await sleep(6000);
  try {
    result.privateTab = { before: (await H("windows", { pid })).split(/\r?\n/).slice(0, 25) };
    result.privateTab.keys = await H("keys", { pid, keys: "ctrl+shift+p" });
    await sleep(2500);
    result.privateTab.chars = await H("settext", { pid, text: `${state.origins.a}/__start?run=${name}` });
    await sleep(2500);
    result.privateTab.after = (await H("windows", { pid })).split(/\r?\n/).slice(0, 25);
  } catch (e) { result.privateTab = { error: String(e.message) }; }
}

// ------------------------------------------------------------------------------------------------ aggregation
function sum(arr, f) { return arr.reduce((a, x) => a + (f(x) || 0), 0); }
function aggregate(run) {
  const rows = [];
  for (const id of state.runs[run].queue) {
    const e = state.byId.get(id);
    const recs = [...state.inst.entries()].filter(([k, v]) => k.startsWith(id + "|") && v.run === run).map(([, v]) => v);
    const snaps = recs.map((r) => r.snap);
    const tops = snaps.filter((s) => s.top && !s.hasOpener);
    const pops = snaps.filter((s) => s.top && s.hasOpener);
    const kinds = {};
    for (const s of snaps) { const k = s.role === "popup" || s.label.startsWith("popup") ? "popup" : s.top && !s.hasOpener ? "top" : "frame:" + s.label; (kinds[k] = kinds[k] || { n: 0, guard: 0 }).n++; if (s.engine && s.engine.guard) kinds[k].guard++; }
    const popupParent = [];
    for (const s of snaps) for (const p of s.popups || []) popupParent.push(p);
    const mo = sum(snaps, (s) => s.mo.unsafe), raf = sum(snaps, (s) => s.raf.unsafe), task = sum(snaps, (s) => s.task.unsafe);
    const baseRaf = sum(snaps, (s) => s.base.unsafe), baseTask = sum(snaps, (s) => s.base.taskUnsafe);
    const exposed = sum(snaps, (s) => (s.mo.exposed || 0) + (s.raf.exposed || 0) + (s.task.exposed || 0));
    const inShieldVisible = sum(snaps, (s) => s.mo.leak + s.raf.leak + s.task.leak);
    const popUnsafe = sum(recs, (r) => r.maxPopUnsafe);
    const audit = { shieldExpected: 0, shieldedOk: 0, missed: [], clearExpected: 0, falseShield: 0, falseList: [], evasiveTotal: 0, evasiveMissed: 0 };
    for (const s of snaps) if (s.audit) { for (const k of ["shieldExpected", "shieldedOk", "clearExpected", "falseShield", "evasiveTotal", "evasiveMissed"]) audit[k] += s.audit[k] || 0; audit.missed.push(...(s.audit.missed || [])); audit.falseList.push(...(s.audit.falseList || [])); }
    const shields = sum(snaps, (s) => s.engine && s.engine.shields);
    const popShields = sum(popupParent, (p) => p.shields || 0);
    const injected = snaps.some((s) => s.engine && s.engine.guard) || popupParent.some((p) => p.guard);
    const blind = [...new Set(snaps.flatMap((s) => s.frames.blind))];
    const hot = e.hot;
    const row = {
      id, kind: e.kind, scope: e.scope, why: e.why || "", docs: snaps.length, hasTop: tops.length > 0, injected, kinds,
      sentinels: sum(snaps, (s) => s.maxSeen), shields: shields + popShields,
      moUnsafe: mo, rafUnsafe: raf + baseRaf, taskUnsafe: task + baseTask, popupUnsafe: popUnsafe, exposed, inShieldVisible,
      moScans: sum(snaps, (s) => s.mo.scans), moSkipped: sum(snaps, (s) => s.mo.skipped), rafScans: sum(snaps, (s) => s.raf.scans), taskScans: sum(snaps, (s) => s.task.scans),
      shieldExpected: audit.shieldExpected, shieldedOk: audit.shieldedOk, missed: audit.missed.slice(0, 6),
      clearExpected: hot ? 0 : audit.clearExpected, falseShield: hot ? 0 : audit.falseShield, falseList: hot ? [] : audit.falseList.slice(0, 4),
      evasiveTotal: audit.evasiveTotal, evasiveMissed: audit.evasiveMissed,
      blind, popups: popupParent.map((p) => ({ created: p.created, closed: p.closed, guard: p.guard, shields: p.shields, rendered: p.sentinelsRendered, unshielded: p.unshielded })).slice(0, 4),
      titles: [...new Set(snaps.map((s) => s.title))].slice(0, 4), extra: snaps.map((s) => s.extra).filter(Boolean).slice(-3), restored: snaps.map((s) => s.restored).filter(Boolean)[0] || null,
      units: snaps.flatMap((s) => s.units || []).slice(0, 8), missInfo: snaps.flatMap((s) => s.missInfo || []).slice(0, 4), shieldInfo: snaps.map((s) => ({ list: s.engine.shieldList, html: s.engine.htmlAttrs, body: s.engine.bodyAttrs })).slice(0, 3),
      errors: [...new Set(snaps.flatMap((s) => s.errors || []))].slice(0, 3), first: { seen: snaps.map((s) => s.firstSeenAt).filter((x) => x != null)[0] ?? null, shield: snaps.map((s) => s.firstShieldAt).filter((x) => x != null)[0] ?? null },
      examples: snaps.flatMap((s) => s.examples || []).slice(0, 3),
    };
    row.leak = row.moUnsafe + row.rafUnsafe + row.taskUnsafe + row.popupUnsafe > 0;
    row.rafLeak = row.rafUnsafe + row.taskUnsafe + row.popupUnsafe > 0;
    row.serverHits = state.hits.get(e.path.split("?")[0]) || 0;
    row.missedRendered = row.missInfo.filter((m) => m.rendered).length;
    row.deferredHidden = row.missInfo.filter((m) => !m.rendered).length;
    row.missedShield = (e.expectShield && row.shields === 0) || (row.missed.length > 0 && (row.missInfo.length === 0 || row.missedRendered > 0));
    rows.push(row);
  }
  return rows;
}

const pad = (s, n) => String(s).slice(0, n).padEnd(n);
function printTable(name, rows) {
  console.log(`\n=== run: ${name} ===`);
  console.log(pad("fixture", 36) + pad("scope", 7) + pad("inj", 5) + pad("sent", 5) + pad("MO", 5) + pad("rAF", 5) + pad("task", 5) + pad("popU", 5) + pad("expose", 7) + pad("shl", 5) + pad("hit/miss", 10) + pad("falseSh", 8) + "notes");
  for (const r of rows) {
    const verdict = r.leak ? (r.scope === "must" ? "LEAK" : r.scope.toUpperCase()) : "";
    const notes = [verdict, r.missedShield ? "MISSED:" + r.missed.slice(0, 2).join(",") : "", r.blind.length ? "blind:" + r.blind.length : "", !r.hasTop ? "NO-TOP-REPORT" : "", r.errors.length ? "err:" + r.errors[0] : ""].filter(Boolean).join(" ");
    console.log(pad(r.id, 36) + pad(r.scope, 7) + pad(r.injected ? "yes" : "NO", 5) + pad(r.sentinels, 5) + pad(r.moUnsafe, 5) + pad(r.rafUnsafe, 5) + pad(r.taskUnsafe, 5) + pad(r.popupUnsafe, 5) + pad(r.exposed, 7) + pad(r.shields, 5) + pad(`${r.shieldedOk}/${r.shieldExpected - r.shieldedOk}`, 10) + pad(r.falseShield, 8) + notes);
  }
}

const TITLE_TERMS = /gta|lucia|jason|ending|hero dies|betray|final mission|TITLE-[A-Z0-9]+-LEAK/i;
function titleAudit(res) {
  const out = { history: [], session: [], captions: [], offendingHistory: [], offendingSession: [], offendingCaptions: [], protectedTitleSeen: false };
  for (const [name, text] of Object.entries(res.fileText || {})) {
    const lines = text.split(/\r?\n/).filter(Boolean);
    if (/history.txt$/i.test(name)) for (const l of lines) { const p = l.split("\t"); const e = { url: p[1], title: p.slice(2).join("\t") }; out.history.push(e); if (TITLE_TERMS.test(e.title) && !/^redteam /.test(e.title)) out.offendingHistory.push(e); if (e.title === "Spoiler-protected page") out.protectedTitleSeen = true; }
    if (/sessions?\.txt$/i.test(name)) for (const l of lines) { const p = l.split("\t"); if (p[0] !== "tab") continue; const e = { url: p[2], title: p.slice(3).join("\t") }; out.session.push(e); if (TITLE_TERMS.test(e.title) && !/^redteam /.test(e.title)) out.offendingSession.push(e); }
  }
  const marks = (res.marks || []).filter((m) => m.event === "start");
  const fxAt = (t) => { let f = null; for (const m of marks) { if (m.t <= t) f = m.fx; } return f; };
  for (const c of res.captions || []) {
    const e = { t: c.t, fx: fxAt(c.t), caption: c.caption }; out.captions.push(e);
    if (TITLE_TERMS.test(c.caption) && !/Spoiler-protected/i.test(c.caption)) out.offendingCaptions.push(e);
  }
  return out;
}

// ------------------------------------------------------------------------------------------------ main
const catalog = buildCatalog();
if (flag("list")) { for (const e of catalog) console.log(e.id, e.core ? "[core]" : "", e.path); console.log(catalog.length, "fixtures"); process.exit(0); }
if (!existsSync(EXE)) { console.error("exe not found: " + EXE); process.exit(2); }
state.catalog = catalog; for (const e of catalog) state.byId.set(e.id, e);
const srv = await startServer();
if (flag("serve")) { state.runs.default = { queue: catalog.map((e) => e.id), done: false }; state.run = "default"; console.log("serve only; try " + state.origins.a + "/__start?run=default"); await sleep(600000); process.exit(0); }
await startKeeper();
await startHelper();
console.log(`servers: A ${state.origins.a}  B ${state.origins.b}; exe ${EXE}; desktop ${DESKTOP}`);
const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const report = { stamp, exe: EXE, origins: state.origins, limitation: "Hostnames like x.com cannot be mapped in Bowser, so fixtures run on 127.0.0.1: site-specific adapters see a generic site. Evidence is DOM-level (no pixels on the hidden desktop).", runs: {} };
for (const name of RUNS) {
  if (!RUN_DEFS[name]) { console.log("unknown run " + name); continue; }
  state.inst.clear();
  let res;
  try { res = await runSuite(name, catalog, srv); } catch (e) { console.log(`[${name}] FAILED: ${e.stack || e.message}`); res = { run: name, error: String(e.message) }; }
  if (!res.error) {
    res.rows = aggregate(name); printTable(name, res.rows);
    res.titleAudit = titleAudit(res);
    const ta = res.titleAudit;
    console.log(`[${name}] titles: history ${ta.history.length} (offending ${ta.offendingHistory.length}), session ${ta.session.length} (offending ${ta.offendingSession.length}), captions ${ta.captions.length} (offending ${ta.offendingCaptions.length}), protected title recorded: ${ta.protectedTitleSeen}`);
    for (const o of [...ta.offendingHistory, ...ta.offendingSession].slice(0, 12)) console.log("   TITLE LEAK:", o.url, "->", o.title);
    for (const o of ta.offendingCaptions.slice(0, 12)) console.log("   CAPTION LEAK:", o.fx, "->", o.caption);
  }
  report.runs[name] = res;
  writeFileSync(join(OUT, `live-${stamp}.json`), JSON.stringify(report, null, 1));
}
await srv.close();
console.log("\nreport: " + join(OUT, `live-${stamp}.json`));
process.exit(0);
