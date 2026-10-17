// Headless screenshots for the T13 thread (toolbars and feeds): the address bar and tab strip with the default and an
// edited list, the toolbar editor sub-page, the Feeds settings rows and the Feeds panel; light and dark, 100% and 150%.
// Headless Edge only; a local static server feeds it the built ui/dist (or the ui/dist committed on
// origin/t6/layout for the "before" run), so nothing touches the desktop or any profile.
//   node render.mjs <outDir> <label: before|after> [nameFilterRegex]
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

const site = mkdtempSync(join(tmpdir(), 't13-site-'));
const files = ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'index.html'];
if (label === 'before') {
  for (const file of files) {
    const shown = spawnSync('git', ['show', `origin/t6/layout:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  }
} else {
  for (const file of files) copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
}
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
const read = (path) => JSON.parse(readFileSync(join(repo, 'ui', 'test', 'fixtures', path), 'utf8'));
const feeds = read('feeds-messages.json');
const settings = read('settings-messages.json').snap;
if (label === 'before') {
  // origin/t6/layout's page knows no `bars` object and the old toolbar list.
  delete settings.d.bars;
  settings.d.toolbarButtons = 'back,forward,reload,site-info,bookmark,newtab,split,reader';
}
const tabs = read('tab-options-messages.json');
writeFileSync(join(site, 'data.js'), `window.__T13 = ${JSON.stringify({ settings, commands: read('commands-snapshot.json'), tabs, feeds })};\n`);
const page = readFileSync(join(site, 'index.html'), 'utf8').replace('<script src="app.js" defer></script>', '<script src="data.js"></script>\n<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), page);

const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };
const server = createServer((req, res) => {
  const name = new URL(req.url, 'http://x').pathname.replace(/^\/+/, '') || 'index.html';
  const file = join(site, name);
  if (!file.startsWith(site) || !existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const base = `http://127.0.0.1:${server.address().port}/index.html`;

const DEFAULT = 'back,forward,reload,site-info,split,reader,extensions,downloads';
const EDITED = 'forward,back,reload,site-info,split,bookmark,newtab,cmd:new-private-tab,chain:2';
const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '' : '@1.5x';
    const add = (name, query, w, h) => shots.push({ name: `${label}-${name}-${scheme}${d}`, query: `${query}&scheme=${scheme}`, w, h, dpr });
    if (label === 'before') {
      add('toolbar-default', 'view=toolbar&scn=toolbar', 1100, 40);
      add('settings-toolbar-row', 'view=settings&scn=settings&section=appearance&act=scroll:.sp-row[data-row="showHomeButton"]', 1100, 700);
      continue;
    }
    add('toolbar-default', `view=toolbar&scn=toolbar&items=${DEFAULT}`, 1100, 40);
    add('toolbar-edited', `view=toolbar&scn=toolbar&items=${EDITED}&tools=cmd_new-private-tab:New_private_tab:7,chain_2:Morning:8&extra=128&feeds=1`, 1100, 40);
    add('toolbar-left-only', 'view=toolbar&scn=toolbar&items=back,reload,site-info&feeds=0', 1100, 40);
    add('tabs-default', 'view=tabs&scn=tabs', 900, 40);
    add('tabs-with-buttons', 'view=tabs&scn=tabs&tabtools=cmd_new-window:New_window:1,chain_2:Morning:2', 900, 40);
    add('settings-edit-toolbars', 'view=settings&scn=settings&section=appearance&act=click:[data-opens="toolbarButtons"] button', 1100, 1500);
    add('settings-feed-rows', 'view=settings&scn=settings&section=address&act=scroll:.sp-row[data-row="feedsOpen"]', 1100, 700);
    add('feeds-panel', 'view=dialog&p=feeds&scn=feeds&fscn=snap', 480, 680);
    add('feeds-preview', 'view=dialog&p=feeds&scn=feeds&fscn=preview', 480, 680);
    add('feeds-empty', 'view=dialog&p=feeds&scn=feeds&fscn=empty', 480, 400);
  }
}
let made = 0;
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
let next = 0;
async function worker() {
  while (next < todo.length) {
    const s = todo[next++];
    const file = join(out, `${s.name}.png`);
    rmSync(file, { force: true });
    const profile = mkdtempSync(join(tmpdir(), 't13-edge-'));
    await new Promise((done) => {
      const child = spawn(edge, [
        '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`,
        `--window-size=${s.w},${s.h + 100}`, '--virtual-time-budget=5000', `--screenshot=${file}`, `${base}?${s.query}&w=${s.w}`,
      ], { stdio: 'ignore' });
      const started = Date.now();
      const poll = setInterval(() => {
        const written = existsSync(file) && statSync(file).size > 0;
        if (written || Date.now() - started > 60000) { clearInterval(poll); setTimeout(done, written ? 300 : 0); }
      }, 300);
      child.on('exit', () => { setTimeout(() => { clearInterval(poll); done(); }, 500); });
    });
    await new Promise((finished) => {
      spawn('powershell', ['-NoProfile', '-Command',
        `Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like '*${profile}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`], { stdio: 'ignore' }).on('exit', finished);
    });
    try { rmSync(profile, { recursive: true, force: true, maxRetries: 5 }); } catch { /* temp */ }
    if (existsSync(file) && statSync(file).size > 0) made += 1;
    else console.error('no screenshot', s.name);
  }
}
await worker();
server.close();
try { rmSync(site, { recursive: true, force: true }); } catch { /* temp */ }
console.log(`rendered ${made} of ${shots.length} shots to ${out}`);
process.exit(0);
