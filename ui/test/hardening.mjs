// Hardening checks: the lint rule list, the CSP shipped in the page against the one Rust serves, and
// the strict colour and tab-count limits of the bridge (mirrors protocol.rs tests).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { RULES, findViolations } from '../lint-rules.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = join(here, '..');
// Every script build.mjs writes: app.js (classic, every view) and the pages it imports on demand.
const BUNDLES = ['app.js', 'settings-page.js', 'history-page.js', 'start-page.js'];
// Text only each page's own code holds, so its absence shows the page left app.js.
const PAGE_MARKERS = {
  'settings-page': ['sp-search-input', 'Settings sections'],
  'history-page': ['hp-search-input', 'hp-empty-title'],
  'start-page': ['sp-widget-grid', 'Speed dial groups'],
};
// protocol.ts imports the rail grammar from rail-logic.ts, so it is bundled rather than transpiled alone.
const bundled = esbuild.buildSync({
  entryPoints: [join(uiRoot, 'src', 'protocol.ts')],
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  write: false,
  logLevel: 'silent',
});
const bundledUrl = `data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].contents).toString('base64')}`;
const { parseInbound, MAX_TABS, MAX_TEXT } = await import(bundledUrl);

const names = (text) => findViolations(text).map((v) => v.rule);

test('every banned sink in the lint list is flagged', () => {
  const samples = {
    innerHTML: 'el.innerHTML = x;',
    outerHTML: 'el.outerHTML = x;',
    insertAdjacentHTML: "el.insertAdjacentHTML('beforeend', x);",
    'eval(': 'eval(code);',
    'new Function': "new Function('return 1');",
    'document.write': "document.write('<b>');",
    'inline event handler attribute': '<b onclick="x()">',
  };
  assert.deepEqual(Object.keys(samples).sort(), RULES.map((r) => r.name).sort(), 'every rule has a sample');
  for (const [name, line] of Object.entries(samples)) {
    assert.deepEqual(names(line), [name], line);
  }
});

test('violations carry 1-based line numbers and clean code passes', () => {
  assert.deepEqual(findViolations('const a = 1;\r\nel.innerHTML = b;\nok();'), [{ line: 2, rule: 'innerHTML' }]);
  assert.deepEqual(findViolations("el.textContent = 'hi'; el.setAttribute('x', y);"), []);
});

test('the built bundles contain no banned sink', () => {
  for (const name of BUNDLES) {
    const bundle = join(uiRoot, 'dist', name);
    assert.ok(existsSync(bundle), `${name}: run npm run build first`);
    assert.deepEqual(findViolations(readFileSync(bundle, 'utf8')), [], name);
  }
});

test('every built script is served by assets.rs and nothing else is', () => {
  const built = readdirSync(join(uiRoot, 'dist')).filter((name) => name.endsWith('.js')).sort();
  assert.deepEqual(built, [...BUNDLES].sort());
  const assets = readFileSync(join(uiRoot, '..', 'src', 'chrome_host', 'assets.rs'), 'utf8');
  const served = [...assets.matchAll(/^\s*"([a-z-]+\.js)" => Some\(Asset \{/gm)].map((m) => m[1]).sort();
  assert.deepEqual(served, built);
});

test('the settings and history pages are not in app.js: it imports them only for their view', () => {
  const app = readFileSync(join(uiRoot, 'dist', 'app.js'), 'utf8');
  assert.match(app, /^("use strict";)?\(\(\)=>\{/, 'app.js stays a classic IIFE script');
  for (const [page, markers] of Object.entries(PAGE_MARKERS)) {
    const own = readFileSync(join(uiRoot, 'dist', `${page}.js`), 'utf8');
    assert.ok(app.includes(`"./${page}.js"`), `app.js names ./${page}.js`);
    assert.match(own, /\bexport\s*\{/, `${page}.js is a module`);
    for (const marker of markers) {
      assert.ok(own.includes(marker), `${page}.js holds ${JSON.stringify(marker)}`);
      assert.ok(!app.includes(marker), `app.js holds ${page} text ${JSON.stringify(marker)}`);
    }
  }
});

test('the page meta CSP matches the header Rust serves, minus directives meta cannot carry', () => {
  const html = readFileSync(join(uiRoot, 'src', 'index.html'), 'utf8');
  const meta = html.match(/<meta http-equiv="Content-Security-Policy" content="([^"]+)">/);
  assert.ok(meta, 'meta CSP present');
  const assets = readFileSync(join(uiRoot, '..', 'src', 'chrome_host', 'assets.rs'), 'utf8');
  const header = assets.match(/Content-Security-Policy: ([^\\]+)\\r\\n/);
  assert.ok(header, 'header CSP present');
  assert.equal(meta[1], header[1]);
  for (const directive of ['frame-ancestors', 'report-uri', 'sandbox']) {
    assert.ok(!meta[1].includes(directive), `${directive} is not valid in a meta CSP`);
  }
});

function stateWithTheme(value) {
  return {
    t: 'state',
    scale: 1,
    pillx: 0,
    pillw: 624,
    menux: 627,
    solid: false,
    dark: true,
    back: false,
    forward: false,
    loading: false,
    progress: 0,
    security: 'none',
    label: '',
    theme: { bg: value },
  };
}

test('theme colours accept exactly the forms Rust accepts', () => {
  for (const good of [
    '#fff',
    '#ffff',
    '#ffffff',
    '#ffffffff',
    '#AbC123',
    'rgb(1, 2, 3)',
    'rgb(1,2,3)',
    'rgba(0,0,0,0.5)',
    'rgba(10 20 30 50%)',
    'rgb(100%, 50%, 0%)',
  ]) {
    assert.equal(parseInbound(stateWithTheme(good))?.theme.bg, good, good);
  }
});

test('theme colours reject everything else, including url() and other functions', () => {
  for (const bad of [
    '',
    '#',
    '#ff',
    '#fffff',
    '#fffffff',
    '#ggg',
    '#fff ',
    ' #fff',
    'red',
    'url(x)',
    'url(https://example.com/x.png)',
    'rgb()',
    'rgb( )',
    'rgb(a,b,c)',
    'rgb(1,2,3',
    'rgb(1,2,3);',
    'rgb(0,0,0)}',
    'rgb(0,0,0)rgb(1,1,1)',
    'rgb(1,2,3)\n',
    'RGB(1,2,3)',
    'rgb(1/2/3)',
    'hsl(0, 0%, 0%)',
    'expression(alert(1))',
    '#fff; background:url(x)',
    `rgba(${'1,'.repeat(40)}1)`,
  ]) {
    assert.equal(parseInbound(stateWithTheme(bad))?.theme.bg, undefined, JSON.stringify(bad));
  }
});

function tab(id) {
  return { id, title: 't', loading: false, pinned: false, muted: false, private: id % 2 === 0, marker: -1, g: 0, gl: false, fav: false };
}

test('a tabs message holds up to MAX_TABS items and no more', () => {
  assert.equal(MAX_TABS, 512);
  const full = Array.from({ length: MAX_TABS }, (_, index) => tab(index + 1));
  assert.equal(parseInbound({ v: 1, t: 'tabs', active: 1, items: full })?.items.length, MAX_TABS);
  const over = [...full, tab(MAX_TABS + 1)];
  assert.equal(parseInbound({ v: 1, t: 'tabs', active: 1, items: over }), null);
});

test('setText truncates at MAX_TEXT UTF-16 units without splitting a surrogate pair', () => {
  const text = 'a'.repeat(MAX_TEXT - 1) + '😀tail';
  const parsed = parseInbound({ t: 'setText', text, select: true });
  assert.equal(parsed.text.length, MAX_TEXT - 1);
  assert.equal(parsed.text, 'a'.repeat(MAX_TEXT - 1));
  assert.equal(parseInbound({ t: 'setText', text: '😀'.repeat(MAX_TEXT / 2 + 100), select: false }).text.length, MAX_TEXT);
});

test('a full toolbar state message is accepted for all chrome views', () => {
  const sample = stateWithTheme('#fff');
  assert.equal(parseInbound(sample)?.t, 'state');
  assert.equal(parseInbound(JSON.stringify(sample))?.scale, 1);
  assert.deepEqual(
    (({ pillx, pillw, menux }) => ({ pillx, pillw, menux }))(parseInbound(sample)),
    { pillx: 0, pillw: 624, menux: 627 },
  );
  const oldState = { ...sample };
  delete oldState.pillx;
  delete oldState.pillw;
  delete oldState.menux;
  assert.equal(parseInbound(oldState)?.pillw, undefined, 'older state messages keep the flex-centred fallback');
  assert.equal(parseInbound({ ...sample, pillx: -1 }), null);
  assert.equal(parseInbound({ ...sample, menux: 1.5 }), null);
  assert.equal(parseInbound({ ...sample, pillw: undefined }), null);
  assert.equal(parseInbound({ ...sample, unexpected: true }), null);
  for (const file of ['main.ts', 'tabs.ts', 'rail.ts']) {
    const source = readFileSync(join(uiRoot, 'src', file), 'utf8');
    assert.match(source, /case 'state':[\s\S]*?applyTheme\(msg\.theme, msg\.solid, msg\.dark[,)]/, file);
  }
});
