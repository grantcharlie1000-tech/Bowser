// Headless before/after screenshots of the Appearance area for the themes pass (after
// benchmarks/results/slim/render.mjs). One headless Edge at a time, each with its own temp profile, served
// from 127.0.0.1. "before" is origin/vivaldi/settings-slim's committed ui/dist, "after" this tree's.
//   node render.mjs <outDir> <before|after> [nameFilterRegex]
// Scenes: the Appearance area as a fresh profile shows it (main view, then behind More options), and with
// every theme option set to a non-default value, so each control's state is seen.
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
const BASE = 'origin/vivaldi/settings-slim';

const site = mkdtempSync(join(tmpdir(), 't8-site-'));
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
// The old page refuses a snapshot with a key it does not know, so "before" gets the snapshot without the theme options.
const snap = (extra) => {
  const d = { ...settings.snap.d };
  if (label === 'before') delete d.themeOptions;
  else if (extra) d.themeOptions = { ...d.themeOptions, ...extra };
  return { ...settings.snap, d };
};
const CUSTOM = {
  theme_schedule: 'time', theme_look: 'dark', theme_light_from: '06:30', theme_dark_from: '21:00',
  theme_background: '#1e2a38', theme_foreground: '#e6edf5', theme_highlight: '#3b82c4',
  theme_colouring: 'unified', theme_accent_page: true, theme_saturation: 60, theme_contrast: 'high',
};
const scene = (extra) => [{ v: 1, t: 'settingsgo', s: 'appearance' }, snap(extra), fx('commands-snapshot.json'), address.snap, fx('protection-snapshot.json'), fx('privacy-snapshot.json'), fx('extensions-messages.json').snap];
writeFileSync(join(site, 'data.js'), `window.__INT = ${JSON.stringify({ scenes: { 's-appearance': scene(), 's-custom': scene(CUSTOM) } })};\n`);
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
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const edgeArgs = (profile, extra) => ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`, ...extra];

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '100' : '150';
    const add = (name, scn, w, h, act = '') => shots.push({ name: `${label}-${name}-${scheme}-${d}`, query: `view=settings&scn=${scn}&scheme=${scheme}${act ? `&act=${encodeURIComponent(act)}` : ''}`, w, h, dpr });
    const open = 'wait:500|click:section[data-area="appearance"] .sp-more-btn|wait:500';
    add('appearance', 's-appearance', 1100, 1000, 'wait:500|top:section[data-area="appearance"]|wait:200');
    add('more', 's-appearance', 1100, 1500, `${open}|top:[data-row="themeLook"]|wait:300`);
    if (label === 'after') {
      add('core-custom', 's-custom', 1100, 1000, 'wait:500|top:section[data-area="appearance"]|wait:200');
      add('more-custom', 's-custom', 1100, 1500, `${open}|top:[data-row="themeLook"]|wait:300`);
      add('file-custom', 's-custom', 1100, 700, `${open}|top:[data-row="themeContrast"]|wait:300`);
    }
  }
}
let made = 0;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
for (const s of todo) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 't8-edge-'));
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
