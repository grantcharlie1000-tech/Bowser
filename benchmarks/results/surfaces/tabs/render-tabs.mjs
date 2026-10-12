// Renders the horizontal tab strip from ui/dist in headless Edge for many scenarios.
//   node render-tabs.mjs <outDir> [nameFilterRegex]
// Reuses benchmarks/results/foundation/harness (fake host, backdrop); only the tab scenarios and
// a favicon are added here.
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const harness = join(here, '..', '..', 'foundation', 'harness');
const dist = resolve(here, '..', '..', '..', '..', 'ui', 'dist');
const out = resolve(process.argv[2] ?? here);
const only = process.argv[3];
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(harness, 'harness.css'), join(site, 'harness.css'));

// Extra scenarios injected into the shared fake host.
const extra = `
window.__favPng = (id) => {
  const c = document.createElement('canvas'); c.width = 32; c.height = 32;
  const g = c.getContext('2d');
  g.fillStyle = ['#e5484d', '#3e63dd', '#30a46c', '#f5a524', '#8e4ec6', '#0090ff'][id % 6];
  g.beginPath(); g.roundRect(2, 2, 28, 28, 7); g.fill();
  g.fillStyle = '#fff'; g.font = 'bold 18px sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
  g.fillText(String.fromCharCode(65 + (id % 26)), 16, 17);
  return c.toDataURL('image/png');
};
window.__EXTRA = (() => {
  const T = (id, title, o = {}) => ({ id, title, loading: false, pinned: false, muted: false, marker: -1, g: 0, gl: false, fav: false, ...o });
  const titles = ['Inbox (12) - Mail', 'GitHub - bowser/bowser', 'Stack Overflow - Where developers learn', 'YouTube', 'Wikipedia, the free encyclopedia',
    'Docs - Quarterly planning', 'Hacker News', 'Reddit - Dive into anything', 'Figma', 'Weather in Reykjavik', 'Calendar - October 2026', 'New tab'];
  const many = (n, active) => ({ v: 1, t: 'tabs', active, items: Array.from({ length: n }, (_, i) => T(i + 1, titles[i % titles.length] + (i >= titles.length ? ' ' + (i + 1) : ''), { fav: i % 3 !== 2 })) });
  const sc = {};
  sc.t1 = many(1, 1);
  sc.t2 = many(2, 2);
  sc.t5 = many(5, 2);
  sc.t25 = many(25, 7);
  sc.groups = { v: 1, t: 'tabs', active: 3, items: [
    T(1, 'Inbox (12) - Mail', { fav: true }),
    T(2, 'Design refs', { g: 1, gl: true, gname: 'Research', gcol: 5, gcollapsed: false, fav: true }),
    T(3, 'Arc browser', { g: 1, fav: true }),
    T(4, 'Zen browser', { g: 1 }),
    T(5, 'Shopping', { g: 2, gl: true, gname: 'Errands', gcol: 1, gcollapsed: true }),
    T(6, 'Cart - Store', { g: 2 }),
    T(7, 'Hacker News', { g: 3, gl: true, gname: '', gcol: 3, gcollapsed: false, fav: true }),
    T(8, 'Reddit', { g: 3 }),
    T(9, 'Plain tab'),
  ] };
  sc.groups25 = { v: 1, t: 'tabs', active: 3, items: [
    ...many(8, 3).items,
    T(101, 'Design refs', { g: 1, gl: true, gname: 'Research', gcol: 5, gcollapsed: false, fav: true }),
    T(102, 'Arc browser', { g: 1, fav: true }), T(103, 'Zen browser', { g: 1 }), T(104, 'Vivaldi', { g: 1 }), T(105, 'Orion', { g: 1 }),
    T(106, 'Plain tab'),
  ] };
  sc.mixed = { v: 1, t: 'tabs', active: 4, items: [
    T(1, 'Pinned mail', { pinned: true, fav: true }), T(2, 'Pinned chat', { pinned: true }), T(3, 'Pinned music', { pinned: true, muted: true, fav: true }),
    T(4, 'Active tab with a rather long title that fades out', { fav: true }),
    T(5, 'Loading page...', { loading: true }),
    T(6, 'Muted video', { muted: true, marker: 3, fav: true }),
    T(7, 'Marked tab', { marker: 5 }),
    T(8, 'Plain tab'),
  ] };
  sc.pinnedscroll = { v: 1, t: 'tabs', active: 20, items: [
    T(1, 'Pinned mail', { pinned: true, fav: true }), T(2, 'Pinned chat', { pinned: true }),
    ...many(24, 20).items.map((x) => ({ ...x, id: x.id + 10 })),
  ] };
  for (const m of Object.values(sc)) for (const it of m.items) if (it.gl) it.gsize = m.items.filter((x) => x.g === it.g).length;
  return sc;
})();
`;
const fake = readFileSync(join(harness, 'fake-host.js'), 'utf8')
  .replace('const scenarios = {', 'const scenarios = { ...window.__EXTRA,')
  // 'scn' for tab scenarios needs the state message first.
  .replace('const listeners = [];', `Object.keys(window.__EXTRA).forEach((k) => { scenarios[k] = [state(), window.__EXTRA[k], ...window.__EXTRA[k].items.filter((x) => x.fav).map((x) => ({ v: 1, t: 'fav', id: x.id, uri: window.__favPng(x.id) }))]; });\n  const listeners = [];`);
writeFileSync(join(site, 'fake-host.js'), extra + fake);
// A tiny inline favicon so real favicons render (two colours, 16px).
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const scn of ['t1', 't2', 't5', 't25', 'groups', 'groups25', 'mixed', 'pinnedscroll']) {
    for (const w of [500, 900, 1400]) shots.push({ name: `${scn}-${w}-${scheme}`, scn, w, scheme });
  }
  for (const dpr of [1.5, 2]) {
    for (const scn of ['mixed', 'groups']) shots.push({ name: `${scn}-900-${scheme}@${dpr}x`, scn, w: 900, scheme, dpr });
  }
}
const base = pathToFileURL(join(site, 'index.html')).href;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Headless Edge writes the PNG and then can linger; poll for the file, then kill that process tree.
async function shoot(s, worker) {
  const url = `${base}?view=tabs&scn=${s.scn}&scheme=${s.scheme}&w=${s.w}`;
  const file = join(out, `tabs-${s.name}.png`);
  rmSync(file, { force: true });
  const child = spawn(edge, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${join(out, `_tabs-profile-${worker}-${Date.now()}`)}`, `--force-device-scale-factor=${s.dpr ?? 1}`,
    `--window-size=${s.w},34`, '--virtual-time-budget=1500', `--screenshot=${file}`, url,
  ], { stdio: 'ignore' });
  let exited = false;
  child.on('exit', () => { exited = true; });
  for (let i = 0; i < 1500 && !exited; i++) {
    await sleep(100);
    if (existsSync(file) && statSync(file).size > 0) { await sleep(200); break; }
  }
  if (!exited) spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  if (!existsSync(file)) console.error('no screenshot for', s.name);
}

const queue = [...todo];
const workers = Array.from({ length: 4 }, async (_, w) => {
  for (let s = queue.shift(); s; s = queue.shift()) await shoot(s, w);
});
await Promise.all(workers);
for (let w = 0; w < 4; w++) { try { rmSync(join(out, `_tabs-profile-${w}`), { recursive: true, force: true }); } catch { /* still locked */ } }
console.log(`rendered ${todo.length} shots to ${out}`);



