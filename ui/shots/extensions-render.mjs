// Screenshots of the settings page's Extensions section and the toolbar's extensions button, in
// headless Edge with a throwaway profile. Builds the UI from src/ into a temporary folder (never
// ui/dist), serves it on localhost with the fake host in extensions-host.js, and writes PNGs here.
//   node shots/extensions-render.mjs [outDir] [nameFilter]   (nameFilter: a regular expression; only those frames)
// Never launches Bowser and never reads a Bowser profile.
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import postcss from 'postcss';
import tailwind from '@tailwindcss/postcss';

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, '..');
const srcDir = join(ui, 'src');
const out = resolve(process.argv[2] ?? join(here, 'extensions'));
mkdirSync(out, { recursive: true });
const work = mkdtempSync(join(tmpdir(), 'bowser-ext-shots-'));
const dist = join(work, 'dist');

await esbuild.build({ entryPoints: [join(srcDir, 'main.ts')], bundle: true, format: 'iife', platform: 'browser', target: 'chrome110', outfile: join(dist, 'app.js'), logLevel: 'warning' });
await esbuild.build({ entryPoints: [join(srcDir, 'settings-page.ts')], bundle: true, format: 'esm', platform: 'browser', target: 'chrome110', outdir: dist, entryNames: '[name]', logLevel: 'warning' });
const cssEntry = join(srcDir, 'styles', 'app.css');
const css = await postcss([tailwind({ base: srcDir })]).process(readFileSync(cssEntry, 'utf8'), { from: cssEntry, to: join(dist, 'app.css'), map: false });
const settingsSnap = JSON.parse(readFileSync(join(ui, 'test', 'fixtures', 'settings-messages.json'), 'utf8')).snap;

const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };
const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://localhost').pathname;
  let body = null;
  if (path === '/dist/app.css') body = css.css;
  else if (path === '/shots/_settings-snap.js') body = `window.__settingsSnap = ${JSON.stringify(settingsSnap)};\n`;
  else if (path.startsWith('/dist/')) body = readFileSync(join(dist, path.slice(6)));
  else if (path.startsWith('/shots/') && !path.includes('..')) body = readFileSync(join(here, path.slice(7)));
  res.writeHead(body === null ? 404 : 200, { 'content-type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(body ?? '');
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const shots = [];
for (const scheme of ['dark', 'light']) {
  shots.push({ name: `settings-extensions-list-${scheme}`, view: 'settings', scn: 'details', w: 1280, h: 900, scheme });
  shots.push({ name: `toolbar-extensions-one-${scheme}`, view: 'toolbar', scn: 'toolbar-one', w: 1280, h: 34, scheme });
  shots.push({ name: `toolbar-extensions-both-${scheme}`, view: 'toolbar', scn: 'toolbar-both', w: 1280, h: 34, scheme });
}
for (const scn of ['confirm', 'busy', 'empty', 'error', 'loading']) shots.push({ name: `settings-extensions-${scn}-dark`, view: 'settings', scn, w: 1280, h: 900, scheme: 'dark' });
shots.push({ name: 'settings-extensions-narrow-dark', view: 'settings', scn: 'list', w: 520, h: 900, scheme: 'dark' });
shots.push({ name: 'toolbar-extensions-menu-dark', view: 'popup', scn: 'menu', w: 300, h: 190, scheme: 'dark' });
shots.push({ name: 'settings-extensions-list-dark@1.5x', view: 'settings', scn: 'details', w: 1280, h: 900, scheme: 'dark', dpr: 1.5 });

const only = process.argv[3] ? new RegExp(process.argv[3]) : null;
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const profile = join(work, 'edge-profile');
for (const s of shots.filter((shot) => !only || only.test(shot.name))) {
  const url = `${origin}/shots/extensions.html?view=${s.view}&scn=${s.scn}&scheme=${s.scheme}&w=${s.w}`;
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
