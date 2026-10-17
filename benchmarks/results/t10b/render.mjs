// Headless before/after screenshots of two-level tab stacks (thread T10b; after benchmarks/results/t6/render.mjs).
// One headless Edge at a time, each with its own temp profile, served from 127.0.0.1.
// "before" is the branch base's committed ui/dist (T10 + T6: Compact stacks, one tab row), "after" this tree's.
// PNGs: <before|after>-<scene>-<dark|light>-<100|150>.png at device scale 1 and 1.5.
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
const BASE = process.env.T10B_BASE || 'HEAD';

const site = mkdtempSync(join(tmpdir(), 't10b-site-'));
for (const file of ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'start-page.js', 'index.html']) {
  if (label === 'before') {
    const shown = spawnSync('git', ['show', `${BASE}:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  } else if (existsSync(join(repo, 'ui', 'dist', file))) copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
}
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}\n');
// The fixtures each side's page parses: the base's for "before", this tree's for "after".
const fx = (name) => {
  const text = label === 'before'
    ? spawnSync('git', ['show', `${BASE}:ui/test/fixtures/${name}`], { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }).stdout
    : readFileSync(join(repo, 'ui', 'test', 'fixtures', name), 'utf8');
  return JSON.parse(text.replace(/^\uFEFF/, ''));
};

// The strip: Inbox, a three-tab "Docs" stack (folded, its middle tab in use), News.
const item = (id, title, extra = {}) => ({ v: undefined, id, title, loading: false, pinned: false, muted: false, private: false, marker: -1, g: 0, gl: false, fav: false, ...extra });
const lead = { gname: 'Docs', gcol: 2, gcollapsed: true, gsize: 3 };
const tabs = {
  v: 1, t: 'tabs', active: 3,
  items: [item(1, 'Inbox'), item(2, 'Docs - Design notes', { g: 9, gl: true, ...lead }), item(3, 'Docs - Release plan', { g: 9 }), item(4, 'Docs - Weekly review', { g: 9 }), item(5, 'News')]
    .map(({ v, ...rest }) => rest),
};
const defaults = fx('tab-options-messages.json').defaults;
const opts = (stackRow) => {
  const { stackRow: _ignored, ...base } = defaults;
  return label === 'before' ? base : { ...base, stackRow };
};
const settings = fx('settings-messages.json');
const scenes = {
  'strip-off': [opts('off'), tabs],
  'strip-regular': [opts('regular'), tabs],
  'strip-compact': [opts('compact'), tabs],
  'settings-stacks': [{ v: 1, t: 'settingsgo', s: 'tabs' }, settings.snap, fx('commands-snapshot.json'), fx('address-messages.json').snap, fx('protection-snapshot.json'), fx('privacy-snapshot.json'), fx('extensions-messages.json').snap],
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
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const edgeArgs = (profile, extra) => ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`, ...extra];

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '100' : '150';
    const add = (name, query, w, h, act = '') => shots.push({ name: `${label}-${name}-${scheme}-${d}`, query: `${query}&scheme=${scheme}${act ? `&act=${encodeURIComponent(act)}` : ''}`, w, h, dpr });
    // The strip host as Rust sizes it: the tab row (28), plus a gap (2) and the second row (28, or 22 compact).
    add('strip', 'view=tabs&scn=strip-off', 900, 28);
    if (label === 'after') {
      add('strip-two-level', 'view=tabs&scn=strip-regular', 900, 58);
      add('strip-two-level-compact', 'view=tabs&scn=strip-compact', 900, 52);
    }
    // Settings: the Tab stacks row (a fifth choice) and, behind More options, "Compact second row".
    add('settings-stacks', 'view=settings&scn=settings-stacks', 1100, 1100,
      'wait:500|click:section[data-area="tabs"] .sp-more-btn|wait:400|top:.sp-row[data-row="tabs_stack_style"]|wait:300');
    if (label === 'after') {
      add('settings-compact-row', 'view=settings&scn=settings-stacks', 1100, 700,
        'wait:500|click:section[data-area="tabs"] .sp-more-btn|wait:400|top:.sp-row[data-row="tabs_workspace_counts"]|wait:300');
    }
  }
}
let made = 0;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
for (const s of todo) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 't10b-edge-'));
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
