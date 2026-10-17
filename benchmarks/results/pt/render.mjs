// Headless before/after screenshots for the Page tools thread (after benchmarks/results/slim/render.mjs,
// whose fake host this reuses). One headless Edge at a time, each with its own temp profile, served from
// 127.0.0.1. "before" is origin/vivaldi/settings-slim's committed ui/dist, "after" this tree's.
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

const site = mkdtempSync(join(tmpdir(), 'pt-site-'));
for (const file of ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'start-page.js', 'index.html']) {
  if (label === 'before') {
    const shown = spawnSync('git', ['show', `${BASE}:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  } else if (existsSync(join(repo, 'ui', 'dist', file))) copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
}
copyFileSync(join(repo, 'benchmarks', 'results', 'slim', 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:#20242c}html.h-light{background:#f4f6f8}\n');
const fx = (name) => JSON.parse(readFileSync(join(repo, 'ui', 'test', 'fixtures', name), 'utf8').replace(/^\uFEFF/, ''));
const settings = fx('settings-messages.json');
const address = fx('address-messages.json');
const pagetools = fx('page-tools-snapshot.json');
const scene = (s) => [{ v: 1, t: 'settingsgo', s }, settings.snap, fx('commands-snapshot.json'), address.snap, pagetools, fx('protection-snapshot.json'), fx('privacy-snapshot.json'), fx('extensions-messages.json').snap];
const NOW = 1_760_000_000;
const notesOld = { v: 1, t: 'snap', p: 'notes', d: {
  notes: [{ id: 1, title: 'Reading list', body: 'Chapters 1 to 4' }, { id: 2, title: 'Quote', body: 'Simple is better' }],
  tasks: [{ id: 3, text: 'Send the draft', complete: false }] } };
const note = (id, title, body, over = {}) => ({ id, title, body, addr: '', created: 0, folder: 0, ...over });
const notesNew = { v: 1, t: 'snap', p: 'notes', d: {
  sort: 'manual',
  folders: [{ id: 10, name: 'Work' }],
  notes: [
    note(1, 'Reading list', 'Chapters 1 to 4', { addr: 'https://docs.example.com/guide', created: NOW }),
    note(2, 'Meeting', '# Agenda\n\n- Roadmap\n- **Budget**\n\nSee [the doc](https://example.com/plan).', { folder: 10, addr: 'https://example.com/plan', created: NOW - 86400 }),
    note(4, 'Capture: Docs', '![Capture](file:///C:/Users/me/Pictures/Bowser/docs.png)', { addr: 'https://docs.example.com/', created: NOW - 3600 }),
  ],
  tasks: [{ id: 3, text: 'Send the draft', complete: false }] } };
const panelShow = { v: 1, t: 'panelshow', p: 'notes' };
const scenes = {
  'set-profile': scene('profile'), 'set-pagetools': scene('pagetools'),
  notes: [panelShow, label === 'before' ? notesOld : notesNew],
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

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '100' : '150';
    const add = (name, query, w, h, act = '') => shots.push({ name: `${label}-${name}-${scheme}-${d}`, query: `${query}&scheme=${scheme}${act ? `&act=${encodeURIComponent(act)}` : ''}`, w, h, dpr });
    if (label === 'before') {
      add('settings-nav', 'view=settings&scn=set-profile', 1100, 900);
      add('notes', 'view=dialog&p=notes&scn=notes', 380, 640, 'wait:600');
    } else {
      add('pagetools', 'view=settings&scn=set-pagetools', 1100, 900, 'wait:500|top:section[data-area="pagetools"]|wait:300');
      add('pagetools-more', 'view=settings&scn=set-pagetools', 1100, 1500, 'wait:500|click:section[data-area="pagetools"] .sp-more-btn|wait:500|top:section[data-area="pagetools"]|wait:300');
      add('notes', 'view=dialog&p=notes&scn=notes', 380, 640, 'wait:600');
      add('notes-open', 'view=dialog&p=notes&scn=notes', 380, 900, 'wait:600|click:.pn-note-row[data-id="2"]|wait:500|click:.pn-note-tools .bw-button|wait:300');
    }
  }
}
let made = 0;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
for (const s of todo) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 'pt-edge-'));
  const url = `${origin}/frame.html?w=${s.w}&h=${s.h}&q=${encodeURIComponent(`${s.query}&w=${s.w}`)}`;
  const child = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`, `--window-size=${Math.max(s.w, 400)},${Math.max(s.h, 200) + 100}`, '--virtual-time-budget=8000', `--screenshot=${file}`, url], { stdio: 'ignore' });
  for (let waited = 0; waited < 60000; waited += 300) {
    await sleep(300);
    if (existsSync(file) && statSync(file).size > 0) { await sleep(300); break; }
  }
  spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 5 }); } catch { /* temp */ }
  if (!existsSync(file)) { console.error('no screenshot', s.name); continue; }
  spawnSync('python', ['-c', `from PIL import Image;i=Image.open(r"${file}");i.crop((0,0,round(${s.w}*${s.dpr}),round(${s.h}*${s.dpr}))).save(r"${file}")`], { encoding: 'utf8' });
  made += 1;
}
server.close();
try { rmSync(site, { recursive: true, force: true }); } catch { /* temp */ }
console.log(`rendered ${made} of ${todo.length} shots to ${out}`);
process.exit(0);
