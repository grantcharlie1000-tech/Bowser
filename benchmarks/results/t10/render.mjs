// Headless before/after screenshots for T10 (tab stacks, tiling and workspaces), after
// benchmarks/results/slim/render.mjs. One headless Edge at a time, each with its own temp profile,
// served from 127.0.0.1. "before" is origin/vivaldi/settings-slim's committed ui/dist (this branch's
// base), "after" this tree's.
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
const BASE = 'origin/vivaldi/settings-slim';
const after = label !== 'before';

const site = mkdtempSync(join(tmpdir(), 't10-site-'));
for (const file of ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'start-page.js', 'index.html']) {
  if (!after) {
    const shown = spawnSync('git', ['show', `${BASE}:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  } else if (existsSync(join(repo, 'ui', 'dist', file))) copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
}
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}\n');
const fx = (name, ref) => {
  const text = ref
    ? spawnSync('git', ['show', `${ref}:ui/test/fixtures/${name}`], { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }).stdout
    : readFileSync(join(repo, 'ui', 'test', 'fixtures', name), 'utf8');
  return JSON.parse(text.replace(/^\uFEFF/, ''));
};
// The before page parses only the snapshot its own build knows.
const ref = after ? null : BASE;
const settings = fx('settings-messages.json', ref);
const address = fx('address-messages.json', ref);
const settingsScene = (s) => [{ v: 1, t: 'settingsgo', s }, settings.snap, fx('commands-snapshot.json', ref), address.snap, fx('protection-snapshot.json', ref), fx('privacy-snapshot.json', ref), fx('extensions-messages.json', ref).snap];

// The rail: two workspaces (one with an emoji), the add button and the bottom buttons. Before: no counts, no emoji.
const railItem = (k, icon, labelText, extra = {}) => ({ k, icon, label: labelText, active: false, dot: false, accent: '', fav: false, ...extra });
const railItems = [
  railItem('customize', 'customize', 'Customize'),
  railItem('ws:0', 'g:briefcase', 'Default', { active: true, ...(after ? { n: 7 } : {}) }),
  railItem('ws:1', 'g:code', 'Work', { accent: '#8b66d9', ...(after ? { n: 3, emoji: '🚀' } : {}) }),
  railItem('ws:2', 'g:book', 'Reading', { accent: '#32a89b', ...(after ? { n: 12 } : {}) }),
  railItem('addws', 'plus', 'New workspace'),
  railItem('bookmarks', 'heart', 'Bookmarks'),
  railItem('history', 'clock', 'History'),
  railItem('menu', 'dots', 'Menu'),
];
// The strip: four tabs, the third marked as the stack target of a held tab (after only).
const tab = (id, title, extra = {}) => ({ id, title, loading: false, pinned: false, muted: false, private: false, marker: -1, g: 0, gl: false, fav: false, ...extra });
const tabs = { v: 1, t: 'tabs', active: 2, items: [tab(1, 'Inbox - Mail'), tab(2, 'Quarterly plan - Docs'), tab(3, 'Morning news'), tab(4, 'Wiki: Tab stacking')] };
const sidebarTabs = { v: 1, t: 'tabs', active: 2, items: [tab(1, 'Inbox - Mail'), tab(2, 'Quarterly plan - Docs'), tab(3, 'Morning news')] };
const scenes = {
  's-tabs': settingsScene('tabs'),
  rail: [{ v: 1, t: 'rail', items: railItems }],
  strip: [{ v: 1, t: 'layout', mode: 'horizontal' }, tabs],
  sidebar: [{ v: 1, t: 'layout', mode: 'vertical' }, { v: 1, t: 'workspace', name: 'Work', icon: 'g:code', ...(after ? { emoji: '🚀' } : {}) }, sidebarTabs],
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
const edgeArgs = (profile, extra) => ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', '--force-prefers-reduced-motion', `--user-data-dir=${profile}`, ...extra];

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '100' : '150';
    const add = (name, query, w, h, act = '') => shots.push({ name: `${label}-${name}-${scheme}-${d}`, query: `${query}&scheme=${scheme}${act ? `&act=${encodeURIComponent(act)}` : ''}`, w, h, dpr });
    // Settings: the Tabs and start page area (its core rows), and its More options opened at the stacks heading.
    add('settings-tabs', 'view=settings&scn=s-tabs', 1100, 1300);
    add('settings-tabs-more', 'view=settings&scn=s-tabs', 1100, 1500,
      `wait:500|click:section[data-area="tabs"] .sp-more-btn|wait:500|top:${after ? '.sp-more .sp-group:has([data-row="tabs_stack_open_in_current"])' : 'section[data-area="tabs"] .sp-more'}|wait:300`);
    if (after) add('settings-workspace-rules', 'view=settings&scn=s-tabs', 1100, 700, 'wait:500|click:[data-opens="workspaceRules"] .bw-button|wait:500');
    add('rail', 'view=rail&scn=rail', 56, 420);
    add('strip', 'view=tabs&scn=strip', 900, 34, after ? 'wait:400|addclass:.tab[data-id="3"]~stack-target|wait:200' : 'wait:400');
    add('sidebar', 'view=tabs&scn=sidebar', 260, 200);
  }
}
let made = 0;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
for (const s of todo) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 't10-edge-'));
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
