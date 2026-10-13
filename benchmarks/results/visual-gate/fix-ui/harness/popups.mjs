// Renders popup-hosted pages (command palette, site info, hover card) from ui/dist over a busy page
// backdrop, so any see-through shows. Headless Edge, light + dark, 1x and 2x.
//   node popups.mjs <outDir> [nameFilterRegex]
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(here, '../../../../../ui/dist');
const out = resolve(process.argv[2]);
const only = process.argv[3];
const site = join(out, '_site-popups');
mkdirSync(site, { recursive: true });
for (const f of ['app.js', 'app.css']) copyFileSync(join(dist, f), join(site, f));

const PNG = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAL0lEQVR4nGP8z8Dwn4ECwESJ5oEzgGWy+EdkDMOvQRhxODRNQ/wiMBxTw4gwAQAAGGoJRD8D8xsAAAAASUVORK5CYII=';
const THEMES = {
  dark: { bg: 'rgba(32,36,44,0.800)', fill: 'rgba(255,255,255,0.110)', border: 'rgba(95,216,219,0.282)', focusBorder: '#b3c9ce', text: '#ffffff', placeholder: '#dde2e9', completion: '#dde2e9', selection: 'rgba(77,163,255,0.251)', hover: 'rgba(255,255,255,0.078)', progress: '#84c8ff' },
  light: { bg: 'rgba(244,246,248,0.800)', fill: 'rgba(255,255,255,0.314)', border: 'rgba(22,143,152,0.282)', focusBorder: '#4a6d73', text: '#14181d', placeholder: '#4b5159', completion: '#50575f', selection: 'rgba(0,103,192,0.251)', hover: 'rgba(255,255,255,0.314)', progress: '#0067c0' },
};
const commands = [
  ['New tab', 'Open a blank tab', 'Ctrl+T'], ['Close tab', 'Close the current tab', 'Ctrl+W'], ['Settings', 'Open browser settings'],
  ['Toggle notes and tasks', 'Show or hide the productivity sidebar', 'Ctrl+Shift+N'], ['Passwords and passkeys', 'Manage saved credentials', 'Ctrl+,'],
  ['Profiles', 'Switch or manage profiles', 'Ctrl+Shift+P'], ['Add bookmark', 'Save the current page', 'Ctrl+D'], ['Reload', 'Reload the current page', 'Ctrl+R'],
  ['Extensions', 'Manage installed extensions', 'Ctrl+Shift+E'],
];
const palette = {
  v: 1, t: 'palette', tok: 1, kind: 'palette', placeholder: 'Search tabs, bookmarks, history and commands', items: [
    { id: 0, s: 'tab', label: 'Web browser - Wikipedia', detail: 'https://en.wikipedia.org/wiki/Web_browser', fav: PNG },
    { id: 1, s: 'tab', label: '', detail: 'about:blank' },
    { id: 2, s: 'tab', label: 'MDN Web Docs', detail: 'https://developer.mozilla.org/en-US/' },
    ...commands.map(([label, detail, acc], i) => ({ id: 10 + i, s: 'command', label, detail, ...(acc ? { acc } : {}) })),
  ],
};
const siteinfo = {
  v: 1, t: 'siteinfo', tok: 1, kind: 'siteinfo', host: 'en.wikipedia.org', origin: 'https://en.wikipedia.org', security: 'secure', summary: 'Connection is secure',
  perms: [{ id: 1, k: 'camera', label: 'Camera', state: 'default' }, { id: 2, k: 'mic', label: 'Microphone', state: 'default' }, { id: 3, k: 'location', label: 'Location', state: 'allow' }, { id: 4, k: 'notifications', label: 'Notifications', state: 'default' }],
  cookies: { state: 'count', n: 0, clear: false }, blocker: { state: 'on', n: 0 }, tracking: 'balanced', zoom: 100, reset: false, blockedHere: false,
};
const card = { v: 1, t: 'hover', tok: 1, kind: 'card', title: 'Example Domain', host: 'example.com', marker: -1, markers: ['#e5484d', '#f5a524', '#30a46c', '#3e63dd', '#8e4ec6'], sleeping: false, fav: '', rows: [], more: 0 };
const cardFav = { ...card, fav: PNG, marker: 2, title: 'A tab with a rather long title that wraps onto a second line', sleeping: true };
const SCN = {
  palette: { view: 'popup', msg: palette, w: 600, h: 520 },
  'palette-filtered': { view: 'popup', msg: palette, w: 600, h: 300, act: "const i=document.querySelector('.bw-palette-input');i.value='re';i.dispatchEvent(new Event('input',{bubbles:true}));" },
  siteinfo: { view: 'popup', msg: siteinfo, w: 360, h: 520 },
  card: { view: 'hover', msg: card, w: 300, h: 140 },
  'card-fav': { view: 'hover', msg: cardFav, w: 300, h: 160 },
};
const busy = (dark) => `html{background:${dark ? '#f2f2f2' : '#ffffff'};color:#111;font:700 34px/1.15 Georgia,serif}body{background:transparent!important}#bg{position:fixed;inset:0;overflow:hidden;padding:8px;word-break:break-all;z-index:-1}`;
const shots = [];
for (const scheme of ['dark', 'light']) for (const dpr of [1, 2]) for (const name of Object.keys(SCN)) shots.push({ name: `${name}-${scheme}${dpr === 2 ? '@2x' : ''}`, scheme, dpr, scn: name });
const todo = shots.filter((s) => !only || new RegExp(only).test(s.name));
for (const s of todo) {
  const c = SCN[s.scn];
  const dir = join(site, s.name.replace(/[^a-z0-9]/gi, '_'));
  mkdirSync(dir, { recursive: true });
  for (const f of ['app.js', 'app.css']) copyFileSync(join(site, f), join(dir, f));
  const dark = s.scheme === 'dark';
  writeFileSync(join(dir, 'host.js'), `(() => { const state = { t: 'state', scale: 1, solid: true, dark: ${dark}, back: true, forward: false, loading: false, progress: 0, security: 'secure', label: 'x', theme: ${JSON.stringify(THEMES[s.scheme])} };
  const msg = ${JSON.stringify(c.msg)}; const ls = [];
  window.chrome = { webview: { postMessage() {}, addEventListener(t, cb) { if (t === 'message') ls.push(cb); } } };
  window.addEventListener('load', () => { const bg = document.createElement('div'); bg.id = 'bg'; bg.textContent = 'Powering everyone with fast and efficient software. '.repeat(40); document.body.prepend(bg);
    for (const m of [state, msg]) for (const cb of ls) cb({ data: JSON.stringify(m) });
    ${c.act ?? ''} });
})();`);
  writeFileSync(join(dir, 'busy.css'), busy(dark));
  writeFileSync(join(dir, 'index.html'), readFileSync(join(dist, 'index.html'), 'utf8')
    .replace('<link rel="stylesheet" href="app.css">', '<link rel="stylesheet" href="app.css">\n<link rel="stylesheet" href="busy.css">')
    .replace('<script src="app.js" defer></script>', '<script src="host.js"></script>\n<script src="app.js" defer></script>'));
  const file = join(out, `popup-${s.name}.png`);
  rmSync(file, { force: true });
  spawnSync('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', `--user-data-dir=${join(tmpdir(), 'bowser-popups-edge')}`,
    `--force-device-scale-factor=${s.dpr}`, `--window-size=${c.w},${c.h}`, '--virtual-time-budget=2500', `--screenshot=${file}`, `${pathToFileURL(join(dir, 'index.html')).href}?view=${c.view}`], { stdio: 'ignore', timeout: 60000 });
}
console.log(`rendered ${todo.length}`);
