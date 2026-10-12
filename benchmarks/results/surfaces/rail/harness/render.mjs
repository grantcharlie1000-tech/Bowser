// Renders the rail and customize panel (light/dark, 100/150/200%, short and tall) from ui/dist.
//   node render.mjs <outDir> [nameFilterRegex]
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(here, '../../../../../ui/dist');
const out = resolve(process.argv[2] ?? join(here, '..'));
const only = process.argv[3];
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
copyFileSync(join(here, 'harness.css'), join(site, 'harness.css'));
writeFileSync(join(site, 'index.html'), readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="fake-host.js"></script>\n<script src="app.js" defer></script>'));

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5, 2]) {
    if (process.env.QUICK && dpr !== 1) continue;
    const d = dpr === 1 ? '' : `@${dpr}x`;
    shots.push({ name: `rail-${scheme}${d}`, view: 'rail', scn: 'rail', w: 30, h: 420, scheme, dpr });
    shots.push({ name: `rail-short-${scheme}${d}`, view: 'rail', scn: 'rail-many', w: 30, h: 480, scheme, dpr });
    shots.push({ name: `customize-${scheme}${d}`, view: 'panel', scn: 'customize', w: 380, h: 900, scheme, dpr });
  }
  shots.push({ name: `rail-overflow-${scheme}`, view: 'rail', scn: 'rail-many', w: 30, h: 300, scheme, dpr: 2 });
  shots.push({ name: `rail-tall-${scheme}`, view: 'rail', scn: 'rail', w: 30, h: 900, scheme, dpr: 1 });
  shots.push({ name: `customize-short-${scheme}`, view: 'panel', scn: 'customize', w: 380, h: 480, scheme, dpr: 1 });
}
// Headless Edge has a minimum window width, so every view is shown in an exact-size iframe and the
// screenshot is cropped to it (Pillow).
writeFileSync(join(site, 'frame.html'), '<!doctype html><meta charset="utf-8"><body style="margin:0;background:#888">' +
  '<script>const p=new URLSearchParams(location.search);const f=document.createElement("iframe");' +
  'f.style.cssText=`border:0;display:block;width:${p.get("w")}px;height:${p.get("h")}px`;' +
  'f.src="index.html?"+p.get("q");document.body.append(f);</script>');
const base = pathToFileURL(join(site, 'frame.html')).href;
// Edge writes the PNG and then often lingers holding its profile, so each shot gets a throwaway
// profile, and the whole process tree is killed as soon as the file is complete.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (const s of shots.filter((x) => !only || new RegExp(only).test(x.name))) {
  const q = encodeURIComponent(`view=${s.view}&scn=${s.scn}&scheme=${s.scheme}&w=${s.w}`);
  const url = `${base}?w=${s.w}&h=${s.h}&q=${q}`;
  const file = join(out, s.name + '.png');
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 'rail-shot-'));
  const child = spawn(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run',
    `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`,
    `--window-size=${Math.max(s.w, 600)},${Math.max(s.h, 200) + 100}`, '--virtual-time-budget=1200', `--screenshot=${file}`, url],
  { stdio: 'ignore' });
  for (let waited = 0; waited < 90000; waited += 500) {
    await sleep(500);
    if (existsSync(file) && statSync(file).size > 0) { await sleep(300); break; }
  }
  spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F']);
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch { /* temp dir; Windows may still hold a handle */ }
  if (!existsSync(file)) { console.error('no screenshot', s.name); continue; }
  const crop = spawnSync('python', ['-c', `from PIL import Image;i=Image.open(r"${file}");i.crop((0,0,round(${s.w}*${s.dpr}),round(${s.h}*${s.dpr}))).save(r"${file}")`], { encoding: 'utf8' });
  if (crop.status !== 0) console.error('crop', s.name, crop.stderr);
}
console.log('done');
