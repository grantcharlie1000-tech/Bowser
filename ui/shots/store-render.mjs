// Screenshots of the extension installation page (bowser://store over ?view=settings) in every
// state, in headless Edge with a throwaway profile. Builds the UI from src/ into a temporary folder
// (never ui/dist), serves it on localhost with the fake host in store-host.js, and writes PNGs to
// shots/store/<label>/ (label: `before` for the old source, `after` for the new one).
//   node shots/store-render.mjs [label] [nameFilter]   (nameFilter: a regular expression)
// Labels starting `icon-` render the package-icon set instead: every icon variant in review and
// ready, dark, light and dark with the red accent, plus idle.
// Never launches Bowser, never reads a Bowser profile and never touches the user's desktop.
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, extname, join } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import postcss from 'postcss';
import tailwind from '@tailwindcss/postcss';
import { storeIcons } from './store-icons.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const out = join(here, 'store', process.argv[2] ?? 'after');
mkdirSync(out, { recursive: true });
const work = mkdtempSync(join(tmpdir(), 'bowser-store-shots-'));
const dist = join(work, 'dist');

await esbuild.build({ entryPoints: [join(srcDir, 'main.ts')], bundle: true, format: 'iife', platform: 'browser', target: 'chrome110', outfile: join(dist, 'app.js'), logLevel: 'warning' });
await esbuild.build({ entryPoints: [join(srcDir, 'settings-page.ts')], bundle: true, format: 'esm', platform: 'browser', target: 'chrome110', outdir: dist, entryNames: '[name]', logLevel: 'warning' });
const cssEntry = join(srcDir, 'styles', 'app.css');
const css = await postcss([tailwind({ base: srcDir })]).process(readFileSync(cssEntry, 'utf8'), { from: cssEntry, to: join(dist, 'app.css'), map: false });

const settingsSnap = JSON.parse(readFileSync(join(here, '..', 'test', 'fixtures', 'settings-messages.json'), 'utf8')).snap;
const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };
const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://localhost').pathname;
  let body = null;
  try {
    if (path === '/dist/app.css') body = css.css;
    else if (path === '/shots/_store-icons.js') body = `window.__storeIcons = ${JSON.stringify(storeIcons())};
`;
    else if (path === '/shots/_settings-snap.js') body = `window.__settingsSnap = ${JSON.stringify(settingsSnap)};
`;
    else if (path.startsWith('/dist/')) body = readFileSync(join(dist, path.slice(6)));
    else if (path.startsWith('/shots/') && !path.includes('..')) body = readFileSync(join(here, path.slice(7)));
  } catch { body = null; }
  res.writeHead(body === null ? 404 : 200, { 'content-type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(body ?? '');
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const STATES = ['idle', 'downloading', 'downloading-determinate', 'verifying', 'review', 'installing', 'ready', 'ready-restart', 'error', 'error-download', 'error-runtime', 'cancelled'];
const shots = [];
const label = process.argv[2] ?? 'after';
if (label.startsWith('icon-')) {
  for (const icon of ['brand', 'tile', 'dark', 'white', 'broken', 'none']) for (const scn of ['review', 'ready']) for (const [scheme, accent] of [['dark', 'stock'], ['light', 'stock'], ['dark', 'red']]) {
    shots.push({ name: `${icon}-${scn}-${scheme}${accent === 'red' ? '-red-accent' : ''}`, scn, scheme, accent, icon, w: 1440, h: 900 });
  }
  for (const scheme of ['dark', 'light']) shots.push({ name: `idle-${scheme}`, scn: 'idle', scheme, w: 1440, h: 900 });
  shots.push({ name: 'brand-installing-dark', scn: 'installing', scheme: 'dark', icon: 'brand', w: 1440, h: 900 });
  shots.push({ name: 'brand-review-dark@2x', scn: 'review', scheme: 'dark', icon: 'brand', w: 720, h: 450, dpr: 2 });
  shots.push({ name: 'white-review-light@2x', scn: 'review', scheme: 'light', icon: 'white', w: 720, h: 450, dpr: 2 });
} else {
for (const scheme of ['dark', 'light']) for (const scn of STATES) shots.push({ name: `${scn}-${scheme}`, scn, scheme, w: 1440, h: scn === 'review' ? 1500 : 900 });
for (const scn of ['downloading', 'review', 'error', 'ready']) shots.push({ name: `${scn}-dark-red-accent`, scn, scheme: 'dark', accent: 'red', w: 1440, h: scn === 'review' ? 1500 : 900 });
for (const scn of ['review', 'error']) shots.push({ name: `${scn}-narrow-light`, scn, scheme: 'light', w: 480, h: scn === 'review' ? 1900 : 900 });
for (const scheme of ['dark', 'light']) shots.push({ name: `error-download-details-open-${scheme}`, scn: 'error-download', scheme, w: 1440, h: 900, act: 'tech' });
shots.push({ name: 'review-sticky-actions-dark', scn: 'review', scheme: 'dark', w: 1440, h: 900 });
shots.push({ name: 'error-dark@1.5x', scn: 'error', scheme: 'dark', w: 1280, h: 800, dpr: 1.5 });
}

const only = process.argv[3] ? new RegExp(process.argv[3]) : null;
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const profile = join(work, 'edge-profile');
for (const s of shots.filter((shot) => !only || only.test(shot.name))) {
  const url = `${origin}/shots/store.html?view=settings&scn=${s.scn}&scheme=${s.scheme}&accent=${s.accent ?? 'stock'}&act=${s.act ?? ''}&icon=${s.icon ?? 'brand'}`;
  const file = join(out, `${s.name}.png`);
  // Async: the server above must keep answering while Edge loads the page.
  const status = await new Promise((done) => {
    const child = spawn(edge, [
      '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', '--disable-extensions',
      `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr ?? 1}`, `--window-size=${s.w},${s.h}`,
      '--virtual-time-budget=4000', `--screenshot=${file}`, url,
    ], { stdio: 'ignore' });
    const timer = setTimeout(() => child.kill(), 60_000);
    child.on('exit', (code) => { clearTimeout(timer); done(code); });
  });
  if (status !== 0) console.error(s.name, status);
  else console.log(file);
}
server.close();
rmSync(work, { recursive: true, force: true });
