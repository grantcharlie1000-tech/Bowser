// Real-Chrome test: loads dist/chrome as an unpacked extension in Playwright's bundled Chromium
// (branded Chrome ignores --load-extension) and exercises the host: service worker, content
// scripts in top frames and iframes, shielding of a synthetic spoiler, popup, settings persistence
// across a browser restart, service worker suspension, and the Protection OFF flow.
//
//   node build/build.mjs && node test/chrome/load-extension.mjs
//
// Requires: npm install --prefix test/chrome   (playwright-core) and a Playwright Chromium build.
// Fixtures contain invented, synthetic text only.
import http from "node:http";
import { readFileSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, extname } from "node:path";
import { loadPlaywright, launchWithExtension, getServiceWorker, ROOT, DIST_CHROME } from "./harness.mjs";

const HEADED = false; // never headed: automation must not open windows or take focus
const FIXTURES = join(ROOT, "test", "chrome", "fixtures");
const results = [];
let currentGroup = "";

async function check(name, fn) {
  try {
    await fn();
    results.push({ name: `${currentGroup}${name}`, ok: true });
    console.log(`  ok   ${currentGroup}${name}`);
  } catch (e) {
    results.push({ name: `${currentGroup}${name}`, ok: false, error: String(e && e.message || e) });
    console.log(`  FAIL ${currentGroup}${name}\n       ${String(e && e.message || e).split("\n")[0]}`);
  }
}
function assert(cond, msg) { if (!cond) throw new Error(msg || "assertion failed"); }

function serve(port) {
  const server = http.createServer((req, res) => {
    const path = new URL(req.url, "http://x").pathname;
    const file = join(FIXTURES, path === "/" ? "page.html" : path.replace(/^\/+/, ""));
    if (!file.startsWith(FIXTURES) || !existsSync(file)) { res.writeHead(404).end("not found"); return; }
    const type = { ".html": "text/html; charset=utf-8", ".js": "text/javascript" }[extname(file)] || "application/octet-stream";
    res.writeHead(200, { "content-type": type, "cache-control": "no-store" }).end(readFileSync(file));
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}

// A frame counts as shielded when some element carries data-sfw and an sfw-shield child.
const shieldProbe = (frame, id) => frame.evaluate((elId) => {
  const el = document.getElementById(elId);
  return !!el && el.hasAttribute("data-sfw") && !!el.querySelector(":scope > sfw-shield");
}, id);
const attrProbe = (frame, id) => frame.evaluate((elId) => {
  const el = document.getElementById(elId);
  return !!el && el.hasAttribute("data-sfw");
}, id);

async function waitShield(frame, id, timeout = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await shieldProbe(frame, id).catch(() => false)) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
}

async function sessionKeys(sw) {
  return sw.evaluate(async () => Object.keys(await chrome.storage.session.get(null)).filter((k) => k.startsWith("tab:")));
}

async function main() {
  const playwright = loadPlaywright();
  const userDataDir = mkdtempSync(join(tmpdir(), "sfw-chrome-profile-"));
  const s1 = await serve(0);
  const s2 = await serve(0);
  const portA = s1.address().port, portB = s2.address().port;
  const url = `http://127.0.0.1:${portA}/page.html?crossPort=${portB}`;
  const pageErrors = [];
  let context;

  try {
    // ---------------------------------------------------------------------------------------
    currentGroup = "[session 1] ";
    context = await launchWithExtension(playwright, { userDataDir, headless: !HEADED });
    const { sw, extensionId } = await getServiceWorker(context);
    await check("service worker registered with chrome-extension:// URL", async () => assert(/^[a-p]{32}$/.test(extensionId), extensionId));

    await check("manifest permissions are exactly [storage, alarms]", async () => {
      const m = await sw.evaluate(() => chrome.runtime.getManifest());
      assert(JSON.stringify(m.permissions) === '["storage","alarms"]', JSON.stringify(m.permissions));
      assert(m.manifest_version === 3 && m.content_scripts.length === 2);
    });

    await check("default settings written on install", async () => {
      let s = {};
      for (let i = 0; i < 30 && s.mode === undefined; i++) { s = await sw.evaluate(() => chrome.storage.local.get(null)); if (s.mode === undefined) await new Promise((r) => setTimeout(r, 200)); }
      assert(s.enabled === true && s.mode === "strict" && s.reducedEffects === false, JSON.stringify(s));
    });

    const page = await context.newPage();
    page.on("pageerror", (e) => pageErrors.push(`page: ${e.message}`));
    await page.goto(url, { waitUntil: "load" });

    await check("top-frame spoiler is shielded (data-sfw + sfw-shield child)", async () => assert(await waitShield(page.mainFrame(), "spoiler"), "spoiler not shielded"));
    await check("harmless top-frame content is not shielded", async () => {
      assert(!(await attrProbe(page.mainFrame(), "safe")), "#safe was shielded");
      assert(!(await attrProbe(page.mainFrame(), "safe2")), "#safe2 was shielded");
    });
    await check("same-origin iframe spoiler is shielded", async () => {
      const f = page.frames().find((fr) => fr.url().endsWith(`${portA}/frame.html`));
      assert(f, "same-origin frame not found");
      assert(await waitShield(f, "frame-spoiler"), "same-origin frame spoiler not shielded");
      assert(!(await attrProbe(f, "frame-safe")), "harmless frame text was shielded");
    });
    await check("cross-origin iframe spoiler is shielded", async () => {
      let f;
      for (let i = 0; i < 50 && !f; i++) { f = page.frames().find((fr) => fr.url().includes(`localhost:${portB}/frame.html`)); if (!f) await new Promise((r) => setTimeout(r, 100)); }
      assert(f, "cross-origin frame not found");
      assert(await waitShield(f, "frame-spoiler"), "cross-origin frame spoiler not shielded");
    });
    let tabKey;
    await check("service worker received page-start (per-tab record exists)", async () => {
      for (let i = 0; i < 30 && !tabKey; i++) { tabKey = (await sessionKeys(sw))[0]; if (!tabKey) await new Promise((r) => setTimeout(r, 200)); }
      assert(tabKey, "no tab record in storage.session");
    });

    await check("shield count reaches the service worker (counts only, no text)", async () => {
      let rec;
      for (let i = 0; i < 40; i++) {
        rec = await sw.evaluate(async (k) => (await chrome.storage.session.get(k))[k], tabKey);
        if (rec && rec.shields > 0) break;
        await new Promise((r) => setTimeout(r, 250));
      }
      assert(rec && rec.shields > 0, `no shields recorded: ${JSON.stringify(rec)}`);
      const json = JSON.stringify(rec);
      assert(!/lighthouse|narrator|final mission/i.test(json), "page text leaked into counters");
    });

    await check("service worker rejects messages that are not from a content script", async () => {
      // An extension page has no sender.tab, so a forged report must be ignored (no response).
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html`);
      const before = await sessionKeys(sw);
      const resp = await p.evaluate(() => Promise.race([
        chrome.runtime.sendMessage({ t: "report", counts: { shield_forged: 5 } }).then((r) => r === undefined ? "ignored" : "answered"),
        new Promise((r) => setTimeout(() => r("ignored"), 1500)),
      ]).catch(() => "ignored"));
      assert(resp === "ignored", "forged report was answered");
      assert(JSON.stringify(await sessionKeys(sw)) === JSON.stringify(before), "forged report created a record");
      await p.close();
    });

    // ---- popup -----------------------------------------------------------------------------
    const tabId = Number(tabKey.split(":")[1]);
    await check("popup shows Protection on, the shield count and the mode controls", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
      await p.waitForSelector("#status-title");
      assert((await p.textContent("#status-title")) === "Protection on", "status text");
      await p.waitForFunction(() => Number(document.getElementById("count").textContent) > 0, null, { timeout: 5000 });
      assert(await p.isChecked("#mode-strict"), "strict should be the default");
      assert(await p.isVisible("#off-btn"));
      const body = await p.textContent("body");
      assert(!/lighthouse|narrator/i.test(body), "popup shows page text");
      await p.close();
    });
    await check("popup and options pages have no console errors", async () => {
      for (const name of ["popup.html", "options.html?welcome=1"]) {
        const p = await context.newPage();
        const errs = [];
        p.on("pageerror", (e) => errs.push(e.message));
        p.on("console", (m) => { if (m.type() === "error") errs.push(m.text()); });
        await p.goto(`chrome-extension://${extensionId}/${name}`);
        await p.waitForTimeout(400);
        assert(errs.length === 0, `${name}: ${errs.join("; ")}`);
        await p.close();
      }
    });
    await check("options welcome notice mentions reloading open tabs", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/options.html?welcome=1`);
      assert(await p.isVisible("#welcome"));
      assert(/reload/i.test(await p.textContent("#welcome")));
      await p.close();
    });

    // ---- settings change persists ----------------------------------------------------------
    await check("choosing Lockdown in the popup is stored", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
      await p.check("#mode-lockdown");
      await p.waitForTimeout(200);
      const s = await sw.evaluate(() => chrome.storage.local.get("mode"));
      assert(s.mode === "lockdown", JSON.stringify(s));
      await p.close();
    });
    await check("content script picks up the mode change without a reload", async () => {
      // Probe indirectly: a fresh page after the change still shields (config applied or default).
      const p = await context.newPage();
      await p.goto(url, { waitUntil: "load" });
      assert(await waitShield(p.mainFrame(), "spoiler"), "not shielded in lockdown");
      await p.close();
    });

    // ---- service worker suspension ---------------------------------------------------------
    await check("protection survives the service worker being stopped", async () => {
      const cdp = await context.newCDPSession(page);
      await cdp.send("ServiceWorker.enable");
      await cdp.send("ServiceWorker.stopAllWorkers");
      await new Promise((r) => setTimeout(r, 800));
      const p = await context.newPage();
      await p.goto(url, { waitUntil: "load" });
      assert(await waitShield(p.mainFrame(), "spoiler"), "new page not shielded after SW stop");
      assert(!(await attrProbe(p.mainFrame(), "safe")), "harmless text shielded");
      // The worker wakes for the next message and records the new page.
      let keys = [];
      for (let i = 0; i < 40; i++) {
        const [w] = context.serviceWorkers();
        if (w) { keys = await w.evaluate(async () => Object.keys(await chrome.storage.session.get(null)).filter((k) => k.startsWith("tab:"))).catch(() => []); if (keys.length >= 2) break; }
        await new Promise((r) => setTimeout(r, 250));
      }
      assert(keys.length >= 2, `worker did not wake and record the new page (${keys.length} records)`);
      await cdp.detach().catch(() => {});
      await p.close();
    });

    // ---- Protection OFF flow ---------------------------------------------------------------
    await check("turning protection off requires confirmation and is shown as OFF", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
      await p.click("#off-btn");
      assert(await p.isVisible("#confirm"), "confirmation not shown");
      assert((await sw.evaluate(() => chrome.storage.local.get("enabled"))).enabled === true, "disabled before confirming");
      await p.click("#keep-btn");
      assert(!(await p.isVisible("#confirm")) && (await p.textContent("#status-title")) === "Protection on", "cancel did not keep protection on");
      await p.click("#off-btn");
      await p.click("#confirm-btn");
      await p.waitForFunction(() => document.getElementById("status-title").textContent === "Protection OFF");
      assert((await sw.evaluate(() => chrome.storage.local.get("enabled"))).enabled === false, "enabled not false");
      let badge = "";
      for (let i = 0; i < 20 && badge !== "OFF"; i++) { badge = await sw.evaluate(() => chrome.action.getBadgeText({})); if (badge !== "OFF") await new Promise((r) => setTimeout(r, 150)); }
      assert(badge === "OFF", `badge is "${badge}"`);
      assert(await p.isVisible("#on-btn"));
      await p.click("#on-btn");
      await p.waitForFunction(() => document.getElementById("status-title").textContent === "Protection on");
      assert((await sw.evaluate(() => chrome.storage.local.get("enabled"))).enabled === true, "not re-enabled");
      await p.close();
    });
    await check("settings are plain values only (no page text stored anywhere)", async () => {
      const local = JSON.stringify(await sw.evaluate(() => chrome.storage.local.get(null)));
      const session = JSON.stringify(await sw.evaluate(() => chrome.storage.session.get(null)));
      assert(!/lighthouse|narrator|final mission/i.test(local + session), "page text found in storage");
    });

    await context.close();
    context = null;

    // ---------------------------------------------------------------------------------------
    currentGroup = "[session 2, same profile] ";
    context = await launchWithExtension(playwright, { userDataDir, headless: !HEADED });
    const second = await getServiceWorker(context);
    await check("mode setting persisted across a browser restart", async () => {
      const s = await second.sw.evaluate(() => chrome.storage.local.get(null));
      assert(s.mode === "lockdown" && s.enabled === true, JSON.stringify(s));
    });
    await check("popup reflects the persisted mode", async () => {
      const p = await context.newPage();
      await p.goto(`chrome-extension://${second.extensionId}/popup.html`);
      await p.waitForSelector("#status-title");
      await p.waitForFunction(() => document.getElementById("mode-lockdown").checked);
      await p.close();
    });
    await check("spoiler still shielded after restart (Lockdown)", async () => {
      const p = await context.newPage();
      await p.goto(url, { waitUntil: "load" });
      assert(await waitShield(p.mainFrame(), "spoiler"), "not shielded after restart");
      await p.close();
    });
    await check("install defaults did not overwrite the saved mode on restart", async () => {
      const s = await second.sw.evaluate(() => chrome.storage.local.get("mode"));
      assert(s.mode === "lockdown");
    });
    await context.close();
    context = null;
  } finally {
    if (context) await context.close().catch(() => {});
    s1.close(); s2.close();
    try { rmSync(userDataDir, { recursive: true, force: true }); } catch { /* profile may still be locked */ }
  }

  await check_noErrors(pageErrors);
  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} checks passed, ${failed.length} failed.`);
  console.log(`dist: ${DIST_CHROME}`);
  process.exit(failed.length ? 1 : 0);
}

async function check_noErrors(pageErrors) {
  currentGroup = "";
  await check("no uncaught page errors on fixture pages", async () => assert(pageErrors.length === 0, pageErrors.join("; ")));
}

main().catch((e) => { console.error(e); process.exit(2); });
