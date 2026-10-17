// Headless before/after screenshots and scroll measurements for the settings-slimming pass (after
// benchmarks/results/look/render.mjs). One headless Edge at a time, each with its own temp profile,
// served from 127.0.0.1. "before" is origin/vivaldi/wave1-review's committed ui/dist, "after" this tree's.
//   node render.mjs <outDir> <before|after> [nameFilterRegex]   screenshots
//   node render.mjs <outDir> <before|after> --measure             scroll heights -> <label>-metrics.json
import { createServer } from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..', '..');
const [outArg, label = 'after', only] = process.argv.slice(2);
if (!outArg) { console.error('usage: node render.mjs <outDir> <before|after> [filter|--measure]'); process.exit(2); }
const measure = only === '--measure';
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const BASE = 'origin/vivaldi/wave1-review';

const site = mkdtempSync(join(tmpdir(), 'slim-site-'));
for (const file of ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'start-page.js', 'index.html']) {
  if (label === 'before') {
    const shown = spawnSync('git', ['show', `${BASE}:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  } else if (existsSync(join(repo, 'ui', 'dist', file))) copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
}
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}\n');
const fx = (name) => JSON.parse(readFileSync(join(repo, 'ui', 'test', 'fixtures', name), 'utf8').replace(/^\uFEFF/, ''));
const settings = fx('settings-messages.json');
const address = fx('address-messages.json');
const scene = (s) => [{ v: 1, t: 'settingsgo', s }, settings.snap, fx('commands-snapshot.json'), address.snap, fx('protection-snapshot.json'), fx('privacy-snapshot.json'), fx('extensions-messages.json').snap];
// Before: the sixteen sections of the integrated page. After: the twelve areas, "Not active yet" and the sub-pages.
const BEFORE = ['profile', 'appearance', 'tabs', 'startpage', 'privacy', 'performance', 'address', 'search', 'bookmarks', 'keyboard', 'commands', 'downloads', 'media', 'extensions', 'updates', 'about'];
const AREAS = ['profile', 'appearance', 'tabs', 'privacy', 'performance', 'address', 'keyboard', 'downloads', 'media', 'extensions', 'updates', 'about'];
const PAGES = { engines: 'address', siteRules: 'privacy', blockLists: 'privacy', deleteData: 'privacy', dashboard: 'privacy', kbEditor: 'keyboard', chains: 'keyboard' };
const scenes = {};
for (const s of new Set([...BEFORE, ...AREAS])) scenes[`s-${s}`] = scene(s);
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
let metrics = null;
const server = createServer((req, res) => {
  if (process.env.SLIM_DEBUG) console.error(req.method, req.url);
  if (req.method === 'POST' && req.url === '/metrics') {
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => { metrics = JSON.parse(body); res.writeHead(204).end(); });
    return;
  }
  const name = new URL(req.url, 'http://x').pathname.replace(/^\/+/, '') || 'index.html';
  const file = join(site, name);
  if (!file.startsWith(site) || !existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const edgeArgs = (profile, extra) => ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`, ...extra];

if (measure) {
  // The default view (everything collapsed) at 1100x900, 100%: the scroll height of the whole page and of each section.
  const profile = mkdtempSync(join(tmpdir(), 'slim-edge-'));
  const url = `${origin}/frame.html?w=1100&h=900&q=${encodeURIComponent(`view=settings&scn=s-profile&scheme=light&w=1100&act=${encodeURIComponent('wait:600|measure')}`)}`;
  const shot = join(profile, 'm.png');
  const child = spawn(edge, edgeArgs(profile, ['--window-size=1100,900', '--virtual-time-budget=8000', `--screenshot=${shot}`, url]), { stdio: 'ignore' });
  for (let waited = 0; waited < 60000 && metrics === null; waited += 300) await sleep(300);
  spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  await sleep(500);
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 5 }); } catch { /* temp */ }
  if (metrics === null) { console.error('no metrics'); process.exit(1); }
  const data = metrics;
  writeFileSync(join(out, `${label}-metrics.json`), `${JSON.stringify(data, null, 2)}\n`);
  console.log(label, 'total', data.total, 'visible rows', data.rows);
  server.close();
  try { rmSync(site, { recursive: true, force: true }); } catch { /* temp */ }
  process.exit(0);
}

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '100' : '150';
    const add = (name, query, w, h, act = '') => shots.push({ name: `${label}-${name}-${scheme}-${d}`, query: `${query}&scheme=${scheme}${act ? `&act=${encodeURIComponent(act)}` : ''}`, w, h, dpr });
    if (label === 'before') {
      for (const s of BEFORE) add(s, `view=settings&scn=s-${s}`, 1100, 1300);
    } else {
      for (const s of AREAS) {
        add(s, `view=settings&scn=s-${s}`, 1100, 1300);
        add(`${s}-more`, `view=settings&scn=s-${s}`, 1100, 2400, `wait:500|click:section[data-area="${s}"] .sp-more-btn|wait:500|top:section[data-area="${s}"] .sp-more|wait:300`);
      }
      add('later', 'view=settings&scn=s-about', 1100, 900, 'wait:500|top:section[data-area="later"]|wait:200');
      add('later-more', 'view=settings&scn=s-about', 1100, 900, 'wait:500|click:section[data-area="later"] .sp-more-btn|wait:400|top:section[data-area="later"]|wait:200');
      for (const [id, area] of Object.entries(PAGES)) add(`page-${id}`, `view=settings&scn=s-${area}`, 1100, 1300, `wait:500|click:[data-opens="${id}"] .bw-button|wait:500`);
      add('search', 'view=settings&scn=s-profile', 1100, 1100, 'wait:500|type:tab audio|wait:400');
    }
    if (scheme === 'light' && dpr === 1) {
      add('metrics', `view=settings&scn=s-profile`, 1100, 900, 'wait:600|measure|wait:300');
    }
  }
}
let made = 0;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
for (const s of todo) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 'slim-edge-'));
  const url = `${origin}/frame.html?w=${s.w}&h=${s.h}&q=${encodeURIComponent(`${s.query}&w=${s.w}`)}`;
  const child = spawn(edge, edgeArgs(profile, [`--force-device-scale-factor=${s.dpr}`, `--window-size=${Math.max(s.w, 600)},${Math.max(s.h, 200) + 100}`, '--virtual-time-budget=8000', `--screenshot=${file}`, url]), { stdio: 'ignore' });
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
console.log(`rendered ${made} of ${todo.length} shots to ${out}`);
process.exit(0);
