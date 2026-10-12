// Renders the menu popup (ui/dist) in headless Edge with menus-host.js standing in for the host.
//   node render-menus.mjs <distDir> <outDir> [nameFilterRegex]
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { deflateSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [distArg, outArg, only] = process.argv.slice(2);
const dist = resolve(distArg);
const out = resolve(outArg);
const site = join(out, '_site');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));
copyFileSync(join(here, 'menus-host.js'), join(site, 'menus-host.js'));

// A valid 16x16 PNG favicon (a rounded blue tile) so the favicon path renders for real.
function crc32(buf) { let c, crc = ~0; for (const b of buf) { c = (crc ^ b) & 255; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; crc = (crc >>> 8) ^ c; } return ~crc >>> 0; }
function chunk(type, data) { const t = Buffer.from(type); const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([t, data]))); return Buffer.concat([len, t, data, crc]); }
const N = 16; const raw = Buffer.alloc((N * 4 + 1) * N);
for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) { const o = y * (N * 4 + 1) + 1 + x * 4; const edge = x < 1 || y < 1 || x > 14 || y > 14; const a = edge ? 0 : 255; raw[o] = 0x30; raw[o + 1] = 0x90 + (y * 6); raw[o + 2] = 0xe0; raw[o + 3] = a; }
const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(N, 0); ihdr.writeUInt32BE(N, 4); ihdr[8] = 8; ihdr[9] = 6;
const png = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
writeFileSync(join(site, 'png.js'), `window.__PNG='data:image/png;base64,${png.toString('base64')}';`);

const host = readFileSync(join(site, 'menus-host.js'), 'utf8').replace(/const png = '[^']*';/, "const png = window.__PNG;");
writeFileSync(join(site, 'menus-host.js'), host);
writeFileSync(join(site, 'harness.css'), `html.h-dark{background:linear-gradient(90deg,#1b2029,#2a2f3a 60%,#1d2a33)}html.h-light{background:linear-gradient(90deg,#dde3ea,#eef1f4 60%,#d8e2ea)}`);
const html = readFileSync(join(dist, 'index.html'), 'utf8')
  .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="harness.css">\n<link rel="stylesheet" href="app.css">')
  .replace('<script src="app.js" defer></script>', '<script src="png.js"></script>\n<script src="menus-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), html);

const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
// [name, menu, w, h, extra query]
const menus = [
  ['main', 'main', 360, 640, ''],
  ['main-profile', 'main', 360, 520, '&drill=2'],
  ['tab-context', 'tab', 320, 620, ''],
  ['workspaces', 'workspaces', 320, 260, ''],
  ['profile', 'profile', 340, 440, ''],
  ['site-info', 'siteinfo', 380, 640, ''],
  ['extensions', 'extensions', 340, 240, ''],
  ['open-tabs', 'opentabs', 380, 200, ''],
  ['tall-scroll', 'tall', 340, 360, '&key=End'],
];
const base = pathToFileURL(join(site, 'index.html')).href;
let count = 0;
for (const scheme of ['dark', 'light']) for (const dpr of [1, 1.5, 2]) for (const [name, menu, w, h, extra] of menus) {
  const label = `${name}-${scheme}@${dpr}x`;
  if (only && !new RegExp(only).test(label)) continue;
  const file = join(out, `${label}.png`);
  const r = spawnSync(edge, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${join(tmpdir(), "bw-menu-edge-" + process.pid + "-" + count)}`, `--force-device-scale-factor=${dpr}`, `--window-size=${w},${h}`,
    '--virtual-time-budget=1500', `--screenshot=${file}`, `${base}?view=popup&menu=${menu}&scheme=${scheme}${extra}`], { encoding: 'utf8', timeout: 25000, killSignal: 'SIGKILL' });
  if (r.status !== 0) console.error(label, r.status, r.stderr);
  count++;
}
console.log(`rendered ${count} shots to ${out}`);
