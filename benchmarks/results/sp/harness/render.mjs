// Renders the start page and the settings page's Start page section from ui/dist in headless Edge
// (Chrome DevTools Protocol, no window on the desktop), before (origin/main's dist) and after, light and dark,
// at 100% and 150%, and measures the page's first paint.
//   node render.mjs <outDir> <beforeDistDir> [nameFilterRegex]
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../../../..');
const afterDist = join(root, 'ui', 'dist');
const out = resolve(process.argv[2] ?? join(here, '..'));
const beforeDist = resolve(process.argv[3] ?? '');
const only = process.argv[4];
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const read = (p) => readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
mkdirSync(out, { recursive: true });

const startFx = JSON.parse(read(join(root, 'ui', 'test', 'fixtures', 'start-page-messages.json')));
const settingsAfter = JSON.parse(read(join(root, 'ui', 'test', 'fixtures', 'settings-messages.json')));
const settingsBefore = existsSync(join(beforeDist, '..', 'test', 'fixtures', 'settings-messages.json'))
  ? JSON.parse(read(join(beforeDist, '..', 'test', 'fixtures', 'settings-messages.json'))) : null;

// ---- scenarios: the messages the host would send ----
const withSettings = (m, settings, extra = {}) => ({ ...m, d: { ...m.d, settings: { ...m.d.settings, ...settings }, ...extra } });
const thumbsFn = (ids) => ({ __thumbs: ids });
const withImages = (m, ids) => ({ ...m, d: { ...m.d, dials: m.d.dials.map((d) => (ids.includes(d.id) ? { ...d, thumb: true } : d)) } });
const manyDials = (m, n) => ({
  ...m,
  d: {
    ...m.d,
    groups: [{ ...m.d.groups[0], n }, m.d.groups[1]],
    dials: Array.from({ length: n }, (_, i) => ({ id: 100 + i, title: `Site number ${i + 1}`, kind: 'link', url: `https://site${i}.example/`, host: `site${i}.example`, n: 0, thumb: false })),
  },
});
const SCN = {
  start: [fixtureState(), startFx.page],
  'start-nowidgets': [fixtureState(), withSettings(startFx.page, { widgets: false }, { widgets: [] })],
  'start-empty': [fixtureState(), startFx.empty],
  'start-images': [fixtureState(), withImages(startFx.page, [3, 4]), thumbsFn([3, 4])],
  'start-small': [fixtureState(), withSettings(manyDials(startFx.page, 14), { thumbSize: 1, columns: 8, widgets: false }, { widgets: [] })],
  'start-large': [fixtureState(), withSettings(withImages(startFx.page, [3, 4]), { thumbSize: 5, columns: 3, titles: 'auto', widgets: false }, { widgets: [] }), thumbsFn([3, 4])],
  'start-custom': [fixtureState(), withSettings(startFx.page, { widgetBg: 'custom', widgetColor: '#336699', widgetHeight: 'tall', widgetColumns: 3 })],
  'start-solid': [fixtureState(), withSettings(startFx.page, { widgetBg: 'solid', widgetColumns: 4 })],
  'start-folder': [fixtureState(), { ...startFx.page, d: { ...startFx.page.d, folder: 5, path: [{ id: 1, title: 'Work' }, { id: 5, title: 'Reading' }], dials: startFx.page.d.dials.slice(0, 2) } }],
  'start-stress': [fixtureState(), manyDials(withSettings(startFx.page, { widgets: true }), 120)],
  'start-guest': [fixtureState(), { ...startFx.empty, d: { ...startFx.empty.d, guest: true } }],
  'settings-top': [fixtureState(), 'SETTINGS_SNAP'],
  'settings-start': [fixtureState(), 'SETTINGS_SNAP'],
  newtab: [fixtureState()],
};
function fixtureState() { return { t: 'state', scale: 1, solid: true, dark: true, back: true, forward: false, loading: false, progress: 0, security: 'secure', label: 'Connection is secure', theme: {} }; }

const shots = [];
const add = (s) => shots.push({ w: 1280, h: 800, ...s });
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '' : '@150';
    add({ site: 'before', name: `before-newtab-${scheme}${d}`, scn: 'newtab', scheme, dpr });
    for (const scn of ['start', 'start-nowidgets', 'start-empty', 'start-images', 'start-small', 'start-large', 'start-custom', 'start-solid', 'start-folder', 'start-guest']) {
      add({ site: 'after', name: `after-${scn}-${scheme}${d}`, scn, scheme, dpr });
    }
    add({ site: 'after', name: `after-start-dialog-${scheme}${d}`, scn: 'start', scheme, dpr, act: 'click:.sp-add:not(.sp-add-folder) .sp-open' });
    add({ site: 'after', name: `after-start-menu-${scheme}${d}`, scn: 'start', scheme, dpr, act: 'click:.sp-group-more' });
    add({ site: 'after', name: `after-start-config-${scheme}${d}`, scn: 'start', scheme, dpr, act: 'click:.sp-widget[data-kind="bookmarks"] .sp-widget-tool[aria-pressed="false"]' });
    if (settingsBefore) add({ site: 'before', name: `before-settings-top-${scheme}${d}`, scn: 'settings-top', scheme, dpr, w: 1100, h: 760, view: 'settings' });
    add({ site: 'after', name: `after-settings-top-${scheme}${d}`, scn: 'settings-top', scheme, dpr, w: 1100, h: 760, view: 'settings' });
    add({ site: 'after', name: `after-settings-startpage-${scheme}${d}`, scn: 'settings-start', scheme, dpr, w: 1100, h: 900, view: 'settings', act: 'click:.sp-nav-item[data-section="startpage"]|wait:700' });
  }
}

// ---- a static server for both sites ----
function writeSite(name, dist, snapshot) {
  const dir = join(out, `_site-${name}`);
  mkdirSync(dir, { recursive: true });
  for (const f of ['app.js', 'app.css', 'start-page.js', 'settings-page.js', 'history-page.js']) if (existsSync(join(dist, f))) copyFileSync(join(dist, f), join(dir, f));
  copyFileSync(join(here, 'host.js'), join(dir, 'host.js'));
  writeFileSync(join(dir, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}body{margin:0}\n');
  const scn = {};
  for (const [key, list] of Object.entries(SCN)) scn[key] = list.map((m) => (m === 'SETTINGS_SNAP' ? snapshot.snap : m));
  // The webpage widget shows a local page here (the fixture's address is a placeholder, and this runs offline).
  const source = `window.__scn = ${JSON.stringify(scn).replaceAll('https://widget.example/clock', `http://127.0.0.1:${port}/${name}/widget.html`)};
`;
  writeFileSync(join(dir, 'widget.html'), '<!doctype html><meta charset="utf-8"><body style="margin:0;font:600 30px Segoe UI,sans-serif;background:#0e3b43;color:#fff;display:grid;place-items:center;height:100vh">12:34</body>');
  writeFileSync(join(dir, 'scenarios.js'), source);
  const html = read(join(dist, 'index.html'))
    .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="app.css">\n<link rel="stylesheet" href="harness.css">')
    .replace('<script src="app.js" defer></script>', '<script src="scenarios.js"></script>\n<script src="host.js"></script>\n<script src="act.js"></script>\n<script src="app.js" defer></script>');
  writeFileSync(join(dir, 'index.html'), html);
  copyFileSync(join(here, 'act.js'), join(dir, 'act.js'));
  return dir;
}
const MIME = { '.html': 'text/html;charset=utf-8', '.js': 'text/javascript;charset=utf-8', '.css': 'text/css;charset=utf-8' };
const sites = {};
const server = createServer((req, res) => {
  const [, site, ...rest] = req.url.split('?')[0].split('/');
  const file = join(sites[site] ?? '', rest.join('/') || 'index.html');
  if (!sites[site] || !existsSync(file)) { res.statusCode = 404; res.end(); return; }
  res.setHeader('Content-Type', MIME[extname(file)] ?? 'application/octet-stream');
  res.setHeader('Cache-Control', 'no-store');
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;
sites.after = writeSite('after', afterDist, settingsAfter);
if (existsSync(join(beforeDist, 'app.js'))) sites.before = writeSite('before', beforeDist, settingsBefore ?? settingsAfter);

// ---- a tiny CDP client ----
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function launch() {
  const profile = mkdtempSync(join(tmpdir(), 'sp-edge-'));
  const debugPort = 9333 + Math.floor(Math.random() * 400);
  const child = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--remote-debugging-port=${debugPort}`, `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore' });
  let endpoint = '';
  for (let i = 0; i < 100 && !endpoint; i++) {
    await sleep(100);
    try { endpoint = (await (await fetch(`http://127.0.0.1:${debugPort}/json/version`)).json()).webSocketDebuggerUrl; } catch { /* not up yet */ }
  }
  if (!endpoint) {
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    throw new Error('Edge did not start');
  }
  const browser = new WebSocket(endpoint);
  await new Promise((r) => browser.addEventListener('open', r));
  let id = 0;
  const pending = new Map();
  const handlers = [];
  browser.addEventListener('message', (e) => {
    const msg = JSON.parse(e.data);
    if (msg.id && pending.has(msg.id)) { const { resolve: ok, reject } = pending.get(msg.id); pending.delete(msg.id); msg.error ? reject(new Error(JSON.stringify(msg.error))) : ok(msg.result); }
    else handlers.forEach((h) => h(msg));
  });
  const call = (method, params = {}, sessionId) => new Promise((resolve2, reject) => {
    const n = ++id;
    pending.set(n, { resolve: resolve2, reject });
    browser.send(JSON.stringify({ id: n, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  return { call, handlers, close: () => { browser.close(); spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); try { rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch { /* temp */ } } };
}

const edgeProc = await launch();
async function page(w, h, dpr) {
  const { targetId } = await edgeProc.call('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await edgeProc.call('Target.attachToTarget', { targetId, flatten: true });
  const c = (m, p) => edgeProc.call(m, p, sessionId);
  await c('Page.enable');
  await c('Runtime.enable');
  await c('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: dpr, mobile: false });
  return { c, close: () => edgeProc.call('Target.closeTarget', { targetId }) };
}
async function run(s) {
  const p = await page(s.w, s.h, s.dpr);
  const view = s.view ?? 'split';
  const url = `http://127.0.0.1:${port}/${s.site}/index.html?view=${view}&scn=${s.scn}&scheme=${s.scheme}`;
  await p.c('Page.navigate', { url });
  // Wait for the first paint after the messages (the host records it), then for the scripted steps.
  for (let i = 0; i < 100; i++) {
    const { result } = await p.c('Runtime.evaluate', { expression: 'JSON.stringify(window.__marks||null)', returnByValue: true });
    if (result.value && result.value !== 'null') break;
    await sleep(50);
  }
  await sleep(450);
  if (s.act) {
    await p.c('Runtime.evaluate', { expression: `window.__runAct(${JSON.stringify(s.act)})`, awaitPromise: true });
    await sleep(500);
  }
  const shot = await p.c('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(out, `${s.name}.png`), Buffer.from(shot.data, 'base64'));
  await p.close();
}
const todo = shots.filter((s) => !only || new RegExp(only).test(s.name));
for (const s of todo) {
  try { await run(s); console.log('ok', s.name); } catch (e) { console.error('FAILED', s.name, e.message); }
}

// ---- timing: from the host's message to the first paint that shows the page ----
async function timing(scn, label, runs) {
  const built = [];
  const warm = [];
  const imports = [];
  for (let i = 0; i < runs; i++) {
    const p = await page(1280, 800, 1);
    await p.c('Page.navigate', { url: `http://127.0.0.1:${port}/after/index.html?view=split&scn=${scn}&scheme=dark` });
    for (let n = 0; n < 200; n++) {
      const { result } = await p.c('Runtime.evaluate', { expression: 'JSON.stringify(window.__marks||null)', returnByValue: true });
      if (result.value && result.value !== 'null') { const m = JSON.parse(result.value); built.push(m.built - m.delivered); break; }
      await sleep(25);
    }
    if (scn !== 'newtab') {
      const r2 = await p.c('Runtime.evaluate', { expression: 'window.__redeliver()', awaitPromise: true, returnByValue: true });
      warm.push(r2.result.value);
    }
    const nav = await p.c('Runtime.evaluate', { expression: 'JSON.stringify(performance.getEntriesByType("resource").filter(r=>r.name.endsWith("start-page.js")).map(r=>r.duration))', returnByValue: true });
    imports.push(JSON.parse(nav.result.value)[0] ?? 0);
    await p.close();
  }
  const q = (arr, f) => { const a = [...arr].sort((x, y) => x - y); return a[Math.min(a.length - 1, Math.floor(a.length * f))].toFixed(1); };
  const second = warm.length ? `; second blank tab in the same window (code already loaded) median ${q(warm, 0.5)} ms, p90 ${q(warm, 0.9)}` : '';
  return `${label}: first page built median ${q(built, 0.5)} ms (p90 ${q(built, 0.9)}, max ${q(built, 1)}) from the host's message, of which start-page.js fetch ${q(imports, 0.5)} ms${second}; ${runs} cold page loads`;
}
if (!only || /timing/.test(only)) {
  const lines = [];
  for (const [scn, label] of [['newtab', 'before: blank new tab (nothing to draw)'], ['start-empty', 'after: start page, empty profile'], ['start', 'after: start page, 3 dials + 6 widgets'], ['start-stress', 'after: start page, 120 dials + 6 widgets']]) {
    lines.push(await timing(scn, label, 20));
    console.log(lines.at(-1));
  }
  writeFileSync(join(out, 'timing.txt'), `${lines.join('\n')}\n`);
}
edgeProc.close();
server.close();
console.log(`done: ${todo.length} screenshots in ${out}`);
