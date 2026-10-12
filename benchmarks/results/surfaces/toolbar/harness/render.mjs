// Renders the toolbar from a ui/dist folder in headless Edge with fake-host.js.
//   node render.mjs <distDir> <outDir> [filterRegex]
import { copyFileSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [distArg, outArg, only] = process.argv.slice(2);
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
copyFileSync(join(here, 'harness.css'), join(site, 'harness.css'));
writeFileSync(join(site, 'index.html'), readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host.js"></script>\n<script src="app.js" defer></script>'));

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const base = pathToFileURL(join(site, 'index.html')).href;
const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const st of ['idle', 'focus', 'loading', 'http', 'cert', 'internal', 'empty', 'long']) {
    shots.push({ name: `${st}-${scheme}-1280`, st, scheme, w: 1280 });
  }
  for (const w of [500, 800, 1920]) shots.push({ name: `idle-${scheme}-${w}`, st: 'idle', scheme, w });
  shots.push({ name: `focus-${scheme}-500`, st: 'focus', scheme, w: 500 });
  shots.push({ name: `flex-loading-${scheme}-500`, st: 'loading', scheme, w: 500, placed: 0 });
  shots.push({ name: `flex-idle-${scheme}-800`, st: 'idle', scheme, w: 800, placed: 0 });
  for (const dpr of [1.5, 2]) {
    shots.push({ name: `idle-${scheme}-1280@${dpr}x`, st: 'idle', scheme, w: 1280, dpr });
    shots.push({ name: `focus-${scheme}-1280@${dpr}x`, st: 'focus', scheme, w: 1280, dpr });
  }
}
const list = shots.filter((x) => !only || new RegExp(only).test(x.name));
const run = (s, lane) => new Promise((done) => {
  // Re-runs only produce what is missing, so a flaky parallel Edge launch can simply be retried.
  try { if (statSync(join(out, s.name + '.png')).size > 0) { done(); return; } } catch { /* render it */ }
  const url = `${base}?st=${s.st}&scheme=${s.scheme}&w=${s.w}&placed=${s.placed ?? 1}`;
  const child = spawn(edge, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${join(out, '_edge' + lane)}`, `--force-device-scale-factor=${s.dpr ?? 1}`,
    `--window-size=${s.w},34`, '--virtual-time-budget=1500', `--screenshot=${join(out, s.name + '.png')}`, url,
  ], { stdio: 'ignore' });
  // Edge writes the PNG and then lingers; finish as soon as the file is non-empty, then kill the tree.
  const file = join(out, s.name + '.png');
  const started = Date.now();
  const poll = setInterval(() => {
    let size = 0;
    try { size = statSync(file).size; } catch { /* not written yet */ }
    if ((size > 0 && Date.now() - started > 1500) || Date.now() - started > 60000) {
      clearInterval(poll);
      spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }).on('exit', () => done());
    }
  }, 500);
});
// Six lanes, one profile dir each, so parallel Edge instances never share a profile.
const lanes = Array.from({ length: 6 }, (_, lane) => list.filter((_, i) => i % 6 === lane));
await Promise.all(lanes.map(async (items, lane) => { for (const s of items) await run(s, lane); }));
console.log('rendered', list.length);
