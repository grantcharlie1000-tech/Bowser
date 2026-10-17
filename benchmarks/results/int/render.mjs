// Headless screenshots of the wave-1 integration: the settings page (Tabs, Start page, Privacy, Address
// bar, Quick commands, Appearance), the tab strip, the start page, Quick commands, the address drop-down
// and the shield, light and dark, at 100% and 150%. One headless Edge at a time, each with its own temp
// profile, served from 127.0.0.1; "before" uses origin/main's committed ui/dist, "after" this tree's.
//   node render.mjs <outDir> <before|after> [nameFilterRegex]
import { createServer } from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..', '..');
const [outArg, label = 'after', only] = process.argv.slice(2);
if (!outArg) { console.error('usage: node render.mjs <outDir> <before|after> [filter]'); process.exit(2); }
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';

const site = mkdtempSync(join(tmpdir(), 'int-site-'));
const files = ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'start-page.js', 'index.html'];
for (const file of files) {
  if (label === 'before') {
    const shown = spawnSync('git', ['show', `origin/main:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  } else if (existsSync(join(repo, 'ui', 'dist', file))) {
    copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
  }
}
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}\n');
const fx = (name) => JSON.parse(readFileSync(join(repo, 'ui', 'test', 'fixtures', name), 'utf8'));
const settings = fx('settings-messages.json');
const popup = fx('popup-pages.json');
const tabs = fx('tab-options-messages.json');
const start = fx('start-page-messages.json');
const suggest = fx('suggest-messages.json');
const sections = (s) => [{ v: 1, t: 'settingsgo', s }, settings.snap, fx('commands-snapshot.json'), fx('address-messages.json').snap, fx('protection-snapshot.json')];
// A palette with a bookmark nickname (T2) typed whole: with auto-open off it ranks the bookmark first.
const palette = { ...popup.palette, autonick: false };
const scenes = {
  'settings-appearance': sections('appearance'),
  'settings-tabs': sections('tabs'),
  'settings-startpage': sections('startpage'),
  'settings-privacy': sections('privacy'),
  'settings-address': sections('address'),
  'settings-commands': sections('commands'),
  tabs: [tabs.opts, tabs.tabs],
  start: [start.page],
  palette: [palette],
  dropdown: [suggest.suggest],
  shield: [popup.siteinfo],
};
writeFileSync(join(site, 'data.js'), `window.__INT = ${JSON.stringify({ scenes })};\n`);
const page = readFileSync(join(site, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="data.js"></script>\n<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), page);
writeFileSync(join(site, 'frame.html'), '<!doctype html><meta charset="utf-8"><body style="margin:0;background:#888">' +
  '<script>const p=new URLSearchParams(location.search);const f=document.createElement("iframe");' +
  'f.style.cssText=`border:0;display:block;width:${p.get("w")}px;height:${p.get("h")}px`;' +
  'f.src="index.html?"+p.get("q");document.body.append(f);</script>');

const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };
const server = createServer((req, res) => {
  const name = new URL(req.url, 'http://x').pathname.replace(/^\/+/, '') || 'index.html';
  const file = join(site, name);
  if (!file.startsWith(site) || !existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '100' : '150';
    const add = (name, query, w, h) => shots.push({ name: `${label}-${name}-${scheme}-${d}`, query: `${query}&scheme=${scheme}`, w, h, dpr });
    for (const s of ['appearance', 'tabs', 'startpage', 'privacy', 'address', 'commands']) add(`settings-${s}`, `view=settings&scn=settings-${s}`, 1100, 900);
    add('tabstrip', 'view=tabs&scn=tabs', 1100, 34);
    add('startpage', 'view=split&scn=start', 1100, 760);
    add('palette', 'view=popup&scn=palette&q=docs', 560, 360);
    add('dropdown', 'view=hover&scn=dropdown', 560, 300);
    add('shield', 'view=popup&scn=shield', 340, 520);
  }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let made = 0;
for (const s of shots.filter((shot) => !only || new RegExp(only).test(shot.name))) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 'int-edge-'));
  const url = `${origin}/frame.html?w=${s.w}&h=${s.h}&q=${encodeURIComponent(`${s.query}&w=${s.w}`)}`;
  const child = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`,
    `--force-device-scale-factor=${s.dpr}`, `--window-size=${Math.max(s.w, 600)},${Math.max(s.h, 200) + 100}`, '--virtual-time-budget=5000', `--screenshot=${file}`, url], { stdio: 'ignore' });
  for (let waited = 0; waited < 60000; waited += 300) {
    await sleep(300);
    if (existsSync(file) && statSync(file).size > 0) { await sleep(300); break; }
  }
  // Only this shot's own browser tree (its own temp profile).
  spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 5 }); } catch { /* temp */ }
  if (!existsSync(file)) { console.error('no screenshot', s.name); continue; }
  const crop = spawnSync('python', ['-c', `from PIL import Image;i=Image.open(r"${file}");i.crop((0,0,round(${s.w}*${s.dpr}),round(${s.h}*${s.dpr}))).save(r"${file}")`], { encoding: 'utf8' });
  if (crop.status !== 0) console.error('crop', s.name, crop.stderr.slice(0, 200));
  made += 1;
}
server.close();
try { rmSync(site, { recursive: true, force: true }); } catch { /* temp */ }
console.log(`rendered ${made} of ${shots.length} shots to ${out}`);
process.exit(0);
