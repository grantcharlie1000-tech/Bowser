// Real-extension test of the signed term-update feed: the REAL dist/chrome in Playwright's Chromium
// (headless=new, muted, no windows, below-normal priority), a local feed server on 127.0.0.1 and the committed
// TEST key. Every hostname except loopback is unresolvable, so nothing can reach the real internet.
//
//   node build/build.mjs && node test/chrome/feed-extension.mjs
//
// Proves: the fetch needs no host permission (the local server sends Access-Control-Allow-Origin: * exactly like
// raw.githubusercontent.com); a verified feed is stored and applied to pages (live and at document_start); rollback,
// unsigned, tampered, wrong-key, oversize, 404, 500 and hanging answers are ignored with a visible status and
// protection untouched; an expired feed is still applied but flagged; the switch stops the downloads; requests carry
// no cookies and no referrer; the override that lets this test trust the test key works only because this is an
// unpacked build talking to loopback.
import http from "node:http";
import os from "node:os";
import { readFileSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, extname } from "node:path";
import { loadPlaywright, launchWithExtension, getServiceWorker, ROOT } from "./harness.mjs";
import { FEED_DIR, loadPrivateKey, makeEnvelope, publicKeyBase64 } from "../../feed/tools/lib.mjs";
import { generate } from "../../feed/tools/keygen.mjs";

try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch { /* ignore */ }

const FIXTURES = join(ROOT, "test", "chrome", "fixtures");
const testKey = loadPrivateKey(join(FEED_DIR, "test-keys", "test-feed-key.pem"));
const TEST_PUB = publicKeyBase64(testKey);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
async function check(name, fn) {
  try { await fn(); results.push({ name, ok: true }); console.log(`  ok   ${name}`); }
  catch (e) { results.push({ name, ok: false, error: String(e && e.message || e) }); console.log(`  FAIL ${name}\n       ${String(e && e.message || e).split("\n")[0]}`); }
}
const assert = (c, m) => { if (!c) throw new Error(m || "assertion failed"); };

const payload = (seq, lexicon, over = {}) => JSON.stringify({ format: 1, seq, issued: "2026-11-17", expires: "2099-01-01", lexicon, ...over });
const feedBody = (seq, terms, over, key = testKey) => makeEnvelope(payload(seq, { add: { entityStrong: terms } }, over), key);

// ---- servers ------------------------------------------------------------------------------------------------
const requests = []; // feed server log: {method, path, headers}
let answer = { status: 404, body: "not found" }; // what /feed.json says right now
function serveFeed() {
  const server = http.createServer((req, res) => {
    requests.push({ method: req.method, url: req.url, headers: req.headers });
    const cors = { "access-control-allow-origin": "*", "cache-control": "max-age=300" }; // like raw.githubusercontent.com
    if (req.url !== "/feed.json") { res.writeHead(404, cors).end("nf"); return; }
    if (answer.hang) return; // never answers
    res.writeHead(answer.status, { ...cors, "content-type": "application/json" }).end(answer.body);
  });
  return new Promise((r) => server.listen(0, "127.0.0.1", () => r(server)));
}
function servePages() {
  const server = http.createServer((req, res) => {
    const path = new URL(req.url, "http://x").pathname;
    const file = join(FIXTURES, path.replace(/^\/+/, ""));
    if (!file.startsWith(FIXTURES) || !existsSync(file)) { res.writeHead(404).end("nf"); return; }
    res.writeHead(200, { "content-type": { ".html": "text/html; charset=utf-8", ".js": "text/javascript" }[extname(file)] || "text/plain", "cache-control": "no-store" }).end(readFileSync(file));
  });
  return new Promise((r) => server.listen(0, "127.0.0.1", () => r(server)));
}

const shielded = (page, id) => page.evaluate((i) => { const el = document.getElementById(i); return !!el && el.hasAttribute("data-sfw") && !!el.querySelector(":scope > sfw-shield"); }, id).catch(() => false);
async function waitShield(page, id, want = true, timeout = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeout) { if ((await shielded(page, id)) === want) return true; await sleep(100); }
  return false;
}

async function main() {
  const playwright = loadPlaywright();
  const feedSrv = await serveFeed();
  const pageSrv = await servePages();
  const feedUrl = `http://127.0.0.1:${feedSrv.address().port}/feed.json`;
  const pageUrl = `http://127.0.0.1:${pageSrv.address().port}/feed-terms.html`;
  const userDataDir = mkdtempSync(join(tmpdir(), "sfw-feed-profile-"));
  let context;
  try {
    context = await launchWithExtension(playwright, { userDataDir, extraArgs: ["--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"] });
    const { sw, extensionId } = await getServiceWorker(context);
    const opts = await context.newPage();
    await opts.goto(`chrome-extension://${extensionId}/options.html`);
    const store = (keys) => opts.evaluate((k) => chrome.storage.local.get(k), keys);
    const setStore = (obj) => opts.evaluate((o) => chrome.storage.local.set(o), obj);
    const checkNow = () => opts.evaluate(() => chrome.runtime.sendMessage({ t: "feed-check" }));
    const statusLine = async () => (await opts.locator("#feed-status").textContent()) || "";
    const waitLine = async (re, timeout = 5000) => {
      const start = Date.now();
      let line = "";
      while (Date.now() - start < timeout) { line = await statusLine(); if (re.test(line)) return line; await sleep(100); }
      throw new Error(`status line never matched ${re}: "${line}"`);
    };
    const offered = () => requests.filter((r) => r.url === "/feed.json").length;

    await check("manifest: permissions are exactly [storage, alarms]; no host permissions; classic worker", async () => {
      const m = await sw.evaluate(() => chrome.runtime.getManifest());
      assert(JSON.stringify(m.permissions) === '["storage","alarms"]', JSON.stringify(m.permissions));
      assert(!m.host_permissions && !m.optional_host_permissions, "host permissions present");
      assert(m.background.type !== "module" && !("update_url" in m), "worker type / update_url");
    });

    await check("the 12-hour alarm is scheduled", async () => {
      let alarm = null;
      for (let i = 0; i < 20 && !alarm; i++) { alarm = await sw.evaluate(() => chrome.alarms.get("sfw-feed")); if (!alarm) await sleep(100); }
      assert(alarm && alarm.periodInMinutes === 720, JSON.stringify(alarm));
    });

    await check("install-time check cannot reach the real host: status is 'could not check', protection keeps working", async () => {
      let s = {};
      for (let i = 0; i < 40 && !s.feedStatus; i++) { s = await store(["feedStatus", "feed"]); if (!s.feedStatus) await sleep(250); }
      assert(s.feedStatus && s.feedStatus.state === "unavailable" && !s.feed, JSON.stringify(s));
      await waitLine(/^Term list: built-in 2026\.11\.17-1\. Could not check for term updates\.$/);
      const page = await context.newPage();
      await page.goto(pageUrl);
      assert(await waitShield(page, "builtin"), "built-in spoiler not shielded while updates are unavailable");
      assert(!(await shielded(page, "newname")), "unknown name shielded without a feed");
      await page.close();
    });

    // The test override: honoured only for an unpacked build and a loopback URL.
    await setStore({ feedDev: { url: feedUrl, key: TEST_PUB } });
    const page = await context.newPage();
    await page.goto(pageUrl);
    assert(await waitShield(page, "builtin"), "setup: built-in spoiler");

    await check("404 (nothing published yet) is quiet: status 'none', no feed stored", async () => {
      answer = { status: 404, body: "not found" };
      const r = await checkNow();
      assert(r.state === "none", JSON.stringify(r));
      await waitLine(/No term update has been published yet\./);
      assert(!(await store("feed")).feed);
    });

    await check("a signed feed is stored, shown in the UI, and applied to the OPEN page live (content added afterwards is covered)", async () => {
      answer = { status: 200, body: feedBody(3, ["harbor master sloane", "pelican bay ferry"]) };
      const r = await checkNow();
      assert(r.state === "ok" && r.seq === 3 && r.updated === true, JSON.stringify(r));
      const f = (await store("feed")).feed;
      assert(f && f.seq === 3 && f.issued === "2026-11-17" && f.lexicon.add.entityStrong.length === 2, JSON.stringify(f));
      await waitLine(/^Term list: built-in 2026\.11\.17-1 \+ update #3 \(2026-11-17\)$/);
      // Already-classified text is not re-read when terms arrive (that needs a runtime rescan hook, see the report);
      // text that appears after the update is classified with the new terms.
      await page.evaluate(() => { for (const [id, text] of [["late-name", "Harbor Master Sloane is such a great character name."], ["late-plain", "Lovely weather this weekend."]]) { const a = document.createElement("article"); a.id = id; const p = document.createElement("p"); p.textContent = text; a.appendChild(p); document.body.appendChild(a); } });
      assert(await waitShield(page, "late-name"), "live page did not apply the new name to new content");
      assert(!(await shielded(page, "late-plain")), "harmless text shielded");
    });

    await check("a fresh page shields the new names at document_start (stored feed loaded with the config)", async () => {
      const p2 = await context.newPage();
      await p2.goto(pageUrl);
      assert(await waitShield(p2, "newname"), "newname");
      assert(await waitShield(p2, "newplace"), "newplace");
      assert(await waitShield(p2, "builtin"), "builtin");
      assert(!(await shielded(p2, "plain")), "plain");
      await p2.close();
    });

    await check("the popup shows the same status line", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/popup.html?tabId=0`);
      await p.waitForFunction(() => /update #3/.test(document.getElementById("feed-status")?.textContent || ""));
      await p.close();
    });

    await check("the request is anonymous: GET /feed.json, no cookie, no referrer, no query, no custom headers", async () => {
      const real = requests.filter((r) => r.url === "/feed.json");
      assert(real.length >= 2, `only ${real.length} request(s)`);
      for (const r of real) {
        assert(r.method === "GET", r.method);
        assert(!r.headers.cookie && !r.headers.authorization && !r.headers.referer, JSON.stringify(r.headers));
        assert(r.headers.origin === undefined || /^chrome-extension:\/\/[a-p]{32}$/.test(r.headers.origin), `origin ${r.headers.origin}`);
        assert(r.headers["cache-control"] === "no-cache" || r.headers.pragma === "no-cache", "cache:no-store must bypass the HTTP cache");
        const extras = Object.keys(r.headers).filter((h) => !["host", "connection", "user-agent", "accept", "accept-encoding", "accept-language", "origin", "sec-fetch-mode", "sec-fetch-site", "sec-fetch-dest", "sec-ch-ua", "sec-ch-ua-mobile", "sec-ch-ua-platform", "cache-control", "pragma", "priority"].includes(h));
        assert(extras.length === 0, `unexpected headers: ${extras}`);
      }
    });

    const before = { feed: (await store("feed")).feed };
    const rejected = async (label, body, code, status = 200) => check(label, async () => {
      answer = { status, body };
      const r = await checkNow();
      assert(r.state === "rejected" && r.code === code, JSON.stringify(r));
      assert(JSON.stringify((await store("feed")).feed) === JSON.stringify(before.feed), "stored feed changed");
      await waitLine(/Term update rejected\./);
      await page.reload();
      assert(await waitShield(page, "newname"), "protection from the last good feed was lost");
      assert(await waitShield(page, "builtin"), "built-in protection lost");
    });
    const stranger = loadPrivateKey(generate().privatePem);
    await rejected("rollback: a validly signed LOWER seq is rejected, the stored feed and protection stay", feedBody(2, ["zz older term"]), "rollback");
    await rejected("wrong key: a feed signed by someone else is rejected", feedBody(9, ["zz stranger term"], {}, stranger), "signature");
    {
      const good = JSON.parse(feedBody(10, ["zz tampered term"]));
      const bytes = Buffer.from(good.payload, "base64");
      bytes[bytes.indexOf(Buffer.from("zz tampered"))] = 0x59;
      await rejected("tampered payload is rejected", JSON.stringify({ ...good, payload: bytes.toString("base64") }), "signature");
      await rejected("unsigned feed (no sig) is rejected", JSON.stringify({ format: 1, payload: good.payload }), "envelope");
    }
    await rejected("garbage (an HTML page) is rejected", "<html><body>Not a feed</body></html>", "envelope");
    await rejected("oversize (300 KB) is rejected before parsing", "x".repeat(300000), "size");

    await check("HTTP 500 and a hanging server are 'unavailable', not rejected, and change nothing", async () => {
      answer = { status: 500, body: "oops" };
      let r = await checkNow();
      assert(r.state === "unavailable" && r.code === "http_500", JSON.stringify(r));
      await waitLine(/Could not check for term updates\./);
      answer = { hang: true };
      const t0 = Date.now();
      r = await checkNow();
      const took = Date.now() - t0;
      assert(r.state === "unavailable" && r.code === "timeout" && took >= 9500 && took < 90000, `${JSON.stringify(r)} after ${took} ms`);
      assert(JSON.stringify((await store("feed")).feed) === JSON.stringify(before.feed));
      await page.reload();
      assert(await waitShield(page, "newname"), "protection lost after an unavailable check");
    });

    await check("a higher seq replaces the feed and reaches the open page; an EXPIRED feed is still applied but flagged", async () => {
      answer = { status: 200, body: feedBody(4, ["harbor master sloane", "pelican bay ferry", "zz fourth term"], { issued: "2026-01-01", expires: "2026-01-31" }) };
      const r = await checkNow();
      assert(r.state === "ok" && r.seq === 4, JSON.stringify(r));
      assert((await store("feed")).feed.seq === 4);
      await waitLine(/^Term list: built-in 2026\.11\.17-1 \+ update #4 \(2026-01-01\)\. Term updates out of date\.$/);
      assert(await opts.locator("#feed-status.warn").count() === 1, "out-of-date line is not highlighted");
      const p3 = await context.newPage();
      await p3.goto(pageUrl);
      assert(await waitShield(p3, "newname"), "expired feed was not applied");
      await p3.close();
    });

    await check("the same seq again is a no-op (status ok, feed untouched)", async () => {
      const stamp = (await store("feed")).feed.fetchedAt;
      const r = await checkNow();
      assert(r.state === "ok" && r.updated === false, JSON.stringify(r));
      assert((await store("feed")).feed.fetchedAt === stamp);
    });

    await check("the switch: off stops the downloads, keeps the stored update; on checks immediately", async () => {
      await setStore({ feedEnabled: false });
      const n = offered();
      const r = await checkNow();
      assert(r.state === "off", JSON.stringify(r));
      assert(offered() === n, "a request was made while updates are off");
      await waitLine(/Term updates are off\./);
      assert(await opts.locator("#feed").isChecked() === false);
      const p4 = await context.newPage();
      await p4.goto(pageUrl);
      assert(await waitShield(p4, "newname"), "the stored update must stay in force while downloads are off");
      await p4.close();
      await opts.locator("label[for=feed]").click(); // the real switch
      for (let i = 0; i < 40 && offered() === n; i++) await sleep(100);
      assert(offered() === n + 1, `turning the switch on did not trigger a check (${offered() - n})`);
      assert((await store("feedEnabled")).feedEnabled === true);
    });

    await check("every request in the whole run went to the feed server only (no other host, no real network)", async () => {
      assert(requests.every((r) => r.url === "/feed.json"), JSON.stringify(requests.filter((r) => r.url !== "/feed.json").map((r) => r.url)));
    });

    await check("without the dev override the production URL and key are used (override is inert for non-loopback URLs)", async () => {
      const r = await sw.evaluate(() => ({
        evil: SFWFeedUpdate.devOverride({ feedDev: { url: "https://evil.example/feed.json", key: "AAAA" } }),
        http: SFWFeedUpdate.devOverride({ feedDev: { url: "http://evil.example/feed.json", key: "AAAA" } }),
        lookalike: SFWFeedUpdate.devOverride({ feedDev: { url: "http://127.0.0.1.evil.example/feed.json", key: "AAAA" } }),
        ok: !!SFWFeedUpdate.devOverride({ feedDev: { url: "http://127.0.0.1:1/feed.json", key: "AAAA" } }),
        prod: SFWFeedConfig.FEED_URL,
      }));
      assert(r.evil === null && r.http === null && r.lookalike === null && r.ok === true, JSON.stringify(r));
      assert(r.prod.startsWith("https://raw.githubusercontent.com/"), r.prod);
    });
  } finally {
    if (context) await context.close().catch(() => {});
    feedSrv.close();
    pageSrv.close();
    try { rmSync(userDataDir, { recursive: true, force: true }); } catch { /* chrome may still hold files */ }
  }
  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} checks passed`);
  process.exit(failed.length ? 1 : 0);
}

main().catch((e) => { console.error(e); process.exit(1); });
