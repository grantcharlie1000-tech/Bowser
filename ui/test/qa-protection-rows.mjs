// Privacy dashboard window rows (headless Chromium via Playwright; skips without it). Every row of the
// Blocking, Site rules, Blocking lists, History and Clear data blocks keeps its label and description
// clear of its control and inside the row, at the window's standard (480 px) and compact (360 px)
// widths. The add forms used to cover their labels, list descriptions ran under their buttons, and the
// five-range Period picker covered "Period" and lost "Everything" off the edge.
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

for (const width of [480, 360]) {
  test(`privacy dashboard rows at ${width} px: no text under a control, nothing past the row`, { skip, timeout: 120000 }, async () => {
    const scenario = scenarios().find((s) => s.name === 'dialog-privacy-protection');
    const o = await openScenario(browser, server.origin, scenario, { size: [width, 560] });
    try {
      const problems = await o.page.evaluate(() => {
        const out = [];
        for (const row of document.querySelectorAll('.pg-row')) {
          if (row.getClientRects().length === 0) continue;
          const label = row.querySelector('.pg-label');
          const control = row.lastElementChild;
          if (!label || control === label.parentElement) continue;
          const box = control.getBoundingClientRect();
          for (const part of label.parentElement.children) {
            const clip = getComputedStyle(part).overflow === 'hidden' ? part.getBoundingClientRect().right : Infinity;
            const range = document.createRange();
            range.selectNodeContents(part);
            const hit = [...range.getClientRects()].some((r) => Math.min(r.right, clip) > box.left + 0.5 && r.top < box.bottom && r.bottom > box.top);
            if (hit) out.push(`${label.textContent}: "${part.textContent}" under the control`);
          }
          if (row.scrollWidth > row.clientWidth + 1) out.push(`${label.textContent}: overflows the row`);
        }
        return out;
      });
      assert.deepEqual(problems, []);
    } finally {
      await o.context.close();
    }
  });
}

test('the Period dropdown shows every range in full, inside the 480 px window', { skip, timeout: 120000 }, async () => {
  const scenario = scenarios().find((s) => s.name === 'dialog-privacy-protection');
  const o = await openScenario(browser, server.origin, scenario, { size: [480, 560] });
  try {
    const period = o.page.locator('[aria-label="Delete browsing data from"]');
    await period.scrollIntoViewIfNeeded();
    await period.click();
    const r = await o.page.evaluate(() => {
      const labels = [...document.querySelectorAll('[role="option"] .bw-menu-label')];
      const list = labels[0]?.closest('[role="listbox"]')?.getBoundingClientRect();
      return { names: labels.map((l) => l.textContent), cut: labels.filter((l) => l.scrollWidth > l.clientWidth + 1).map((l) => l.textContent), inside: list ? list.left >= 0 && list.right <= innerWidth : false };
    });
    assert.deepEqual(r.names, ['Past hour', 'Past day', 'Past week', 'Past 4 weeks', 'Everything']);
    assert.deepEqual(r.cut, [], 'no range is truncated');
    assert.ok(r.inside, 'the list stays inside the window');
  } finally {
    await o.context.close();
  }
});
