// Interaction latency probe for the HTML chrome PAGE (agent F QA harness). Headless Chromium via
// Playwright on Linux, NOT native WebView2: no compositor-to-screen time, no Win32 input queue, no
// WebView2 PostWebMessage transport, no GPU. It measures what the page's own script and layout add
// between an input event (its event.timeStamp) or a host message and the DOM change / next frame.
// Use the numbers to compare two builds on the same machine, never as user-visible latency.
//
//   node benchmarks/scripts/qa-latency.mjs [--dist ui/dist] [--runs 40] [--json out.json]
//
// Probes (median / p95 / max, ms):
//   tab-click-intent      pointerdown on an inactive tab -> `tab select` posted to the host
//   tab-switch-render     host `tabs` (new active) delivered -> DOM shows it (MutationObserver) -> next frame after that
//   omnibox-key-dom       keydown in the address field -> input value updated (input event)
//   omnibox-key-edit      keydown -> `edit` posted to the host (coalesced to one per frame by design)
//   menu-button-intent    pointerdown on the main menu button -> `open menu` posted
//   menu-open-render      host `menu` delivered -> rows in the DOM -> `size` posted -> next frame
//   settings-toggle-dom   pointerdown on a settings switch -> aria-checked flips
//   settings-toggle-save  pointerdown -> `set save` posted to the host
import { writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadChromium, openScenario, serveDist } from '../../ui/test/qa-lib.mjs';
import { scenarios, mainMenu, tabsPopulated } from '../../ui/test/fixtures/qa/scenarios.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i >= 0 ? argv[i + 1] : fallback; };
const dist = resolve(opt('dist', join(here, '..', '..', 'ui', 'dist')));
const RUNS = Number(opt('runs', 40));
const chromium = loadChromium();
if (!chromium) { console.error('Playwright Chromium is not available'); process.exit(2); }
const browser = await chromium.launch();
const server = await serveDist(dist);
const find = (name) => scenarios().find((s) => s.name === name);
const stats = (xs) => {
  const v = xs.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (!v.length) return { n: 0 };
  const q = (p) => v[Math.min(v.length - 1, Math.floor(p * v.length))];
  return { n: v.length, median: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), max: +v.at(-1).toFixed(2) };
};
const results = {};

// Records event.timeStamp of the first pointerdown/keydown (capture phase) of each run.
const ARM = () => {
  window.__qaT = {};
  for (const type of ['pointerdown', 'keydown', 'input']) {
    window.addEventListener(type, (e) => { window.__qaT[type] ??= type === 'input' ? performance.now() : e.timeStamp; }, { capture: true });
  }
};
const reset = (page) => page.evaluate(() => { window.__qaT = {}; window.__qaSent.length = 0; });
const sentAt = (page, pred) => page.waitForFunction(pred, null, { timeout: 3000 }).then(() => true, () => false);

// ---- tab strip ----
{
  const s = find('tabs-h-populated');
  const o = await openScenario(browser, server.origin, s, { size: s.size.standard });
  await o.page.evaluate(ARM);
  const intent = [], render = [], frame = [];
  for (let i = 0; i < RUNS; i++) {
    await reset(o.page);
    const target = i % 2 ? 7 : 2;
    await o.page.click(`[data-id="${target}"] .title, [data-id="${target}"]`, { timeout: 2000 }).catch(() => {});
    if (await sentAt(o.page, () => window.__qaSent.some((m) => m.t === 'tab' && m.a === 'select'))) {
      intent.push(await o.page.evaluate(() => window.__qaSent.find((m) => m.t === 'tab' && m.a === 'select').__at - window.__qaT.pointerdown));
    }
    // The host answers with the new active tab.
    const r = await o.page.evaluate(async ({ msg }) => {
      const el = document.querySelector(`[data-id="${msg.active}"]`);
      const shown = () => !!el && el.isConnected && el.matches('.active, [aria-selected="true"]');
      let applied = NaN;
      const t0 = performance.now();
      const mo = new MutationObserver(() => { if (Number.isNaN(applied) && shown()) applied = performance.now(); });
      mo.observe(document.getElementById('app'), { attributes: true, subtree: true, childList: true });
      window.__qaDeliver(msg);
      if (shown()) applied = performance.now();
      for (let f = 0; f < 10 && Number.isNaN(applied); f++) await new Promise((ok) => requestAnimationFrame(() => ok()));
      await Promise.resolve();
      mo.disconnect();
      await new Promise((ok) => requestAnimationFrame(() => ok()));
      return { apply: applied - t0, frame: performance.now() - t0 };
    }, { msg: { ...tabsPopulated, active: target } });
    render.push(r.apply); frame.push(r.frame);
  }
  results['tab-click-intent'] = stats(intent);
  results['tab-switch-render (apply)'] = stats(render);
  results['tab-switch-render (to next frame)'] = stats(frame);
  await o.context.close();
}

// ---- omnibox typing and menu button ----
{
  const s = find('toolbar-focused');
  const o = await openScenario(browser, server.origin, s, { size: s.size.standard });
  await o.page.evaluate(ARM);
  await o.page.evaluate(() => { const i = document.querySelector('input.omni'); i.focus(); i.select(); });
  const dom = [], edit = [];
  for (let i = 0; i < RUNS; i++) {
    await reset(o.page);
    await o.page.keyboard.press(String.fromCharCode(97 + (i % 26)));
    if (await sentAt(o.page, () => window.__qaSent.some((m) => m.t === 'edit'))) {
      const r = await o.page.evaluate(() => ({ dom: window.__qaT.input - window.__qaT.keydown, edit: window.__qaSent.find((m) => m.t === 'edit').__at - window.__qaT.keydown }));
      dom.push(r.dom); edit.push(r.edit);
    }
  }
  results['omnibox-key-dom'] = stats(dom);
  results['omnibox-key-edit'] = stats(edit);
  const menu = [];
  for (let i = 0; i < RUNS; i++) {
    await reset(o.page);
    await o.page.click('.tb-menu', { timeout: 2000 }).catch(() => {});
    if (await sentAt(o.page, () => window.__qaSent.some((m) => m.t === 'open' && m.a === 'menu'))) {
      menu.push(await o.page.evaluate(() => window.__qaSent.find((m) => m.t === 'open' && m.a === 'menu').__at - window.__qaT.pointerdown));
    }
  }
  results['menu-button-intent'] = stats(menu);
  await o.context.close();
}

// ---- menu popup render ----
{
  const s = find('menu-tab');
  const o = await openScenario(browser, server.origin, s, { size: s.size.standard, act: false });
  const rows = [], size = [], frame = [];
  for (let i = 0; i < RUNS; i++) {
    const r = await o.page.evaluate(async ({ msg }) => {
      window.__qaSent.length = 0;
      const t0 = performance.now();
      window.__qaDeliver(msg);
      const n = document.querySelectorAll('[role=menuitem], [role=menuitemcheckbox]').length;
      const dom = n > 0 ? performance.now() - t0 : NaN;
      await new Promise((ok) => requestAnimationFrame(() => ok()));
      const f = performance.now() - t0;
      const sz = window.__qaSent.find((m) => m.t === 'size');
      return { dom, frame: f, size: sz ? sz.__at - t0 : NaN };
    }, { msg: { ...mainMenu, tok: 100 + i } });
    rows.push(r.dom); size.push(r.size); frame.push(r.frame);
  }
  results['menu-open-render (rows in DOM)'] = stats(rows);
  results['menu-open-render (size posted)'] = stats(size);
  results['menu-open-render (to next frame)'] = stats(frame);
  await o.context.close();
}

// ---- settings switch ----
{
  const s = find('settings-appearance');
  const o = await openScenario(browser, server.origin, s, { size: s.size.standard });
  await o.page.evaluate(ARM);
  const dom = [], save = [];
  const sw = await o.page.$('[role=switch]:not([aria-disabled="true"]):not(:disabled)');
  for (let i = 0; sw && i < RUNS; i++) {
    await reset(o.page);
    const before = await sw.getAttribute('aria-checked');
    await o.page.evaluate((el) => {
      const mo = new MutationObserver(() => { window.__qaT.flip ??= performance.now(); mo.disconnect(); });
      mo.observe(el, { attributes: true, attributeFilter: ['aria-checked'] });
    }, sw);
    await sw.click({ timeout: 2000 }).catch(() => {});
    const flipped = (await sw.getAttribute('aria-checked')) !== before;
    if (flipped) dom.push(await o.page.evaluate(() => window.__qaT.flip - window.__qaT.pointerdown));
    if (await sentAt(o.page, () => window.__qaSent.some((m) => m.t === 'set'))) {
      save.push(await o.page.evaluate(() => window.__qaSent.find((m) => m.t === 'set').__at - window.__qaT.pointerdown));
    }
  }
  results['settings-toggle-dom'] = stats(dom);
  results['settings-toggle-save'] = stats(save);
  await o.context.close();
}

await browser.close();
await server.close();
const report = { engine: 'headless Chromium (Playwright), Linux; NOT native WebView2', dist, runs: RUNS, created: new Date().toISOString(), results };
console.log(JSON.stringify(report, null, 1));
if (opt('json')) writeFileSync(resolve(opt('json')), JSON.stringify(report, null, 1));
