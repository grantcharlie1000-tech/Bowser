// Renders the panel surfaces from a ui/dist folder in headless Edge with a fake host.
//   node render-panels.mjs <distDir> <outDir> [nameFilterRegex]
// Matrix: panel x data (empty|full) x scheme x width/dpr. Output: <outDir>/<panel>-<data>-<scheme>-w<W>[@dpr].png
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [distArg, outArg, only] = process.argv.slice(2);
if (!distArg || !outArg) { console.error('usage: node render-panels.mjs <distDir> <outDir> [filter]'); process.exit(2); }
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'fake-host-panels.js'), join(site, 'fake-host-panels.js'));
writeFileSync(join(site, 'harness.css'), `
html.h-dark { background: #20242c; }
html.h-light { background: #f4f6f8; }
html.h-reveal .pn-row:nth-of-type(3) { background: var(--bw-hover); }
html.h-reveal .pn-row:nth-of-type(3) .pn-actions { opacity: 1; }
html.h-reveal .pn-row:nth-of-type(3) .pn-meta, html.h-reveal .pn-row:nth-of-type(3) .pn-peek { opacity: 0; }
`);
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="app.css">\n<link rel="stylesheet" href="harness.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host-panels.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const base = pathToFileURL(join(site, 'index.html')).href;
const panelsView = [['history', 'panel'], ['bookmarks', 'panel'], ['notes', 'dialog']];
const shots = [];
for (const scheme of ['light', 'dark']) {
  for (const data of ['full', 'empty']) {
    for (const [p, view] of panelsView) {
      for (const w of [280, 360, 480]) shots.push({ p, view, data, scheme, w, h: 640, dpr: 1 });
      for (const dpr of [1.5, 2]) shots.push({ p, view, data, scheme, w: 360, h: 640, dpr });
    }
    for (const p of ['settings', 'privacy']) {
      for (const w of [560, 900]) shots.push({ p, view: 'dialog', data, scheme, w, h: 700, dpr: 1 });
      for (const dpr of [1.5, 2]) shots.push({ p, view: 'dialog', data, scheme, w: 900, h: 700, dpr });
    }
  }
}
// Headless Edge keeps running for minutes after writing the screenshot, so poll for the file and
// kill the process tree instead of waiting for exit. Several shots run at once, each with its own profile.
import { existsSync, statSync, rmSync } from 'node:fs';
import { spawn } from 'node:child_process';
const suffix = process.env.HARNESS_SUFFIX ?? '';
const todo = shots.map((s) => {
  const base = `${s.p}-${s.data}-${s.scheme}-w${s.w}${s.dpr === 1 ? '' : `@${s.dpr}x`}`;
  return { s, base, name: `${base}${suffix}` };
}).filter(({ base }) => !only || new RegExp(only).test(base));
let next = 0;
let done = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function worker(id) {
  while (next < todo.length) {
    const { s, name } = todo[next++];
    const file = join(out, `${name}.png`);
    if (existsSync(file) && !process.env.HARNESS_FORCE) { done += 1; continue; }
    const profileName = `_edge-profile-${process.pid}-${id}`;
    const profile = join(out, profileName);
    const url = `${base}?view=${s.view}&p=${s.p}&scn=${s.p}-${s.data}&scheme=${s.scheme}&w=${s.w}${process.env.HARNESS_QUERY ?? ''}`;
    const child = spawn(edge, [
      '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
      `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`,
      `--window-size=${s.w},${s.h}`, '--virtual-time-budget=2000', `--screenshot=${file}`, url,
    ], { stdio: 'ignore' });
    let last = -1;
    for (let i = 0; i < 300; i += 1) {
      await sleep(200);
      if (existsSync(file)) {
        const size = statSync(file).size;
        if (size > 0 && size === last) break;
        last = size;
      }
    }
    spawnSync('wmic', ['process', 'where', `CommandLine like '%${profileName}%' and Name='msedge.exe'`, 'call', 'terminate'], { stdio: 'ignore' });
    child.kill();
    await sleep(150);
    try { rmSync(profile, { recursive: true, force: true }); } catch { /* reused by this worker's next shot */ }
    if (!existsSync(file)) console.error('missing', name);
    done += 1;
  }
}
await Promise.all([0, 1].map(worker));
console.log(`rendered ${done} shots to ${out}`);
process.exit(0);
