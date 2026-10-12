// Renders every HTML chrome view from a ui/dist folder in headless Edge, with fake-host.js
// replaying host messages, so CSS changes can be compared pixel for pixel.
//   node render.mjs <distDir> <outDir> [label] [nameFilterRegex]
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [distArg, outArg, label = 'run', only] = process.argv.slice(2);
if (!distArg || !outArg) {
  console.error('usage: node render.mjs <distDir> <outDir> [label]');
  process.exit(2);
}
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, `_site-${label}`);
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
copyFileSync(join(here, 'harness.css'), join(site, 'harness.css'));
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const shots = [];
for (const scheme of ['dark', 'light']) {
  shots.push(
    { name: `toolbar-${scheme}`, view: 'toolbar', scn: 'toolbar', w: 1280, h: 34, scheme },
    { name: `toolbar-narrow-${scheme}`, view: 'toolbar', scn: 'toolbar', w: 700, h: 34, scheme },
    { name: `toolbar-flex-${scheme}`, view: 'toolbar', scn: 'toolbar-flex', w: 900, h: 34, scheme },
    { name: `tabs-${scheme}`, view: 'tabs', scn: 'tabs', w: 1280, h: 30, scheme },
    { name: `tabs-narrow-${scheme}`, view: 'tabs', scn: 'tabs', w: 700, h: 30, scheme },
    { name: `sidebar-${scheme}`, view: 'tabs', scn: 'sidebar', w: 260, h: 420, scheme },
    { name: `rail-${scheme}`, view: 'rail', scn: 'rail', w: 30, h: 420, scheme },
    { name: `popup-${scheme}`, view: 'popup', scn: 'popup', w: 320, h: 360, scheme },
    { name: `panel-${scheme}`, view: 'panel', scn: 'panel', w: 360, h: 240, scheme },
  );
}
for (const dpr of [1.25, 1.5, 2]) {
  shots.push({ name: `toolbar-dark@${dpr}x`, view: 'toolbar', scn: 'toolbar', w: 1280, h: 34, scheme: 'dark', dpr });
  shots.push({ name: `rail-dark@${dpr}x`, view: 'rail', scn: 'rail', w: 30, h: 420, scheme: 'dark', dpr });
}

const base = pathToFileURL(join(site, 'index.html')).href;
for (const s of shots.filter((shot) => !only || new RegExp(only).test(shot.name))) {
  const url = `${base}?view=${s.view}&scn=${s.scn}&scheme=${s.scheme}&w=${s.w}`;
  const file = join(out, `${label}-${s.name}.png`);
  const result = spawnSync(edge, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${join(out, '_edge-profile')}`, `--force-device-scale-factor=${s.dpr ?? 1}`,
    `--window-size=${s.w},${s.h}`, '--virtual-time-budget=1500', `--screenshot=${file}`, url,
  ], { encoding: 'utf8' });
  if (result.status !== 0) console.error(s.name, result.status, result.stderr);
}
console.log(`rendered ${shots.length} shots to ${out}`);
