// Generates 1280x800 Web Store screenshots from the REAL built extension (dist/chrome) running in
// headless, muted Chromium. Fixtures contain invented text only; no game imagery is used.
//
//   node build/build.mjs && node store/make-screenshots.mjs
//
// The script refuses to write the shield screenshot unless the page really got shielded, so a
// screenshot can never show behaviour the extension does not have.
import http from "node:http";
import { mkdirSync, writeFileSync, readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadPlaywright, launchWithExtension, getServiceWorker } from "../test/chrome/harness.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "store", "screenshots");
const FIXTURES = join(ROOT, "test", "chrome", "fixtures");

const caption = (title, sub, img) => `<!doctype html><meta charset="utf-8"><style>
  html,body{margin:0;width:1280px;height:800px;overflow:hidden}
  body{font-family:"Segoe UI",system-ui,sans-serif;color:#fff;display:grid;grid-template-columns:1fr;align-items:center;gap:56px;padding:0 96px;
    background:radial-gradient(120% 140% at 0% 0%,#4a37bd 0%,#241a63 55%,#14102f 100%)}
  h1{font-size:54px;line-height:1.05;margin:0 0 18px;letter-spacing:-.02em}
  p{font-size:24px;line-height:1.4;margin:0;color:#d8d2ff;max-width:520px}
  small{display:block;margin-top:28px;font-size:15px;color:#a79fe0}
  img{position:absolute;right:96px;top:50px;height:700px;width:auto;border-radius:16px;box-shadow:0 24px 60px rgb(0 0 0/.45);display:block}
</style><body><div><h1>${title}</h1><p>${sub}</p><small>Unofficial fan tool. Not affiliated with Rockstar Games or Take-Two.</small></div><img src="${img}"></body>`;

async function main() {
  const playwright = loadPlaywright();
  mkdirSync(OUT, { recursive: true });
  const server = await new Promise((r) => { const s = http.createServer((q, res) => {
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(readFileSync(join(FIXTURES, "feed.html")));
  }); s.listen(0, "127.0.0.1", () => r(s)); });
  const profile = mkdtempSync(join(tmpdir(), "sfw-shots-"));
  const context = await launchWithExtension(playwright, { userDataDir: profile, headless: true });
  try {
    const { sw, extensionId } = await getServiceWorker(context);
    const base = `chrome-extension://${extensionId}`;
    const page = await context.newPage();
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.waitForFunction(() => document.querySelector('#p2[data-sfw]') && document.querySelector('#p2 > sfw-shield'), null, { timeout: 8000 })
      .catch(() => { throw new Error("The fixture spoiler was not shielded: the runtime is not working, refusing to produce a shield screenshot."); });
    await page.waitForTimeout(500);
    const clean = await page.evaluate(() => !document.querySelector("#p1[data-sfw]") && !document.querySelector("#p3[data-sfw]"));
    if (!clean) throw new Error("harmless posts were shielded; not capturing");
    writeFileSync(join(OUT, "01-spoilers-covered.png"), await page.screenshot({ clip: { x: 0, y: 0, width: 1280, height: 800 } }));

    const tabKey = (await sw.evaluate(async () => Object.keys(await chrome.storage.session.get(null)).find((k) => k.startsWith("tab:"))));
    const popup = await context.newPage();
    await popup.setViewportSize({ width: 360, height: 640 });
    await popup.goto(`${base}/popup.html?tabId=${tabKey.split(":")[1]}`);
    await popup.waitForFunction(() => Number(document.getElementById("count").textContent) > 0, null, { timeout: 5000 });
    const popupPng = await popup.screenshot({ fullPage: true });
    const comp = await context.newPage();
    await comp.setViewportSize({ width: 1280, height: 800 });
    await comp.setContent(caption("Choose how cautious it is", "Lockdown, Strict or Balanced. Turning protection off always needs a deliberate confirmation.", `data:image/png;base64,${popupPng.toString("base64")}`));
    writeFileSync(join(OUT, "02-popup-modes.png"), await comp.screenshot());

    const opts = await context.newPage();
    await opts.setViewportSize({ width: 1280, height: 800 });
    await opts.goto(`${base}/options.html`);
    await opts.waitForSelector("#status-title");
    writeFileSync(join(OUT, "03-settings-and-limits.png"), await opts.screenshot());
    console.log("Wrote 3 screenshots to store/screenshots");
  } finally {
    await context.close().catch(() => {});
    server.close();
    try { rmSync(profile, { recursive: true, force: true }); } catch { /* locked profile */ }
  }
}
main().catch((e) => { console.error(e.message); process.exit(1); });
