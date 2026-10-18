// Computed styles of the built chrome stylesheet (dist/app.css), measured for real in headless Edge (no
// window is shown; the test skips itself when Edge is not installed). Each case lays out a small piece of
// chrome markup with the classes the surfaces use and reads back what the browser computed, so a rule
// that is outranked or never matches fails here even when its text looks right.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const CSS = pathToFileURL(join(here, '..', 'dist', 'app.css')).href;

const EDGE = [
  process.env.BOWSER_TEST_EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
].find((path) => path && existsSync(path));
const skip = EDGE ? false : 'Microsoft Edge is not installed';

/**
 * Loads `body` (markup) under dist/app.css, runs `script` (a function body that fills `out`, with `$`
 * as querySelector and `cs` as getComputedStyle), and returns `out`. `prelude` is extra script that
 * runs first (a bundled module, for instance).
 */
export function measure(body, script, prelude = '') {
  const dir = mkdtempSync(join(tmpdir(), 'bowser-styles-test-'));
  try {
    const driver = `
window.addEventListener('load', function () {
  var out = { errors: [] };
  var $ = function (s) { return document.querySelector(s); };
  var cs = function (s) { var n = typeof s === 'string' ? $(s) : s; if (!n) throw new Error('missing ' + s); return getComputedStyle(n); };
  try { ${script} } catch (e) { out.errors.push(String(e && e.stack || e)); }
  var m = document.createElement('meta'); m.name = 'st-out'; m.content = encodeURIComponent(JSON.stringify(out));
  document.head.appendChild(m);
});`;
    const file = join(dir, 'page.html');
    writeFileSync(file, `<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="${CSS}"></head><body>${body}<script>${prelude}</script><script>${driver}</script></body></html>`);
    const profile = join(dir, 'profile');
    mkdirSync(profile);
    const result = spawnSync(EDGE, [
      '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--allow-file-access-from-files',
      `--user-data-dir=${profile}`, '--window-size=1000,800', '--virtual-time-budget=3000', '--dump-dom', pathToFileURL(file).href,
    ], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 120000 });
    const match = /name="st-out" content="([^"]*)"/.exec(result.stdout ?? '');
    assert.ok(match, `no report (exit ${result.status}) ${String(result.stderr).slice(0, 300)}`);
    const report = JSON.parse(decodeURIComponent(match[1]));
    assert.deepEqual(report.errors, [], 'driver errors');
    return report;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test('address_bar_show off hides only the address part of the pill: the search field and page buttons stay', { skip }, () => {
  const pill = (hidden) => `<div class="pill"${hidden ? ' data-hidden="true"' : ''}>
    <button class="tb-site"></button><div class="omni-field"><input class="omni-input"></div><button class="tb-hist"></button><button class="tb-qr"></button>
    <div class="omni-search"><input class="omni-search-input"></div><button class="tb-zoom"></button></div>`;
  const r = measure(`<div id="app"><div id="shown">${pill(false)}</div><div id="hidden">${pill(true)}</div></div>`, `
    var parts = ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr', '.omni-search', '.tb-zoom'];
    out.shown = {}; out.hidden = {};
    parts.forEach(function (p) { out.shown[p] = cs('#shown ' + p).opacity; out.hidden[p] = cs('#hidden ' + p).opacity; });
    $('#hidden .omni-input').focus();
    out.focused = cs('#hidden .omni-field').opacity;
  `);
  for (const part of ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr', '.omni-search', '.tb-zoom']) assert.equal(r.shown[part], '1', `${part} shows normally`);
  for (const part of ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr']) assert.equal(r.hidden[part], '0', `${part} is hidden with the address field`);
  assert.equal(r.hidden['.omni-search'], '1', 'the search field stays');
  assert.equal(r.hidden['.tb-zoom'], '1', 'the page buttons stay');
  assert.equal(r.focused, '1', 'Ctrl+L (focus in the field) brings the address field back');
});
