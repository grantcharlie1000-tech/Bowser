// Headless screenshots for the T4 Commands thread: the Quick commands palette (empty, typed with a completion,
// filter prefix, calculator), the Ctrl+F1 cheat sheet and the Keyboard / Quick commands settings sections, light
// and dark, at 100% and 150%. Headless Edge only; a local static server feeds it the built ui/dist (or the
// ui/dist committed on origin/main for the "before" run), so nothing touches the desktop or any profile.
//   node render.mjs <outDir> <label: before|after> [nameFilterRegex]
import { createServer } from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..', '..');
const [outArg, label = 'after', only] = process.argv.slice(2);
if (!outArg) {
  console.error('usage: node render.mjs <outDir> <before|after> [filter]');
  process.exit(2);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';

// The site: the UI build under test, the fake host and the fixtures as one script.
const site = mkdtempSync(join(tmpdir(), 't4-site-'));
const files = ['app.js', 'app.css', 'settings-page.js', 'history-page.js', 'index.html'];
if (label === 'before') {
  for (const file of files) {
    const shown = spawnSync('git', ['show', `origin/main:ui/dist/${file}`], { cwd: repo, encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
    if (shown.status === 0) writeFileSync(join(site, file), shown.stdout);
  }
} else {
  for (const file of files) copyFileSync(join(repo, 'ui', 'dist', file), join(site, file));
}
copyFileSync(join(here, 'fake-host.js'), join(site, 'fake-host.js'));
const read = (path) => JSON.parse(readFileSync(join(repo, 'ui', 'test', 'fixtures', path), 'utf8'));
const popupPages = read('popup-pages.json');
const commands = read('commands-snapshot.json');
// A fuller palette than the contract fixture: several of every type, the way Rust would send them.
const sample = [
  ['tab', 'Bowser release notes', 'https://github.com/example/bowser/releases', ''],
  ['tab', 'Daily notes', 'https://notes.example/today', ''],
  ['tab', 'Weather in Leeds', 'https://weather.example/leeds', ''],
  ['closed', 'Rust by Example', 'https://doc.rust-lang.org/rust-by-example/', ''],
  ['workspace', 'Work', '3 tabs', ''],
  ['workspace', 'Home', '1 tab', ''],
  ['command', 'New tab', 'Open a blank tab', 'Ctrl+T'],
  ['command', 'New private tab', 'Browse without saving history, cookies or site data', 'Ctrl+Shift+P'],
  ['command', 'Settings', 'Open browser settings', ''],
  ['command', 'Keyboard shortcuts', 'Show every shortcut', 'Ctrl+F1'],
  ['action', 'Reader view', 'Read this page without the clutter', 'Ctrl+Alt+R'],
  ['action', 'Reload', 'Reload the current page', 'F5'],
  ['bookmark', 'Bowser design notes', 'https://example.org/design  ·  Work', ''],
  ['bookmark', 'The Rust Programming Language', 'https://doc.rust-lang.org/book/  ·  Reading list', ''],
  ['note', 'Shopping', 'milk, tea and bread', ''],
  ['note', 'Release checklist', 'tag the build, publish notes, update pin', ''],
  ['history', 'Rust documentation', 'https://doc.rust-lang.org/', ''],
];
const oldTypes = new Set(['tab', 'command', 'bookmark', 'history']);
const palette = label === 'before'
  ? {
    // origin/main's palette knows four types and none of the new fields.
    v: 1, t: 'palette', tok: 8, kind: 'palette', placeholder: 'Search tabs, bookmarks, history and commands',
    items: sample.filter(([s]) => oldTypes.has(s)).map(([s, label, detail, acc], id) => ({ id, s, label, detail, ...(acc ? { acc } : {}) })),
  }
  : {
    ...popupPages.palette,
    items: sample.map(([s, label, detail, acc], id) => ({ id, s, label, detail, ...(acc ? { acc } : {}) })),
  };
const sheet = {
  ...popupPages.palette,
  tok: 9,
  kind: 'shortcuts',
  placeholder: 'Filter shortcuts',
  mode: 'shortcuts',
  types: [],
  limit: 0,
  items: commands.d.commands
    .filter((c) => c.chords.length > 0)
    .map((c, id) => ({ id, s: 'command', label: c.label, detail: '', acc: c.chords.join('   '), g: c.category })),
};
writeFileSync(join(site, 'data.js'), `window.__T4 = ${JSON.stringify({ settings: read('settings-messages.json').snap, commands, palette, sheet })};\n`);
const page = readFileSync(join(site, 'index.html'), 'utf8').replace('<script src="app.js" defer></script>', '<script src="data.js"></script>\n<script src="fake-host.js"></script>\n<script src="app.js" defer></script>');
writeFileSync(join(site, 'index.html'), page);

const types = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };
const server = createServer((req, res) => {
  const name = new URL(req.url, 'http://x').pathname.replace(/^\/+/, '') || 'index.html';
  const file = join(site, name);
  if (!file.startsWith(site) || !existsSync(file)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const base = `http://127.0.0.1:${server.address().port}/index.html`;

const shots = [];
for (const scheme of ['dark', 'light']) {
  for (const dpr of [1, 1.5]) {
    const d = dpr === 1 ? '' : '@1.5x';
    const add = (name, query, w, h) => shots.push({ name: `${label}-${name}-${scheme}${d}`, query: `${query}&scheme=${scheme}`, w, h, dpr });
    add('palette-empty', 'view=popup&scn=palette', 560, 420);
    add('palette-completion', 'view=popup&scn=palette&q=rea', 560, 420);
    add('palette-prefix', 'view=popup&scn=palette&q=bkm:%20rust', 560, 300);
    add('palette-calc', 'view=popup&scn=palette&q=12*(3%2B4)', 560, 300);
    // The three rows that said "Has no effect yet" although they act (Appearance > Shortcuts).
    add('settings-shortcut-rows', 'view=settings&scn=settings&section=appearance&act=scroll:.sp-row[data-row="shiftRightClickContext"]', 1100, 700);
    if (label === 'after') {
      add('palette-sheet', 'view=popup&scn=sheet', 560, 460);
      add('settings-keyboard', 'view=settings&scn=settings&section=keyboard', 1100, 900);
      add('settings-shortcuts', 'view=settings&scn=settings&section=keyboard&act=scroll:.sp-kb', 1100, 900);
      add('settings-capture', 'view=settings&scn=settings&section=keyboard&act=scroll:.sp-kb-row[data-command="history"]|click:.sp-kb-row[data-command="history"] .sp-kb-add', 1100, 900);
      add('settings-quick', 'view=settings&scn=settings&section=commands', 1100, 900);
      add('settings-chains', 'view=settings&scn=settings&section=commands&act=scroll:.sp-chains|click:.sp-chain-row button[aria-label="Edit Morning"]', 1100, 900);
    }
  }
}
let made = 0;
for (const s of shots.filter((shot) => !only || new RegExp(only).test(shot.name))) {
  const file = join(out, `${s.name}.png`);
  rmSync(file, { force: true });
  const profile = mkdtempSync(join(tmpdir(), 't4-edge-'));
  await new Promise((done) => {
    const child = spawn(edge, [
    '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', `--user-data-dir=${profile}`, `--force-device-scale-factor=${s.dpr}`,
    `--window-size=${s.w},${s.h + 100}`, '--virtual-time-budget=5000', `--screenshot=${file}`, `${base}?${s.query}&w=${s.w}`,
    ], { stdio: 'ignore' });
    const timer = setTimeout(() => { spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); done(); }, 60000);
    child.on('exit', () => { clearTimeout(timer); done(); });
  });
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch { /* temp */ }
  if (existsSync(file) && statSync(file).size > 0) made += 1;
  else console.error('no screenshot', s.name);
}
server.close();
try { rmSync(site, { recursive: true, force: true }); } catch { /* temp */ }
console.log(`rendered ${made} of ${shots.length} shots to ${out}`);
process.exit(0);
