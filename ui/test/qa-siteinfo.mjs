// Site information in a short work area (headless Chromium via Playwright; skips without it). Rust
// clamps the popup to the work area, so a taller page must scroll: its rows keep their height (no
// overlap) and the last control is reachable; the size it reports is still the content's own.
import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadChromium, openScenario, serveDist } from './qa-lib.mjs';
import { scenarios } from './fixtures/qa/scenarios.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(process.env.BOWSER_QA_DIST ?? join(here, '..', 'dist'));
const chromium = loadChromium();
const skip = !chromium ? 'Playwright Chromium is not available' : !existsSync(join(dist, 'app.js')) ? `${dist} is not built` : false;
let browser = null;
let server = null;
before(async () => { if (skip) return; browser = await chromium.launch(); server = await serveDist(dist); });
after(async () => { await browser?.close(); await server?.close(); });

test('site information scrolls inside a short work area: rows never overlap and Reset permissions is reachable', { skip, timeout: 120000 }, async () => {
  const scenario = scenarios().find((s) => s.name === 'siteinfo');
  const tall = await openScenario(browser, server.origin, scenario, { size: [1280, 900] });
  const natural = await tall.page.evaluate(() => document.querySelector('.bw-siteinfo').offsetHeight);
  await tall.context.close();
  const o = await openScenario(browser, server.origin, scenario, { size: [1280, 420] });
  try {
    const r = await o.page.evaluate(() => {
      const page = document.querySelector('.bw-siteinfo');
      const rows = [...page.children].filter((n) => n.getClientRects().length > 0).map((n) => n.getBoundingClientRect());
      let overlaps = 0;
      for (let i = 1; i < rows.length; i += 1) if (rows[i].top < rows[i - 1].bottom - 0.5) overlaps += 1;
      const reset = [...page.querySelectorAll('button')].find((b) => /Reset permissions/.test(b.textContent));
      page.scrollTop = page.scrollHeight;
      const box = reset.getBoundingClientRect();
      return { overlaps, scrollable: page.scrollHeight > page.clientHeight, visible: box.top >= 0 && box.bottom <= innerHeight, cap: page.offsetHeight };
    });
    assert.equal(r.overlaps, 0, 'rows keep their height');
    assert.ok(r.scrollable, 'the capped page scrolls');
    assert.ok(r.visible, 'scrolling reaches Reset permissions');
    assert.ok(r.cap <= 420, `capped to the window (${r.cap})`);
    const sizes = (await o.sent()).filter((m) => m.t === 'size');
    assert.equal(sizes.at(-1)?.h, natural, 'the reported height is the content height, not the cap');
  } finally {
    await o.context.close();
  }
});
