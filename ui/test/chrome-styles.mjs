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
    // The pill being open (the suggestion list is up) brings the field back: a real class, so this is
    // the computed style.
    $('#hidden .pill').classList.add('is-open');
    out.open = {}; parts.forEach(function (p) { out.open[p] = cs('#hidden ' + p).opacity; });
    $('#hidden .pill').classList.remove('is-open');
    // Focus in the field (Ctrl+L) brings it back through :has(.omni-field:focus-within). Chromium only
    // matches :focus/:focus-within while the page itself has focus, and a headless Edge on a CI runner
    // can start without it (document.hasFocus() false), so the shipped rule is checked two ways: its
    // own selector, read back from the CSSOM, with :focus-within swapped for a marker attribute (always
    // deterministic), and the real computed opacity whenever the page does have focus.
    var find = function (rules) {
      for (var i = 0; i < rules.length; i++) {
        var r = rules[i];
        if (r.selectorText && r.selectorText.indexOf('data-hidden="true"') >= 0 && r.selectorText.indexOf(':focus-within') >= 0) return r;
        if (r.cssRules) { var hit = find(r.cssRules); if (hit) return hit; }
      }
      return null;
    };
    var rule = null;
    for (var s = 0; s < document.styleSheets.length && !rule; s++) rule = find(document.styleSheets[s].cssRules);
    if (!rule) throw new Error('no data-hidden rule with :focus-within in app.css');
    out.rule = { selector: rule.selectorText, opacity: rule.style.opacity };
    var marked = rule.selectorText.split(':focus-within').join('[data-st-focus]');
    out.byRule = { before: {}, focused: {} };
    parts.forEach(function (p) { out.byRule.before[p] = $('#hidden ' + p).matches(marked); });
    $('#hidden .omni-field').setAttribute('data-st-focus', '');
    parts.forEach(function (p) { out.byRule.focused[p] = $('#hidden ' + p).matches(marked); });
    $('#hidden .omni-field').removeAttribute('data-st-focus');
    $('#hidden .omni-input').focus();
    out.active = document.activeElement === $('#hidden .omni-input');
    out.hasFocus = document.hasFocus();
    out.focused = cs('#hidden .omni-field').opacity;
  `);
  for (const part of ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr', '.omni-search', '.tb-zoom']) assert.equal(r.shown[part], '1', `${part} shows normally`);
  for (const part of ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr']) assert.equal(r.hidden[part], '0', `${part} is hidden with the address field`);
  assert.equal(r.hidden['.omni-search'], '1', 'the search field stays');
  assert.equal(r.hidden['.tb-zoom'], '1', 'the page buttons stay');
  for (const part of ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr']) assert.equal(r.open[part], '1', `${part} comes back while the pill is open`);
  assert.equal(r.rule.opacity, '0', 'the matched rule is the one that hides the parts');
  assert.match(r.rule.selector, /:not\([^)]*:has\(\.omni-field:focus-within\)/, 'focus inside the field is one of the exceptions');
  for (const part of ['.tb-site', '.omni-field', '.tb-hist', '.tb-qr']) {
    assert.equal(r.byRule.before[part], true, `${part}: the hiding rule matches while the field is unfocused`);
    assert.equal(r.byRule.focused[part], false, `${part}: the hiding rule stops matching once the field has focus`);
  }
  for (const part of ['.omni-search', '.tb-zoom']) assert.equal(r.byRule.before[part], false, `${part} is never matched by the hiding rule`);
  assert.equal(r.active, true, 'the hidden field still takes focus (Ctrl+L reaches it)');
  if (r.hasFocus) assert.equal(r.focused, '1', 'Ctrl+L (focus in the field) brings the address field back');
  else console.log(`# headless page has no focus (document.hasFocus() false): computed :focus-within check covered by the CSSOM selector check; opacity read ${r.focused}`);
});

test('tabs_close_always and tabs_close_left reach the vertical sidebar rows; collapsed and pinned tiles still hide it', { skip }, () => {
  const row = (extra = '') => `<div class="side-tab${extra}"><span class="side-face"></span><span class="side-title">Tab</span><span class="side-flags"></span><button class="side-close"></button></div>`;
  const list = (cls, rows) => `<div class="side-list${cls}" style="width:240px">${rows}</div>`;
  const r = measure(`
    <div id="app" class="sidebar"><div id="plain">${list('', row())}</div><div id="always">${list(' side-close-always', row())}</div>
    <div id="left">${list(' side-close-left side-close-always', row())}</div></div>`, `
    out.plain = cs('#plain .side-close').display;
    out.always = cs('#always .side-close').display;
    var close = $('#left .side-close').getBoundingClientRect(), face = $('#left .side-face').getBoundingClientRect();
    out.leftFirst = close.right <= face.left;
    out.rightFirst = $('#always .side-close').getBoundingClientRect().left > $('#always .side-face').getBoundingClientRect().right;
  `);
  const collapsed = measure(`<div id="app" class="sidebar is-collapsed">${list(' side-close-always', row())}</div>`, `
    out.display = cs('.side-close').display;
  `);
  assert.equal(r.plain, 'none', 'off: the close button waits for hover');
  assert.equal(r.always, 'grid', 'close-always: every unpinned row shows it');
  assert.equal(r.rightFirst, true, 'by default it is at the end of the row');
  assert.equal(r.leftFirst, true, 'close-left: it comes before the favicon');
  assert.equal(collapsed.display, 'none', 'a collapsed sidebar keeps hiding it');
});

test('a narrow inactive strip tab drops its close button, except under tabs_close_always', { skip }, () => {
  const tab = (active) => `<div class="tab${active ? ' active' : ''}" style="flex:none;width:100px"><span class="ico"></span><span class="title">A tab</span><button class="close bw-icon-btn"></button></div>`;
  const strip = (id, cls) => `<div id="${id}"><div id="app" class="strip${cls}">${tab(false)}${tab(true)}</div></div>`;
  const plain = measure(strip('s', ''), `out.inactive = cs('.tab:not(.active) .close').display; out.active = cs('.tab.active .close').display;`);
  const always = measure(strip('s', ' close-always'), `var c = cs('.tab:not(.active) .close'); out.display = c.display; out.opacity = c.opacity;`);
  assert.equal(plain.inactive, 'none', 'narrow and inactive: the title keeps the room');
  assert.notEqual(plain.active, 'none', 'the active tab keeps its close button');
  assert.notEqual(always.display, 'none', 'close-always keeps it on narrow tabs too');
  assert.notEqual(always.opacity, '0', 'and drawn');
});

test('font_size x ui_zoom (fontScale) scales chrome text and ui_font changes its face; the defaults are the design sizes', { skip }, async () => {
  const esbuild = await import('esbuild');
  const out = esbuild.buildSync({
    stdin: { contents: "export { applyTheme } from './view';", resolveDir: join(here, '..', 'src'), loader: 'ts' },
    bundle: true, format: 'iife', globalName: 'BW', platform: 'browser', write: false, logLevel: 'silent',
  });
  const prelude = Buffer.from(out.outputFiles[0].contents).toString('utf8').replace(/<\/script/gi, String.raw`<\/script`);
  const markup = `<div id="app"><span id="chrome">Tab title</span><p id="caption" style="font-size:var(--bw-text-caption);line-height:var(--bw-leading-caption)">Caption</p>
    <p id="body" style="font-size:var(--bw-text-body);line-height:var(--bw-leading-body)">Body</p><h2 id="sub" style="font-size:var(--bw-text-subtitle)">Subtitle</h2>
    <h1 class="pm-title" id="display">Display</h1></div>`;
  const read = `
    var pick = function () { var r = {}; ['body', '#chrome', '#caption', '#body', '#sub', '#display'].forEach(function (s) { var c = cs(s === 'body' ? document.body : s); r[s] = { size: c.fontSize, leading: c.lineHeight, family: c.fontFamily }; }); return r; };
    out.initial = pick();
    BW.applyTheme({}, false, true, 100, '');
    out.defaults = pick();
    BW.applyTheme({}, false, true, 125, 'Consolas');
    out.scaled = pick();
    BW.applyTheme({}, false, true, 100, '');
    out.back = pick();`;
  const r = measure(markup, read, prelude);
  const sizes = (set) => Object.fromEntries(Object.entries(set).map(([k, v]) => [k, v.size]));
  assert.deepEqual(sizes(r.defaults), { body: '12px', '#chrome': '12px', '#caption': '12px', '#body': '14px', '#sub': '20px', '#display': '28px' }, 'scale 100: the design px sizes');
  assert.deepEqual(r.defaults, r.initial, 'the default state message changes nothing');
  assert.equal(r.defaults['#caption'].leading, '16px');
  assert.match(r.defaults.body.family, /^"Segoe UI Variable", "Segoe UI", system-ui, sans-serif$/);
  assert.deepEqual(sizes(r.scaled), { body: '15px', '#chrome': '15px', '#caption': '15px', '#body': '17.5px', '#sub': '25px', '#display': '35px' }, 'fontScale 125 scales every text token');
  assert.equal(r.scaled['#caption'].leading, '20px', 'line heights follow the text');
  assert.match(r.scaled['#chrome'].family, /^"?Consolas"?, "Segoe UI Variable"/, 'ui_font comes first, the stock stack after it');
  assert.deepEqual(r.back, r.defaults, 'back to the defaults: back to the stock face and sizes');
});
