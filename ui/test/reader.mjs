// Reader view script (src/reader/reader.js), run for real in headless Edge against fixture pages.
// There is no jsdom here, so each fixture is wrapped with a small driver that runs the script the same
// way Rust does (a function expression called with an options object), then reports what it did through
// a <meta> tag that `--dump-dom` prints. The test skips itself when Edge is not installed.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const readerDir = join(here, '..', '..', 'src', 'reader');
const fixtures = join(here, 'fixtures', 'reader');
let SCRIPT = readFileSync(join(readerDir, 'reader.js'), 'utf8');
if (process.env.BWR_DEBUG) SCRIPT = SCRIPT.replace("return res('err', null, 0, 0);", "return 'ERR:' + (x && x.stack);");
const CSS = readFileSync(join(readerDir, 'reader.css'), 'utf8');
const BAR = readFileSync(join(readerDir, 'bar.css'), 'utf8');

const EDGE = [
  process.env.BOWSER_TEST_EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
].find((path) => path && existsSync(path));
const skip = EDGE ? false : 'Microsoft Edge is not installed';

// Rust's thresholds for showing the toolbar button (src/reader/mod.rs: MIN_ARTICLE_WORDS, MAX_LINK_PERCENT).
const MIN_WORDS = 250;
const MAX_LINK = 35;
const RESULT = /^r2\|(ok|none|err)\|(off|reader|calm)\|\d{1,7}\|\d{1,3}\|(auto|light|sepia|dark)\|\d{2}\|[a-z]{0,3}$/;

const DRIVER = `
(function () {
  var cfg = __CFG__;
  var R = __SCRIPT__;
  var out = { errors: [] };
  var shadows = [];
  var realAttach = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (i) { var r = realAttach.call(this, i); shadows.push(r); return r; };
  var base = { k: __CSS__, b: __BAR__, c: { bg: '#101010', fg: '#eeeeee', ac: '#8ab4f8', bd: '#444444', fl: '#222222' }, d: true, t: 'auto', s: 19 };
  function call(extra) { var o = {}, k; for (k in base) o[k] = base[k]; for (k in extra) o[k] = extra[k]; return R(o); }
  function snap() { return document.documentElement.outerHTML; }
  function vis(sel) {
    var l = document.querySelectorAll(sel), i;
    for (i = 0; i < l.length; i++) if (l[i].getClientRects().length > 0 && getComputedStyle(l[i]).visibility !== 'hidden') return true;
    return false;
  }
  function done() {
    var m = document.createElement('meta'); m.name = 'bwr-out'; m.content = encodeURIComponent(JSON.stringify(out));
    document.head.appendChild(m);
    document.body.textContent = '';
  }
  try {
    var before = snap();
    out.probe = call({ a: 'probe' });
    var t0 = Date.now();
    out.toggle = call({ a: 'toggle', m: cfg.mode });
    out.ms = Date.now() - t0;
    if (!cfg.minimal) {
      var root = document.querySelector('[data-bwr-k=r]');
      out.root = root ? root.tagName.toLowerCase() + (root.id ? '#' + root.id : '') + (root.className ? '.' + root.className : '') : null;
      out.shown = {}; out.hidden = {};
      (cfg.show || []).forEach(function (s) { out.shown[s] = vis(s); });
      (cfg.hide || []).forEach(function (s) { out.hidden[s] = vis(s); });
      if (root) {
        var cs = getComputedStyle(root);
        out.width = root.getBoundingClientRect().width;
        out.fontSize = cs.fontSize; out.lineHeight = cs.lineHeight; out.fontFamily = cs.fontFamily;
        out.h1 = (root.querySelector('h1') || {}).textContent || null;
        out.bodyMargin = getComputedStyle(document.body).margin;
      }
      out.overflow = getComputedStyle(document.documentElement).overflowY + '/' + getComputedStyle(document.body).overflowY;
      out.sheets = document.adoptedStyleSheets.length;
      if (cfg.mode === 'reader' && root) {
        var bar = shadows[0], bt = {};
        bar.querySelectorAll('button').forEach(function (b) { bt[b.getAttribute('aria-label')] = b; });
        out.buttons = Object.keys(bt);
        bt['Larger text'].click(); out.afterLarger = getComputedStyle(root).fontSize;
        bt['Smaller text'].click(); bt['Smaller text'].click(); out.afterSmaller = getComputedStyle(root).fontSize;
        bt['dark theme'].click();
        out.dark = { bg: getComputedStyle(document.documentElement).getPropertyValue('--bwr-bg').trim(), fg: getComputedStyle(root).color, pressed: bt['dark theme'].getAttribute('aria-pressed'), scheme: getComputedStyle(document.documentElement).colorScheme };
        bt['sepia theme'].click(); out.sepia = getComputedStyle(document.documentElement).getPropertyValue('--bwr-bg').trim();
        bt['light theme'].click(); out.light = getComputedStyle(document.documentElement).getPropertyValue('--bwr-bg').trim();
        bt['auto theme'].click(); out.auto = getComputedStyle(document.documentElement).getPropertyValue('--bwr-bg').trim();
        out.persisted = call({ a: 'probe' });
      }
      if (cfg.switchTo) { out.switched = call({ a: 'toggle', m: cfg.switchTo }); out.switchedRoot = !!document.querySelector('[data-bwr-k=r]'); }
    }
    out.exit = call({ a: 'toggle', m: cfg.switchTo || cfg.mode });
    out.exitAgain = call({ a: 'exit' });
    out.restored = snap() === before;
    if (!out.restored) { var a = snap(), i = 0; while (i < a.length && a[i] === before[i]) i++; out.diff = a.slice(Math.max(0, i - 80), i + 120) + ' || ' + before.slice(Math.max(0, i - 80), i + 120); }
    out.sheetsAfter = document.adoptedStyleSheets.length;
  } catch (e) { out.errors.push(String(e && e.stack || e)); }
  done();
})();
`;

/** Runs the driver against one fixture in headless Edge and returns its report. */
function run(name, cfg, html = readFileSync(join(fixtures, name), 'utf8')) {
  const dir = mkdtempSync(join(tmpdir(), 'bowser-reader-test-'));
  try {
    const driver = DRIVER.replace('__CFG__', () => JSON.stringify(cfg))
      .replace('__SCRIPT__', () => SCRIPT.trim().replace(/;$/, ''))
      .replace('__CSS__', () => JSON.stringify(CSS))
      .replace('__BAR__', () => JSON.stringify(BAR));
    const file = join(dir, 'page.html');
    writeFileSync(file, html.replace('</body>', () => `<script>${driver}</script></body>`));
    const profile = join(dir, 'profile');
    mkdirSync(profile);
    const result = spawnSync(EDGE, [
      '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
      `--user-data-dir=${profile}`, '--window-size=1000,800', '--dump-dom', pathToFileURL(file).href,
    ], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, timeout: 60000 });
    const match = /name="bwr-out" content="([^"]*)"/.exec(result.stdout ?? '');
    assert.ok(match, `${name}: no report (exit ${result.status}) ${String(result.stderr).slice(0, 300)}`);
    const report = JSON.parse(decodeURIComponent(match[1]));
    if (process.env.BWR_DEBUG) console.log(name, JSON.stringify(report, null, 1));
    assert.deepEqual(report.errors, [], `${name}: driver errors`);
    return report;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const parse = (text) => {
  assert.match(text, RESULT, 'result has the fixed shape');
  const [, code, state, words, link, theme, size] = text.split('|');
  return { code, state, words: Number(words), link: Number(link), theme, size: Number(size) };
};
const eligible = (r) => r.words >= MIN_WORDS && r.link <= MAX_LINK;

const ARTICLES = [
  {
    file: 'article-semantic.html', root: 'article#story',
    show: ['article#story h1', 'article#story p', 'img#lead', 'blockquote#quote', 'figcaption'],
    hide: ['header#top', 'nav', 'aside#side', 'footer#foot', '#cookie-banner', '#newsletter-popup', '.share-bar', 'iframe#embed', '#comment-form'],
  },
  {
    file: 'article-divs.html', root: 'div#entry.entry-content',
    show: ['.entry-content p', '.entry-content h1'],
    hide: ['#header', '#sidebar', '#comments', '#footer', '.post-meta'],
  },
  {
    file: 'article-wiki.html', root: 'main#content.mw-body',
    show: ['#firstHeading', '.mw-parser-output p', '.infobox', 'h2#h'],
    hide: ['#left-nav', '.navbox', '#toc', '.mw-editsection', '#footer'],
  },
  {
    file: 'article-title-outside.html', root: 'div#body.story-body',
    show: ['.story-body p', '.story-body h1'], hide: ['.masthead', '.rail', 'footer'], title: 'Council backs a night ferry for the first time',
  },
  { file: 'article-cjk.html', root: 'article', show: ['article p', 'article h1'], hide: ['nav'] },
];

for (const fixture of ARTICLES) {
  test(`reader: ${fixture.file} is found, restyled in place and restored exactly`, { skip }, (t) => {
    const r = run(fixture.file, { mode: 'reader', show: fixture.show, hide: fixture.hide });
    const probe = parse(r.probe);
    assert.equal(probe.code, 'ok');
    assert.equal(probe.state, 'off');
    assert.ok(eligible(probe), `looks like an article: ${r.probe}`);
    const toggled = parse(r.toggle);
    assert.deepEqual([toggled.code, toggled.state], ['ok', 'reader'], r.toggle);
    assert.equal(r.root, fixture.root, 'chose the content block');
    for (const [selector, visible] of Object.entries(r.shown)) assert.ok(visible, `${selector} stays visible`);
    for (const [selector, visible] of Object.entries(r.hidden)) assert.ok(!visible, `${selector} is hidden`);
    assert.ok(r.width <= 1000 && r.width > 300, `measure is bounded: ${r.width}`);
    assert.ok(r.width < 820, `about 68ch at 19px (${r.width})`);
    assert.equal(r.fontSize, '19px');
    assert.ok(Math.abs(parseFloat(r.lineHeight) - 19 * 1.6) < 0.01, r.lineHeight);
    assert.match(r.fontFamily, /Georgia/);
    assert.equal(r.bodyMargin, '0px');
    assert.ok(r.h1 && r.h1.length > 5, 'there is a title');
    if (fixture.title) assert.equal(r.h1, fixture.title);
    assert.equal(r.sheets, 1, 'one constructed sheet, no <style> element');
    // Reader controls: A-/A+ and the four themes.
    assert.deepEqual(r.buttons, ['Smaller text', 'Larger text', 'auto theme', 'light theme', 'sepia theme', 'dark theme']);
    assert.equal(r.afterLarger, '21px');
    assert.equal(r.afterSmaller, '17px');
    assert.equal(r.dark.bg, '#17181a');
    assert.equal(r.dark.fg, 'rgb(216, 216, 214)');
    assert.equal(r.dark.pressed, 'true');
    assert.equal(r.dark.scheme, 'dark');
    assert.equal(r.sepia, '#f4ecd8');
    assert.equal(r.light, '#fbfbf9');
    assert.equal(r.auto, '#101010', 'auto follows the colours Rust passed in');
    const persisted = parse(r.persisted);
    assert.deepEqual([persisted.state, persisted.theme, persisted.size], ['reader', 'auto', 17], 'prefs round-trip to Rust');
    // Toggle again leaves reader view, and the document is byte for byte what it was.
    const exited = parse(r.exit);
    assert.deepEqual([exited.code, exited.state], ['ok', 'off']);
    assert.equal(exited.theme, 'auto');
    assert.equal(exited.size, 17);
    assert.equal(parse(r.exitAgain).state, 'off');
    assert.ok(r.restored, `document restored: ${r.diff}`);
    assert.equal(r.sheetsAfter, 0);
    if (fixture.file === 'article-cjk.html') t.diagnostic(`reader execution: ${r.ms} ms; unchanged budget: <1500 ms`);
    assert.ok(r.ms < 1500, `took ${r.ms} ms`);
  });
}

for (const [file, why] of [
  ['non-article-home.html', 'a front page of link blurbs'],
  ['non-article-search.html', 'search results'],
  ['non-article-app.html', 'a dashboard'],
]) {
  test(`reader: ${file} (${why}) is not offered`, { skip }, () => {
    const r = run(file, { mode: 'reader', minimal: true });
    const probe = parse(r.probe);
    assert.ok(!eligible(probe), `not article-like: ${r.probe}`);
    const toggled = parse(r.toggle);
    // Either nothing qualified, or the command still worked; never a half applied page.
    assert.ok(r.restored, `document restored: ${r.diff}`);
    assert.ok(toggled.code === 'none' || toggled.code === 'ok');
  });
}

test('reader: a page with no article text reports none and changes nothing', { skip }, () => {
  const r = run('non-article-app.html', { mode: 'reader', minimal: true });
  const toggled = parse(r.toggle);
  assert.deepEqual([toggled.code, toggled.state], ['none', 'off']);
  assert.ok(r.restored);
});

test('reader: reader view works under a strict CSP that forbids every stylesheet', { skip }, () => {
  const r = run('csp-article.html', { mode: 'reader', show: ['article h1'], hide: [] });
  assert.equal(parse(r.toggle).state, 'reader');
  assert.equal(r.sheets, 1, 'a constructed sheet is exempt from style-src');
  assert.equal(r.fontSize, '19px', 'and it applied');
  assert.ok(r.restored, r.diff);
});

test('distractions mode hides overlays, banners and sticky bars, frees scrolling, and restores', { skip }, () => {
  const r = run('calm-overlay.html', {
    mode: 'calm', show: ['#page h1', '#page p'], hide: ['#consent', '#modal', '#sticky-head'],
  });
  const toggled = parse(r.toggle);
  assert.deepEqual([toggled.code, toggled.state], ['ok', 'calm']);
  assert.equal(r.root, null, 'no reader restyle');
  for (const [selector, visible] of Object.entries(r.shown)) assert.ok(visible, `${selector} stays visible`);
  for (const [selector, visible] of Object.entries(r.hidden)) assert.ok(!visible, `${selector} is hidden`);
  assert.doesNotMatch(r.overflow, /hidden/, 'the scroll lock the overlay set is released');
  assert.ok(r.restored, r.diff);
  assert.equal(r.sheetsAfter, 0);
});

test('switching from distractions mode to reader view, then leaving, restores the page', { skip }, () => {
  const r = run('article-semantic.html', { mode: 'calm', switchTo: 'reader', show: [], hide: ['#cookie-banner'] });
  assert.equal(parse(r.toggle).state, 'calm');
  assert.equal(parse(r.switched).state, 'reader');
  assert.equal(r.switchedRoot, true);
  assert.equal(parse(r.exit).state, 'off');
  assert.ok(r.restored, r.diff);
});

test('hostile page: tampered natives and throwing getters give a fixed-shape result and no half-applied page', { skip }, () => {
  const r = run('hostile.html', { mode: 'reader', minimal: true });
  assert.match(r.probe, RESULT);
  assert.match(r.toggle, RESULT);
  assert.match(r.exit, RESULT);
  assert.ok(r.restored, `page left as it was: ${r.diff}`);
  assert.ok(r.ms < 1500);
});

test('big page: scanning stays within its time budget and still restores', { skip }, () => {
  const sentence = 'The harbour authority said on Tuesday that the new timetable would take effect in spring, after months of consultation';
  const html = `<!doctype html><html><head><title>Big</title></head><body><article id="story"><h1>Big</h1>${
    Array.from({ length: 6000 }, () => `<p>${sentence}, with residents, traders and councils.</p>`).join('')
  }</article>${Array.from({ length: 8000 }, (_, i) => `<div class="x${i}"><span>x</span></div>`).join('')}</body></html>`;
  const r = run('big-dom (generated)', { mode: 'reader', minimal: true }, html);
  assert.match(r.toggle, RESULT);
  assert.ok(r.ms < 2500, `took ${r.ms} ms`);
  assert.ok(r.restored, r.diff);
});

test('the fixtures cover article-like and non-article pages', () => {
  const files = readdirSync(fixtures);
  assert.ok(files.filter((f) => f.startsWith('article-')).length >= 4);
  assert.ok(files.filter((f) => f.startsWith('non-article-')).length >= 3);
});

test('the script holds to its safety rules', () => {
  // No eval, no network, no storage, no dynamic code, no HTML-string sinks.
  assert.doesNotMatch(SCRIPT, /\beval\s*\(|new\s+Function|innerHTML|outerHTML|insertAdjacentHTML|document\.write|fetch\s*\(|XMLHttpRequest|sendBeacon|WebSocket|localStorage|sessionStorage|document\.cookie|postMessage|import\s*\(/);
  assert.doesNotMatch(CSS + BAR, /@import|url\(|expression\(|javascript:/i);
  // The only output is the fixed-shape string.
  assert.match(SCRIPT, /'r2\|'/);
  assert.ok(SCRIPT.length < 13500, `script is ${SCRIPT.length} bytes`);
});
