// Renders panels, customize (incl. select states), dialogs and sidebar focus from ui/dist.
//   node render.mjs <outDir> [nameFilterRegex]
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const surfaces = resolve(here, '..', '..');
const dist = resolve(here, '../../../../../ui/dist');
const out = resolve(process.argv[2] ?? join(here, '..'));
const only = process.argv[3];
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(surfaces, 'panels/harness/fake-host-panels.js'), join(site, 'fake-host-panels.js'));
copyFileSync(join(surfaces, 'rail/harness/fake-host.js'), join(site, 'fake-host-rail.js'));
copyFileSync(join(surfaces, 'sidebar/harness/fake-host.js'), join(site, 'fake-host-sidebar.js'));
copyFileSync(join(here, 'act.js'), join(site, 'act.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}\n');
const html = (host) => readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="app.css">\n<link rel="stylesheet" href="harness.css">')
  .replace('<script src="app.js" defer></script>', `<script src="${host}"></script>\n<script src="app.js" defer></script>\n<script src="act.js"></script>`);
for (const host of ['panels', 'rail', 'sidebar']) writeFileSync(join(site, `index-${host}.html`), html(`fake-host-${host}.js`));
writeFileSync(join(site, 'frame.html'), '<!doctype html><meta charset="utf-8"><body style="margin:0;background:#888">' +
  '<script>const p=new URLSearchParams(location.search);const f=document.createElement("iframe");' +
  'f.style.cssText=`border:0;display:block;width:${p.get("w")}px;height:${p.get("h")}px`;' +
  'f.src=p.get("page")+"?"+p.get("q");document.body.append(f);</script>');

const shots = [];
const add = (s) => shots.push({ dpr: 1, act: '', ...s });
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 2]) {
    const d = dpr === 1 ? '' : '@2x';
    for (const p of ['history', 'bookmarks']) add({ name: `${p}-${scheme}${d}`, host: 'panels', q: `view=panel&p=${p}&scn=${p}-full`, w: 360, h: 640, scheme, dpr });
    add({ name: `notes-${scheme}${d}`, host: 'panels', q: 'view=dialog&p=notes&scn=notes-full', w: 360, h: 640, scheme, dpr });
    for (const p of ['settings', 'privacy']) add({ name: `${p}-${scheme}${d}`, host: 'panels', q: `view=dialog&p=${p}&scn=${p}-full`, w: 900, h: 700, scheme, dpr });
    add({ name: `customize-${scheme}${d}`, host: 'rail', q: 'view=panel&scn=customize', w: 380, h: 900, scheme, dpr });
  }
  const cust = (name, act, h = 900, w = 380) => add({ name: `customize-${name}-${scheme}`, host: 'rail', q: 'view=panel&scn=customize', w, h, scheme, act });
  cust('preset-open', 'click:.cust-form.is-stack .bw-select-button', 900);
  cust('preset-keys', 'focus:.cust-form.is-stack .bw-select-button|key:ArrowDown|key:ArrowDown|key:ArrowDown', 900);
  cust('editor', 'click:button[aria-label="Edit Work"]', 900);
  cust('editor-icon-open', 'click:button[aria-label="Edit Work"]|click:button[aria-label="Icon for Work"]', 900);
  cust('editor-colour-open', 'click:button[aria-label="Edit Work"]|click:button[aria-label="Colour for Work"]', 900);
  cust('focus-toggle', 'focus:.bw-toggle', 480);
  cust('delete', 'click:button[aria-label="Delete Work"]', 600);
  add({ name: `sidebar-focus-${scheme}`, host: 'sidebar', q: 'view=tabs&n=8&pinned=1&groups=1', w: 240, h: 360, scheme, act: 'focus:.side-tab.is-active' });
  add({ name: `history-focus-${scheme}`, host: 'panels', q: 'view=panel&p=history&scn=history-full', w: 360, h: 420, scheme, act: 'focus:.pn-row' });
  add({ name: `settings-focus-${scheme}`, host: 'panels', q: 'view=dialog&p=settings&scn=settings-full', w: 900, h: 500, scheme, act: 'click:.pg-nav-item:nth-child(2)|focus:.bw-toggle' });
  add({ name: `settings-blocking-${scheme}`, host: 'panels', q: 'view=dialog&p=settings&scn=settings-full', w: 900, h: 700, scheme, act: 'click:.pg-nav-item:nth-child(3)|focus:.bw-segmented button[aria-checked=true]' });
  add({ name: `notes-tasks-${scheme}`, host: 'panels', q: 'view=dialog&p=notes&scn=notes-full', w: 360, h: 640, scheme, act: 'click:.pn-tabs > button:nth-child(2)' });
}

const frame = pathToFileURL(join(site, 'frame.html')).href;
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const todo = shots.filter((x) => !only || new RegExp(only).test(x.name));
let next = 0;
async function worker() {
  while (next < todo.length) {
    const s = todo[next++];
    const q = encodeURIComponent(`${s.q}&scheme=${s.scheme}&w=${s.w}${s.act ? `&act=${s.act}` : ''}`);
    const url = `${frame}?w=${s.w}&h=${s.h}&page=index-${s.host}.html&q=${q}`;
    const file = join(out, `${s.name}.png`);
    rmSync(file, { force: true });
    const profile = mkdtempSync(join(tmpdir(), 'p2-shot-'));
    const child = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', `--user-data-dir=${profile}`,
      `--force-device-scale-factor=${s.dpr}`, `--window-size=${Math.max(s.w, 600)},${Math.max(s.h, 200) + 100}`, '--virtual-time-budget=6000', `--screenshot=${file}`, url], { stdio: 'ignore' });
    for (let waited = 0; waited < 60000; waited += 400) {
      await sleep(400);
      if (existsSync(file) && statSync(file).size > 0) { await sleep(300); break; }
    }
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    try { rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch { /* temp */ }
    if (!existsSync(file)) { console.error('no screenshot', s.name); continue; }
    const crop = spawnSync('python', ['-c', `from PIL import Image;i=Image.open(r"${file}");i.crop((0,0,round(${s.w}*${s.dpr}),round(${s.h}*${s.dpr}))).save(r"${file}")`], { encoding: 'utf8' });
    if (crop.status !== 0) console.error('crop', s.name, crop.stderr);
  }
}
await Promise.all([worker(), worker(), worker()]);
console.log('done', todo.length);
process.exit(0);
