// Screenshots of the Privacy and Appearance sections of the settings page and of the shield (the
// site information popup), in headless Edge with a throwaway profile. Builds the UI from a source
// folder into a temporary folder (never ui/dist), serves it on localhost with the fake host in
// t3-host.js, and writes PNGs into the output folder.
//   node shots/t3-render.mjs <outDir> [nameFilter] [--before]   (T3_POOL=4 Edge instances at once; existing PNGs are kept unless T3_REDO=1)
// `--before` builds origin/main's ui/src (read with git, never checked out) and replays what that
// build was sent, so the same scenarios can be shot before and after a change. Names are
// `<before|after>-<scenario>-<dark|light>-<100|150>.png`. Never launches Bowser and never reads a
// Bowser profile.
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import postcss from 'postcss';
import tailwind from '@tailwindcss/postcss';

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, '..');
const args = process.argv.slice(2).filter((arg) => !arg.startsWith('--'));
const before = process.argv.includes('--before');
const out = resolve(args[0] ?? join(here, 't3'));
const only = args[1] ? new RegExp(args[1]) : null;
mkdirSync(out, { recursive: true });
const work = mkdtempSync(join(tmpdir(), 'bowser-t3-shots-'));
const dist = join(work, 'dist');

let srcDir = join(ui, 'src');
if (before) {
  // origin/main's sources, read with git into the temporary folder.
  const tar = join(work, 'before.tar');
  const archive = spawnSync('git', ['archive', '--format=tar', '-o', tar, 'origin/main', 'ui/src'], { cwd: join(ui, '..'), encoding: 'utf8' });
  if (archive.status !== 0) throw new Error(`git archive failed: ${archive.stderr}`);
  const unpack = spawnSync('tar', ['-xf', tar, '-C', work], { encoding: 'utf8' });
  if (unpack.status !== 0) throw new Error(`tar failed: ${unpack.stderr}`);
  srcDir = join(work, 'ui', 'src');
}

await esbuild.build({ entryPoints: [join(srcDir, 'main.ts')], bundle: true, format: 'iife', platform: 'browser', target: 'chrome110', outfile: join(dist, 'app.js'), logLevel: 'warning' });
await esbuild.build({ entryPoints: [join(srcDir, 'settings-page.ts')], bundle: true, format: 'esm', platform: 'browser', target: 'chrome110', outdir: dist, entryNames: '[name]', logLevel: 'warning' });
const cssEntry = join(srcDir, 'styles', 'app.css');
const css = await postcss([tailwind({ base: srcDir })]).process(readFileSync(cssEntry, 'utf8'), { from: cssEntry, to: join(dist, 'app.css'), map: false });

// ---- what the fake host sends ----
const fixtures = (name) => JSON.parse(readFileSync(join(ui, 'test', 'fixtures', name), 'utf8'));
const settings = fixtures('settings-messages.json').snap;
const privacy = fixtures('privacy-snapshot.json');
const popup = fixtures('popup-pages.json').siteinfo;
const protection = {
  v: 1, t: 'snap', p: 'protection',
  d: {
    level: 'ads', cookieNotices: true, retention: '3m', websiteAppearance: 'auto', forceDark: false, privateTheme: 'follow', readable: true, guest: false,
    current: null,
    sites: [
      { site: 'example.com', level: 'none', cookies: 'default' },
      { site: 'news.example', level: 'trackers', cookies: 'hide' },
      { site: 'bank.example', level: 'ads', cookies: 'show' },
    ],
    allow: ['example.com', 'news.example'],
    sources: [
      { id: 'bowser-trackers', name: 'Bowser tracker list', kind: 'trackers', enabled: true, origin: 'builtin', url: '', rules: 412, removable: false },
      { id: 'bowser-ads', name: 'Bowser ad list', kind: 'ads', enabled: true, origin: 'builtin', url: '', rules: 598, removable: false },
      { id: 'bowser-cookies', name: 'Bowser cookie notices', kind: 'cookies', enabled: true, origin: 'builtin', url: '', rules: 183, removable: false },
      { id: 'u1', name: 'lists.example', kind: 'cookies', enabled: true, origin: 'url', url: 'https://lists.example/cookie-notices.txt', rules: 2214, removable: true },
      { id: 'u2', name: 'my-filters.txt', kind: 'ads', enabled: false, origin: 'file', url: '', rules: 41, removable: true },
    ],
    blocked: { page: 12, session: 1840 }, result: null, busy: false,
  },
};
const siteinfo = { ...popup, perms: popup.perms, blocker: { state: 'on', n: 7, level: 'trackers', notices: true } };
const siteinfoBefore = { ...popup, blocker: { state: 'on', n: 7 } };
const dataJs = `window.__t3 = ${JSON.stringify({ settings, privacy, protection, siteinfo, siteinfoBefore })};\n`;

const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };
const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://localhost').pathname;
  let body = null;
  if (path === '/dist/app.css') body = css.css;
  else if (path === '/shots/_t3-data.js') body = dataJs;
  else if (path.startsWith('/dist/')) body = readFileSync(join(dist, path.slice(6)));
  else if (path.startsWith('/shots/') && !path.includes('..')) body = readFileSync(join(here, path.slice(7)));
  res.writeHead(body === null ? 404 : 200, { 'content-type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(body ?? '');
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const shots = [];
const add = (scn, view, w, h, scheme, dpr) => shots.push({ scn, view, w, h, scheme, dpr });
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    add('privacy', 'settings', 1280, 900, scheme, dpr);
    add('privacy-sites', 'settings', 1280, 900, scheme, dpr);
    add('privacy-lists', 'settings', 1280, 900, scheme, dpr);
    add('privacy-delete', 'settings', 1280, 900, scheme, dpr);
    add('privacy-delete-armed', 'settings', 1280, 900, scheme, dpr);
    add('appearance', 'settings', 1280, 900, scheme, dpr);
    add('shield', 'popup', 320, 520, scheme, dpr);
  }
}
add('shield-private', 'popup', 320, 420, 'dark', 1);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const stage = before ? 'before' : 'after';
/** Ends Edge and every process it started (a plain kill leaves its children holding the profile). */
const killTree = (child) => { try { spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); } catch { /* already gone */ } };
const todo = shots
  .filter((s) => !(before && ['privacy-sites', 'privacy-lists', 'privacy-delete-armed', 'shield-private'].includes(s.scn)))
  .map((s) => ({ ...s, name: `${stage}-${s.scn}-${s.scheme}-${Math.round(s.dpr * 100)}` }))
  .filter((s) => (!only || only.test(s.name)) && !(process.env.T3_REDO !== '1' && existsSync(join(out, `${s.name}.png`))));
const POOL = Number(process.env.T3_POOL ?? 4);
let next = 0;
const shoot = async (worker) => {
  const profile = join(work, `edge-profile-${worker}`);
  while (next < todo.length) {
    const s = todo[next++];
    const url = `${origin}/shots/t3.html?view=${s.view}&scn=${s.scn}&scheme=${s.scheme}${before ? '&mode=before' : ''}`;
    const file = join(out, `${s.name}.png`);
    // Async: the server above must keep answering while Edge loads the page.
    const status = await new Promise((done) => {
      const child = spawn(edge, [
        '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', '--disable-extensions',
        `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`, `--window-size=${s.w},${s.h}`,
        '--virtual-time-budget=4000', `--screenshot=${file}`, url,
      ], { stdio: 'ignore' });
      // Headless Edge can linger after it has written the picture: take the file as the answer.
      const timer = setTimeout(() => killTree(child), 90_000);
      const watch = setInterval(() => {
        if (existsSync(file) && statSync(file).size > 0) { clearInterval(watch); setTimeout(() => killTree(child), 400); }
      }, 250);
      child.on('exit', (code) => { clearTimeout(timer); clearInterval(watch); done(existsSync(file) ? 0 : code ?? 1); });
    });
    if (status !== 0) console.error(s.name, status);
    else console.log(file);
  }
};
await Promise.all(Array.from({ length: POOL }, (_, worker) => shoot(worker)));
server.close();
try { rmSync(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 }); } catch { /* a lingering Edge process; the folder is in the temp directory */ }
