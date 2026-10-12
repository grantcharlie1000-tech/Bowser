// Hardening checks: the lint rule list, the CSP shipped in the page against the one Rust serves, and
// the strict colour and tab-count limits of the bridge (mirrors protocol.rs tests).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { RULES, findViolations } from '../lint-rules.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = join(here, '..');
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
const { parseInbound, MAX_TABS } = await import(bundledUrl);

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

test('the built bundle contains no banned sink', () => {
  const bundle = join(uiRoot, 'dist', 'app.js');
  assert.ok(existsSync(bundle), 'run npm run build first');
  assert.deepEqual(findViolations(readFileSync(bundle, 'utf8')), []);
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
  return { id, title: 't', loading: false, pinned: false, muted: false, marker: -1, g: 0, gl: false, fav: false };
}

test('a tabs message holds up to MAX_TABS items and no more', () => {
  assert.equal(MAX_TABS, 512);
  const full = Array.from({ length: MAX_TABS }, (_, index) => tab(index + 1));
  assert.equal(parseInbound({ v: 1, t: 'tabs', active: 1, items: full })?.items.length, MAX_TABS);
  const over = [...full, tab(MAX_TABS + 1)];
  assert.equal(parseInbound({ v: 1, t: 'tabs', active: 1, items: over }), null);
});

test('setText truncates at 2048 UTF-16 units without splitting a surrogate pair', () => {
  const text = 'a'.repeat(2047) + '😀tail';
  const parsed = parseInbound({ t: 'setText', text, select: true });
  assert.equal(parsed.text.length, 2047);
  assert.equal(parsed.text, 'a'.repeat(2047));
  assert.equal(parseInbound({ t: 'setText', text: '😀'.repeat(1200), select: false }).text.length, 2048);
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
    assert.match(source, /case 'state':[\s\S]*?applyTheme\(msg\.theme, msg\.solid, msg\.dark\)/, file);
  }
});
