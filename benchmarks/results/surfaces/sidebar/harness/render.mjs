// node render.mjs <distDir> <outDir> [nameRegex]   renders the sidebar matrix with headless Edge.
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, rmSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const fnd = join(here, '..', '..', '..', 'foundation', 'harness');
const [distArg, outArg, only] = process.argv.slice(2);
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
copyFileSync(join(fnd, 'harness.css'), join(site, 'harness.css'));
writeFileSync(join(site, 'index.html'), readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host.js"></script>\n<script src="app.js" defer></script>'));
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const base = pathToFileURL(join(site, 'index.html')).href;
const cases = [];
for (const scheme of ['dark', 'light']) {
  for (const w of [180, 240, 320]) {
    cases.push({ name: `${scheme}-w${w}-1tab`, q: 'n=1', w, h: 300, scheme });
    cases.push({ name: `${scheme}-w${w}-8tabs-groups-pinned`, q: 'n=8&pinned=2&groups=1', w, h: 420, scheme });
    cases.push({ name: `${scheme}-w${w}-40tabs`, q: 'n=40&pinned=3&groups=2&active=20', w, h: 520, scheme });
  }
  cases.push({ name: `${scheme}-w240-empty`, q: 'n=0', w: 240, h: 240, scheme });
  cases.push({ name: `${scheme}-w240-collapsed`, q: 'n=8&pinned=1&groups=2&collapsed=1', w: 240, h: 420, scheme });
  cases.push({ name: `${scheme}-w240-nofav`, q: 'n=8&fav=0&ws=g:globe', w: 240, h: 360, scheme });
}
for (const scheme of ['dark', 'light']) cases.push({ name: `${scheme}-w240-focus`, q: 'n=8&pinned=1&groups=1&focus=3', w: 240, h: 360, scheme });
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1.5, 2]) cases.push({ name: `${scheme}-w240-8tabs-groups-pinned@${dpr}x`, q: 'n=8&pinned=2&groups=1', w: 240, h: 420, scheme, dpr });
}
for (const ws of ['briefcase', 'home', 'book', 'cart', 'game', 'music', 'heart', 'star', 'bolt', 'globe', 'leaf']) {
  cases.push({ name: `ws-${ws}`, q: `n=1&ws=g:${ws}&wsname=${ws}`, w: 240, h: 96, scheme: 'dark' });
}
const todo = cases.filter((x) => !only || new RegExp(only).test(x.name));
const run = (c, slotNo) => new Promise((done) => {
  const slot = `${slotNo}-${c.name.replace(/[^a-z0-9]/gi, '_')}`;
  const file = join(out, `${c.name}.png`);
  const url = `${base}?view=tabs&${c.q}&scheme=${c.scheme}&w=${c.w}`;
  const p = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${join(tmpdir(), `bowser-sidebar-edge-${slot}`)}`, `--force-device-scale-factor=${c.dpr ?? 1}`, `--window-size=${c.w},${c.h}`,
    '--virtual-time-budget=2000', `--screenshot=${file}`, url], { stdio: 'ignore' });
  rmSync(file, { force: true });
  let last = -1;
  let stable = 0;
  const timer = setInterval(() => {
    const size = existsSync(file) ? statSync(file).size : -1;
    stable = size > 0 && size === last ? stable + 1 : 0;
    last = size;
    if (stable >= 2) { clearInterval(timer); spawnSync('powershell', ['-NoProfile', '-Command', `Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like '*bowser-sidebar-edge-${slot}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`], { stdio: 'ignore' }); }
  }, 500);
  p.on('exit', () => { clearInterval(timer); done(); });
});
await Promise.all(Array.from({ length: 6 }, async (_, slot) => {
  for (let i = slot; i < todo.length; i += 6) await run(todo[i], slot);
}));
console.log('done', todo.length);
