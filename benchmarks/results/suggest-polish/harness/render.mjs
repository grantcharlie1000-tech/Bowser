// Renders the omnibox dropdown (hover view) from ui/dist in headless Edge with fake-host.js, in
// light and dark at 100%, 150% and 200%, plus a narrow width and a long search query.
//   node render.mjs <distDir> <outDir> [label] [--dom]
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const dom = args.includes('--dom');
const [distArg, outArg, label = 'harness'] = args.filter((a) => a !== '--dom');
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, `_site-${label}`);
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const base = pathToFileURL(join(site, 'index.html')).href;
const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5, 2]) shots.push({ name: `${scheme}@${dpr}x`, q: `scheme=${scheme}&w=624`, w: 624, h: 280, dpr });
  shots.push({ name: `${scheme}-narrow`, q: `scheme=${scheme}&w=480`, w: 480, h: 280, dpr: 1 });
  shots.push({ name: `${scheme}-search`, q: `scheme=${scheme}&w=480&set=search&sel=0`, w: 480, h: 60, dpr: 1 });
}
for (const s of dom ? shots.slice(0, 1) : shots) {
  const url = `${base}?view=hover&${s.q}`;
  const common = ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${join(out, '_edge-profile')}`, `--force-device-scale-factor=${s.dpr}`, `--window-size=${s.w},${s.h}`,
    '--virtual-time-budget=1500'];
  if (dom) {
    const r = spawnSync(edge, [...common, '--dump-dom', url], { encoding: 'utf8' });
    console.log(r.stdout);
    continue;
  }
  const file = join(out, `${label}-${s.name}.png`);
  const r = spawnSync(edge, [...common, `--screenshot=${file}`, url], { encoding: 'utf8' });
  if (r.status !== 0) console.error(s.name, r.status, r.stderr);
}
if (!dom) console.log(`rendered ${shots.length} shots to ${out}`);
