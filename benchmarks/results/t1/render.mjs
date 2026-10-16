// Renders the T1 (tabs) surfaces from a ui/dist folder in headless Edge, light and dark, at 100% and 150%.
//   node render.mjs <distDir> <outDir> <label> <settingsFixtureDir> [nameFilterRegex]
// `label` is "before" (an origin/main dist) or "after" (this branch). The Sessions panel, the tab marks
// and the hover thumbnail did not exist before, so those scenarios only render "after".
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [distArg, outArg, label = 'after', fixtureArg, only] = process.argv.slice(2);
if (!distArg || !outArg || !fixtureArg) {
  console.error('usage: node render.mjs <distDir> <outDir> <label> <settingsFixtureDir> [filter]');
  process.exit(2);
}
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, `_site-${label}`);
mkdirSync(site, { recursive: true });
// The settings page is a second bundle that app.js loads on demand.
for (const f of ['app.js', 'app.css', 'settings-page.js', 'history-page.js']) if (existsSync(join(dist, f))) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'harness', 'fake-host.js'), join(site, 'fake-host.js'));
writeFileSync(join(site, 'harness.css'), 'html.h-dark{background:linear-gradient(90deg,#1b2029,#2a2f3a 60%,#1d2a33)}html.h-light{background:linear-gradient(90deg,#dde3ea,#eef1f4 60%,#d8e2ea)}\n');

// A small page-like JPEG for the hover thumbnail.
const thumbPath = join(out, '_thumb.jpg');
spawnSync('python', ['-c', `
from PIL import Image, ImageDraw
im = Image.new("RGB", (240, 150), (246, 247, 249))
d = ImageDraw.Draw(im)
d.rectangle((0, 0, 240, 22), fill=(40, 90, 160))
d.rectangle((14, 34, 150, 44), fill=(60, 64, 72))
for i in range(5): d.rectangle((14, 58 + i * 14, 226 - (i % 3) * 30, 64 + i * 14), fill=(190, 196, 206))
d.rectangle((164, 34, 226, 90), fill=(224, 228, 236))
im.save(r"${thumbPath}", quality=70)
`], { stdio: 'inherit' });
const thumb = `data:image/jpeg;base64,${readFileSync(thumbPath).toString('base64')}`;
const settings = JSON.parse(readFileSync(join(resolve(fixtureArg), 'settings-messages.json'), 'utf8')).snap.d;
let before = settings;
const oldFixture = join(resolve(fixtureArg), 'settings-messages-before.json');
if (existsSync(oldFixture)) before = JSON.parse(readFileSync(oldFixture, 'utf8')).snap.d;
writeFileSync(join(site, 'fixtures.js'), `window.__t1=${JSON.stringify({ settings, settingsBefore: before, thumb })};\n`);
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="app.css">\n<link rel="stylesheet" href="harness.css">' + (existsSync(join(site, 'settings-page.js')) ? '\n<link rel="modulepreload" href="settings-page.js">' : ''))
  .replace('<script src="app.js" defer></script>', '<script src="fixtures.js"></script>\n<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const shots = [];
const add = (s) => shots.push({ dpr: 1, ...s });
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '' : '@1.5x';
    const strip = (name, scn, w = 1000) => add({ name: `${name}-${scheme}${d}`, view: 'tabs', scn, w, h: 30, scheme, dpr });
    if (label === 'before') {
      strip('tabs', 'tabs-before');
      add({ name: `sidebar-${scheme}${d}`, view: 'tabs', scn: 'sidebar-before', w: 260, h: 340, scheme, dpr });
      add({ name: `settings-${scheme}${d}`, view: 'settings', scn: 'settings-before', w: 1000, h: 1800, scheme, dpr });
      add({ name: `hover-${scheme}${d}`, view: 'hover', scn: 'hover-before', w: 320, h: 200, scheme, dpr });
    } else {
      strip('tabs', 'tabs-marks');
      strip('tabs-bar', 'tabs-bar');
      strip('tabs-ring', 'tabs-ring');
      strip('tabs-scroll', 'tabs-scroll', 700);
      add({ name: `sidebar-${scheme}${d}`, view: 'tabs', scn: 'sidebar-marks', w: 260, h: 340, scheme, dpr });
      add({ name: `settings-${scheme}${d}`, view: 'settings', scn: 'settings-tabs', w: 1000, h: 1800, scheme, dpr });
      add({ name: `sessions-${scheme}${d}`, view: 'dialog&p=sessions', scn: 'sessions', w: 420, h: 480, scheme, dpr });
      add({ name: `sessions-empty-${scheme}${d}`, view: 'dialog&p=sessions', scn: 'sessions-empty', w: 420, h: 300, scheme, dpr });
      add({ name: `hover-${scheme}${d}`, view: 'hover', scn: 'hover-preview', w: 320, h: 360, scheme, dpr });
    }
  }
}

const base = pathToFileURL(join(site, 'index.html')).href;
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const todo = shots.filter((shot) => !only || new RegExp(only).test(shot.name));
let next = 0;
async function worker() {
  while (next < todo.length) {
    const s = todo[next++];
    const file = join(out, `${label}-${s.name}.png`);
    rmSync(file, { force: true });
    const profile = mkdtempSync(join(tmpdir(), 't1-shot-'));
    const url = `${base}?view=${s.view}&scn=${s.scn}&scheme=${s.scheme}&w=${s.w}`;
    let child = null;
    for (let attempt = 0; attempt < 8 && !child; attempt++) {
      // A busy machine sometimes refuses to start a process (EAGAIN/UNKNOWN); wait and try again.
      child = await new Promise((done) => {
        const proc = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--allow-file-access-from-files', '--force-prefers-reduced-motion', '--no-first-run', `--user-data-dir=${profile}`,
          `--force-device-scale-factor=${s.dpr}`, `--window-size=${s.w},${s.h}`, '--virtual-time-budget=5000', `--screenshot=${file}`, url], { stdio: 'ignore' });
        proc.once('error', () => done(null));
        proc.once('spawn', () => done(proc));
      });
      if (!child) await sleep(1500);
    }
    if (!child) { console.error('could not start Edge for', s.name); continue; }
    for (let waited = 0; waited < 40000; waited += 400) {
      await sleep(400);
      if (existsSync(file) && statSync(file).size > 0) { await sleep(300); break; }
    }
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    try { rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch { /* temp */ }
    if (!existsSync(file)) console.error('no screenshot', s.name);
  }
}
await Promise.all(Array.from({ length: Number(process.env.T1_JOBS || 3) }, worker));
console.log(`rendered ${todo.length} shots to ${out}`);
process.exit(0);
