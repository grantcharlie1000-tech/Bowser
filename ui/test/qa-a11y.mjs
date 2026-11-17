// Automated accessibility and motion checks of every HTML chrome surface (agent F QA harness), on the
// BUILT chrome (ui/dist, or BOWSER_QA_DIST), in headless Chromium through Playwright. Skips cleanly when
// Playwright or its browser is missing (e.g. a Windows runner without it). Not WebView2: computed
// styles, layout, the accessibility tree and keyboard focus are Chromium's own, but fonts (no Segoe UI),
// window glass and native popup placement differ.
//
// Checks per surface (fixtures/qa/scenarios.mjs):
//   names     every interactive role in Chromium's accessibility tree has a non-empty name
//   ids       no duplicate ids
//   hidden    nothing with the `hidden` attribute still renders
//   state     every radio group on screen shows its current value (one radio checked)
//   focus     each keyboard Tab stop changes its look (outline, shadow, border, fill, pseudo, parent)
//             and is on screen
//   contrast  text >= 4.5:1 (3:1 large) against its composited background, light and dark; when the
//             window material shows through, over an approximation of Mica (flagged)
//   targets   WCAG 2.2 2.5.8: 24x24 CSS px or the spacing exception
//   clip      popups at the size they asked for, and at a short work area: nothing unreachable
//   motion    under prefers-reduced-motion: no transition/animation over 83 ms except opacity
//
// Known defects are listed in KNOWN with a defect id: those findings run as `todo` subtests (reported,
// not failing) so the file can enter CI now and tightens as fixes land. Anything else fails.
import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { existsSync } from 'node:fs';
import { auditPage, loadChromium, openScenario, serveDist } from './qa-lib.mjs';
import { scenarios } from './fixtures/qa/scenarios.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(process.env.BOWSER_QA_DIST ?? join(here, '..', 'dist'));
const chromium = loadChromium();
const skip = !chromium ? 'Playwright Chromium is not available' : !existsSync(join(dist, 'app.js')) ? `${dist} is not built` : false;

/**
 * Known defects (docs: the agent F QA report). `scenario` and `match` are regexes over the scenario
 * name and the finding's text (selector, role, property ...). Remove an entry once its fix lands.
 */
const KNOWN = [
  { id: 'F-1', check: 'hidden', scenario: /^toolbar/, match: /\.tb-(hist|qr)\b/, note: 'typed-history and QR buttons render while hidden (.bw-icon-btn display beats [hidden]; address.css has no .tb-hist/.tb-qr[hidden] rule)' },
  { id: 'F-1', check: 'targets', scenario: /^toolbar-(empty|focused)/, match: /\.tb-(hist|qr)\b/, note: 'follows from F-1: buttons that should be hidden are measured' },
  { id: 'F-2', check: 'names', scenario: /^tabs-v-populated$/, match: /side-pinned.*side-tab/, note: 'pinned tile in the vertical sidebar has no accessible name (sidebar.ts removes aria-label; .side-title is not rendered on a tile)' },
  { id: 'F-3', check: 'focus', scenario: /^(history|downloads)-page/, match: /(hp|dp)-toast-undo/, note: 'toast Undo button is a Tab stop while the toast is invisible (not hidden until the first toast)' },
  { id: 'F-4', check: 'clip', scenario: /^siteinfo/, match: /bw-siteinfo/, note: 'site information is taller than a short work area and nothing scrolls: bottom rows unreachable once Rust clamps the popup' },
  { id: 'F-6', check: 'contrast', scenario: /^(panel-(history|bookmarks)|bookmark-bar)$/, match: /pn-favicon/, note: 'letter favicon tile in light scheme: white letter on a pale tile (~1.75:1)' },
  { id: 'F-7', check: 'contrast', scenario: /^store-/, match: /bs-nav > button/, note: 'store side navigation, selected item, light scheme 3.88:1' },
  { id: 'F-8', check: 'targets', scenario: /^toolbar/, match: /\.pill > button\.bw-icon-btn\.tb-(hist|qr|reader|translate|split)\b/, note: 'adjacent 20x20 pill buttons, under 24x24 without the spacing exception (WCAG 2.5.8)' },
  { id: 'F-9', check: 'focus', scenario: /^toolbar/, match: /omni-search-input|input\.omni\b/, note: 'address and search fields: only the caret marks keyboard focus (the pill lift is shared with every pill control)' },
  { id: 'F-10', check: 'clip', scenario: /^palette-populated$/, match: /bw-palette-row/, note: 'palette rows past a short work area are unreachable (no scroller inside the clamped popup)' },
  { id: 'F-11', check: 'targets', scenario: /^(settings-|store-)/, match: /sp-ext-toggle-details/, note: '"What it declares" is 20 px tall next to the row actions' },
  { id: 'F-13', check: 'state', scenario: /^dialog-privacy/, match: /Blocking level|Website appearance|Private tab theme/, note: 'privacy dashboard window drops the protection snapshot (dialog.ts passes only p === token): blocking level, appearance and private theme show no value' },
  { id: 'F-14', check: 'state', scenario: /^(settings-(?!guest)|store-)/, match: /"Presets"/, note: 'layout Presets radiogroup has nothing checked for the Rust fixture layout (matches no preset and Saved is empty)' },
  { id: 'F-12', check: 'contrast', scenario: /^split-picker$/, match: /bw-row-meta/, note: 'split picker host text 4.09:1 in light over the window material (assumed backdrop)' },
];

const finding = {
  names: (f) => `${f.role} ${f.selector}`,
  ids: (f) => `#${f.id} x${f.n}`,
  state: (f) => `radiogroup with nothing checked: ${f.selector} "${f.name}" ${f.options}`,
  hidden: (f) => `${f.selector} display:${f.display} ${f.w}x${f.h} "${f.name}"`,
  focus: (f) => `${f.why}: ${f.selector}`,
  contrast: (f) => `${f.ratio}:1<${f.need} ${f.selector}${f.pseudo ?? ''} "${f.text}"${f.assumedBackdrop ? ' (assumed backdrop)' : ''}`,
  targets: (f) => `${f.w}x${f.h} ${f.selector} near ${f.near}`,
  clip: (f) => `outside ${f.viewport}: ${f.selector} ${f.rect}`,
  motion: (f) => `${f.kind} ${f.property} ${f.ms}ms ${f.selector}`,
};
const failsOf = (check, result) => {
  if (!result) return [];
  if (check === 'ids' || check === 'motion' || check === 'hidden' || check === 'state') return result;
  return result.fails ?? [];
};

/** Splits findings into known (by defect id) and new, and asserts on the new ones only. */
async function judge(t, scenario, label, check, result) {
  const lines = failsOf(check, result).map(finding[check]);
  const known = new Map();
  const fresh = [];
  for (const line of lines) {
    const k = KNOWN.find((d) => d.check === check && d.scenario.test(scenario.name) && d.match.test(line));
    if (k) known.set(k.id, [...(known.get(k.id) ?? []), line]);
    else fresh.push(line);
  }
  await t.test(`${check} (${label})`, () => { assert.deepEqual(fresh, [], `${scenario.name} ${label}: ${check} findings`); });
  for (const [id, items] of known) {
    const d = KNOWN.find((x) => x.id === id);
    await t.test(`${check} (${label}) known ${id}: ${d.note}`, { todo: `${id} ${d.note}` }, () => { assert.deepEqual(items, []); });
  }
}

let browser = null;
let server = null;
before(async () => { if (skip) return; browser = await chromium.launch(); server = await serveDist(dist); });
after(async () => { await browser?.close(); await server?.close(); });

for (const s of scenarios()) {
  test(`${s.name} (${s.surface}, ?${s.query})`, { skip, timeout: 120000 }, async (t) => {
    const popupLike = s.fit || ['popup', 'panel', 'dialog'].includes(s.surface);
    // Dark, standard size: everything but motion.
    {
      const o = await openScenario(browser, server.origin, s, { scheme: 'dark', size: s.size.standard });
      try {
        assert.deepEqual(o.errors, [], 'no script errors while the scenario loads');
        const checks = ['names', 'ids', 'hidden', 'state', 'contrast', 'targets', ...(popupLike ? ['clip'] : []), 'focus'];
        const r = await auditPage(o.page, checks);
        for (const check of checks) await judge(t, s, 'dark', check, r[check]);
      } finally { await o.context.close(); }
    }
    // Light: contrast only (the rest does not depend on the scheme).
    {
      const o = await openScenario(browser, server.origin, s, { scheme: 'light', size: s.size.standard });
      try { await judge(t, s, 'light', 'contrast', (await auditPage(o.page, ['contrast'])).contrast); } finally { await o.context.close(); }
    }
    // Compact size: popups and menus in a short work area; bars in a narrow window.
    {
      const o = await openScenario(browser, server.origin, s, { scheme: 'dark', size: s.size.compact });
      try {
        const checks = popupLike ? ['clip', 'targets'] : ['targets'];
        const r = await auditPage(o.page, checks);
        for (const check of checks) await judge(t, s, 'compact', check, r[check]);
      } finally { await o.context.close(); }
    }
    // Reduced motion.
    {
      const o = await openScenario(browser, server.origin, s, { scheme: 'dark', size: s.size.standard, reducedMotion: 'reduce' });
      try { await judge(t, s, 'reduced motion', 'motion', (await auditPage(o.page, ['motion'])).motion); } finally { await o.context.close(); }
    }
  });
}
