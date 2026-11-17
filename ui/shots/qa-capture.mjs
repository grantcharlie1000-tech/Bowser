// QA capture and audit of a BUILT chrome (agent F harness). Loads every surface of
// ui/test/fixtures/qa/scenarios.mjs from a dist folder in headless Chromium (Playwright), takes PNGs
// for light + dark, device scale 1 / 1.25 / 1.5 / 2, standard and compact sizes, runs the automated
// checks of ui/test/qa-lib.mjs, and writes <out>/index.html (contact sheet + findings) and
// <out>/audit.json. Headless Chromium on Linux is NOT WebView2 on Windows: no Segoe UI, no Mica or
// acrylic, no native popup windows or DPI scaling by Win32. Use it for before/after comparison of
// the page itself, not as native proof.
//
//   node ui/shots/qa-capture.mjs --dist ui/dist --out /path/to/qa/F/main [--filter regex]
//        [--quick] [--no-shots] [--no-audit] [--jobs 4]
//   --quick: scale 1 and 1.5 only. Run it again with another --dist (e.g. the integrated build) and
//   another --out to compare two builds; file names are identical between runs.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { auditPage, loadChromium, openScenario, serveDist } from '../test/qa-lib.mjs';
import { scenarios } from '../test/fixtures/qa/scenarios.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i >= 0 ? argv[i + 1] : fallback; };
const flag = (name) => argv.includes(`--${name}`);
const dist = resolve(opt('dist', join(here, '..', 'dist')));
const out = resolve(opt('out', join(here, 'qa-out')));
const only = opt('filter') ? new RegExp(opt('filter')) : null;
const jobs = Number(opt('jobs', 4));
const DPRS = flag('quick') ? [1, 1.5] : [1, 1.25, 1.5, 2];
const SCHEMES = ['light', 'dark'];
const SIZES = ['standard', 'compact'];

const chromium = loadChromium();
if (!chromium) { console.error('Playwright Chromium is not available'); process.exit(2); }
mkdirSync(join(out, 'png'), { recursive: true });
const all = scenarios().filter((s) => !only || only.test(s.name));
const browser = await chromium.launch();
const server = await serveDist(dist);
const started = Date.now();

/** Runs `tasks` (async fns) `jobs` at a time. */
async function pool(tasks) {
  let next = 0;
  const results = [];
  await Promise.all(Array.from({ length: jobs }, async () => {
    while (next < tasks.length) { const i = next++; results[i] = await tasks[i](); }
  }));
  return results;
}

// ---- screenshots ----
const shots = [];
if (!flag('no-shots')) {
  const tasks = [];
  for (const s of all) for (const size of SIZES) for (const scheme of SCHEMES) for (const dpr of DPRS) {
    tasks.push(async () => {
      const file = `png/${s.name}--${scheme}--${size}--${Math.round(dpr * 100)}.png`;
      try {
        const o = await openScenario(browser, server.origin, s, { scheme, dpr, size: s.size[size] });
        await o.page.screenshot({ path: join(out, file), animations: 'disabled', caret: 'hide' });
        const vp = o.page.viewportSize();
        await o.context.close();
        shots.push({ scenario: s.name, scheme, size, dpr, file, viewport: [vp.width, vp.height], requested: o.requestedSize, errors: o.errors });
      } catch (e) { shots.push({ scenario: s.name, scheme, size, dpr, file: null, error: String(e.message).split('\n')[0] }); }
    });
  }
  await pool(tasks);
  console.log(`shots: ${shots.filter((x) => x.file).length}/${tasks.length} in ${Math.round((Date.now() - started) / 1000)} s`);
}

// ---- audit ----
const audit = [];
if (!flag('no-audit')) {
  const tasks = [];
  for (const s of all) for (const size of SIZES) for (const scheme of SCHEMES) {
    tasks.push(async () => {
      const row = { scenario: s.name, owner: s.owner, surface: s.surface, scheme, size };
      try {
        const o = await openScenario(browser, server.origin, s, { scheme, dpr: 1, size: s.size[size] });
        const checks = ['names', 'ids', 'hidden', 'state', 'contrast', 'targets'];
        if (s.fit || s.surface === 'popup' || s.surface === 'panel' || s.surface === 'dialog') checks.push('clip');
        if (scheme === 'dark' && size === 'standard') checks.push('focus');
        Object.assign(row, await auditPage(o.page, checks), { requested: o.requestedSize, errors: o.errors });
        await o.context.close();
        if (scheme === 'dark' && size === 'standard') {
          const m = await openScenario(browser, server.origin, s, { scheme, dpr: 1, size: s.size[size], reducedMotion: 'reduce' });
          row.motion = (await auditPage(m.page, ['motion'])).motion;
          await m.context.close();
        }
      } catch (e) { row.error = String(e.message).split('\n')[0]; }
      audit.push(row);
    });
  }
  await pool(tasks);
  console.log(`audit: ${audit.length} runs in ${Math.round((Date.now() - started) / 1000)} s`);
}
await browser.close();
await server.close();

const order = new Map(all.map((s, i) => [s.name, i]));
const byScenario = (a, b) => order.get(a.scenario) - order.get(b.scenario);
shots.sort((a, b) => byScenario(a, b) || a.size.localeCompare(b.size) || a.scheme.localeCompare(b.scheme) || a.dpr - b.dpr);
audit.sort((a, b) => byScenario(a, b) || a.size.localeCompare(b.size) || a.scheme.localeCompare(b.scheme));
writeFileSync(join(out, 'audit.json'), JSON.stringify({ dist, created: new Date().toISOString(), engine: 'headless Chromium (Playwright), not WebView2', shots, audit }, null, 1));

// ---- contact sheet ----
const esc = (v) => String(v).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const counts = (r) => ({
  names: r.names?.fails.length ?? 0, ids: r.ids?.length ?? 0, hidden: r.hidden?.length ?? 0, state: r.state?.length ?? 0, contrast: r.contrast?.fails.length ?? 0,
  targets: r.targets?.fails.length ?? 0, clip: r.clip?.count ?? 0, focus: r.focus?.fails.length ?? 0, motion: r.motion?.length ?? 0,
});
const list = (items, fmt) => (items && items.length ? `<ul>${items.slice(0, 12).map((x) => `<li>${esc(fmt(x))}</li>`).join('')}${items.length > 12 ? `<li>... ${items.length - 12} more</li>` : ''}</ul>` : '');
let html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Bowser QA contact sheet</title><style>
:root{color-scheme:light dark;--bg:#f4f5f7;--fg:#15181d;--muted:#5b6370;--card:#fff;--line:#d9dde3;--bad:#b42318}
@media (prefers-color-scheme:dark){:root{--bg:#16181c;--fg:#e8eaee;--muted:#9aa3b0;--card:#1f2228;--line:#2e333b;--bad:#ff8a80}}
body{margin:0;padding:16px 24px;background:var(--bg);color:var(--fg);font:13px/1.45 system-ui,sans-serif}
h1{font-size:20px;margin:0 0 4px}h2{font-size:15px;margin:28px 0 6px}p.note{color:var(--muted);max-width:90ch}
.grid{display:flex;flex-wrap:wrap;gap:10px;align-items:flex-start}figure{margin:0;background:var(--card);border:1px solid var(--line);border-radius:8px;padding:6px;max-width:100%}
figure img{display:block;max-width:min(640px,100%);height:auto;image-rendering:auto;background:repeating-conic-gradient(#8882 0 25%,#0000 0 50%) 0 0/12px 12px}
figcaption{color:var(--muted);font-size:11px;margin-top:4px}table{border-collapse:collapse;margin:6px 0}td,th{border:1px solid var(--line);padding:3px 7px;text-align:left;vertical-align:top}
.bad{color:var(--bad);font-weight:600}details{margin:4px 0}ul{margin:2px 0 6px 18px;padding:0}nav a{margin-right:8px}
</style></head><body><h1>Bowser QA contact sheet</h1>
<p class="note">dist: ${esc(relative(process.cwd(), dist) || dist)} &middot; ${esc(new Date().toISOString())} &middot; headless Chromium via Playwright on Linux. This is NOT native WebView2/Win32: fonts fall back from Segoe UI, glass/Mica is not drawn (state.solid=true), native popup windows, DPI scaling and clipping regions are not exercised. Popup pages are shown at the size they asked for with <code>size</code>, clamped to the scenario's work area.</p><nav>`;
html += all.map((s) => `<a href="#${s.name}">${esc(s.name)}</a>`).join('') + '</nav>';
for (const s of all) {
  html += `<h2 id="${s.name}">${esc(s.name)} <small>(${esc(s.surface)}, area ${esc(s.owner)}, ?${esc(s.query)})</small></h2>`;
  const rows = audit.filter((r) => r.scenario === s.name);
  if (rows.length) {
    html += '<table><tr><th>run</th><th>names</th><th>dup ids</th><th>[hidden] shown</th><th>no checked radio</th><th>contrast</th><th>targets</th><th>clip</th><th>focus</th><th>motion</th></tr>';
    for (const r of rows) {
      const c = counts(r);
      html += `<tr><td>${esc(r.scheme)} ${esc(r.size)}${r.error ? ` <span class="bad">${esc(r.error)}</span>` : ''}</td>${Object.values(c).map((n) => `<td class="${n ? 'bad' : ''}">${n}</td>`).join('')}</tr>`;
    }
    html += '</table>';
    for (const r of rows) {
      const c = counts(r);
      if (!Object.values(c).some(Boolean)) continue;
      html += `<details><summary>${esc(r.scheme)} ${esc(r.size)} findings</summary>`
        + list(r.names?.fails, (f) => `no accessible name: ${f.role} ${f.selector}`)
        + list(r.ids, (f) => `duplicate id #${f.id} x${f.n}`)
        + list(r.state, (f) => `radiogroup with nothing checked: ${f.selector} "${f.name}" ${f.options}`)
        + list(r.hidden, (f) => `rendered despite [hidden]: ${f.selector} display:${f.display} ${f.w}x${f.h} "${f.name}"`)
        + list(r.contrast?.fails, (f) => `contrast ${f.ratio}:1 < ${f.need}: "${f.text}" ${f.selector} (${f.fg} on ${f.bg}${f.pseudo ? ' ' + f.pseudo : ''})`)
        + list(r.targets?.fails, (f) => `target ${f.w}x${f.h}: ${f.selector} "${f.name}" near ${f.near}`)
        + list(r.clip?.fails, (f) => `outside viewport ${f.viewport}: ${f.selector} at ${f.rect}`)
        + list(r.focus?.fails, (f) => `${f.why}: ${f.selector} "${f.name}"`)
        + list(r.motion, (f) => `reduced motion: ${f.kind} ${f.property} ${f.ms}ms on ${f.selector}`)
        + '</details>';
    }
  }
  html += '<div class="grid">';
  for (const sh of shots.filter((x) => x.scenario === s.name)) {
    html += sh.file
      ? `<figure><a href="${sh.file}"><img loading="lazy" src="${sh.file}" alt="${esc(`${s.name} ${sh.scheme} ${sh.size} ${sh.dpr}x`)}" style="width:${Math.min(640, sh.viewport[0])}px"></a><figcaption>${esc(sh.scheme)} &middot; ${esc(sh.size)} ${sh.viewport.join('x')} &middot; ${sh.dpr}x${sh.requested ? ` &middot; asked ${sh.requested.join('x')}` : ''}${sh.errors?.length ? ` <span class="bad">errors: ${esc(sh.errors.join('; '))}</span>` : ''}</figcaption></figure>`
      : `<figure><figcaption class="bad">${esc(sh.scheme)} ${esc(sh.size)} ${sh.dpr}x: ${esc(sh.error)}</figcaption></figure>`;
  }
  html += '</div>';
}
html += '</body></html>';
writeFileSync(join(out, 'index.html'), html);
console.log(`wrote ${join(out, 'index.html')}`);
