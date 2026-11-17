// The cover follows the page's own theme, not only the OS scheme (engine/shield.js themeOf, styles.js data-sfw-theme).
// Light page under a dark OS -> light cover; dark page under a light OS -> dark cover; a light card on a dark page -> light;
// no background at all but color-scheme: dark -> dark. Every case runs under the frame-exact recorder: 0 unsafe frames.
//
// usage: node test/e2e/theme.mjs   (headless chrome-headless-shell only, muted)
import assert from "node:assert/strict";
import { startServer } from "../fixtures/server.mjs";
import { launch, instrumentedContext, FrameRecorder } from "./lib.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const srv = await startServer();
const browser = await launch({ beginFrame: true });
let failed = 0;

async function probeCase(name, page, os, expect) {
  const ctx = await instrumentedContext(browser, { config: { mode: "strict" }, detection: "real", probe: true });
  const p = await ctx.newPage();
  await p.emulateMedia({ colorScheme: os });
  await p.goto("about:blank");
  const rec = new FrameRecorder(ctx, p);
  await rec.start();
  await p.goto(`http://example.test:${srv.portA}/pages/${page}`, { waitUntil: "load" });
  await p.waitForFunction(() => document.querySelector("sfw-shield"), null, { timeout: 8000 });
  await sleep(300);
  await rec.waitFrames(12, 3000);
  const got = await p.evaluate(() => {
    const host = document.querySelector("sfw-shield");
    return { theme: host.getAttribute("data-sfw-theme"), fg: getComputedStyle(host).getPropertyValue("--fg").trim(), hostCount: document.querySelectorAll("sfw-shield").length };
  });
  await rec.stop();
  const probe = await p.evaluate(() => { const q = window.__probe; return q.unsafe + q.leak + q.taskUnsafe + q.taskLeak; });
  await ctx.close();
  try {
    assert.equal(got.theme, expect.theme, `${name}: data-sfw-theme`);
    assert.equal(got.fg, expect.fg, `${name}: cover text colour ${JSON.stringify(got)}`);
    assert.equal(rec.unsafe, 0, `${name}: pixel-unsafe frames ${rec.unsafe} of ${rec.shots}`);
    assert.equal(probe, 0, `${name}: DOM probe unsafe`);
    console.log(`ok   ${name} theme=${got.theme} fg=${got.fg} frames=${rec.shots}`);
  } catch (e) { failed++; console.log(`FAIL ${name}: ${String(e.message).split("\n")[0]}`); }
}

const LIGHT_FG = "#1c1e29"; // --fg of the light cover (read from the overlay host: the cover itself is in a closed root)
const DARK_FG = "#f4f2fa";
await probeCase("light page under a dark OS -> light cover", "theme-light.html", "dark", { theme: "light", fg: LIGHT_FG });
await probeCase("dark page under a light OS -> dark cover", "theme-dark.html", "light", { theme: "dark", fg: DARK_FG });
await probeCase("light page under a light OS -> light cover", "theme-light.html", "light", { theme: "light", fg: LIGHT_FG });
await probeCase("dark page under a dark OS -> dark cover", "theme-dark.html", "dark", { theme: "dark", fg: DARK_FG });
await probeCase("light card on a dark page under a dark OS -> light cover", "theme-nested.html", "dark", { theme: "light", fg: LIGHT_FG });
await probeCase("color-scheme dark without a background under a light OS -> dark cover", "theme-scheme.html", "light", { theme: "dark", fg: DARK_FG });

await browser.close();
srv.close?.();
console.log(failed ? `\n${failed} theme case(s) FAILED` : "\nall theme cases passed");
process.exit(failed ? 1 : 0);
