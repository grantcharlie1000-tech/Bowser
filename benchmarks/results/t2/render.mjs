// Renders the address bar thread's views from a ui/dist folder in headless Edge.
//   node render.mjs <distDir> <outDir> <label> [nameFilterRegex]
// `before` uses the dist of origin/main (which lacks the new views, so their shots show the old page
// or nothing); `after` uses this branch's dist. Light and dark, at 100% and 150%.
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { createServer } from 'node:http';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..', '..');
const [distArg, outArg, label = 'run', only] = process.argv.slice(2);
if (!distArg || !outArg) {
  console.error('usage: node render.mjs <distDir> <outDir> <label> [filter]');
  process.exit(2);
}
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, `_site-${label}`);
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css', 'settings-page.js']) {
  try { copyFileSync(join(dist, f), join(site, f)); } catch { /* an older dist may lack a file */ }
}
const fixtures = {
  address: JSON.parse(readFileSync(join(repo, 'ui/test/fixtures/address-messages.json'), 'utf8')),
  settings: JSON.parse(readFileSync(join(repo, 'ui/test/fixtures/settings-messages.json'), 'utf8')),
};
writeFileSync(join(site, 'fixtures.js'), `window.__fixtures = ${JSON.stringify(fixtures)};\n`);
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:linear-gradient(90deg,#1b2029,#2a2f3a 60%,#1d2a33)}html.h-light{background:linear-gradient(90deg,#dde3ea,#eef1f4 60%,#d8e2ea)}\n');
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fixtures.js"></script>\n<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '' : '@150';
    const add = (name, view, scn, w, h, extra = '') => shots.push({ name: `${name}-${scheme}${d}`, view, scn, w, h, scheme, dpr, extra });
    add('toolbar', 'toolbar', 'toolbar', 1000, 34);
    add('toolbar-search-field', 'toolbar', 'toolbar-search', 1000, 34);
    add('toolbar-search-button', 'toolbar', 'toolbar-button', 1000, 34);
    add('toolbar-hidden', 'toolbar', 'toolbar-hidden', 1000, 34);
    add('dropdown', 'hover', 'hover', 560, 250);
    add('settings-address', 'settings', 'settings', 1000, 900, '&s=address');
    add('settings-search', 'settings', 'settings', 1000, 900, '&s=search');
    add('settings-bookmarks', 'settings', 'settings', 1000, 700, '&s=bookmarks');
    add('settings-bar-rows', 'settings', 'settings', 1000, 900, '&s=appearance');
    add('dialog-qr', 'dialog', 'qr', 360, 470, '&p=address');
    add('dialog-bookmark', 'dialog', 'bookmark', 400, 520, '&p=address');
    add('bar', 'bar', 'bar', 1000, 28);
    add('bar-icons', 'bar', 'bar-icons', 1000, 28);
  }
}
// The settings page is a module the bundle imports when its view opens: a file:// page cannot import it,
// so the pictures are taken from a local server on this machine (nothing leaves it).
const types = { '.html': 'text/html;charset=utf-8', '.js': 'text/javascript;charset=utf-8', '.css': 'text/css;charset=utf-8' };
const server = createServer((request, response) => {
  const name = (request.url ?? '/').split('?')[0].replace(/^\/+/, '') || 'index.html';
  try {
    const body = readFileSync(join(site, name.replace(/\.\./g, '')));
    response.writeHead(200, { 'content-type': types[name.slice(name.lastIndexOf('.'))] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((ready) => server.listen(0, '127.0.0.1', ready));
const origin = `http://127.0.0.1:${server.address().port}`;
// Headless Edge will not open a window narrower than about 500px, so every picture is an iframe of its
// exact size inside a wider page, and the screenshot is cropped to it.
writeFileSync(join(site, 'frame.html'), '<!doctype html><meta charset="utf-8"><body style="margin:0;background:#888">' +
  '<script>const p=new URLSearchParams(location.search);const f=document.createElement("iframe");' +
  'f.style.cssText=`border:0;display:block;width:${p.get("w")}px;height:${p.get("h")}px`;' +
  'f.src="index.html?"+p.get("q");document.body.append(f);</script>');
const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms));
let count = 0;
for (const s of shots.filter((shot) => !only || new RegExp(only).test(shot.name))) {
  const query = `view=${s.view}&scn=${s.scn}&scheme=${s.scheme}&w=${s.w}${s.extra}`;
  const url = `${origin}/frame.html?w=${s.w}&h=${s.h}&q=${encodeURIComponent(query)}`;
  const file = join(out, `${label}-${s.name}.png`);
  rmSync(file, { force: true });
  // A fresh profile for each picture, and the process ended once the file is there: a lingering Edge
  // would take the next launch over and write nothing.
  const profile = mkdtempSync(join(tmpdir(), 't2-shot-'));
  const child = spawn(edge, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`,
    `--window-size=${Math.max(s.w, 600)},${Math.max(s.h, 200) + 100}`, '--virtual-time-budget=2500', `--screenshot=${file}`, url,
  ], { stdio: 'ignore' });
  for (let waited = 0; waited < 45000; waited += 300) {
    await sleep(300);
    if (existsSync(file) && statSync(file).size > 0) { await sleep(200); break; }
  }
  spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch { /* temp */ }
  if (!existsSync(file)) { console.error('no screenshot', s.name); continue; }
  const crop = spawnSync('python', ['-c', `from PIL import Image;i=Image.open(r"${file}");i.crop((0,0,round(${s.w}*${s.dpr}),round(${s.h}*${s.dpr}))).save(r"${file}")`], { encoding: 'utf8' });
  if (crop.status !== 0) console.error('crop', s.name, crop.stderr.slice(0, 200));
  count += 1;
}
server.close();
console.log(`rendered ${count} shots to ${out}`);
