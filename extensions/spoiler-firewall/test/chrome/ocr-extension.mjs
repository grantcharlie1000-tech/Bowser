// Real-extension OCR test: the REAL dist/chrome (optional manifest, nothing patched) in Playwright's Chromium
// (headless=new, muted, no windows, below-normal priority).
//
//   node build/build.mjs && node test/chrome/ocr-extension.mjs
//
// How the optional permission is granted: the options page switch calls chrome.permissions.request({permissions:
// ["offscreen"]}) from a real click (a user gesture). "offscreen" carries no install warning, so Chrome grants it
// without a prompt and this works headless; the test asserts the grant really happened (chrome.offscreen appears,
// permissions.contains turns true) and that switching off really removes it. No host permission exists anywhere in
// the manifest: the cross-origin image fetch works from the content_scripts <all_urls> match patterns alone, which
// the "EVIDENCE" check proves against an image host that sends no CORS headers.
//
// Also measured: accuracy and latency of the bundled Tesseract on test/fixtures/ocr-images (served from img.cdn.test,
// cross-origin, scored by the shipped classifier + policy), privacy plumbing (no cookies, only loopback is reachable,
// no OCR text in storage), recreation after the offscreen document closes, memory.
import http from "node:http";
import os from "node:os";
import { readFileSync, existsSync, mkdtempSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, extname } from "node:path";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { loadPlaywright, launchWithExtension, getServiceWorker, ROOT, DIST_CHROME } from "./harness.mjs";

try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch { /* ignore */ }
const require = createRequire(import.meta.url);
const SFW = require(join(ROOT, "dist", "node", "engine.cjs"));
const FIX = join(ROOT, "test", "fixtures");
const EXPECTED = JSON.parse(readFileSync(join(FIX, "ocr-expected.json"), "utf8"));
const IMAGES = Object.keys(EXPECTED);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const report = {};
let group = "";

async function check(name, fn) {
  try { await fn(); results.push({ name: group + name, ok: true }); console.log(`  ok   ${group}${name}`); }
  catch (e) { results.push({ name: group + name, ok: false, error: String(e && e.message || e) }); console.log(`  FAIL ${group}${name}\n       ${String(e && e.message || e).split("\n")[0]}`); }
}
const assert = (c, m) => { if (!c) throw new Error(m || "assertion failed"); };
const pct = (a, p) => { const s = a.slice().sort((x, y) => x - y); return s.length ? s[Math.min(s.length - 1, Math.floor(s.length * p))] : 0; };

// ---- fixture server: pages on x.com (X adapter -> high risk), images on img.cdn.test (cross-origin, no CORS headers) ----
const seen = []; // {host, path, cookie}
function serve() {
  const server = http.createServer((req, res) => {
    const u = new URL(req.url, "http://x");
    seen.push({ host: req.headers.host, path: u.pathname, cookie: req.headers.cookie || "" });
    const file = join(FIX, decodeURIComponent(u.pathname).replace(/^\/+/, ""));
    if (!file.startsWith(FIX) || !existsSync(file)) { res.writeHead(404).end("nf"); return; }
    let body = readFileSync(file);
    const v = u.searchParams.get("v");
    // trailing bytes after IEND are ignored by PNG decoders but change the SHA-256, defeating the content cache
    if (v && file.endsWith(".png")) body = Buffer.concat([body, Buffer.from("x".repeat(Number(v) % 4096 + 1))]);
    const type = { ".html": "text/html; charset=utf-8", ".png": "image/png", ".svg": "image/svg+xml", ".css": "text/css" }[extname(file)] || "application/octet-stream";
    res.writeHead(200, { "content-type": type, "cache-control": "no-store" }).end(body);
  });
  return new Promise((r) => server.listen(0, "127.0.0.1", () => r(server)));
}

function chromeProcs(marker) {
  const ps = `Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" | Where-Object { $_.CommandLine -like '*${marker}*' } | ForEach-Object { $t='browser'; if ($_.CommandLine -match '--type=([a-z-]+)') { $t=$Matches[1] }; $x=if ($_.CommandLine -match '--extension-process') {'ext'} else {''}; "$($_.ProcessId)|$t|$x|$($_.WorkingSetSize)" }`;
  const r = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", ps], { encoding: "utf8" });
  return (r.stdout || "").split(/\r?\n/).filter(Boolean).map((l) => { const [pid, type, ext, ws] = l.split("|"); return { pid: Number(pid), type, ext: ext === "ext", ws: Number(ws) }; });
}

// everything except the two fixture hosts and loopback is unresolvable, so any other request would fail loudly
const RULES = "MAP x.com 127.0.0.1, MAP img.cdn.test 127.0.0.1, MAP * ~NOTFOUND, EXCLUDE 127.0.0.1";
const verdict = (text, mode = "strict") => SFW.policy.decide(SFW.classify.text(text, { hot: 0, source: "ocr" }), { mode, hot: 0, kind: "media", hasMedia: true, highRisk: true, ocrDone: true });
const askSW = (sw, url, kind = "img") => sw.evaluate(([u, k]) => handleOcr({ t: "ocr", url: u, kind: k }), [url, kind]);
// direct to the offscreen scanner with diagnostics (per-pass confidence); never available to content scripts
const askDebug = (sw, url) => sw.evaluate(async (u) => { await ensureOffscreen(); return chrome.runtime.sendMessage({ t: "ocr-run", url: u, kind: "img", debug: true }); }, url);
const stats = (sw) => sw.evaluate(() => chrome.runtime.sendMessage({ t: "ocr-stats" }));
const offscreenCount = (sw) => sw.evaluate(() => chrome.runtime.getContexts({ contextTypes: ["OFFSCREEN_DOCUMENT"] })).then((c) => c.length);

async function main() {
  const playwright = loadPlaywright();
  const srvA = await serve();
  const srvB = await serve();
  const portA = srvA.address().port, portB = srvB.address().port;
  const imgUrl = (name, v) => `http://img.cdn.test:${portB}/ocr-images/${name}${v != null ? `?v=${v}` : ""}`;
  const feed = (q) => `http://x.com:${portA}/ocr-pages/ext-feed.html?img=${portB}&${q}`;
  let s = null;
  try {
    const userDataDir = mkdtempSync(join(tmpdir(), "sfw-ocrprof-"));
    const context = await launchWithExtension(playwright, { userDataDir, extensionDir: DIST_CHROME, headless: true, extraArgs: [`--host-resolver-rules=${RULES}`] });
    const { sw, extensionId } = await getServiceWorker(context, 30000);
    s = { context, sw, extensionId };
    // the worker writes the default settings on install and opens the welcome page: wait for both, then close it
    for (let i = 0; i < 50; i++) { if ((await sw.evaluate(() => chrome.storage.local.get("mode"))).mode) break; await sleep(100); }
    await sleep(500);
    for (const p of context.pages()) if (p.url().startsWith("chrome-extension://")) await p.close().catch(() => {});
    await context.addCookies([{ name: "session", value: "secret-cookie", domain: "img.cdn.test", path: "/" }]);
    const marker = userDataDir.split(/[\\/]/).pop();
    const memBefore = chromeProcs(marker);
    report.chromium = await sw.evaluate(() => navigator.userAgent);

    // ================= nothing granted yet ==========================================================================
    group = "[not granted] ";
    await check("manifest: permissions [storage, alarms] + optional [offscreen]; no host permissions, required or optional; wasm-only CSP", async () => {
      const m = await sw.evaluate(() => chrome.runtime.getManifest());
      assert(JSON.stringify(m.permissions) === '["storage","alarms"]', JSON.stringify(m.permissions));
      assert(JSON.stringify(m.optional_permissions) === '["offscreen"]', JSON.stringify(m.optional_permissions));
      assert(!m.optional_host_permissions && !m.host_permissions && !m.web_accessible_resources);
      assert(m.content_security_policy.extension_pages === "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'");
    });
    await check("chrome.offscreen is undefined and permissions.contains is false", async () => {
      assert((await sw.evaluate(() => typeof chrome.offscreen)) === "undefined");
      assert((await sw.evaluate(() => chrome.permissions.contains({ permissions: ["offscreen"] }))) === false);
    });
    await check("OCR request is 'unavailable' while the scan is off, and also with the flag on but no permission (never an error, never text)", async () => {
      assert((await askSW(sw, imgUrl("ocr-textfree.png"))).status === "unavailable");
      await sw.evaluate(() => chrome.storage.local.set({ ocr: true }));
      const r = await askSW(sw, imgUrl("ocr-textfree.png"));
      assert(r.status === "unavailable", JSON.stringify(r));
    });
    await check("page with the flag on but no permission: no image is released; each gets the 'could not check' cover (strict)", async () => {
      const p = await context.newPage();
      await p.goto(feed("set=a&v=1"), { waitUntil: "load" });
      await p.waitForFunction(() => document.querySelectorAll('.photo[data-sfw~="shield"]').length >= 6, null, { timeout: 20000 });
      const released = await p.evaluate(() => [...document.querySelectorAll("img")].filter((i) => i.getAttribute("data-sfw-m") === "ok").length);
      assert(released === 0, "released without a check: " + released);
      await p.close();
      await sw.evaluate(() => chrome.storage.local.set({ ocr: false }));
    });
    await check("options page: switch off by default; copy says on-device, nothing uploaded, off until allowed; welcome page mentions it", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html?welcome=1`);
      await p.waitForSelector("#ocr");
      assert(!(await p.isChecked("#ocr")), "default must be off");
      const t = await p.textContent("#ocr-card");
      assert(/on your device/i.test(t) && /nothing is uploaded/i.test(t) && /off until you allow/i.test(t), t.slice(0, 200));
      assert(/Scan images for text/i.test(await p.textContent("#welcome")));
      await p.close();
    });
    await check("popup: turning the scan on opens the settings page instead of prompting inside the popup", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/popup.html`);
      await p.waitForSelector("#ocr");
      const opened = context.waitForEvent("page", { timeout: 8000 });
      await p.click("#ocr-card label.name");
      const np = await opened;
      await np.waitForLoadState();
      assert(/options\.html\?ocr=1/.test(np.url()), np.url());
      assert(!(await p.isChecked("#ocr")));
      await np.close(); await p.close();
    });
    await check("a denied request leaves the switch off and says so (permission API answering false)", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html`);
      await p.waitForSelector("#ocr");
      await p.evaluate(() => { chrome.permissions.request = () => Promise.resolve(false); });
      await p.click("#ocr-card label.name");
      await p.waitForSelector("#ocr-note:not([hidden])");
      assert(!(await p.isChecked("#ocr")));
      assert(/not granted/i.test(await p.textContent("#ocr-note")));
      assert((await sw.evaluate(() => chrome.storage.local.get("ocr"))).ocr !== true);
      await p.close();
    });

    // ================= the real grant ==============================================================================
    group = "[grant] ";
    await check("switching on from the options page (a real click) grants the permission and stores ocr:true", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html`);
      await p.waitForSelector("#ocr");
      await p.click("#ocr-card label.name");
      await p.waitForFunction(() => document.getElementById("ocr").checked, null, { timeout: 8000 });
      await sleep(300);
      assert((await sw.evaluate(() => chrome.storage.local.get("ocr"))).ocr === true, "setting stored");
      assert((await sw.evaluate(() => chrome.permissions.contains({ permissions: ["offscreen"] }))) === true, "permission really granted");
      assert((await sw.evaluate(() => typeof chrome.offscreen)) === "object", "chrome.offscreen exists now");
      assert(!(await p.isVisible("#ocr-note")), "no error note");
      await p.close();
    });
    await check("EVIDENCE: no host permission was requested or granted, yet the offscreen document reads a cross-origin image that sends no CORS headers", async () => {
      const m = await sw.evaluate(() => chrome.runtime.getManifest());
      assert(!m.host_permissions && !m.optional_host_permissions);
      assert(JSON.stringify(m.content_scripts.map((c) => c.matches)) === '[["<all_urls>"],["<all_urls>"]]');
      report.permissionsGetAll = await sw.evaluate(() => chrome.permissions.getAll()); // the origin Chrome lists is the content-script pattern
      const t0 = Date.now();
      const r = await askDebug(sw, imgUrl("ocr-tweet-clean.png", 4242));
      report.coldFirstImageMs = Date.now() - t0; // includes creating the offscreen document and starting the worker (wasm + model)
      assert(r.status === "ok", "cross-origin fetch failed without host permission: " + JSON.stringify(r).slice(0, 200));
      report.crossOriginFetchWithoutHostPermission = "ok";
    });

    // ================= accuracy and latency ========================================================================
    group = "[granted] ";
    await check("accuracy: every fixture is judged right by the shipped classifier+policy (strict)", async () => {
      const rows = [];
      for (const name of IMAGES) {
        const t0 = Date.now();
        const r = await askDebug(sw, imgUrl(name, 31337));
        const ms = Date.now() - t0;
        assert(r.status === "ok", `${name}: ${JSON.stringify(r)}`);
        const v = verdict(r.text);
        rows.push({ name, expectedSpoiler: EXPECTED[name].spoiler, verdict: v, ok: (v === "shield") === EXPECTED[name].spoiler, chars: r.text.length, ms, passes: (r.dbg || []).map((d) => d.pass.slice(0, 4) + ":" + d.conf + "/" + d.chars).join(" "), preview: r.text.replace(/\s+/g, " ").slice(0, 60) });
      }
      report.accuracy = rows;
      for (const r of rows) console.log(`         ${r.ok ? "ok  " : "MISS"} ${r.name.padEnd(22)} expect=${r.expectedSpoiler ? "shield" : "clear "} got=${r.verdict.padEnd(6)} ${String(r.ms).padStart(5)}ms [${r.passes}]`);
      const wrong = rows.filter((r) => !r.ok);
      assert(wrong.length === 0, "misjudged: " + wrong.map((r) => r.name).join(", "));
    });
    await check("latency: warm single-image requests through the service worker (cache defeated), p50/p95", async () => {
      const lat = [];
      for (let round = 1; round <= 4; round++) {
        for (const name of IMAGES) {
          const t0 = Date.now();
          const r = await askSW(sw, imgUrl(name, round * 97 + IMAGES.indexOf(name)));
          lat.push(Date.now() - t0);
          assert(r.status === "ok", `${name}: ${JSON.stringify(r)}`);
        }
      }
      report.warmLatencyMs = { n: lat.length, p50: pct(lat, 0.5), p95: pct(lat, 0.95), max: Math.max(...lat), min: Math.min(...lat) };
      console.log(`         warm latency over ${lat.length} requests: p50=${report.warmLatencyMs.p50}ms p95=${report.warmLatencyMs.p95}ms max=${report.warmLatencyMs.max}ms`);
    });
    await check("cache: the same URL, and the same bytes under a new URL, are answered without a recognition", async () => {
      const a0 = await stats(sw);
      let t0 = Date.now();
      const a = await askSW(sw, imgUrl("ocr-tweet-spoiler.png", 97 * 1 + 4));
      const dtUrl = Date.now() - t0;
      t0 = Date.now();
      const b = await askSW(sw, imgUrl("ocr-tweet-spoiler.png", 97 * 1 + 4) + "&alias=1"); // new URL, identical bytes
      const dtHash = Date.now() - t0;
      const a1 = await stats(sw);
      assert(a.status === "ok" && b.status === "ok" && a.text === b.text);
      assert(a1.stats.cacheHits === a0.stats.cacheHits + 1, "URL cache hit " + JSON.stringify([a0.stats, a1.stats]));
      assert(a1.stats.hashHits === a0.stats.hashHits + 1, "content-hash cache hit");
      report.cacheMs = { url: dtUrl, hash: dtHash };
    });

    // ================= safety limits ===============================================================================
    await check("non-image content type is skipped (not read, not an error)", async () => {
      const r = await askSW(sw, `http://img.cdn.test:${portB}/ocr-pages/ext-feed.html`);
      assert(r.status === "skipped", JSON.stringify(r));
    });
    await check("404 is an error; hostile or malformed URLs are rejected without touching the network", async () => {
      const r1 = await askSW(sw, `http://img.cdn.test:${portB}/ocr-images/missing.png`);
      assert(r1.status === "error", JSON.stringify(r1));
      const before = seen.length;
      for (const bad of ["file:///C:/Windows/win.ini", "javascript:alert(1)", "chrome-extension://abc/x.png", "ftp://x/y.png"]) {
        const r = await askSW(sw, bad);
        assert(r.status === "error", bad + " -> " + JSON.stringify(r));
      }
      assert(seen.length === before, "no request may have been made for rejected URLs");
    });
    await check("messages that are not from our content scripts are ignored (forged OCR request from an extension page)", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html`);
      const r = await p.evaluate(() => Promise.race([chrome.runtime.sendMessage({ t: "ocr", url: "data:image/png;base64,AAAA", kind: "img" }).then((x) => x === undefined ? "ignored" : "answered"), new Promise((res) => setTimeout(() => res("ignored"), 2000))]).catch(() => "ignored"));
      assert(r === "ignored", r);
      await p.close();
    });
    await check("data: URL image is read by the scanner directly", async () => {
      const png = readFileSync(join(FIX, "ocr-images", "ocr-tweet-spoiler.png")).toString("base64");
      const r = await askSW(sw, "data:image/png;base64," + png);
      assert(r.status === "ok" && verdict(r.text) === "shield", JSON.stringify(r).slice(0, 160));
    });

    // ================= end to end through real pages and the real content script ====================================
    // Note: three shielded items in 30 s make a feed "warm" (engine rule), after which every item with media is shielded by
    // policy. The sets below keep to two spoilers per page so the OCR verdicts, not that rule, are what is measured.
    const settle = async (p, n) => p.waitForFunction((k) => document.querySelectorAll("img[data-sfw-m=ok]").length + document.querySelectorAll('article[data-sfw~="shield"]').length >= k, n, { timeout: 90000, polling: 200 });
    const outcome = (p) => p.evaluate(() => [...document.querySelectorAll("article[data-fixture]")].map((a) => ({ f: a.getAttribute("data-fixture"), shielded: a.getAttribute("data-sfw") === "shield", released: !!a.querySelector("img[data-sfw-m=ok]") })));
    for (const [set, count] of [["a", 6], ["b", 3]]) {
      await check(`page on a high-risk host, fixture set ${set}: spoiler images shield their whole tweet, clean and text-free images are released`, async () => {
        const p = await context.newPage();
        const errors = [];
        p.on("pageerror", (e) => errors.push(e.message));
        const t0 = Date.now();
        await p.goto(feed(`set=${set}&v=${set === "a" ? 5 : 6}`), { waitUntil: "load" });
        await settle(p, count);
        report["pageSettleMs_" + set] = Date.now() - t0;
        await sleep(400);
        const rows = (await outcome(p)).map((g) => ({ ...g, want: EXPECTED[g.f].spoiler }));
        console.log("         " + rows.map((r) => `${r.f.replace("ocr-", "").replace(".png", "")}:${r.shielded ? "SHIELD" : r.released ? "released" : "held"}`).join(" "));
        const bad = rows.filter((r) => (r.want && !r.shielded) || (!r.want && (r.shielded || !r.released)));
        assert(bad.length === 0, "wrong outcome for " + bad.map((r) => r.f).join(", "));
        assert(errors.length === 0, errors.join("; "));
        await p.close();
      });
    }
    for (const mode of ["blob", "data"]) {
      await check(`${mode}: image created by the page itself is read (the content script sends the bytes of a blob:) and its tweet is shielded`, async () => {
        const p = await context.newPage();
        await p.goto(feed(`mode=${mode}`), { waitUntil: "load" });
        await p.waitForFunction(() => document.querySelector("article[data-sfw~=shield]"), null, { timeout: 40000, polling: 200 });
        assert((await p.evaluate(() => document.querySelector("img").src)).startsWith(mode + ":"), "image really is a " + mode + ": URL");
        await p.close();
      });
    }
    await check("requests carry no cookies and nothing but the fixture hosts was contacted", async () => {
      const fromCdn = seen.filter((r) => r.host.startsWith("img.cdn.test"));
      assert(fromCdn.length > 20, "image server was used: " + fromCdn.length);
      assert(fromCdn.every((r) => !r.cookie), "a request carried a cookie: " + JSON.stringify(fromCdn.find((r) => r.cookie)));
      assert(seen.every((r) => /^(x\.com|img\.cdn\.test)/.test(r.host)), "unexpected host " + JSON.stringify(seen.find((r) => !/^(x\.com|img\.cdn\.test)/.test(r.host))));
    });
    await check("OCR text is never persisted: storage.local/session hold no recognised text", async () => {
      const dump = JSON.stringify(await sw.evaluate(async () => ({ l: await chrome.storage.local.get(null), s: await chrome.storage.session.get(null) })));
      assert(!/ending|lucia|jason|leaked|pasta|tomato/i.test(dump), dump.slice(0, 200));
    });
    await check("offscreen document: counters and memory footprint", async () => {
      const st = await stats(sw);
      const after = chromeProcs(marker);
      const sum = (list, f) => list.filter(f).reduce((a, p) => a + p.ws, 0);
      const mb = (n) => Math.round(n / 1048576);
      report.memory = {
        offscreenStats: st.stats, workers: st.workers, primarySlots: st.primarySlots, relaxedSimd: st.relaxedSimd, jsHeapMB: mb(st.heap),
        extensionRendererWorkingSetMB: mb(sum(after, (p) => p.ext)), allRenderersMB: mb(sum(after, (p) => p.type === "renderer")), renderersBeforeEnablingMB: mb(sum(memBefore, (p) => p.type === "renderer")),
        processes: after.map((p) => `${p.type}${p.ext ? "(ext)" : ""}:${mb(p.ws)}MB`),
      };
      console.log("         " + JSON.stringify(report.memory));
      assert(st.workers >= 1);
    });

    // ================= lifecycle ===================================================================================
    await check("the offscreen document can be closed and is recreated on demand", async () => {
      await sw.evaluate(() => chrome.offscreen.closeDocument());
      assert((await offscreenCount(sw)) === 0);
      const t0 = Date.now();
      const r = await askSW(sw, imgUrl("ocr-tweet-clean.png", 7001));
      report.recreateMs = Date.now() - t0;
      assert(r.status === "ok", JSON.stringify(r).slice(0, 120));
      assert((await offscreenCount(sw)) === 1);
    });
    await check("protection OFF makes the scanner unavailable (and closes it); turning it back on restores it", async () => {
      await sw.evaluate(() => chrome.storage.local.set({ enabled: false }));
      assert((await askSW(sw, imgUrl("ocr-textfree.png", 6))).status === "unavailable");
      let n = 1;
      for (let i = 0; i < 30 && n; i++) { await sleep(200); n = await offscreenCount(sw); }
      assert(n === 0, "offscreen document still open with protection off");
      await sw.evaluate(() => chrome.storage.local.set({ enabled: true }));
      assert((await askSW(sw, imgUrl("ocr-textfree.png", 7))).status === "ok");
    });
    await check("revoking the permission (as chrome://extensions would) switches the setting off by itself", async () => {
      await sw.evaluate(() => chrome.permissions.remove({ permissions: ["offscreen"] }));
      let v;
      for (let i = 0; i < 30; i++) { v = (await sw.evaluate(() => chrome.storage.local.get("ocr"))).ocr; if (v === false) break; await sleep(100); }
      assert(v === false, "ocr setting after revoke: " + v);
      assert((await askSW(sw, imgUrl("ocr-textfree.png", 8))).status === "unavailable");
    });
    await check("switching off from the options page removes the permission and closes the scanner", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html`);
      await p.waitForSelector("#ocr");
      await p.click("#ocr-card label.name");
      await p.waitForFunction(() => document.getElementById("ocr").checked, null, { timeout: 8000 });
      assert((await askSW(sw, imgUrl("ocr-textfree.png", 9))).status === "ok");
      await p.click("#ocr-card label.name");
      await p.waitForFunction(() => !document.getElementById("ocr").checked);
      let has = true;
      for (let i = 0; i < 30 && has; i++) { await sleep(150); has = await sw.evaluate(() => chrome.permissions.contains({ permissions: ["offscreen"] })); }
      assert(has === false, "permission still held after switching off");
      assert((await sw.evaluate(() => chrome.storage.local.get("ocr"))).ocr === false);
      let n = 1;
      for (let i = 0; i < 30 && n; i++) { await sleep(200); n = await offscreenCount(sw); }
      assert(n === 0, "scanner still open");
      await p.close();
    });
  } finally {
    if (s) await s.context.close().catch(() => {});
    srvA.close(); srvB.close();
  }
  const failed = results.filter((r) => !r.ok);
  mkdirSync(join(ROOT, "test", "e2e", "out"), { recursive: true });
  writeFileSync(join(ROOT, "test", "e2e", "out", "ocr-extension.json"), JSON.stringify({ results, report }, null, 1));
  console.log(`\n${results.length - failed.length}/${results.length} checks passed, ${failed.length} failed.`);
  console.log("REPORT " + JSON.stringify({ chromium: report.chromium, coldFirstImageMs: report.coldFirstImageMs, warmLatencyMs: report.warmLatencyMs, cacheMs: report.cacheMs, pageSettleMs_a: report.pageSettleMs_a, pageSettleMs_b: report.pageSettleMs_b, recreateMs: report.recreateMs, memory: report.memory && { jsHeapMB: report.memory.jsHeapMB, extensionRendererWorkingSetMB: report.memory.extensionRendererWorkingSetMB, renderersBeforeEnablingMB: report.memory.renderersBeforeEnablingMB, workers: report.memory.workers } }));
  process.exit(failed.length ? 1 : 0);
}

main().catch((e) => { console.error(e); process.exit(2); });
