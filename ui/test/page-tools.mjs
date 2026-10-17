// Page tools (thread T7): the settings section's pure rules and wire contract, and the two page scripts
// run for real in headless Edge: spatial navigation (src/spatial.js) and the reader view's typography
// options (src/reader/reader.js). The Edge tests skip themselves when Edge is not installed.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const bundle = (entry) => {
  const out = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', entry)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};
globalThis.window = { chrome: { webview: { postMessage() {}, addEventListener() {} } } };
const logic = await bundle('page-tools-logic.ts');
const page = await bundle('settings-page-logic.ts');

const defaults = JSON.parse(readFileSync(join(here, 'fixtures', 'page-tools-snapshot.json'), 'utf8'));

test('the snapshot Rust writes parses, and anything partial or extended is refused', () => {
  const parsed = logic.parsePageToolsSnapshot(defaults.d);
  assert.ok(parsed);
  assert.equal(parsed.s.pt_capture_name, '{host} {date} {time}');
  assert.equal(parsed.s.pt_reader_line, 16);
  assert.equal(logic.parsePageToolsSnapshot({ ...defaults.d, extra: 1 }), null);
  assert.equal(logic.parsePageToolsSnapshot({ ...defaults.d, s: { ...defaults.d.s, pt_zoom_scope: 'window' } }), null);
  assert.equal(logic.parsePageToolsSnapshot({ ...defaults.d, s: { ...defaults.d.s, pt_reader_size: 99 } }), null);
  const { pt_spatial_nav: _drop, ...partial } = defaults.d.s;
  assert.equal(logic.parsePageToolsSnapshot({ ...defaults.d, s: partial }), null);
});

test('a change is one key and value, and a value Rust would refuse never leaves the page', () => {
  assert.deepEqual(logic.setFields('pt_spatial_nav', true), { a: 'ptset', text: 'pt_spatial_nav=true' });
  assert.deepEqual(logic.setFields('pt_reader_line', 20), { a: 'ptset', text: 'pt_reader_line=20' });
  assert.equal(logic.setFields('pt_reader_line', 11), null);
  assert.equal(logic.setFields('pt_reader_size', 13), null);
  assert.equal(logic.setFields('pt_reader_width', 121), null);
  assert.equal(logic.setFields('pt_capture_dir', '\\\\server\\share'), null);
  assert.equal(logic.setFields('pt_capture_dir', 'C:\\a\\..\\b'), null);
  assert.deepEqual(logic.setFields('pt_capture_dir', 'D:\\Shots'), { a: 'ptset', text: 'pt_capture_dir=D:\\Shots' });
  assert.deepEqual(logic.setFields('pt_capture_dir', ''), { a: 'ptset', text: 'pt_capture_dir=' });
  assert.equal(logic.setFields('pt_capture_name', '{host}/{date}'), null);
  assert.equal(logic.setFields('pt_capture_name', '{nope}'), null);
  assert.equal(logic.setFields('pt_capture_name', '{host'), null);
  assert.deepEqual(logic.setFields('pt_capture_name', '{title} - {mode}'), { a: 'ptset', text: 'pt_capture_name={title} - {mode}' });
  assert.equal(logic.setFields('dd_max_results', 3), null, 'not this section\'s key');
});

test('Page tools is one short block: a few core rows and the rest behind More options', () => {
  const rows = page.ROWS.filter((row) => row.section === 'pagetools');
  assert.deepEqual(rows.map((row) => row.id).sort(), logic.PAGE_TOOLS_ROWS.map((row) => row.id).sort());
  const core = rows.filter((row) => page.rowTier(row) === 'core').map((row) => row.id);
  assert.deepEqual(core.sort(), ['ptCaptureDir', 'ptCaptureName', 'ptSpatial', 'ptWheelZoom', 'ptZoomScope']);
  assert.ok(rows.filter((row) => page.rowTier(row) === 'more').length >= 8);
  assert.equal(rows.filter((row) => page.rowTier(row) === 'later').length, 0, 'no row is a dead toggle');
  assert.ok(page.AREAS.some((area) => area.key === 'pagetools'));
});

// ---- headless Edge ----

const EDGE = [
  process.env.BOWSER_TEST_EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
].find((path) => path && existsSync(path));
const skip = EDGE ? false : 'Microsoft Edge is not installed';

/** Loads `html` with `driver` (a script body that fills `out`) and returns the report it left in a <meta>. */
function runPage(html, driver) {
  const dir = mkdtempSync(join(tmpdir(), 'bowser-pt-test-'));
  try {
    const wrapped = `(function(){var out={errors:[]};try{${driver}}catch(e){out.errors.push(String(e&&e.stack||e));}var m=document.createElement('meta');m.name='pt-out';m.content=encodeURIComponent(JSON.stringify(out));document.head.appendChild(m);})();`;
    const file = join(dir, 'page.html');
    writeFileSync(file, html.replace('</body>', () => `<script>${wrapped}</script></body>`));
    const profile = join(dir, 'profile');
    mkdirSync(profile);
    const result = spawnSync(EDGE, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${profile}`, '--window-size=1000,800', '--dump-dom', pathToFileURL(file).href], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, timeout: 60000 });
    const match = /name="pt-out" content="([^"]*)"/.exec(result.stdout ?? '');
    assert.ok(match, `no report (exit ${result.status}) ${String(result.stderr).slice(0, 300)}`);
    const report = JSON.parse(decodeURIComponent(match[1]));
    assert.deepEqual(report.errors, []);
    return report;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const SPATIAL = readFileSync(join(root, 'src', 'spatial.js'), 'utf8').trim().replace(/;$/, '');

const GRID = `<!doctype html><meta charset=utf-8><style>body{margin:0}a{position:absolute;display:block;width:80px;height:30px;background:#ccc}</style><body>
<a id=a1 href="https://example.test/1" style="left:50px;top:50px">1</a><a id=a2 href="https://example.test/2" style="left:250px;top:50px">2</a>
<a id=a3 href="https://example.test/3" style="left:50px;top:200px">3</a><a id=a4 href="https://example.test/4" style="left:250px;top:200px">4</a>
<a id=hid href="https://example.test/h" style="left:150px;top:120px;display:none">h</a><a id=nohref style="left:150px;top:120px">x</a>
<input id=box style="position:absolute;left:50px;top:400px"><textarea id=ta style="position:absolute;left:300px;top:400px"></textarea></body>`;

const keys = `
function press(key, mods) { var e = new KeyboardEvent('keydown', Object.assign({ key: key, bubbles: true, cancelable: true }, mods || {})); var r = document.dispatchEvent ? window.dispatchEvent(e) : true; return { handled: e.defaultPrevented }; }
function at() { var a = document.activeElement; return a && a.id ? a.id : ''; }
`;

test('spatial navigation: Shift+arrows move between links, plain arrows and text fields are left alone', { skip }, () => {
  const report = runPage(GRID, `${keys}
    var R = ${SPATIAL};
    out.installedAgain = (function(){ return true; })();
    out.noShift = press('ArrowDown', {}); out.noShiftAt = at();
    out.first = press('ArrowDown', { shiftKey: true }); out.firstAt = at();
    press('ArrowRight', { shiftKey: true }); out.right = at();
    press('ArrowDown', { shiftKey: true }); out.down = at();
    press('ArrowLeft', { shiftKey: true }); out.left = at();
    press('ArrowUp', { shiftKey: true }); out.up = at();
    out.marked = document.querySelectorAll('[data-bw-spatial]').length + ':' + (document.activeElement.hasAttribute('data-bw-spatial') ? 'on' : 'off');
    document.getElementById('box').focus();
    out.inField = press('ArrowDown', { shiftKey: true }); out.inFieldAt = at();
    document.getElementById('ta').focus();
    out.inArea = press('ArrowRight', { shiftKey: true }); out.inAreaAt = at();
    document.activeElement.blur();
    out.ctrl = press('ArrowDown', { shiftKey: true, ctrlKey: true });
    var s = window[Symbol.for('bowser.spatial.v1')];
    s.off();
    out.afterOff = press('ArrowDown', { shiftKey: true }); out.offGone = typeof window[Symbol.for('bowser.spatial.v1')];
  `);
  // The script is a self-invoking expression: it ran when the page loaded the driver's `R`.
  assert.equal(report.noShift.handled, false, 'plain arrows scroll as usual');
  assert.equal(report.noShiftAt, '');
  assert.equal(report.firstAt, 'a1', 'with nothing focused, Down starts from the top');
  assert.equal(report.right, 'a2');
  assert.equal(report.down, 'a4');
  assert.equal(report.left, 'a3');
  assert.equal(report.up, 'a1');
  assert.equal(report.marked, '1:on', 'the focused link is marked, and only that one');
  assert.equal(report.inField.handled, false, 'Shift+arrow in a text field selects text');
  assert.equal(report.inArea.handled, false);
  assert.equal(report.ctrl.handled, false, 'Ctrl+Shift+arrow is not taken');
  assert.equal(report.afterOff.handled, false, 'switched off, the keys are the page\'s again');
  assert.equal(report.offGone, 'undefined');
});

test('spatial navigation does nothing until it is added, and adding it twice adds one handler', { skip }, () => {
  const report = runPage(GRID, `${keys}
    out.before = press('ArrowDown', { shiftKey: true });
    var run = new Function('return ' + ${JSON.stringify(SPATIAL)});
    run(); run();
    out.after = press('ArrowDown', { shiftKey: true }); out.at = at();
    var count = 0; var real = EventTarget.prototype.addEventListener;
  `);
  assert.equal(report.before.handled, false);
  assert.equal(report.after.handled, true);
  assert.equal(report.at, 'a1');
});

const READER = readFileSync(join(root, 'src', 'reader', 'reader.js'), 'utf8').trim().replace(/;$/, '');
const CSS = readFileSync(join(root, 'src', 'reader', 'reader.css'), 'utf8');
const BAR = readFileSync(join(root, 'src', 'reader', 'bar.css'), 'utf8');
const ARTICLE = readFileSync(join(here, 'fixtures', 'reader', 'article-semantic.html'), 'utf8');

const readerDriver = (extra) => `
  var shadows = [], real = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (i) { var r = real.call(this, i); shadows.push(r); return r; };
  var R = ${READER};
  var o = Object.assign({ a: 'toggle', m: 'reader', t: 'auto', s: 19, d: false, k: ${JSON.stringify(CSS)}, b: ${JSON.stringify(BAR)}, c: { bg: '#fff', fg: '#111', ac: '#06c', bd: '#ccc', fl: '#eee' } }, ${JSON.stringify(extra)});
  var before = document.documentElement.outerHTML;
  out.result = R(o);
  var r = document.querySelector('[data-bwr-k=r]'), cs = getComputedStyle(r);
  out.font = cs.fontFamily; out.size = cs.fontSize; out.line = cs.lineHeight; out.mode = cs.writingMode; out.htmlMode = getComputedStyle(document.documentElement).writingMode;
  out.maxWidth = cs.maxWidth; out.width = r.getBoundingClientRect().width;
  out.bar = shadows.length ? Array.from(shadows[0].querySelectorAll('button,span')).map(function (b) { return b.getAttribute('aria-label') || b.textContent; }) : null;
  out.barRole = shadows.length ? shadows[0].querySelector('.b').getAttribute('role') : null;
  out.exit = R(Object.assign({}, o, { a: 'exit' }));
  out.restored = document.documentElement.outerHTML === before;
`;

test('reader view: font, size, line height and column width come from the settings', { skip }, () => {
  const plain = runPage(ARTICLE, readerDriver({}));
  assert.match(plain.font, /Georgia/);
  assert.equal(plain.size, '19px');
  assert.ok(Math.abs(parseFloat(plain.line) - 19 * 1.6) < 0.01);
  assert.equal(plain.mode, 'horizontal-tb');
  const custom = runPage(ARTICLE, readerDriver({ f: 'mono', s: 24, l: 20, w: 50 }));
  assert.match(custom.font, /Consolas/);
  assert.equal(custom.size, '24px');
  assert.ok(Math.abs(parseFloat(custom.line) - 24 * 2.0) < 0.01, custom.line);
  assert.ok(custom.width < plain.width, `a 50ch column is narrower than 68ch (${custom.width} vs ${plain.width})`);
  const sans = runPage(ARTICLE, readerDriver({ f: 'sans' }));
  assert.match(sans.font, /Segoe UI|system-ui/);
  const wide = runPage(ARTICLE, readerDriver({ w: 120 }));
  assert.ok(wide.width > plain.width, 'a 120ch column is wider than 68ch');
  // Out-of-range values fall back to the defaults instead of reaching the page's styles.
  const bad = runPage(ARTICLE, readerDriver({ f: 'comic', l: 99, w: 5 }));
  assert.match(bad.font, /Georgia/);
  assert.ok(Math.abs(parseFloat(bad.line) - 19 * 1.6) < 0.01);
  assert.ok(Math.abs(bad.width - plain.width) < 1);
  for (const r of [plain, custom, sans, wide, bad]) assert.ok(r.restored, 'the document is restored exactly');
});

test('reader view: vertical text runs top to bottom', { skip }, () => {
  const vertical = runPage(ARTICLE, readerDriver({ v: true }));
  assert.equal(vertical.mode, 'vertical-rl');
  assert.equal(vertical.htmlMode, 'vertical-rl');
  assert.ok(vertical.restored);
  assert.equal(runPage(ARTICLE, readerDriver({ v: false })).htmlMode, 'horizontal-tb');
});

test('reader view: the settings icon (control bar) and the reading time are each optional', { skip }, () => {
  const both = runPage(ARTICLE, readerDriver({ g: true, r: true }));
  assert.ok(both.bar.includes('Larger text') && both.bar.some((label) => /^\d+ min read$/.test(label)), both.bar.join('|'));
  assert.equal(both.barRole, 'toolbar');
  const controlsOnly = runPage(ARTICLE, readerDriver({ g: true, r: false }));
  assert.ok(controlsOnly.bar.includes('Larger text') && !controlsOnly.bar.some((label) => /min read/.test(label)));
  const timeOnly = runPage(ARTICLE, readerDriver({ g: false, r: true }));
  assert.ok(!timeOnly.bar.includes('Larger text') && timeOnly.bar.some((label) => /min read/.test(label)), timeOnly.bar.join('|'));
  assert.equal(timeOnly.barRole, 'status');
  const neither = runPage(ARTICLE, readerDriver({ g: false, r: false }));
  assert.equal(neither.bar, null, 'nothing is added to the page');
  for (const r of [both, controlsOnly, timeOnly, neither]) assert.ok(r.restored);
});
